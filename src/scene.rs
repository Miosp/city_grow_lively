use crate::renderer::Renderer;
use anyhow::Result;

/// Trait for scene rendering logic (the "frontend")
pub trait Scene {
    /// Prepare the renderer before drawing (called before begin_draw)
    fn prepare_render(&mut self, renderer: &mut Renderer) -> Result<()>;

    /// Render the scene using the provided renderer
    fn render(&mut self, renderer: &mut Renderer, delta_time: f32) -> Result<()>;

    /// Handle resize events
    fn on_resize(&mut self, width: u32, height: u32);

    /// Check if the scene is currently animating and needs rendering
    fn is_animating(&self) -> bool;
}
