use crate::{
    renderer::{Renderer, draw_operation::DrawOperation},
    scene::Scene,
};
use anyhow::Result;
use bitvec::vec::BitVec;
use rand::rngs::ThreadRng;
use rand::{RngExt, seq::IndexedRandom};
use std::collections::{HashMap, HashSet};
use tracing::debug;
use windows::Win32::Graphics::Direct2D::Common::{D2D_RECT_F, D2D1_COLOR_F};
use windows_numerics::Vector2;

// Rendering constants
const LINE_WIDTH: f32 = 2.0;
const LINE_OFFSET: f32 = LINE_WIDTH / 2.0;
const MARGIN: f32 = LINE_WIDTH / 2.0;

// Utility structs

const POSITIONS: [Pos; 4] = [
    Pos { x: 1, y: 0 },  // East
    Pos { x: -1, y: 0 }, // West
    Pos { x: 0, y: 1 },  // South
    Pos { x: 0, y: -1 }, // North
];

enum Event {
    BranchOff {
        parent_id: u32,
        child_id: u32,
        parent_pos: Pos,
        child_pos: Pos,
        parent_mode: BranchMode,
        child_color: Hsla,
    },
    Move {
        branch_id: u32,
        from: Pos,
        to: Pos,
        mode: BranchMode,
        color: Hsla,
        own_fields_tip: Pos,
    },
}

/// Painter state tracks drawing history for each branch
struct PainterState {
    draw_history: HashMap<u32, Vec<DrawOperation>>,
    main_branches: HashSet<u32>, // Track which branches are main branches
}

enum BranchOffResult {
    Success {
        new_parent: Branch,
        child: Branch,
        pos: Pos,
        event: Event,
    },
    Failure {
        branch: Branch,
    },
}

struct Grid {
    data: BitVec,
    size_x: u32,
    size_y: u32,
}

impl Grid {
    fn new(size_x: u32, size_y: u32) -> Self {
        let mut data = BitVec::repeat(false, (size_x * size_y) as usize);
        data.shrink_to_fit();

        Self {
            data,
            size_x,
            size_y,
        }
    }

    fn get(&self, x: u32, y: u32) -> Option<bool> {
        if x < self.size_x && y < self.size_y {
            Some(self.data[(y * self.size_x + x) as usize])
        } else {
            None
        }
    }

    fn set(&mut self, x: u32, y: u32, value: bool) {
        if x < self.size_x && y < self.size_y {
            self.data.set((y * self.size_x + x) as usize, value);
        }
    }

    fn fill(&mut self, value: bool) {
        self.data.fill(value);
    }

    fn random_pos(&mut self, rng: &mut ThreadRng) -> Pos {
        let x = rng.random_range(0..self.size_x);
        let y = rng.random_range(0..self.size_y);
        Pos::new(x as i32, y as i32)
    }

    fn get_free_neighbors(&self, pos: Pos) -> Vec<Pos> {
        POSITIONS
            .iter()
            .filter_map(|&dir| {
                pos.try_add(dir)
                    .take_if(|new_pos| self.get(new_pos.x as u32, new_pos.y as u32) == Some(false))
            })
            .collect()
    }
}

#[derive(Debug, Clone, Copy)]
struct Hsla {
    h: u8,
    s: u8,
    l: u8,
    a: u8,
}

impl Hsla {
    fn new(h: u8, s: u8, l: u8, a: u8) -> Self {
        Self { h, s, l, a }
    }

    const fn to_d2d_color(self) -> D2D1_COLOR_F {
        let h = (self.h as f32 / 255.0) * 360.0;
        let s = self.s as f32 / 255.0;
        let l = self.l as f32 / 255.0;

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
            a: self.a as f32 / 255.0,
        }
    }
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

    fn try_add(&self, other: Pos) -> Option<Pos> {
        self.x.checked_add(other.x).and_then(|new_x| {
            self.y
                .checked_add(other.y)
                .map(|new_y| Pos::new(new_x, new_y))
        })
    }

    fn try_sub(&self, other: Pos) -> Option<Pos> {
        self.x.checked_sub(other.x).and_then(|new_x| {
            self.y
                .checked_sub(other.y)
                .map(|new_y| Pos::new(new_x, new_y))
        })
    }
}

