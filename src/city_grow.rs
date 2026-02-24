use crate::{
    renderer::{Renderer, draw_operation::DrawOperation},
    scene::Scene,
};
use anyhow::Result;
use rand::RngExt as _;
use rand::rngs::ThreadRng;
use tracing::{debug, error, info};
use windows::Win32::Graphics::Direct2D::Common::{D2D_RECT_F, D2D1_COLOR_F};
use windows::Win32::Graphics::Direct2D::ID2D1CommandList;
use windows_numerics::Vector2;

pub struct CityGrowConfig {
    pub initial_size: u8,
    pub life_time: u16,
    pub life_time_branch: u16,
    pub prop_city_to_land: f32,
    pub prop_land_to_city: f32,
    pub prop_branch_off: f32,
    pub prop_branch_off_land: f32,
    pub prop_branch_off_to_main: f32,
    pub branch_fall_off: f32,
    pub change_hue_new_main: f32,
    pub start_branches: u8,
    pub show_reverse: bool,
    pub fill_city: bool,
    pub max_steps_back: u16,
    pub lightness_default: u8,
    pub lightness_branch: u8,
    pub line_thickness: f32,
    // Reverse animation performance options
    pub reverse_actions_per_frame: usize,
    pub reverse_render_every_n_frames: usize,
    pub reverse_update_every_n_frames: usize, // Only remove actions every N frames for efficiency
}

impl Default for CityGrowConfig {
    fn default() -> Self {
        Self {
            initial_size: 3,
            life_time: 8000,
            life_time_branch: 15,
            prop_city_to_land: 12.0,
            prop_land_to_city: 0.003,
            prop_branch_off: 15.0,
            prop_branch_off_land: 6.0,
            prop_branch_off_to_main: 1.0,
            branch_fall_off: 50.0,
            change_hue_new_main: 9.0,
            start_branches: 3,
            show_reverse: true,
            fill_city: true,
            max_steps_back: 300,
            lightness_default: 140,
            lightness_branch: 60,
            line_thickness: 2.0,
            // Smooth reverse animation: small incremental updates
            reverse_actions_per_frame: 30,
            reverse_render_every_n_frames: 1,
            reverse_update_every_n_frames: 3,
        }
    }
}

/// Chunk size for command list caching (number of steps per chunk)
const CHUNK_SIZE: usize = 50;

/// Position on the grid
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct Pos {
    x: i32,
    y: i32,
}

impl Pos {
    fn new(x: i32, y: i32) -> Self {
        Self { x, y }
    }

    fn to_idx(&self, cell_count_x: i32) -> usize {
        (self.y * cell_count_x + self.x) as usize
    }

    fn from_idx(idx: usize, cell_count_x: i32) -> Self {
        let idx = idx as i32;
        let y = idx / cell_count_x;
        let x = idx - y * cell_count_x;
        Self { x, y }
    }
}

/// Drawing action for history (for reverse animation)
#[derive(Debug, Clone)]
enum DrawAction {
    Line {
        from_x: f32,
        from_y: f32,
        to_x: f32,
        to_y: f32,
    },
    Rect {
        x: f32,
        y: f32,
        width: f32,
        height: f32,
    },
}

/// Branch state
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum BranchState {
    Running,
    Stopped,
}

/// Branch mode
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum BranchMode {
    City,
    Land,
}

/// A growing branch
#[derive(Clone)]
struct Branch {
    pos: Pos,
    state: BranchState,
    mode: BranchMode,
    expand_direction: Pos,
    own_fields: Vec<Pos>,
    age: u32,
    life_time: u32,
    hue: f32,
    saturation: f32,
    lightness: f32,
    history: Vec<DrawAction>,
    rendered_count: usize, // Track how many actions have been rendered
    pending_erasures: Vec<DrawAction>, // Actions to erase in next render

    // Cached color values (performance optimization)
    cached_color: D2D1_COLOR_F,
    cached_secondary_color: D2D1_COLOR_F,

    // Chunked command lists for efficient reverse rendering
    chunks: Vec<ID2D1CommandList>,
    chunk_start_idx: usize, // Index in history where the next chunk should start
}

