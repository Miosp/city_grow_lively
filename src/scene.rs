use crate::renderer::Renderer;
use anyhow::Result;
use rand::RngExt as _;
use tracing::{debug, info};
use windows::Win32::Graphics::Direct2D::Common::D2D1_COLOR_F;
use windows_numerics::Vector2;

/// Trait for scene rendering logic (the "frontend")
pub trait Scene {
    /// Update scene state (called every frame)
    fn update(&mut self, delta_time: f32);

    /// Render the scene using the provided renderer
    fn render(&self, renderer: &Renderer) -> Result<()>;

    /// Handle resize events
    fn on_resize(&mut self, width: u32, height: u32);
}

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
}

impl Branch {
    fn new(pos: Pos, life_time: u32) -> Self {
        let mut rng = rand::rng();
        Self {
            pos,
            state: BranchState::Running,
            mode: BranchMode::City,
            expand_direction: Pos::new(0, 0),
            own_fields: vec![pos],
            age: 0,
            life_time,
            hue: rng.random_range(0.0..360.0),
            saturation: 100.0,
            lightness: 55.0, // lightness_default = 140/255 * 100 ≈ 55%
            history: Vec::new(),
        }
    }

    fn get_color(&self) -> D2D1_COLOR_F {
        hsl_to_rgb(self.hue, self.saturation, self.lightness)
    }

    fn get_secondary_color(&self) -> D2D1_COLOR_F {
        let mut color = hsl_to_rgb(self.hue, self.saturation, self.lightness);
        color.a = 0.25;
        color
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
                x: 2.0 * size * left_top.x as f32 + margin + offset,
                y: 2.0 * size * left_top.y as f32 + margin + offset,
                width: 2.0 * size - 2.0 * margin,
                height: 2.0 * size - 2.0 * margin,
            });

            // Second rectangle (mirrored)
            let perp_x = -perp_x;
            let perp_y = -perp_y;
            let imaginary = Pos::new(last_position.x + perp_x, last_position.y + perp_y);
            let left_top = Pos::new(to_pos.x.min(imaginary.x), to_pos.y.min(imaginary.y));

            actions.push(DrawAction::Rect {
                x: 2.0 * size * left_top.x as f32 + margin + offset,
                y: 2.0 * size * left_top.y as f32 + margin + offset,
                width: 2.0 * size - 2.0 * margin,
                height: 2.0 * size - 2.0 * margin,
            });
        }

        // Draw line
        actions.push(DrawAction::Line {
            from_x: 2.0 * size * from_pos.x as f32 + offset,
            from_y: 2.0 * size * from_pos.y as f32 + offset,
            to_x: 2.0 * size * to_pos.x as f32 + offset,
            to_y: 2.0 * size * to_pos.y as f32 + offset,
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
                let mut rng = rand::rng();
                self.age = rng.random_range(0..=self.age);
            }
        }

        let mut rng = rand::rng();
        let idx = rng.random_range(0..free_fields.len());
        Some(free_fields[idx])
    }

    fn set_expand_direction(&mut self, cells: &[u8], cell_count_x: i32, cell_count_y: i32) {
        let free_fields = self.get_free_fields(self.pos, cells, cell_count_x, cell_count_y);
        if free_fields.is_empty() {
            return;
        }

        let mut rng = rand::rng();
        let idx = rng.random_range(0..free_fields.len());
        let target_pos = free_fields[idx];
        self.expand_direction = Pos::new(target_pos.x - self.pos.x, target_pos.y - self.pos.y);
    }

    fn set_main(&mut self, change_hue: f32, life_time: u32) {
        self.saturation = 100.0;
        self.lightness = 55.0;
        self.hue = (self.hue + change_hue) % 360.0;
        self.life_time = life_time;
    }

    fn branch_off(
        &mut self,
        size: f32,
        cells: &[u8],
        cell_count_x: i32,
        cell_count_y: i32,
        life_time_branch: u32,
        fill_city: bool,
    ) -> Option<Branch> {
        if self.own_fields.len() <= 1 {
            return None;
        }

        let search_pos = self.own_fields[self.own_fields.len() - 1];
        let free_fields = self.get_free_fields(search_pos, cells, cell_count_x, cell_count_y);
        if free_fields.is_empty() {
            return None;
        }

        let mut rng = rand::rng();
        let idx = rng.random_range(0..free_fields.len());
        let new_pos = free_fields[idx];

        let actions = self.create_line(new_pos, Some(search_pos), size, fill_city);
        self.history.extend(actions);

        let mut new_branch = Branch::new(self.pos, life_time_branch);
        new_branch.hue = self.hue;
        new_branch.lightness = 23.0; // lightness_branch = 60/255 * 100 ≈ 23%
        new_branch.life_time = life_time_branch;

        Some(new_branch)
    }
}

