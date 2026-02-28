use crate::{
    ext::color_ext::D2DColorExt,
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

const POSITIONS: [Pos; 4] = [
    Pos { x: 1, y: 0 },  // East
    Pos { x: -1, y: 0 }, // West
    Pos { x: 0, y: 1 },  // South
    Pos { x: 0, y: -1 }, // North
];

enum Event {
    BranchOff {
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

struct PainterState {
    draw_history: HashMap<u32, Vec<DrawOperation>>,
    main_branches: HashSet<u32>,
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

    fn is_position_valid(&self, pos: &Pos) -> bool {
        pos.x >= 0 && pos.x < self.size_x as i32 && pos.y >= 0 && pos.y < self.size_y as i32
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
    pub saturation_main: u8,
    pub saturation_branch: u8,
    pub city_rect_alpha: f32,
    pub scale: f32,
    pub reverse_actions_per_frame: usize,
    pub land_directional_bias: f32,
}

impl Default for CityGrowConfig {
    fn default() -> Self {
        Self {
            life_time: 8000,
            life_time_branch: 15,
            prop_city_to_land: 0.12,
            prop_land_to_city: 0.03,
            prop_branch_off_city: 0.15,
            prop_branch_off_land: 0.06,
            prop_branch_off_to_main: 0.02,
            branch_fall_off: 50.0,
            change_hue_new_main: 11,
            start_branches: 3,
            max_steps_back: 50,
            lightness_default: 140,
            lightness_branch: 60,
            saturation_main: 255,
            saturation_branch: 255,
            city_rect_alpha: 0.35,
            scale: 2.0,
            reverse_actions_per_frame: 50,
            land_directional_bias: 3.0,
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
        let color = Hsla::new(hue, config.saturation_main, config.lightness_default, 255);

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
        if self.mode == BranchMode::City && rng.random::<f32>() < config.prop_city_to_land {
            return Self {
                expand_direction: self
                    .expand_direction(grid, rng)
                    .unwrap_or(self.expand_direction),
                mode: BranchMode::Land,
                ..self
            };
        } else if self.mode == BranchMode::Land && rng.random::<f32>() < config.prop_land_to_city {
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
        target_position
            .try_sub(self.pos)
            .filter(|pos| grid.is_position_valid(pos))
    }

    /// If no free neighbors, try backtracking up to max_steps_back to find a position with free neighbors.
    /// If such a position is not found, return None to indicate the branch should die.
    fn set_next_position(self, grid: &Grid, config: &CityGrowConfig) -> Option<Self> {
        if grid.get_free_neighbors(self.pos).is_empty() {
            let num_positions_to_search =
                (config.max_steps_back as usize).min(self.own_fields.len());
            let new_position = self
                .own_fields
                .iter()
                .rev()
                .take(num_positions_to_search)
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
            let preferred = self
                .pos
                .try_add(self.expand_direction)
                .filter(|pos| grid.is_position_valid(pos));

            if let Some(preferred) = preferred.filter(|p| neighbors.contains(p)) {
                if rng.random_ratio(
                    neighbors.len() as u32,
                    (neighbors.len() as f32 * config.land_directional_bias).round() as u32,
                ) {
                    return (self, *neighbors.choose(rng).unwrap());
                }
                return (self, preferred);
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

        let child = Self {
            id: rng.random(),
            pos: selected_neighbor,
            mode: BranchMode::City,
            expand_direction: Pos::new(0, 0),
            own_fields: vec![selected_neighbor],
            age: 0,
            life_time: config.life_time_branch,
            color: Hsla::new(
                self.color.h,
                config.saturation_branch,
                config.lightness_branch,
                255,
            ),
        };

        let branch_event = Event::BranchOff {
            child_id: child.id,
            parent_pos: search_pos,
            child_pos: selected_neighbor,
            parent_mode: self.mode,
            child_color: child.color,
        };

        BranchOffResult::Success {
            new_parent: self,
            child,
            pos: selected_neighbor,
            event: branch_event,
        }
    }
}

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
        let mut i = 0;

        while i < self.branch_list.len() {
            let branch = self.branch_list.swap_remove(i);
            let scaled_chance = self.config.branch_chance(branch.mode)
                * (1.0 + self.config.branch_fall_off)
                / (self.config.branch_fall_off + branch_count as f32);

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

                        let child =
                            if self.rng.random::<f32>() < self.config.prop_branch_off_to_main {
                                let promoted_child = Branch {
                                    color: Hsla::new(
                                        ((child.color.h + self.config.change_hue_new_main) as u16
                                            % 256) as u8,
                                        self.config.saturation_main,
                                        self.config.lightness_default,
                                        255,
                                    ),
                                    life_time: self.config.life_time,
                                    ..child
                                };
                                self.painter_state.main_branches.insert(promoted_child.id);
                                promoted_child
                            } else {
                                child
                            };

                        self.branch_list.push(child);
                        self.branch_list.push(new_parent);
                        let last = self.branch_list.len() - 1;
                        self.branch_list.swap(i, last);
                        i += 1;
                    }
                    BranchOffResult::Failure { branch } => {
                        self.branch_list.push(branch);
                        let last = self.branch_list.len() - 1;
                        self.branch_list.swap(i, last);
                        i += 1;
                    }
                }
            } else {
                self.branch_list.push(branch);
                let last = self.branch_list.len() - 1;
                self.branch_list.swap(i, last);
                i += 1;
            }
        }

        events
    }

    fn process_stepping(&mut self) -> Vec<Event> {
        let mut events = Vec::with_capacity(self.branch_list.len());
        let mut i = 0;

        while i < self.branch_list.len() {
            let branch = self.branch_list.swap_remove(i);
            if let Some((new_branch, pos, next_pos, own_fields_tip)) =
                branch.step_branch(&self.grid, &self.config, &mut self.rng)
            {
                self.grid.set(next_pos.x as u32, next_pos.y as u32, true);
                events.push(Event::Move {
                    branch_id: new_branch.id,
                    from: pos,
                    to: next_pos,
                    mode: new_branch.mode,
                    color: new_branch.color,
                    own_fields_tip,
                });
                self.branch_list.push(new_branch);
                let last = self.branch_list.len() - 1;
                self.branch_list.swap(i, last);
                i += 1;
            }
        }

        events
    }

    /// Helper: Convert grid position to screen coordinates
    fn grid_to_screen(&self, pos: Pos) -> Vector2 {
        Vector2 {
            X: pos.x as f32 * 2.0 * self.config.scale + self.config.scale / 2.0,
            Y: pos.y as f32 * 2.0 * self.config.scale + self.config.scale / 2.0,
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
            left: corner.x as f32 * 2.0 * self.config.scale + self.config.scale,
            top: corner.y as f32 * 2.0 * self.config.scale + self.config.scale,
            right: corner.x as f32 * 2.0 * self.config.scale
                + self.config.scale
                + (2.0 * self.config.scale - self.config.scale),
            bottom: corner.y as f32 * 2.0 * self.config.scale
                + self.config.scale
                + (2.0 * self.config.scale - self.config.scale),
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
            } => (
                *child_id,
                *parent_pos,
                *child_pos,
                *parent_mode,
                *child_color,
                *parent_pos,
            ),
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
        let fade_color = d2d_color.with_alpha(self.config.city_rect_alpha);
        for rect in &rects_to_draw {
            renderer.draw_filled_rect(rect, &fade_color)?;
        }

        // Draw the line
        renderer.draw_line(screen_from, screen_to, &d2d_color, self.config.scale)?;

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
            self.config.scale,
        ));

        Ok(())
    }

    /// Consolidate consecutive lines into polylines for more efficient rendering
    fn consolidate_lines(operations: &[DrawOperation]) -> Vec<DrawOperation> {
        if operations.is_empty() {
            return Vec::new();
        }

        let mut result = Vec::new();
        let mut current_polyline_points: Vec<Vector2> = Vec::new();
        let mut current_thickness = 0.0;

        for op in operations {
            match op {
                DrawOperation::Line {
                    start,
                    end,
                    thickness,
                    ..
                } => {
                    // Start new polyline or continue existing one
                    if current_polyline_points.is_empty() {
                        current_polyline_points.push(*start);
                        current_polyline_points.push(*end);
                        current_thickness = *thickness;
                    } else if (current_polyline_points.last().unwrap().X - start.X).abs() < 0.01
                        && (current_polyline_points.last().unwrap().Y - start.Y).abs() < 0.01
                        && (current_thickness - thickness).abs() < 0.01
                    {
                        // Connected line with same thickness - add to polyline
                        current_polyline_points.push(*end);
                    } else {
                        // Disconnected or different thickness - flush current polyline
                        if current_polyline_points.len() > 2 {
                            result.push(DrawOperation::Polyline {
                                points: current_polyline_points.clone(),
                                color: D2D1_COLOR_F {
                                    r: 0.0,
                                    g: 0.0,
                                    b: 0.0,
                                    a: 1.0,
                                },
                                thickness: current_thickness,
                            });
                        } else if current_polyline_points.len() == 2 {
                            result.push(DrawOperation::Line {
                                start: current_polyline_points[0],
                                end: current_polyline_points[1],
                                color: D2D1_COLOR_F {
                                    r: 0.0,
                                    g: 0.0,
                                    b: 0.0,
                                    a: 1.0,
                                },
                                thickness: current_thickness,
                            });
                        }
                        current_polyline_points.clear();
                        current_polyline_points.push(*start);
                        current_polyline_points.push(*end);
                        current_thickness = *thickness;
                    }
                }
                _ => {
                    // Non-line operation - flush current polyline and add operation
                    if current_polyline_points.len() > 2 {
                        result.push(DrawOperation::Polyline {
                            points: current_polyline_points.clone(),
                            color: D2D1_COLOR_F::black(),
                            thickness: current_thickness,
                        });
                    } else if current_polyline_points.len() == 2 {
                        result.push(DrawOperation::Line {
                            start: current_polyline_points[0],
                            end: current_polyline_points[1],
                            color: D2D1_COLOR_F::black(),
                            thickness: current_thickness,
                        });
                    }
                    current_polyline_points.clear();

                    // Convert operation to black version for erasure
                    let black_op = match op {
                        DrawOperation::FilledRect { rect, .. } => DrawOperation::FilledRect {
                            rect: *rect,
                            color: D2D1_COLOR_F::black(),
                        },
                        DrawOperation::Rect {
                            rect, thickness, ..
                        } => DrawOperation::Rect {
                            rect: *rect,
                            color: D2D1_COLOR_F::black(),
                            thickness: *thickness,
                        },
                        DrawOperation::Polyline {
                            points, thickness, ..
                        } => DrawOperation::Polyline {
                            points: points.clone(),
                            color: D2D1_COLOR_F::black(),
                            thickness: *thickness,
                        },
                        _ => continue,
                    };
                    result.push(black_op);
                }
            }
        }

        // Flush remaining polyline
        if current_polyline_points.len() > 2 {
            result.push(DrawOperation::Polyline {
                points: current_polyline_points,
                color: D2D1_COLOR_F::black(),
                thickness: current_thickness,
            });
        } else if current_polyline_points.len() == 2 {
            result.push(DrawOperation::Line {
                start: current_polyline_points[0],
                end: current_polyline_points[1],
                color: D2D1_COLOR_F::black(),
                thickness: current_thickness,
            });
        }

        result
    }

    /// Batch erase operations in MIN blend mode for efficiency
    fn batch_erase(&self, renderer: &Renderer, operations: &[DrawOperation]) -> Result<()> {
        if operations.is_empty() {
            return Ok(());
        }

        // Consolidate consecutive lines into polylines
        let optimized_ops = Self::consolidate_lines(operations);

        // Set MIN blend mode once for all operations
        renderer.set_min_blend();

        // Use batch drawing for efficiency
        renderer.draw_batch(&optimized_ops)?;

        // Restore normal blend mode
        renderer.set_normal_blend();

        Ok(())
    }

    /// Process reverse animation step
    /// Non-main branches erase first, then main branches
    fn reverse_step(&mut self, renderer: &Renderer) -> Result<bool> {
        if self.painter_state.draw_history.is_empty() {
            return Ok(true); // Done reversing
        }

        let (main_branch_ids, non_main_branch_ids): (Vec<u32>, Vec<u32>) = self
            .painter_state
            .draw_history
            .keys()
            .copied()
            .partition(|branch_id| self.painter_state.main_branches.contains(branch_id));

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

        // Batch erase all entries efficiently (consolidates lines into polylines)
        let entries_to_erase: Vec<DrawOperation> = all_entries_to_erase.into_iter().rev().collect();
        self.batch_erase(renderer, &entries_to_erase)?;

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
            renderer.clear(D2D1_COLOR_F::black());
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