impl Branch {
    fn new(pos: Pos, life_time: u32, lightness: f32) -> Self {
        let mut rng = rand::rng();
        let hue = rng.random_range(0.0..360.0);
        let saturation = 100.0;

        // Pre-calculate colors
        let cached_color = hsl_to_rgb(hue, saturation, lightness);
        let mut cached_secondary_color = cached_color;
        cached_secondary_color.a = 0.25;

        Self {
            pos,
            state: BranchState::Running,
            mode: BranchMode::City,
            expand_direction: Pos::new(0, 0),
            own_fields: vec![pos],
            age: 0,
            life_time,
            hue,
            saturation,
            lightness,
            history: Vec::new(),
            rendered_count: 0,
            pending_erasures: Vec::new(),
            cached_color,
            cached_secondary_color,
            chunks: Vec::new(),
            chunk_start_idx: 0,
        }
    }

    fn get_color(&self) -> D2D1_COLOR_F {
        self.cached_color
    }

    fn get_secondary_color(&self) -> D2D1_COLOR_F {
        self.cached_secondary_color
    }

    fn update_cached_colors(&mut self) {
        self.cached_color = hsl_to_rgb(self.hue, self.saturation, self.lightness);
        self.cached_secondary_color = self.cached_color;
        self.cached_secondary_color.a = 0.25;
    }

    fn create_line(
        &mut self,
        to_pos: Pos,
        from_pos: Option<Pos>,
        size: f32,
        fill_city: bool,
    ) -> Vec<DrawAction> {
        let from_pos = from_pos.unwrap_or(self.pos);
        let mut actions = Vec::new();

        let line_width = if self.mode == BranchMode::Land {
            2.0
        } else {
            2.0
        };
        let offset = line_width / 2.0;
        let margin = line_width / 2.0;

        // Pre-calculate scale factors (performance optimization)
        let scale = 2.0 * size;
        let margin_offset = margin + offset;
        let rect_size = scale - 2.0 * margin;

        // Fill rectangles for city mode
        if fill_city && self.mode == BranchMode::City && !self.own_fields.is_empty() {
            let last_position = self.own_fields[self.own_fields.len() - 1];

            // Perpendicular vector
            let perp_x = to_pos.y - last_position.y;
            let perp_y = to_pos.x - last_position.x;

            // First rectangle
            let imaginary = Pos::new(last_position.x + perp_x, last_position.y + perp_y);
            let left_top = Pos::new(to_pos.x.min(imaginary.x), to_pos.y.min(imaginary.y));

            actions.push(DrawAction::Rect {
                x: scale * left_top.x as f32 + margin_offset,
                y: scale * left_top.y as f32 + margin_offset,
                width: rect_size,
                height: rect_size,
            });

            // Second rectangle (mirrored)
            let perp_x = -perp_x;
            let perp_y = -perp_y;
            let imaginary = Pos::new(last_position.x + perp_x, last_position.y + perp_y);
            let left_top = Pos::new(to_pos.x.min(imaginary.x), to_pos.y.min(imaginary.y));

            actions.push(DrawAction::Rect {
                x: scale * left_top.x as f32 + margin_offset,
                y: scale * left_top.y as f32 + margin_offset,
                width: rect_size,
                height: rect_size,
            });
        }

        // Draw line
        actions.push(DrawAction::Line {
            from_x: scale * from_pos.x as f32 + offset,
            from_y: scale * from_pos.y as f32 + offset,
            to_x: scale * to_pos.x as f32 + offset,
            to_y: scale * to_pos.y as f32 + offset,
        });

        self.pos = to_pos;
        self.own_fields.push(to_pos);

        actions
    }

    fn move_to_new_pos(
        &mut self,
        cells: &[u8],
        cell_count_x: i32,
        cell_count_y: i32,
        max_steps_back: usize,
    ) -> bool {
        let start = self.own_fields.len().saturating_sub(max_steps_back);
        for i in (start..self.own_fields.len()).rev() {
            let test_pos = self.own_fields[i];
            if !self
                .get_free_fields(test_pos, cells, cell_count_x, cell_count_y)
                .is_empty()
            {
                self.pos = test_pos;
                return true;
            }
        }
        false
    }