pub struct CityGrowConfig {
    pub life_time: u16,
    pub life_time_branch: u16,
    pub prop_city_to_land: f32,
    pub prop_land_to_city: f32,
    pub prop_branch_off_city: f32,
    pub prop_branch_off_land: f32,
    pub prop_branch_off_to_main: f32,
    pub branch_fall_off: f32,
    pub change_hue_new_main: u8,
    pub start_branches: u8,
    pub max_steps_back: u16,
    pub lightness_default: u8,
    pub lightness_branch: u8,
    pub scale: f32,
    pub reverse_actions_per_frame: usize,
    pub land_directional_bias: f32,
}

impl Default for CityGrowConfig {
    fn default() -> Self {
        Self {
            life_time: 8000,
            life_time_branch: 15,
            prop_city_to_land: 12.0,
            prop_land_to_city: 0.003,
            prop_branch_off_city: 15.0,
            prop_branch_off_land: 6.0,
            prop_branch_off_to_main: 1.0,
            branch_fall_off: 50.0,
            change_hue_new_main: 9,
            start_branches: 3,
            max_steps_back: 50,
            lightness_default: 140,
            lightness_branch: 60,
            scale: 2.0,
            reverse_actions_per_frame: 50,
            land_directional_bias: 2.5,
        }
    }
}

impl CityGrowConfig {
    pub fn branch_chance(&self, mode: BranchMode) -> f32 {
        match mode {
            BranchMode::City => self.prop_branch_off_city,
            BranchMode::Land => self.prop_branch_off_land,
        }
    }
}

/// Branch mode
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BranchMode {
    City, // Random walk
    Land, // Directional expansion
}

/// A growing branch
#[derive(Clone)]
struct Branch {
    id: u32,
    pos: Pos, // Current position
    mode: BranchMode,
    expand_direction: Pos,
    own_fields: Vec<Pos>,
    age: u16,
    life_time: u16,
    color: Hsla,
}

impl Branch {
    fn new(pos: Pos, config: &CityGrowConfig, rng: &mut ThreadRng) -> Self {
        let hue: u8 = rng.random_range(0..=255);

        // Pre-calculate colors
        let color = Hsla::new(hue, 255, config.lightness_default, 255);

        Self {
            id: rng.random(),
            pos,
            mode: BranchMode::City,
            expand_direction: Pos::new(0, 0),
            own_fields: vec![pos],
            age: 0,
            life_time: config.life_time,
            color,
        }
    }

    pub fn step_branch(
        self,
        grid: &Grid,
        config: &CityGrowConfig,
        rng: &mut ThreadRng,
    ) -> Option<(Branch, Pos, Pos, Pos)> {
        if self.age >= self.life_time {
            return None;
        }

        let (new_branch, next_move) = if (self.life_time - self.age) < config.life_time_branch {
            Self {
                mode: BranchMode::City,
                ..self
            }
        } else {
            self.transition_modes(grid, config, rng)
        }
        .set_next_position(grid, config)?
        .find_next_move(grid, config, rng);

        let pos = new_branch.pos;
        let own_fields_tip = new_branch
            .own_fields
            .last()
            .copied()
            .unwrap_or(new_branch.pos);
        let new_branch = Self {
            pos: next_move,
            own_fields: {
                let mut fields = new_branch.own_fields;
                fields.push(next_move);
                fields
            },
            age: new_branch.age + 1,
            ..new_branch
        };
        Some((new_branch, pos, next_move, own_fields_tip))
    }

    fn transition_modes(self, grid: &Grid, config: &CityGrowConfig, rng: &mut ThreadRng) -> Self {
        if self.mode == BranchMode::City && rng.random::<f32>() < config.prop_city_to_land / 100.0 {
            return Self {
                expand_direction: self
                    .expand_direction(grid, rng)
                    .unwrap_or(self.expand_direction),
                mode: BranchMode::Land,
                ..self
            };
        } else if self.mode == BranchMode::Land
            && rng.random::<f32>() < config.prop_land_to_city / 100.0
        {
            return Self {
                mode: BranchMode::City,
                age: rng.random_range(0..=self.age),
                ..self
            };
        }
        self
    }

    fn expand_direction(&self, grid: &Grid, rng: &mut ThreadRng) -> Option<Pos> {
        let available_neighbors = grid.get_free_neighbors(self.pos);
        if available_neighbors.is_empty() {
            return None;
        }
        let target_position = available_neighbors[rng.random_range(0..available_neighbors.len())];
        target_position.try_sub(self.pos)
    }

