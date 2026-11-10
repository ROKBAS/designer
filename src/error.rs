//! Shared error types for the application.

use thiserror::Error;

use crate::config::ConfigError;
use crate::monitor::MonitorError;

/// Convenient result alias used across the application.
pub type AppResult<T> = Result<T, AppError>;

/// Top level application error.
#[derive(Debug, Error)]
pub enum AppError {
    #[error("monitor error: {0}")]
    Monitor(#[from] MonitorError),

    #[error("configuration error: {0}")]
    Config(#[from] ConfigError),

    #[error("tray icon error: {0}")]
    Tray(#[from] tray_icon::Error),

    #[error("tray icon image error: {0}")]
    Icon(#[from] tray_icon::BadIcon),

    #[error("worker thread error: {0}")]
    Worker(String),

    #[error("I/O error: {0}")]
    Io(#[from] std::io::Error),
}