    fn get_free_fields(
        &self,
        pos: Pos,
        cells: &[u8],
        cell_count_x: i32,
        cell_count_y: i32,
    ) -> Vec<Pos> {
        let mut free_fields = Vec::new();

        let idx = pos.to_idx(cell_count_x);

        debug!(
            "get_free_fields: pos=({}, {}), idx={}, cell_count=({}, {}), cells.len()={}",
            pos.x,
            pos.y,
            idx,
            cell_count_x,
            cell_count_y,
            cells.len()
        );

        // Check East (right)
        let east_bounds = pos.x + 1 < cell_count_x;
        let east_idx = idx + 1;
        let east_valid = east_idx < cells.len();
        let east_free = east_valid && cells[east_idx] == 0;
        debug!(
            "  East: bounds={}, idx={}, valid={}, free={}",
            east_bounds, east_idx, east_valid, east_free
        );

        if east_bounds {
            let check_idx = idx + 1;
            if check_idx < cells.len() && cells[check_idx] == 0 {
                free_fields.push(Pos::new(pos.x + 1, pos.y));
            }
        }

        // Check West (left)
        if pos.x > 0 {
            let check_idx = idx.wrapping_sub(1);
            if check_idx < cells.len() && cells[check_idx] == 0 {
                free_fields.push(Pos::new(pos.x - 1, pos.y));
            }
        }

        // Check South (down)
        if pos.y + 1 < cell_count_y {
            let check_idx = idx + cell_count_x as usize;
            if check_idx < cells.len() && cells[check_idx] == 0 {
                free_fields.push(Pos::new(pos.x, pos.y + 1));
            }
        }

        // Check North (up)
        if pos.y > 0 {
            let check_idx = idx.wrapping_sub(cell_count_x as usize);
            if check_idx < cells.len() && cells[check_idx] == 0 {
                free_fields.push(Pos::new(pos.x, pos.y - 1));
            }
        }

        free_fields
    }

    fn find_next_move(
        &mut self,
        cells: &[u8],
        cell_count_x: i32,
        cell_count_y: i32,
        life_time_branch: u32,
        max_steps_back: usize,
        rng: &mut ThreadRng,
    ) -> Option<Pos> {
        if self.state != BranchState::Running {
            return None;
        }

        let mut free_fields = self.get_free_fields(self.pos, cells, cell_count_x, cell_count_y);

        if free_fields.is_empty() {
            debug!(
                "No free fields at pos ({}, {}), trying to backtrack. Own fields: {}",
                self.pos.x,
                self.pos.y,
                self.own_fields.len()
            );
            if self.move_to_new_pos(cells, cell_count_x, cell_count_y, max_steps_back) {
                return self.find_next_move(
                    cells,
                    cell_count_x,
                    cell_count_y,
                    life_time_branch,
                    max_steps_back,
                    rng,
                );
            }
            debug!("Branch stopped - no moves available");
            self.state = BranchState::Stopped;
            return None;
        }

        if self.life_time - self.age < life_time_branch {
            self.mode = BranchMode::City;
        } else if self.mode == BranchMode::Land {
            let expand_field = Pos::new(
                self.pos.x + self.expand_direction.x,
                self.pos.y + self.expand_direction.y,
            );

            if free_fields
                .iter()
                .any(|f| f.x == expand_field.x && f.y == expand_field.y)
            {
                // Favor expand direction
                for _ in 0..10 {
                    free_fields.push(expand_field);
                }
            } else {
                // Switch to city mode
                self.mode = BranchMode::City;
                self.age = rng.random_range(0..=self.age);
            }
        }

        let idx = rng.random_range(0..free_fields.len());
        Some(free_fields[idx])
    }

    fn set_expand_direction(
        &mut self,
        cells: &[u8],
        cell_count_x: i32,
        cell_count_y: i32,
        rng: &mut ThreadRng,
    ) {
        let free_fields = self.get_free_fields(self.pos, cells, cell_count_x, cell_count_y);
        if free_fields.is_empty() {
            return;
        }

        let idx = rng.random_range(0..free_fields.len());
        let target_pos = free_fields[idx];
        self.expand_direction = Pos::new(target_pos.x - self.pos.x, target_pos.y - self.pos.y);
    }

