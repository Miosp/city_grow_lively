#![windows_subsystem = "windows"]

use std::{env::current_exe, path::PathBuf};

use anyhow::{Context, Result};
use app::App;
use tracing::{debug, info};
use tracing_appender::non_blocking::WorkerGuard;
use window::Window;

use crate::{city_grow::CityGrowScene, window::WindowConfigBuilder};

mod app;
mod city_grow;
mod renderer;
mod scene;
mod window;

fn initialize_logging() -> WorkerGuard {
    // Get log path next to the executable
    let log_dir = current_exe()
        .ok()
        .and_then(|p| p.parent().map(|p| p.to_path_buf()))
        .unwrap_or_else(|| PathBuf::from("."));

    let file_appender = tracing_appender::rolling::never(&log_dir, "city_grow.log");
    let (non_blocking, guard) = tracing_appender::non_blocking(file_appender);

    tracing_subscriber::fmt()
        .with_ansi(false)
        .with_writer(non_blocking)
        .with_target(false)
        .with_level(true)
        .with_line_number(true)
        // .with_max_level(tracing::Level::DEBUG)
        .init();

    debug!(
        "Log file location: {}",
        log_dir.join("city_grow.log").display()
    );

    guard
}

fn main() -> Result<()> {
    let _guard = initialize_logging();
    info!("Starting City Grow animation");

    let scene = CityGrowScene::new(1920, 1080); // Initial size, will be updated on first resize
    let app = App::new(scene);
    let _window = Window::create(
        WindowConfigBuilder::default()
            .title("City Grow".to_string())
            .fullscreen(true) // Borderless fullscreen for Lively wallpaper
            .target_framerate(60)
            .build()?,
        app,
    )
    .context("Failed to create window")?;

    debug!("Entering message loop");
    let result = Window::run_message_loop().context("Message loop failed");
    info!("Exiting");

    drop(_guard); // Keep guard alive by explicitly dropping it at the end

    result
}
