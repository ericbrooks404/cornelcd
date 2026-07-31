//! Windowless entry point for the tray daemon.
//!
//! Identical to `cornelcd daemon`, but built for the GUI subsystem so Windows
//! does not attach a console window. On Linux the attribute is inert and this
//! is simply an alias.

#![cfg_attr(windows, windows_subsystem = "windows")]

use std::process::ExitCode;
use std::time::Duration;

fn main() -> ExitCode {
    // No CLI here on purpose: anything that needs to print belongs in the
    // console binary. Interval matches the daemon default.
    match cornelcd::run_daemon(Duration::from_secs(1)) {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("cornelcdw: {e}");
            ExitCode::FAILURE
        }
    }
}