    fn set_main(&mut self, change_hue: f32, life_time: u32, lightness: f32) {
        self.saturation = 100.0;
        self.lightness = lightness;
        self.hue = (self.hue + change_hue) % 360.0;
        self.life_time = life_time;
        self.update_cached_colors();
    }

    fn branch_off(
        &mut self,
        size: f32,
        cells: &[u8],
        cell_count_x: i32,
        cell_count_y: i32,
        life_time_branch: u32,
        fill_city: bool,
        lightness_branch: f32,
        rng: &mut ThreadRng,
    ) -> Option<Branch> {
        if self.own_fields.len() <= 1 {
            return None;
        }

        let search_pos = self.own_fields[self.own_fields.len() - 1];
        let free_fields = self.get_free_fields(search_pos, cells, cell_count_x, cell_count_y);
        if free_fields.is_empty() {
            return None;
        }

        let idx = rng.random_range(0..free_fields.len());
        let new_pos = free_fields[idx];

        let actions = self.create_line(new_pos, Some(search_pos), size, fill_city);
        self.history.extend(actions);

        let mut new_branch = Branch::new(self.pos, life_time_branch, lightness_branch);
        new_branch.hue = self.hue;
        new_branch.life_time = life_time_branch;
        new_branch.update_cached_colors();

        Some(new_branch)
    }
}

/// CityGrow scene - procedurally growing city visualization
pub struct CityGrowScene {
    // Configuration
    config: CityGrowConfig,

    // Grid state
    cells: Vec<u8>,
    cell_count_x: i32,
    cell_count_y: i32,
    size: f32,

    // Branches
    branch_list: Vec<Branch>,
    all_branches: Vec<Branch>,

    // State
    reverse_running: bool,
    fading_out: bool,
    fade_alpha: f32,
    needs_initial_clear: bool,
    needs_renderer_reset: bool,
    width: f32,
    height: f32,

    // Performance: reusable RNG
    rng: ThreadRng,

    // Time-based reverse animation (accumulator for consistent speed at any FPS)
    reverse_time_accumulator: f32,
}

impl CityGrowScene {
    pub fn new(width: u32, height: u32) -> Self {
        Self::with_config(width, height, CityGrowConfig::default())
    }

    /// Draw erasures (black lines/rects) using COPY blend mode for pixel-perfect erasure
    fn draw_erasures(erasures: &[DrawAction], renderer: &Renderer) -> Result<()> {
        let black = D2D1_COLOR_F {
            r: 0.0,
            g: 0.0,
            b: 0.0,
            a: 1.0,
        };

        // Convert erasures to DrawOperations using the same logic as forward rendering
        let mut operations = Vec::new();
        Self::actions_to_polyline_operations(erasures, &black, &black, &mut operations);

        // Set MIN blend mode - O = Min(S, D), so black (0) always wins
        // This handles partial pixel coverage correctly (unlike COPY which blends based on coverage)
        renderer.set_min_blend();

        // Batch draw the erasures
        renderer.draw_batch(&operations)?;

        // Restore normal blend mode
        renderer.set_normal_blend();

        Ok(())
    }

    pub fn with_config(width: u32, height: u32, config: CityGrowConfig) -> Self {
        let size = config.initial_size as f32;
        let cell_count_x = (width as f32 / size / 2.0).round() as i32;
        let cell_count_y = (height as f32 / size / 2.0).round() as i32;
        let cells = vec![0u8; (cell_count_x * cell_count_y) as usize];

        let mut scene = Self {
            cells,
            cell_count_x,
            cell_count_y,
            size,
            branch_list: Vec::new(),
            all_branches: Vec::new(),
            config,
            reverse_running: false,
            fading_out: false,
            fade_alpha: 0.0,
            needs_renderer_reset: false,
            needs_initial_clear: true,

            width: width as f32,
            height: height as f32,
            rng: rand::rng(),
            reverse_time_accumulator: 0.0,
        };

        scene.initialize(scene.config.start_branches as usize);
        scene
    }

