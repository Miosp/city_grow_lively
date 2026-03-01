use crate::{renderer::Renderer, scene::Scene, window::WindowHandler};
use anyhow::Result;
use std::time::Instant;
use tracing::{debug, error, info};
use windows::Win32::Foundation::HWND;
use windows::Win32::UI::WindowsAndMessaging::{KillTimer, SetTimer};

/// Application state that manages the renderer and scene
pub struct App<S: Scene> {
    renderer: Option<Renderer>,
    scene: S,
    last_frame_time: Instant,
    frame_count: u32,
    timer_active: bool,
    window_shown: bool,
}

const TIMER_ID: usize = 1;

impl<S: Scene> App<S> {
    pub fn new(scene: S) -> Self {
        Self {
            renderer: None,
            scene,
            last_frame_time: Instant::now(),
            frame_count: 0,
            timer_active: true,
            window_shown: false,
        }
    }

    fn ensure_initialized(&mut self, hwnd: HWND, width: u32, height: u32) -> bool {
        if self.renderer.is_some() {
            return true;
        }

        match Renderer::new(hwnd, width, height, true) {
            // Enable vsync for smooth wallpaper rendering
            Ok(renderer) => {
                debug!(
                    "Renderer initialized successfully with size {}x{}",
                    width, height
                );
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
            .as_mut()
            .ok_or_else(|| anyhow::anyhow!("Renderer not initialized"))?;

        // Calculate delta time
        let now = Instant::now();
        let delta = now.duration_since(self.last_frame_time).as_secs_f32();
        self.last_frame_time = now;

        // Prepare renderer (must be before begin_draw)
        self.scene.prepare_render(renderer)?;

        // Render
        renderer.begin_draw();
        self.scene.render(renderer, delta)?;
        renderer.end_draw()?;

        self.frame_count += 1;
        if self.frame_count.is_multiple_of(60) {
            debug!("Rendered {} frames", self.frame_count);
        }
        Ok(())
    }
}

impl<S: Scene> WindowHandler for App<S> {
    fn on_paint(&mut self, hwnd: HWND) {
        // During active animation, timer handles all rendering
        // Return immediately to avoid any redundant work
        if self.timer_active {
            return;
        }

        // Get current window size
        let (width, height) = unsafe {
            use windows::Win32::UI::WindowsAndMessaging::GetClientRect;
            let mut rect = windows::Win32::Foundation::RECT::default();
            if GetClientRect(hwnd, &mut rect).is_ok() {
                (
                    (rect.right - rect.left) as u32,
                    (rect.bottom - rect.top) as u32,
                )
            } else {
                (1280, 720) // fallback
            }
        };

        // Only handle paint when idle (timer stopped)
        if !self.ensure_initialized(hwnd, width, height) {
            return;
        }

        // Render the current frame
        if let Err(e) = self.render_frame() {
            error!("Render error: {:?}", e);
        }
    }

    fn on_timer(&mut self, hwnd: HWND) {
        // Get current window size
        let (width, height) = unsafe {
            use windows::Win32::UI::WindowsAndMessaging::GetClientRect;
            let mut rect = windows::Win32::Foundation::RECT::default();
            if GetClientRect(hwnd, &mut rect).is_ok() {
                (
                    (rect.right - rect.left) as u32,
                    (rect.bottom - rect.top) as u32,
                )
            } else {
                (1280, 720) // fallback
            }
        };

        if !self.ensure_initialized(hwnd, width, height) {
            return;
        }

        // If scene started animating again but timer was stopped, restart it
        if !self.timer_active && self.scene.is_animating() {
            unsafe {
                SetTimer(Some(hwnd), TIMER_ID, 16, None);
            }
            self.timer_active = true;
            debug!("Animation resumed, timer restarted");
        }

        // Check if scene is still animating
        if self.scene.is_animating() {
            if let Err(e) = self.render_frame() {
                error!("Render error: {:?}", e);
            }
        } else if self.timer_active {
            // Animation complete, stop timer
            unsafe {
                let _ = KillTimer(Some(hwnd), TIMER_ID);
            }
            self.timer_active = false;
            info!("Animation complete, timer stopped - entering idle state");
        }
    }

    fn on_resize(&mut self, hwnd: HWND, width: u32, height: u32) {
        debug!(width, height, "Window resized");

        // Show window on first resize (after Lively has positioned it)
        if !self.window_shown {
            unsafe {
                use windows::Win32::UI::WindowsAndMessaging::{SW_SHOW, ShowWindow};
                ShowWindow(hwnd, SW_SHOW);
            }
            self.window_shown = true;
            debug!("Window shown after initial resize");
        }

        // Recreate renderer with new size
        self.renderer = None;

        // Notify scene
        self.scene.on_resize(width, height);

        // Force re-initialization with correct size
        self.ensure_initialized(hwnd, width, height);
    }

    fn on_destroy(&mut self) {
        info!("Application shutting down");
    }
}