/// CityGrow scene - procedurally growing city visualization
pub struct CityGrowScene {
    // Grid state
    cells: Vec<u8>,
    cell_count_x: i32,
    cell_count_y: i32,
    size: f32,

    // Branches
    branch_list: Vec<Branch>,
    all_branches: Vec<Branch>,

    // Configuration
    life_time: u32,
    life_time_branch: u32,
    prop_city2land: f32,
    prop_land2city: f32,
    prop_branch_off: f32,
    prop_branch_off_land: f32,
    prop_branch_off_to_main: f32,
    branch_fall_off: f32,
    change_hue_new_main: f32,
    max_steps_back: usize,
    fill_city: bool,
    show_reverse: bool,

    // State
    reverse_running: bool,
    width: f32,
    height: f32,
}

impl CityGrowScene {
    pub fn new(width: u32, height: u32) -> Self {
        let size = 3.0;
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
            life_time: 8000,
            life_time_branch: 15,
            prop_city2land: 12.0,
            prop_land2city: 0.003,
            prop_branch_off: 15.0,
            prop_branch_off_land: 6.0,
            prop_branch_off_to_main: 1.0,
            branch_fall_off: 50.0,
            change_hue_new_main: 9.0,
            max_steps_back: 300,
            fill_city: true,
            show_reverse: true,
            reverse_running: false,
            width: width as f32,
            height: height as f32,
        };