    fn initialize(&mut self, start_branches: usize) {
        self.initialize_with_clear(start_branches, true);
    }

    fn initialize_with_clear(&mut self, start_branches: usize, clear: bool) {
        self.cells.fill(0);
        self.branch_list.clear();
        self.all_branches.clear();
        self.reverse_running = false;
        self.fading_out = false;
        self.fade_alpha = 0.0;
        self.needs_initial_clear = clear;
        self.reverse_time_accumulator = 0.0;

        // Force renderer to reset to non-incremental state to clear old intermediate bitmap
        // This prevents old content from persisting across cycles
        self.needs_renderer_reset = true;

        let lightness_default = self.config.lightness_default as f32 / 255.0 * 100.0;

        for i in 0..start_branches {
            let idx = self.rng.random_range(0..self.cells.len());
            let pos = Pos::from_idx(idx, self.cell_count_x);
            let branch = Branch::new(pos, self.config.life_time as u32, lightness_default);
            // Mark initial cell as occupied
            self.cells[pos.to_idx(self.cell_count_x)] = 1;
            debug!(
                "Branch {} initialized at ({}, {}) - idx {} / {} cells",
                i,
                pos.x,
                pos.y,
                idx,
                self.cells.len()
            );
            self.branch_list.push(branch);
        }
        info!("Initialized {} branches", start_branches);
    }

    /// Helper function to flush accumulated polyline points
    fn flush_polyline(
        points: &mut Vec<Vector2>,
        color: &D2D1_COLOR_F,
        operations: &mut Vec<DrawOperation>,
    ) {
        if points.len() >= 2 {
            if points.len() == 2 {
                // Single segment: use Line for simplicity
                operations.push(DrawOperation::line(points[0], points[1], *color, 2.0));
            } else {
                // Multiple segments: create Polyline
                operations.push(DrawOperation::polyline(std::mem::take(points), *color, 2.0));
            }
        }
        points.clear();
    }

    /// Convert actions to operations, grouping consecutive connected lines into polylines
    fn actions_to_polyline_operations(
        actions: &[DrawAction],
        primary_color: &D2D1_COLOR_F,
        secondary_color: &D2D1_COLOR_F,
        operations: &mut Vec<DrawOperation>,
    ) {
        let mut current_polyline_points: Vec<Vector2> = Vec::new();
        const EPSILON: f32 = 0.001;

        for action in actions {
            match action {
                DrawAction::Line {
                    from_x,
                    from_y,
                    to_x,
                    to_y,
                } => {
                    let start = Vector2::new(*from_x, *from_y);
                    let end = Vector2::new(*to_x, *to_y);

                    if current_polyline_points.is_empty() {
                        // Start new polyline
                        current_polyline_points.push(start);
                        current_polyline_points.push(end);
                    } else {
                        // Check if this line connects to the previous one
                        let last_end = *current_polyline_points.last().unwrap();
                        let dx = last_end.X - start.X;
                        let dy = last_end.Y - start.Y;
                        let distance = (dx * dx + dy * dy).sqrt();

                        if distance < EPSILON {
                            // Connected: add only the endpoint
                            current_polyline_points.push(end);
                        } else {
                            // Disconnected: flush current polyline and start new one
                            Self::flush_polyline(
                                &mut current_polyline_points,
                                primary_color,
                                operations,
                            );
                            current_polyline_points.push(start);
                            current_polyline_points.push(end);
                        }
                    }
                }
                DrawAction::Rect {
                    x,
                    y,
                    width,
                    height,
                } => {
                    // Flush any pending polyline before rectangle
                    Self::flush_polyline(&mut current_polyline_points, primary_color, operations);
                    // Add rectangle as separate operation with secondary color
                    operations.push(DrawOperation::filled_rect(
                        D2D_RECT_F {
                            left: *x,
                            top: *y,
                            right: *x + *width,
                            bottom: *y + *height,
                        },
                        *secondary_color,
                    ));
                }
            }
        }

        // Flush remaining polyline
        Self::flush_polyline(&mut current_polyline_points, primary_color, operations);
    }
}

