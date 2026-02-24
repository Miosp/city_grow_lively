#![windows_subsystem = "windows"]

use anyhow::{Context, Result};
use app::App;
use tracing::{debug, info};
use window::Window;

use crate::city_grow::CityGrowScene;

mod app;
mod city_grow;
mod renderer;
mod scene;
mod window;

fn main() -> Result<()> {
    // Get log path next to the executable
    let log_dir = std::env::current_exe()
        .ok()
        .and_then(|p| p.parent().map(|p| p.to_path_buf()))
        .unwrap_or_else(|| std::path::PathBuf::from("."));

    let file_appender = tracing_appender::rolling::never(&log_dir, "city_grow.log");
    let (non_blocking, _guard) = tracing_appender::non_blocking(file_appender);

    tracing_subscriber::fmt()
        .with_ansi(false)
        .with_writer(non_blocking)
        .with_target(false)
        .with_level(true)
        .with_line_number(true)
        // .with_max_level(tracing::Level::DEBUG)
        .init();

    info!("Starting City Grow application");
    debug!(
        "Log file location: {}",
        log_dir.join("city_grow.log").display()
    );
    // Create scene (frontend logic)
    let scene = CityGrowScene::new(1920, 1080);

    // Create app (ties everything together)
    let app = App::new(scene);

    // Create window (backend)
    let _window = Window::create("City Grow", app).context("Failed to create window")?;

    debug!("Entering message loop");

    // Run message loop
    let result = Window::run_message_loop().context("Message loop failed");

    info!("Exiting");

    // Keep guard alive by explicitly dropping it at the end
    drop(_guard);

    result
}
