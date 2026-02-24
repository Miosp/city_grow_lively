use windows::Win32::Graphics::Direct2D::Common::{D2D_RECT_F, D2D1_COLOR_F};
use windows_numerics::Vector2;

/// Batch drawing operation
#[derive(Clone)]
pub enum DrawOperation {
    Line {
        start: Vector2,
        end: Vector2,
        color: D2D1_COLOR_F,
        thickness: f32,
    },
    Rect {
        rect: D2D_RECT_F,
        color: D2D1_COLOR_F,
        thickness: f32,
    },
    FilledRect {
        rect: D2D_RECT_F,
        color: D2D1_COLOR_F,
    },
    Polyline {
        points: Vec<Vector2>,
        color: D2D1_COLOR_F,
        thickness: f32,
    },
}

impl DrawOperation {
    /// Create a line drawing operation
    pub fn line(start: Vector2, end: Vector2, color: D2D1_COLOR_F, thickness: f32) -> Self {
        Self::Line {
            start,
            end,
            color,
            thickness,
        }
    }

    /// Create a rectangle outline drawing operation
    pub fn rect(rect: D2D_RECT_F, color: D2D1_COLOR_F, thickness: f32) -> Self {
        Self::Rect {
            rect,
            color,
            thickness,
        }
    }

    /// Create a filled rectangle drawing operation
    pub fn filled_rect(rect: D2D_RECT_F, color: D2D1_COLOR_F) -> Self {
        Self::FilledRect { rect, color }
    }

    /// Create a polyline drawing operation
    pub fn polyline(points: Vec<Vector2>, color: D2D1_COLOR_F, thickness: f32) -> Self {
        Self::Polyline {
            points,
            color,
            thickness,
        }
    }
}