    fn set_next_position(self, grid: &Grid, config: &CityGrowConfig) -> Option<Self> {
        if grid.get_free_neighbors(self.pos).is_empty() {
            let positions_to_search = (config.max_steps_back as usize).min(self.own_fields.len());
            let new_position = self
                .own_fields
                .iter()
                .rev()
                .take(positions_to_search)
                .find(|pos| !grid.get_free_neighbors(**pos).is_empty())
                .copied();
            if let Some(new_pos) = new_position {
                return Some(Branch {
                    pos: new_pos,
                    ..self
                });
            }
            return None;
        }
        Some(self)
    }

    fn find_next_move(
        self,
        grid: &Grid,
        config: &CityGrowConfig,
        rng: &mut ThreadRng,
    ) -> (Self, Pos) {
        let neighbors = grid.get_free_neighbors(self.pos);
        if self.mode == BranchMode::Land {
            let preferred = self.pos.try_add(self.expand_direction);

            if let Some(preferred) = preferred.filter(|p| neighbors.contains(p)) {
                if rng.random_ratio(
                    neighbors.len() as u32,
                    (neighbors.len() as f32 * config.land_directional_bias).round() as u32,
                ) {
                    return (self, preferred);
                }
                return (self, *neighbors.choose(rng).unwrap());
            }
            let new_target = *neighbors.choose(rng).unwrap();
            let new_direction = new_target
                .try_sub(self.pos)
                .unwrap_or(self.expand_direction);
            (
                Self {
                    expand_direction: new_direction,
                    ..self
                },
                new_target,
            )
        } else {
            (self, *neighbors.choose(rng).unwrap())
        }
    }

    fn try_branch_off(
        self,
        grid: &Grid,
        config: &CityGrowConfig,
        rng: &mut ThreadRng,
    ) -> BranchOffResult {
        if self.own_fields.len() <= 1 {
            return BranchOffResult::Failure { branch: self };
        }
        let search_pos = *self.own_fields.last().unwrap();
        let selected_neighbor =
            if let Some(neighbor) = grid.get_free_neighbors(search_pos).choose(rng) {
                *neighbor
            } else {
                return BranchOffResult::Failure { branch: self };
            };

        // Parent stays unchanged - child owns the new position
        let new_parent = self;

        let child = Self {
            id: rng.random(),
            pos: selected_neighbor,
            mode: BranchMode::City,
            expand_direction: Pos::new(0, 0),
            own_fields: vec![selected_neighbor],
            age: 0,
            life_time: config.life_time_branch,
            color: Hsla::new(new_parent.color.h, 255, config.lightness_branch, 255),
        };

        let branch_event = Event::BranchOff {
            parent_id: new_parent.id,
            child_id: child.id,
            parent_pos: search_pos,
            child_pos: selected_neighbor,
            parent_mode: new_parent.mode,
            child_color: child.color,
        };

        BranchOffResult::Success {
            new_parent,
            child,
            pos: selected_neighbor,
            event: branch_event,
        }
    }
}

/// CityGrow scene - procedurally growing city visualization
pub struct CityGrowScene {
    config: CityGrowConfig,
    grid: Grid,
    branch_list: Vec<Branch>,
    reverse_running: bool,
    painter_state: PainterState,

    needs_initial_clear: bool,
    screen_width: f32,
    screen_height: f32,

    rng: ThreadRng,
}

impl CityGrowScene {
    pub fn new(width: u32, height: u32) -> Self {
        Self::with_config(width, height, CityGrowConfig::default())
    }

    pub fn with_config(width: u32, height: u32, config: CityGrowConfig) -> Self {
        let cell_count_x = (width as f32 / config.scale / 2.0).round() as u32;
        let cell_count_y = (height as f32 / config.scale / 2.0).round() as u32;

        let mut scene = Self {
            grid: Grid::new(cell_count_x, cell_count_y),
            branch_list: Vec::new(),
            config,
            reverse_running: false,
            painter_state: PainterState {
                draw_history: HashMap::new(),
                main_branches: HashSet::new(),
            },
            needs_initial_clear: true,

            screen_width: width as f32,
            screen_height: height as f32,
            rng: rand::rng(),
        };

        scene.initialize(scene.config.start_branches as usize);
        scene
    }

