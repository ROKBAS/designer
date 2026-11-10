#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

//! DDC/CI monitor brightness tray application.
//!
//! The tray icon exposes per-monitor brightness and contrast controls. All
//! hardware access happens on a worker thread; see [`worker`] and
//! [`monitor::controller`] for details.

mod app;
mod config;
mod error;
mod logging;
mod monitor;
mod tray;
mod worker;

use std::io::Write;
use std::process::ExitCode;
use std::time::Duration;

use log::{error, info};
use single_instance::SingleInstance;

use crate::error::AppResult;
use crate::monitor::controller::MonitorController;

const INSTANCE_KEY: &str = "designer-ddc-tray";

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();

    if args.iter().any(|arg| arg == "--help" || arg == "-h") {
        print_help();
        return ExitCode::SUCCESS;
    }

    logging::init();
    logging::install_panic_hook();
    info!(
        "designer {} starting; log file: {}",
        env!("CARGO_PKG_VERSION"),
        config::log_path().display()
    );

    if args.iter().any(|arg| arg == "--dump") {
        return match run_dump() {
            Ok(()) => ExitCode::SUCCESS,
            Err(error) => {
                error!("diagnostic dump failed: {error}");
                eprintln!("error: {error}");
                ExitCode::FAILURE
            }
        };
    }

    let instance = match SingleInstance::new(INSTANCE_KEY) {
        Ok(instance) => instance,
        Err(error) => {
            error!("could not create the single-instance guard: {error}");
            eprintln!("error: could not create the single-instance guard: {error}");
            return ExitCode::FAILURE;
        }
    };

    if !instance.is_single() {
        info!("another instance is already running; exiting");
        return ExitCode::SUCCESS;
    }

    match app::run() {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            error!("fatal error: {error}");
            eprintln!("error: {error}");
            ExitCode::FAILURE
        }
    }
}

fn print_help() {
    let text = "\
designer - DDC/CI monitor brightness tray app

USAGE:
    designer            Start the tray application
    designer --dump     Print DDC/CI monitor diagnostics and exit
    designer --help     Show this help
";
    // Write directly and ignore errors: release builds have no console.
    let mut stdout = std::io::stdout();
    let _ = stdout.write_all(text.as_bytes());
    let _ = stdout.flush();
}

fn run_dump() -> AppResult<()> {
    let mut controller = MonitorController::new(Duration::from_millis(50));
    controller.refresh();
    let text = controller.dump();

    // Ignore write errors so the diagnostic mode also works from a build
    // without a console; the same text is written to the log file.
    let mut stdout = std::io::stdout();
    let _ = stdout.write_all(text.as_bytes());
    let _ = stdout.flush();
    info!("diagnostic dump:\n{text}");

    Ok(())
}