impl Scene for CityGrowScene {
    fn is_animating(&self) -> bool {
        // Animation running if reversing OR fading out OR any branch active
        if self.reverse_running || self.fading_out {
            return true;
        }

        self.branch_list
            .iter()
            .any(|b| b.state == BranchState::Running)
    }

    fn update(&mut self, delta_time: f32) {
        debug!(
            "Update called: {} active branches, {} total branches",
            self.branch_list.len(),
            self.all_branches.len()
        );

        if self.reverse_running {
            if !self.config.show_reverse {
                return;
            }

            // Time-based reverse animation (consistent speed at any FPS)
            // Update interval: 1.0 / 60.0 ≈ 0.0167 seconds (60 updates per second)
            // Higher frequency + fewer actions per update = smoother animation
            const REVERSE_UPDATE_INTERVAL: f32 = 1.0 / 60.0;

            self.reverse_time_accumulator += delta_time;

            // Only remove actions when enough time has accumulated
            if self.reverse_time_accumulator >= REVERSE_UPDATE_INTERVAL {
                self.reverse_time_accumulator -= REVERSE_UPDATE_INTERVAL;

                debug!(
                    "Removing actions (time-based, delta={:.3}s)",
                    delta_time
                );

                // Calculate actions to remove per branch
                let actions_per_update = self.config.reverse_actions_per_frame;
                let reverse_points_per_branch = (actions_per_update as f32
                    / self.all_branches.len().max(1) as f32)
                    .ceil() as usize;

                self.all_branches.retain_mut(|branch| {
                    let to_remove = branch.history.len().min(reverse_points_per_branch);
                    let new_len = branch.history.len() - to_remove;

                    // Save removed actions to pending_erasures for incremental erasure
                    if new_len < branch.history.len() {
                        branch
                            .pending_erasures
                            .extend(branch.history.drain(new_len..));

                        // Pop chunks if history shrunk below chunk boundary
                        while !branch.chunks.is_empty() && new_len < branch.chunk_start_idx {
                            branch.chunks.pop();
                            branch.chunk_start_idx =
                                branch.chunk_start_idx.saturating_sub(CHUNK_SIZE);
                        }
                    }

                    !branch.history.is_empty()
                });
            }

            if self.all_branches.is_empty() {
                // Reversing finished, transition to fade-out
                self.reverse_running = false;
                self.fading_out = true;
                self.fade_alpha = 0.0;
                info!("Starting fade-out to black");
            }

            return;
        }

        // Handle fade-out after reverse completes
        if self.fading_out {
            // Fade duration: 1.5 seconds
            const FADE_DURATION: f32 = 1.5;
            let fade_speed = delta_time / FADE_DURATION;

            self.fade_alpha += fade_speed;

            if self.fade_alpha >= 1.0 {
                self.fade_alpha = 1.0;
                // Fade complete, restart animation
                info!("Fade complete, restarting animation");
                self.initialize_with_clear(self.config.start_branches as usize, true);
            }

            return;
        }

        // Create branch-offs
        let mut new_branches = Vec::new();
        let branch_count = self.branch_list.len() as f32;
        let lightness_default = self.config.lightness_default as f32 / 255.0 * 100.0;
        let lightness_branch = self.config.lightness_branch as f32 / 255.0 * 100.0;

        for branch in &mut self.branch_list {
            let scaled_branch_off = self.config.prop_branch_off
                * (1.0 + self.config.branch_fall_off)
                / (self.config.branch_fall_off + branch_count);
            let scaled_branch_off_land = self.config.prop_branch_off_land
                * (1.0 + self.config.branch_fall_off)
                / (self.config.branch_fall_off + branch_count);

            let should_branch = if branch.mode == BranchMode::City {
                self.rng.random_range(0.0..1.0) <= scaled_branch_off / 100.0
            } else {
                self.rng.random_range(0.0..1.0) <= scaled_branch_off_land / 100.0
            };

            if should_branch
                && let Some(mut new_branch) = branch.branch_off(
                    self.size,
                    &self.cells,
                    self.cell_count_x,
                    self.cell_count_y,
                    self.config.life_time_branch as u32,
                    self.config.fill_city,
                    lightness_branch,
                    &mut self.rng,
                )
            {
                if self.rng.random_range(0.0..1.0) <= self.config.prop_branch_off_to_main / 100.0 {
                    new_branch.set_main(
                        self.config.change_hue_new_main,
                        self.config.life_time as u32,
                        lightness_default,
                    );
                }
                new_branches.push(new_branch);
            }
        }

        // Add new branches to active list only
        self.branch_list.extend(new_branches);

        // Draw moves for all branches
        for branch in &mut self.branch_list {
            if branch.age >= branch.life_time {
                branch.state = BranchState::Stopped;
                continue;
            }

            // Mode transitions
            if branch.mode == BranchMode::City
                && self.rng.random_range(0.0..1.0) <= self.config.prop_city_to_land / 100.0
            {
                branch.mode = BranchMode::Land;
                branch.set_expand_direction(
                    &self.cells,
                    self.cell_count_x,
                    self.cell_count_y,
                    &mut self.rng,
                );
            } else if branch.mode == BranchMode::Land
                && self.rng.random_range(0.0..1.0) <= self.config.prop_land_to_city
            {
                branch.mode = BranchMode::City;
                branch.age = self.rng.random_range(0..=branch.age);
            }

            if let Some(new_pos) = branch.find_next_move(
                &self.cells,
                self.cell_count_x,
                self.cell_count_y,
                self.config.life_time_branch as u32,
                self.config.max_steps_back as usize,
                &mut self.rng,
            ) {
                let actions = branch.create_line(new_pos, None, self.size, self.config.fill_city);
                branch.history.extend(actions);
                branch.age += 1;
                self.cells[new_pos.to_idx(self.cell_count_x)] = 1;
            }
        }

        // Move stopped branches from branch_list to all_branches (preserving their history)
        // Phase 2 & 3: Build cached operations and command lists for stopped branches
        let mut stopped_branches: Vec<Branch> = Vec::new();
        self.branch_list.retain(|b| {
            if b.state == BranchState::Running {
                true
            } else {
                stopped_branches.push(b.clone());
                false
            }
        });
        self.all_branches.extend(stopped_branches);

        if self.branch_list.is_empty() {
            self.reverse_running = true;
            // Reset time accumulator so removal starts immediately
            self.reverse_time_accumulator = 0.0;
            debug!(
                "Reverse starting: {} branches in all_branches",
                self.all_branches.len()
            );
        }
    }