    fn initialize(&mut self, start_branches: usize) {
        self.initialize_with_clear(start_branches, true);
    }

    fn initialize_with_clear(&mut self, start_branches: usize, clear: bool) {
        self.grid.fill(false);
        self.branch_list.clear();
        self.reverse_running = false;
        self.painter_state.draw_history.clear();
        self.painter_state.main_branches.clear();
        self.needs_initial_clear = clear;

        self.branch_list = (0..start_branches)
            .map(|_| {
                let pos = self.grid.random_pos(&mut self.rng);
                let branch = Branch::new(pos, &self.config, &mut self.rng);
                self.grid.set(pos.x as u32, pos.y as u32, true);
                // All initial branches are main branches
                self.painter_state.main_branches.insert(branch.id);
                debug!("Branch initialized at ({}, {})", pos.x, pos.y);
                branch
            })
            .collect();
        debug!("Initialized {} branches", start_branches);
    }

    fn process_branching(&mut self) -> Vec<Event> {
        let mut events = Vec::new();
        let branch_count = self.branch_list.len();

        self.branch_list = self
            .branch_list
            .drain(..)
            .flat_map(|branch| {
                let scaled_chance = (self.config.branch_chance(branch.mode)
                    * (1.0 + self.config.branch_fall_off)
                    / (self.config.branch_fall_off + branch_count as f32))
                    / 100.0;

                if self.rng.random::<f32>() < scaled_chance {
                    match branch.try_branch_off(&self.grid, &self.config, &mut self.rng) {
                        BranchOffResult::Success {
                            new_parent,
                            child,
                            pos,
                            event,
                        } => {
                            self.grid.set(pos.x as u32, pos.y as u32, true);
                            events.push(event);

                            let child = if self.rng.random::<f32>()
                                < self.config.prop_branch_off_to_main / 100.0
                            {
                                let promoted_child = Branch {
                                    color: Hsla::new(
                                        ((child.color.h + self.config.change_hue_new_main) as u16
                                            % 256) as u8,
                                        255,
                                        self.config.lightness_default,
                                        255,
                                    ),
                                    life_time: self.config.life_time,
                                    ..child
                                };
                                // Mark promoted child as main branch
                                self.painter_state.main_branches.insert(promoted_child.id);
                                promoted_child
                            } else {
                                child
                            };

                            vec![new_parent, child]
                        }
                        BranchOffResult::Failure { branch } => vec![branch],
                    }
                } else {
                    vec![branch]
                }
            })
            .collect();
        events
    }

    fn process_stepping(&mut self) -> Vec<Event> {
        let mut events = Vec::new();

        self.branch_list = self
            .branch_list
            .drain(..)
            .filter_map(|branch| {
                match branch.step_branch(&self.grid, &self.config, &mut self.rng) {
                    Some((new_branch, pos, next_pos, own_fields_tip)) => {
                        self.grid.set(next_pos.x as u32, next_pos.y as u32, true);
                        events.push(Event::Move {
                            branch_id: new_branch.id,
                            from: pos,
                            to: next_pos,
                            mode: new_branch.mode,
                            color: new_branch.color,
                            own_fields_tip,
                        });
                        Some(new_branch)
                    }
                    None => None,
                }
            })
            .collect();

        events
    }

    /// Helper: Convert grid position to screen coordinates
    fn grid_to_screen(&self, pos: Pos) -> Vector2 {
        Vector2 {
            X: pos.x as f32 * 2.0 * self.config.scale + LINE_OFFSET,
            Y: pos.y as f32 * 2.0 * self.config.scale + LINE_OFFSET,
        }
    }

    /// Helper: Compute fill rectangle for city mode fills
    fn compute_fill_rect(
        &self,
        own_fields_tip: Pos,
        to_pos: Pos,
        perpendicular: Pos,
    ) -> D2D_RECT_F {
        let imaginary_point = Pos::new(
            own_fields_tip.x + perpendicular.x,
            own_fields_tip.y + perpendicular.y,
        );
        let corner = Pos::new(
            to_pos.x.min(imaginary_point.x),
            to_pos.y.min(imaginary_point.y),
        );
        D2D_RECT_F {
            left: corner.x as f32 * 2.0 * self.config.scale + MARGIN + LINE_OFFSET,
            top: corner.y as f32 * 2.0 * self.config.scale + MARGIN + LINE_OFFSET,
            right: corner.x as f32 * 2.0 * self.config.scale
                + MARGIN
                + LINE_OFFSET
                + (2.0 * self.config.scale - 2.0 * MARGIN),
            bottom: corner.y as f32 * 2.0 * self.config.scale
                + MARGIN
                + LINE_OFFSET
                + (2.0 * self.config.scale - 2.0 * MARGIN),
        }
    }

