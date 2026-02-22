use crate::{renderer::Renderer, scene::Scene, window::WindowHandler};
use anyhow::Result;
use std::time::Instant;
use tracing::{error, info};
use windows::Win32::Foundation::HWND;

/// Application state that manages the renderer and scene
pub struct App<S: Scene> {
    renderer: Option<Renderer>,
    scene: S,
    last_frame_time: Instant,
    frame_count: u32,
}

impl<S: Scene> App<S> {
    pub fn new(scene: S) -> Self {
        Self {
            renderer: None,
            scene,
            last_frame_time: Instant::now(),
            frame_count: 0,
        }
    }

    fn ensure_initialized(&mut self, hwnd: HWND) -> bool {
        if self.renderer.is_some() {
            return true;
        }

        match Renderer::new(hwnd) {
            Ok(renderer) => {
                info!("Renderer initialized successfully");
                self.renderer = Some(renderer);
                true
            }
            Err(e) => {
                error!("Failed to initialize renderer: {:?}", e);
                false
            }
        }
    }

    fn render_frame(&mut self) -> Result<()> {
        let renderer = self
            .renderer
            .as_ref()
            .ok_or_else(|| anyhow::anyhow!("Renderer not initialized"))?;

        // Calculate delta time
        let now = Instant::now();
        let delta = now.duration_since(self.last_frame_time).as_secs_f32();
        self.last_frame_time = now;

        // Update scene
        self.scene.update(delta);

        // Render
        renderer.begin_draw();
        self.scene.render(renderer)?;
        renderer.end_draw()?;

        self.frame_count += 1;
        if self.frame_count % 60 == 0 {
            info!("Rendered {} frames", self.frame_count);
        }
        Ok(())
    }
}

impl<S: Scene> WindowHandler for App<S> {
    fn on_paint(&mut self, hwnd: HWND) {
        if self.ensure_initialized(hwnd)
            && let Err(e) = self.render_frame()
        {
            error!("Render error: {:?}", e);
        }
    }

    fn on_timer(&mut self, hwnd: HWND) {
        if self.ensure_initialized(hwnd)
            && let Err(e) = self.render_frame()
        {
            error!("Render error: {:?}", e);
        }
    }

    fn on_resize(&mut self, hwnd: HWND, width: u32, height: u32) {
        info!(width, height, "Window resized");

        // Recreate renderer with new size
        self.renderer = None;

        // Notify scene
        self.scene.on_resize(width, height);

        // Force re-initialization on next paint
        self.ensure_initialized(hwnd);
    }

    fn on_destroy(&mut self) {
        info!("Application shutting down");
    }
}