        scene.initialize(3); // Start with 3 branches
        scene
    }

    fn initialize(&mut self, start_branches: usize) {
        self.cells.fill(0);
        self.branch_list.clear();
        self.all_branches.clear();
        self.reverse_running = false;

        let mut rng = rand::rng();
        for i in 0..start_branches {
            let idx = rng.random_range(0..self.cells.len());
            let pos = Pos::from_idx(idx, self.cell_count_x);
            let branch = Branch::new(pos, self.life_time);
            // Mark initial cell as occupied
            self.cells[pos.to_idx(self.cell_count_x)] = 1;
            info!(
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

    fn draw_action(
        &self,
        renderer: &Renderer,
        action: &DrawAction,
        color: &D2D1_COLOR_F,
    ) -> Result<()> {
        let brush = renderer.create_solid_brush(*color)?;
        let ctx = renderer.context();

        unsafe {
            match action {
                DrawAction::Line {
                    from_x,
                    from_y,
                    to_x,
                    to_y,
                } => {
                    // Windows Foundation Numerics uses a specific Matrix/Vector type
                    // For Direct2D, we can just pass the raw coordinates
                    ctx.DrawLine(
                        Vector2::new(*from_x, *from_y),
                        Vector2::new(*to_x, *to_y),
                        &brush,
                        2.0,
                        None,
                    );
                }
                DrawAction::Rect {
                    x,
                    y,
                    width,
                    height,
                } => {
                    use windows::Win32::Graphics::Direct2D::Common::D2D_RECT_F;
                    brush.SetColor(color);
                    ctx.FillRectangle(
                        &D2D_RECT_F {
                            left: *x,
                            top: *y,
                            right: x + width,
                            bottom: y + height,
                        },
                        &brush,
                    );
                }
            }
        }

        Ok(())
    }

    fn erase_action(&self, renderer: &Renderer, action: &DrawAction) -> Result<()> {
        let black = D2D1_COLOR_F {
            r: 0.0,
            g: 0.0,
            b: 0.0,
            a: 1.0,
        };
        self.draw_action(renderer, action, &black)
    }
}

impl Scene for CityGrowScene {
    fn update(&mut self, _delta_time: f32) {
        debug!(
            "Update called: {} active branches, {} total branches",
            self.branch_list.len(),
            self.all_branches.len()
        );

        if self.reverse_running {
            if !self.show_reverse {
                return;
            }

            let reverse_points_per_branch =
                (50.0 / self.all_branches.len().max(1) as f32).ceil() as usize;

            self.all_branches.retain_mut(|branch| {
                let to_remove = branch.history.len().min(reverse_points_per_branch);
                branch.history.truncate(branch.history.len() - to_remove);
                !branch.history.is_empty()
            });

            if self.all_branches.is_empty() {
                self.initialize(3); // Restart
            }

            return;
        }

        // Create branch-offs
        let mut new_branches = Vec::new();
        let mut rng = rand::rng();
        let branch_count = self.branch_list.len() as f32;

        for branch in &mut self.branch_list {
            let scaled_branch_off = self.prop_branch_off * (1.0 + self.branch_fall_off)
                / (self.branch_fall_off + branch_count);
            let scaled_branch_off_land = self.prop_branch_off_land * (1.0 + self.branch_fall_off)
                / (self.branch_fall_off + branch_count);

            let should_branch = if branch.mode == BranchMode::City {
                rng.random_range(0.0..1.0) <= scaled_branch_off / 100.0
            } else {
                rng.random_range(0.0..1.0) <= scaled_branch_off_land / 100.0
            };

            if should_branch
                && let Some(mut new_branch) = branch.branch_off(
                    self.size,
                    &self.cells,
                    self.cell_count_x,
                    self.cell_count_y,
                    self.life_time_branch,
                    self.fill_city,
                )
            {
                if rng.random_range(0.0..1.0) <= self.prop_branch_off_to_main / 100.0 {
                    new_branch.set_main(self.change_hue_new_main, self.life_time);
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
                && rng.random_range(0.0..1.0) <= self.prop_city2land / 100.0
            {
                branch.mode = BranchMode::Land;
                branch.set_expand_direction(&self.cells, self.cell_count_x, self.cell_count_y);
            } else if branch.mode == BranchMode::Land
                && rng.random_range(0.0..1.0) <= self.prop_land2city
            {
                branch.mode = BranchMode::City;
                branch.age = rng.random_range(0..=branch.age);
            }

            if let Some(new_pos) = branch.find_next_move(
                &self.cells,
                self.cell_count_x,
                self.cell_count_y,
                self.life_time_branch,
                self.max_steps_back,
            ) {
                let actions = branch.create_line(new_pos, None, self.size, self.fill_city);
                branch.history.extend(actions);
                branch.age += 1;
                self.cells[new_pos.to_idx(self.cell_count_x)] = 1;
            }
        }

        // Move stopped branches from branch_list to all_branches  (preserving their history)
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
        }
    }

    fn render(&self, renderer: &Renderer) -> Result<()> {
        // Clear background to black
        renderer.clear(D2D1_COLOR_F {
            r: 0.0,
            g: 0.0,
            b: 0.0,
            a: 1.0,
        });

        let active_actions: usize = self.branch_list.iter().map(|b| b.history.len()).sum();
        let stopped_actions: usize = self.all_branches.iter().map(|b| b.history.len()).sum();
        debug!(
            "Rendering {} active branches ({} actions) + {} stopped branches ({} actions)",
            self.branch_list.len(),
            active_actions,
            self.all_branches.len(),
            stopped_actions
        );

        // Draw stopped branches first (from all_branches)
        for branch in &self.all_branches {
            let primary_color = branch.get_color();
            let secondary_color = branch.get_secondary_color();

            for action in &branch.history {
                match action {
                    DrawAction::Rect { .. } => {
                        self.draw_action(renderer, action, &secondary_color)?;
                    }
                    DrawAction::Line { .. } => {
                        self.draw_action(renderer, action, &primary_color)?;
                    }
                }
            }
        }

        // Draw active branches (from branch_list)
        for branch in &self.branch_list {
            let primary_color = branch.get_color();
            let secondary_color = branch.get_secondary_color();

            for action in &branch.history {
                match action {
                    DrawAction::Rect { .. } => {
                        self.draw_action(renderer, action, &secondary_color)?;
                    }
                    DrawAction::Line { .. } => {
                        self.draw_action(renderer, action, &primary_color)?;
                    }
                }
            }
        }

        Ok(())
    }

    fn on_resize(&mut self, width: u32, height: u32) {
        self.width = width as f32;
        self.height = height as f32;
        self.cell_count_x = (width as f32 / self.size / 2.0).round() as i32;
        self.cell_count_y = (height as f32 / self.size / 2.0).round() as i32;
        self.cells = vec![0u8; (self.cell_count_x * self.cell_count_y) as usize];
        self.initialize(3);
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
