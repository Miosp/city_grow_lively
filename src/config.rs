use std::path::{Path, PathBuf};

use crate::city_grow::CityGrowSceneConfig;
use anyhow::Result;
use config::Config;
use serde::{Deserialize, Serialize};

#[derive(Default, Serialize, Deserialize)]
pub struct CityGrowConfig {
    pub app: AppConfig,
    pub scene: CityGrowSceneConfig,
}

#[derive(Serialize, Deserialize)]
pub struct AppConfig {
    pub framerate: u32,
    pub default_width: u32,
    pub default_height: u32,
    pub log_level: LogLevel,
}

impl Default for AppConfig {
    fn default() -> Self {
        Self {
            framerate: 60,
            default_width: 1920,
            default_height: 1080,
            log_level: LogLevel::Info,
        }
    }
}

impl CityGrowConfig {
    pub fn exists(path: &Path) -> bool {
        Self::config_path_from_dir(path).exists()
    }

    pub fn write_default(path: &Path) -> Result<()> {
        let default_config = Self::default();
        let mut writer = std::fs::File::create(Self::config_path_from_dir(path))?;
        serde_saphyr::to_io_writer(&mut writer, &default_config)?;
        Ok(())
    }

    pub fn load_config(path: &Path) -> Result<Self> {
        let config = Config::builder()
            .add_source(config::File::from(Self::config_path_from_dir(path)))
            .build()?;
        let city_grow_config: CityGrowConfig = config.try_deserialize()?;
        Ok(city_grow_config)
    }

    fn config_path_from_dir(app_dir: &Path) -> PathBuf {
        app_dir.join("city_grow.yaml")
    }
}

#[derive(Debug, Serialize, Deserialize)]
pub enum LogLevel {
    Error,
    Warn,
    Info,
    Debug,
    Trace,
}

impl From<LogLevel> for tracing::Level {
    fn from(level: LogLevel) -> Self {
        match level {
            LogLevel::Error => tracing::Level::ERROR,
            LogLevel::Warn => tracing::Level::WARN,
            LogLevel::Info => tracing::Level::INFO,
            LogLevel::Debug => tracing::Level::DEBUG,
            LogLevel::Trace => tracing::Level::TRACE,
        }
    }
}