    /// Draw a move event (line + optional fill rectangles for city mode)
    fn draw_move(&mut self, renderer: &Renderer, event: &Event) -> Result<()> {
        let (branch_id, from_pos, to_pos, mode, color, own_fields_tip) = match event {
            Event::Move {
                branch_id,
                from: from_pos,
                to: to_pos,
                mode,
                color,
                own_fields_tip,
            } => (
                *branch_id,
                *from_pos,
                *to_pos,
                *mode,
                *color,
                *own_fields_tip,
            ),
            Event::BranchOff {
                child_id,
                parent_pos,
                child_pos,
                parent_mode,
                child_color,
                ..
            } => {
                // Birth line uses parent's mode and fromPos = toPos (no backtracking yet)
                (
                    *child_id,
                    *parent_pos,
                    *child_pos,
                    *parent_mode,
                    *child_color,
                    *parent_pos,
                )
            }
        };

        let screen_from = self.grid_to_screen(from_pos);
        let screen_to = self.grid_to_screen(to_pos);
        let d2d_color = color.to_d2d_color();

        // Compute rectangles before borrowing history
        let mut rects_to_draw = Vec::new();
        if mode == BranchMode::City {
            // Calculate direction of the line being drawn
            let direction = Pos::new(to_pos.x - from_pos.x, to_pos.y - from_pos.y);

            // Perpendicular is 90-degree rotation: (-dy, dx)
            let perpendicular = Pos::new(-direction.y, direction.x);

            // Only draw rectangles if there's actual movement
            if perpendicular.x != 0 || perpendicular.y != 0 {
                let rect1 = self.compute_fill_rect(own_fields_tip, to_pos, perpendicular);
                let rect2 = self.compute_fill_rect(
                    own_fields_tip,
                    to_pos,
                    Pos::new(-perpendicular.x, -perpendicular.y),
                );
                rects_to_draw.push(rect1);
                rects_to_draw.push(rect2);
            }
        }

        // Draw fill rectangles
        let fade_color = D2D1_COLOR_F {
            r: d2d_color.r,
            g: d2d_color.g,
            b: d2d_color.b,
            a: 0.25,
        };
        for rect in &rects_to_draw {
            renderer.draw_filled_rect(rect, &fade_color)?;
        }

        // Draw the line
        renderer.draw_line(screen_from, screen_to, &d2d_color, LINE_WIDTH)?;

        // Store in history for reverse animation
        let branch_history = self
            .painter_state
            .draw_history
            .entry(branch_id)
            .or_default();

        for rect in rects_to_draw {
            branch_history.push(DrawOperation::filled_rect(rect, fade_color));
        }
        branch_history.push(DrawOperation::line(
            screen_from,
            screen_to,
            d2d_color,
            LINE_WIDTH,
        ));

        Ok(())
    }

    /// Erase a draw operation (for reverse animation)
    fn erase_entry(&self, renderer: &Renderer, entry: &DrawOperation) -> Result<()> {
        let black = D2D1_COLOR_F {
            r: 0.0,
            g: 0.0,
            b: 0.0,
            a: 1.0,
        };

        // Set MIN blend mode for pixel-perfect erasure
        renderer.set_min_blend();

        match entry {
            DrawOperation::FilledRect { rect, .. } => {
                renderer.draw_filled_rect(rect, &black)?;
            }
            DrawOperation::Line {
                start,
                end,
                thickness,
                ..
            } => {
                renderer.draw_line(*start, *end, &black, *thickness)?;
            }
            DrawOperation::Rect {
                rect, thickness, ..
            } => {
                renderer.draw_rect(rect, &black, *thickness)?;
            }
            DrawOperation::Polyline {
                points, thickness, ..
            } => {
                renderer.draw_polyline(points, &black, *thickness)?;
            }
        }

        // Restore normal blend mode
        renderer.set_normal_blend();

        Ok(())
    }