    fn prepare_render(&mut self, renderer: &mut Renderer) -> Result<()> {
        // Force reset renderer state if reinitializing (to clear old intermediate bitmap)
        if self.needs_renderer_reset {
            renderer.non_incremental();
            renderer.clear_cached_scene();
            self.needs_renderer_reset = false;
        }

        // Create chunks for branches outside of active drawing session
        // Process all branches (both stopped and active)
        for branch in self
            .all_branches
            .iter_mut()
            .chain(self.branch_list.iter_mut())
        {
            while branch.history.len() >= branch.chunk_start_idx + CHUNK_SIZE {
                let mut chunk_ops = Vec::new();
                let chunk_end = branch.chunk_start_idx + CHUNK_SIZE;
                let primary_color = branch.get_color();
                let secondary_color = branch.get_secondary_color();

                Self::actions_to_polyline_operations(
                    &branch.history[branch.chunk_start_idx..chunk_end],
                    &primary_color,
                    &secondary_color,
                    &mut chunk_ops,
                );

                match renderer.create_command_list(&chunk_ops) {
                    Ok(cmd_list) => {
                        branch.chunks.push(cmd_list);
                        branch.chunk_start_idx = chunk_end;
                    }
                    Err(e) => {
                        error!("Failed to create command list chunk: {:?}", e);
                        break;
                    }
                }
            }
        }

        // Switch to appropriate rendering mode BEFORE begin_draw
        // Use incremental mode for both forward and reverse to preserve frame content
        if self.needs_initial_clear {
            renderer.incremental_no_copy()?;
        } else {
            renderer.incremental()?;
        }
        Ok(())
    }

