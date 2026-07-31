//! Drive the LCD screens on a Corne Max keyboard over raw HID.
//!
//! Exposed as a library so two binaries can share it: `cornelcd` (console, for
//! the CLI) and `cornelcdw` (GUI subsystem, for the tray daemon). Windows gives
//! a console-subsystem process a console window, which is unwanted for
//! something that lives in the tray — the same reason python ships pythonw.exe.

pub mod activity;
pub mod anim;
pub mod clawd;
pub mod paths;
pub mod proto;
pub mod render;
pub mod screens;
#[cfg(target_os = "linux")]
pub mod sysinfo;
pub mod tray;
pub mod usage;

use proto::{Half, Keyboard};
use std::time::Duration;

/// Tray daemon: worker thread drives the keyboard, tray thread owns the UI.
///
/// The worker reconnects on its own, so unplugging the keyboard, flashing it,
/// or suspending the machine are all non-events — and if this process dies the
/// firmware watchdog simply returns Clawd to his autonomous routine.
pub fn run_daemon(interval: Duration) -> Result<(), Box<dyn std::error::Error>> {
    use std::sync::atomic::Ordering;

    let shared = tray::Shared::new();
    let worker_state = shared.clone();

    std::thread::spawn(move || {
        let mut kb: Option<Keyboard> = None;
        let mut w = activity::Watcher::new();
        let mut last_tally = None;

        while !worker_state.quit.load(Ordering::Relaxed) {
            if kb.is_none() {
                match Keyboard::open() {
                    Ok(k) => {
                        kb = Some(k);
                        worker_state.connected.store(true, Ordering::Relaxed);
                    }
                    Err(_) => {
                        worker_state.connected.store(false, Ordering::Relaxed);
                        std::thread::sleep(Duration::from_secs(3));
                        continue;
                    }
                }
            }

            if !worker_state.enabled.load(Ordering::Relaxed) {
                std::thread::sleep(interval);
                continue;
            }

            let Some(k) = kb.as_ref() else { continue };
            let step = (|| -> Result<(), Box<dyn std::error::Error>> {
                let state = w.poll()?;
                k.set_activity(state)?;
                worker_state.set_status(activity::state_name(state));

                let t = w.tally();
                if Some(t) != last_tally {
                    let lines = [
                        format!("BASH {}", t.bash),
                        format!("EDIT {}", t.edit),
                        format!("WRITE {}", t.write),
                        format!("WEB {}", t.web),
                    ];
                    for h in Half::both() {
                        for (i, line) in lines.iter().enumerate() {
                            k.set_tally(h, i as u8, line)?;
                        }
                    }
                    last_tally = Some(t);
                }
                Ok(())
            })();

            // Any write failure almost always means the keyboard went away;
            // drop the handle and let the reconnect path pick it up.
            if step.is_err() {
                kb = None;
                last_tally = None;
                worker_state.connected.store(false, Ordering::Relaxed);
            }

            std::thread::sleep(interval);
        }
    });

    tray::run(shared)
}
