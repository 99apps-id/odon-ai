use std::net::TcpStream;
use std::sync::Mutex;
use std::time::{Duration, Instant};

use tauri::{Manager, RunEvent};
use tauri_plugin_shell::process::CommandChild;
use tauri_plugin_shell::ShellExt;

/// The daemon's loopback address. The desktop shell and the daemon ship
/// together, so the port is part of the product contract (the frontend
/// already targets it).
const DAEMON_ADDR: &str = "127.0.0.1:4096";

/// Holds the spawned daemon sidecar so it can be stopped with the app.
/// Empty when a daemon was already running (its lifecycle is the
/// operator's, not the shell's).
struct DaemonChild(Mutex<Option<CommandChild>>);

fn daemon_reachable() -> bool {
    TcpStream::connect(DAEMON_ADDR).is_ok()
}

/// Waits briefly for the sidecar daemon to accept connections. The
/// window loads the embedded frontend meanwhile; only the first API
/// calls depend on this, so a timeout is a degraded start, not a fatal
/// one.
fn wait_for_daemon() {
    let deadline = Instant::now() + Duration::from_secs(20);
    while Instant::now() < deadline {
        if daemon_reachable() {
            return;
        }
        std::thread::sleep(Duration::from_millis(200));
    }
}

/// Spawns the bundled odond sidecar unless a daemon already answers on
/// the loopback port. Failures are reported on stderr and the app still
/// opens: in development the daemon is usually started separately.
fn spawn_daemon(app: &tauri::App) {
    if daemon_reachable() {
        return;
    }
    let workspace = app
        .path()
        .home_dir()
        .map(|home| home.to_string_lossy().into_owned())
        .unwrap_or_else(|_| ".".to_string());
    let sidecar = match app.shell().sidecar("odond") {
        Ok(command) => command,
        Err(error) => {
            eprintln!("odon-desktop: odond sidecar unavailable: {error}");
            return;
        }
    };
    match sidecar
        .args(["serve", "--bind", DAEMON_ADDR, "--workspace", &workspace])
        .spawn()
    {
        Ok((receiver, child)) => {
            // Drain the sidecar's output stream so a chatty daemon can
            // never block on a full pipe.
            tauri::async_runtime::spawn(async move {
                let mut receiver = receiver;
                while receiver.recv().await.is_some() {}
            });
            if let Some(state) = app.try_state::<DaemonChild>() {
                if let Ok(mut slot) = state.0.lock() {
                    *slot = Some(child);
                }
            }
            wait_for_daemon();
        }
        Err(error) => {
            eprintln!("odon-desktop: failed to start odond sidecar: {error}");
        }
    }
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_shell::init())
        .manage(DaemonChild(Mutex::new(None)))
        .setup(|app| {
            spawn_daemon(app);
            Ok(())
        })
        .build(tauri::generate_context!())
        .expect("error while building the ODON desktop application")
        .run(|app_handle, event| {
            if let RunEvent::Exit = event {
                if let Some(state) = app_handle.try_state::<DaemonChild>() {
                    if let Ok(mut slot) = state.0.lock() {
                        if let Some(child) = slot.take() {
                            let _ = child.kill();
                        }
                    }
                }
            }
        });
}
