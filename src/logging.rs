//! File based logging suited for a GUI application without a console.

use std::fs::{self, OpenOptions};

use log::LevelFilter;
#[cfg(debug_assertions)]
use simplelog::{ColorChoice, TermLogger, TerminalMode};
use simplelog::{CombinedLogger, Config as LogConfig, SharedLogger, WriteLogger};

use crate::config;

/// Initialise logging to `%APPDATA%\designer\designer.log` plus stderr.
///
/// Release builds use the Windows subsystem and therefore have no console;
/// the log file is the primary diagnostic channel.
pub fn init() {
    let path = config::log_path();
    if let Some(parent) = path.parent()
        && let Err(error) = fs::create_dir_all(parent)
    {
        eprintln!(
            "could not create log directory {}: {error}",
            parent.display()
        );
    }

    let file_level = if cfg!(debug_assertions) {
        LevelFilter::Debug
    } else {
        LevelFilter::Info
    };

    let mut loggers: Vec<Box<dyn SharedLogger>> = Vec::new();

    match OpenOptions::new().create(true).append(true).open(&path) {
        Ok(file) => loggers.push(WriteLogger::new(file_level, LogConfig::default(), file)),
        Err(error) => eprintln!("could not open log file {}: {error}", path.display()),
    }

    // A release build uses the Windows subsystem, so stderr is not attached to
    // anything useful; only keep the console logger for debug runs.
    #[cfg(debug_assertions)]
    loggers.push(TermLogger::new(
        LevelFilter::Debug,
        LogConfig::default(),
        TerminalMode::Stderr,
        ColorChoice::Auto,
    ));

    if let Err(error) = CombinedLogger::init(loggers) {
        eprintln!("could not initialise logging: {error}");
    }
}

/// Log panics instead of losing them in a windowless process.
pub fn install_panic_hook() {
    std::panic::set_hook(Box::new(|info| {
        log::error!("application panicked: {info}");
        let location = info
            .location()
            .map(|location| location.to_string())
            .unwrap_or_else(|| "unknown location".to_owned());
        log::error!("panic location: {location}");
    }));
}