    fn render(&mut self, renderer: &mut Renderer) -> Result<()> {
        // Clear background to black only once at start
        if self.needs_initial_clear {
            renderer.clear(D2D1_COLOR_F {
                r: 0.0,
                g: 0.0,
                b: 0.0,
                a: 1.0,
            });
            self.needs_initial_clear = false;
        }

        let mut operations = Vec::new();

        // Handle reverse animation with incremental erasure using COPY blend mode
        if self.reverse_running {
            // Draw black lines/rects over the items being removed
            let mut erasure_count = 0;
            for branch in &mut self.all_branches {
                if !branch.pending_erasures.is_empty() {
                    Self::draw_erasures(&branch.pending_erasures, renderer)?;
                    erasure_count += branch.pending_erasures.len();
                    branch.pending_erasures.clear();
                }
            }

            if erasure_count > 0 {
                debug!("Erased {} actions with COPY blend", erasure_count);
            }
            return Ok(());
        }

        // Collect NEW actions from stopped branches using polyline optimization
        for branch in &mut self.all_branches {
            let primary_color = branch.get_color();
            let secondary_color = branch.get_secondary_color();
            let start_idx = branch.rendered_count;
            let end_idx = branch.history.len();

            if start_idx < end_idx {
                // Use polyline grouping for new actions
                Self::actions_to_polyline_operations(
                    &branch.history[start_idx..end_idx],
                    &primary_color,
                    &secondary_color,
                    &mut operations,
                );
            }
            branch.rendered_count = end_idx;
        }

        // Collect NEW actions from active branches using polyline optimization
        for branch in &mut self.branch_list {
            let primary_color = branch.get_color();
            let secondary_color = branch.get_secondary_color();
            let start_idx = branch.rendered_count;
            let end_idx = branch.history.len();

            if start_idx < end_idx {
                // Use polyline grouping for new actions
                Self::actions_to_polyline_operations(
                    &branch.history[start_idx..end_idx],
                    &primary_color,
                    &secondary_color,
                    &mut operations,
                );
            }
            branch.rendered_count = end_idx;
        }

        // Batch draw all new operations
        if !operations.is_empty() {
            renderer.draw_batch(&operations)?;
            debug!(
                "Rendered {} new actions ({} active branches, {} stopped branches)",
                operations.len(),
                self.branch_list.len(),
                self.all_branches.len()
            );
        }

        // Draw fade overlay if fading out
        if self.fading_out && self.fade_alpha > 0.0 {
            let fade_rect = D2D_RECT_F {
                left: 0.0,
                top: 0.0,
                right: self.width,
                bottom: self.height,
            };
            let fade_color = D2D1_COLOR_F {
                r: 0.0,
                g: 0.0,
                b: 0.0,
                a: self.fade_alpha,
            };
            renderer.draw_filled_rect(&fade_rect, &fade_color)?;
        }

        Ok(())
    }

    fn on_resize(&mut self, width: u32, height: u32) {
        self.width = width as f32;
        self.height = height as f32;
        self.cell_count_x = (width as f32 / self.size / 2.0).round() as i32;
        self.cell_count_y = (height as f32 / self.size / 2.0).round() as i32;
        self.cells = vec![0u8; (self.cell_count_x * self.cell_count_y) as usize];
        let start_branches = if self.config.start_branches > 0 {
            self.config.start_branches as usize
        } else {
            3
        };
        self.initialize(start_branches);
    }
}

// Helper function to convert HSL to RGB
fn hsl_to_rgb(h: f32, s: f32, l: f32) -> D2D1_COLOR_F {
    let s = s / 100.0;
    let l = l / 100.0;

    let c = (1.0 - (2.0 * l - 1.0).abs()) * s;
    let h = h / 60.0;
    let x = c * (1.0 - ((h % 2.0) - 1.0).abs());
    let m = l - c / 2.0;

    let (r, g, b) = if h < 1.0 {
        (c, x, 0.0)
    } else if h < 2.0 {
        (x, c, 0.0)
    } else if h < 3.0 {
        (0.0, c, x)
    } else if h < 4.0 {
        (0.0, x, c)
    } else if h < 5.0 {
        (x, 0.0, c)
    } else {
        (c, 0.0, x)
    };

    D2D1_COLOR_F {
        r: r + m,
        g: g + m,
        b: b + m,
        a: 1.0,
    }
}