    /// Process reverse animation step
    /// Non-main branches erase first, then main branches
    fn reverse_step(&mut self, renderer: &Renderer) -> Result<bool> {
        let history_size = self.painter_state.draw_history.len();
        if history_size == 0 {
            return Ok(true); // Done reversing
        }

        // Separate branches into main and non-main
        let mut main_branch_ids: Vec<u32> = Vec::new();
        let mut non_main_branch_ids: Vec<u32> = Vec::new();

        for branch_id in self.painter_state.draw_history.keys() {
            if self.painter_state.main_branches.contains(branch_id) {
                main_branch_ids.push(*branch_id);
            } else {
                non_main_branch_ids.push(*branch_id);
            }
        }

        // Decide which branches to process (non-main first, then main)
        let branches_to_process = if !non_main_branch_ids.is_empty() {
            &non_main_branch_ids
        } else {
            &main_branch_ids
        };

        // Calculate how many entries to erase per branch
        let entries_per_branch = (self.config.reverse_actions_per_frame.max(1) as f32
            / branches_to_process.len().max(1) as f32)
            .ceil() as usize;

        // Collect entries to erase
        let mut all_entries_to_erase = Vec::new();
        let mut branches_to_remove = Vec::new();

        // Process selected branches
        for branch_id in branches_to_process {
            if let Some(history) = self.painter_state.draw_history.get_mut(branch_id) {
                if history.is_empty() {
                    branches_to_remove.push(*branch_id);
                    continue;
                }

                let count = entries_per_branch.min(history.len());
                let to_erase: Vec<DrawOperation> = history.drain(history.len() - count..).collect();
                all_entries_to_erase.extend(to_erase);
            }
        }

        // Erase all entries (now we only have immutable self reference)
        for entry in all_entries_to_erase.iter().rev() {
            self.erase_entry(renderer, entry)?;
        }

        // Remove empty branches
        for branch_id in branches_to_remove {
            self.painter_state.draw_history.remove(&branch_id);
            self.painter_state.main_branches.remove(&branch_id);
        }

        Ok(self.painter_state.draw_history.is_empty())
    }
}

impl Scene for CityGrowScene {
    fn is_animating(&self) -> bool {
        true
    }

    fn prepare_render(&mut self, renderer: &mut Renderer) -> Result<()> {
        renderer.incremental_no_copy()?;
        Ok(())
    }

    fn render(&mut self, renderer: &mut Renderer, _delta_time: f32) -> Result<()> {
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

        // Handle reverse animation
        if self.reverse_running {
            let done = self.reverse_step(renderer)?;
            if done {
                // Restart the animation
                debug!("Reverse animation complete, restarting");
                self.initialize(self.config.start_branches as usize);
            }
            return Ok(());
        }

        // Generate events for this frame
        let events = {
            let mut events = self.process_branching();
            events.extend(self.process_stepping());
            events
        };

        // Separate events into non-main and main branch events for proper layering
        // Non-main branches are drawn first (appear below), main branches last (appear on top)
        let mut non_main_events = Vec::new();
        let mut main_events = Vec::new();

        for event in events {
            let branch_id = match &event {
                Event::Move { branch_id, .. } => *branch_id,
                Event::BranchOff { child_id, .. } => *child_id,
            };

            if self.painter_state.main_branches.contains(&branch_id) {
                main_events.push(event);
            } else {
                non_main_events.push(event);
            }
        }

        // Draw non-main branches first (background)
        for event in &non_main_events {
            self.draw_move(renderer, event)?;
        }

        // Draw main branches last (foreground - on top)
        for event in &main_events {
            self.draw_move(renderer, event)?;
        }

        // Check if all branches are exhausted
        if self.branch_list.is_empty() && !self.reverse_running {
            debug!("All branches exhausted, starting reverse animation");
            self.reverse_running = true;
        }

        Ok(())
    }

    fn on_resize(&mut self, width: u32, height: u32) {
        self.screen_width = width as f32;
        self.screen_height = height as f32;

        let cell_count_x = (self.screen_width / self.config.scale / 2.0).round() as u32;
        let cell_count_y = (self.screen_height / self.config.scale / 2.0).round() as u32;
        self.grid = Grid::new(cell_count_x, cell_count_y);

        self.initialize(self.config.start_branches as usize);
    }
}
