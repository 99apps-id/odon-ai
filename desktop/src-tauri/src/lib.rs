use std::io::Write;
use std::net::TcpStream;
use std::path::PathBuf;
use std::process::{Child, Command, Stdio};
use std::sync::Mutex;
use std::time::{Duration, Instant};

use tauri::{Manager, RunEvent};

/// The daemon's loopback address. The desktop shell and the daemon ship
/// together, so the port is part of the product contract (the frontend
/// already targets it).
const DAEMON_ADDR: &str = "127.0.0.1:4096";

/// Holds the spawned daemon process so it can be stopped with the app.
/// Empty when a daemon was already running (its lifecycle is the
/// operator's, not the shell's).
struct DaemonChild(Mutex<Option<Child>>);

/// Appends one line to odon-desktop.log in the app log directory. The
/// shell has no console when launched from a desktop menu, so this log
/// is the only evidence when daemon startup misbehaves.
fn log_line(app: &tauri::AppHandle, message: &str) {
    let path = app
        .path()
        .app_log_dir()
        .map(|dir| dir.join("odon-desktop.log"))
        .unwrap_or_else(|_| PathBuf::from("/tmp/odon-desktop.log"));
    if let Some(parent) = path.parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    if let Ok(mut file) = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(&path)
    {
        let _ = writeln!(file, "{message}");
    }
    eprintln!("odon-desktop: {message}");
}

fn daemon_reachable() -> bool {
    TcpStream::connect(DAEMON_ADDR).is_ok()
}

fn daemon_stably_reachable() -> bool {
    for _ in 0..3 {
        if !daemon_reachable() {
            return false;
        }
        std::thread::sleep(Duration::from_millis(100));
    }
    true
}

/// Waits briefly for the daemon to accept connections, reporting
/// whether it came up. The window loads the embedded frontend
/// meanwhile; only the first API calls depend on this.
fn wait_for_daemon() -> bool {
    let deadline = Instant::now() + Duration::from_secs(20);
    while Instant::now() < deadline {
        if daemon_reachable() {
            return true;
        }
        std::thread::sleep(Duration::from_millis(200));
    }
    daemon_reachable()
}

/// The odond binary bundled next to this executable (the Tauri
/// externalBin sidecar layout in every bundle target).
fn sidecar_path() -> Option<PathBuf> {
    let exe = std::env::current_exe().ok()?;
    let dir = exe.parent()?;
    let candidate = dir.join(if cfg!(windows) { "odond.exe" } else { "odond" });
    candidate.exists().then_some(candidate)
}

/// Starts the bundled daemon unless one already answers on the
/// loopback port. The spawn is a plain child process next to this
/// executable, deliberately not a plugin-mediated sidecar: a previous
/// plugin-scoped spawn could fail silently in a menu-launched app and
/// leave the window talking to nothing. Every outcome lands in the log.
fn spawn_daemon(app: &tauri::App) {
    let handle = app.handle().clone();
    if daemon_stably_reachable() {
        log_line(&handle, "daemon already answering on 127.0.0.1:4096; not spawning");
        return;
    }
    let Some(odond) = sidecar_path() else {
        log_line(
            &handle,
            "odond sidecar not found next to the app executable; start `odond serve` manually",
        );
        return;
    };
    let workspace = app
        .path()
        .home_dir()
        .map(|home| home.to_string_lossy().into_owned())
        .unwrap_or_else(|_| ".".to_string());
    let log_path = app
        .path()
        .app_log_dir()
        .map(|dir| dir.join("odond.log"))
        .unwrap_or_else(|_| PathBuf::from("/tmp/odond.log"));
    if let Some(parent) = log_path.parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    let open_log = || {
        std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(&log_path)
            .map(Stdio::from)
            .unwrap_or_else(|_| Stdio::null())
    };
    let (stdout, stderr) = (open_log(), open_log());

    log_line(
        &handle,
        &format!("spawning daemon: {} serve --bind {DAEMON_ADDR} --workspace {workspace}", odond.display()),
    );
    match Command::new(&odond)
        .args(["serve", "--bind", DAEMON_ADDR, "--workspace", &workspace])
        .stdout(stdout)
        .stderr(stderr)
        .spawn()
    {
        Ok(child) => {
            if let Some(state) = app.try_state::<DaemonChild>() {
                if let Ok(mut slot) = state.0.lock() {
                    *slot = Some(child);
                }
            }
            if wait_for_daemon() {
                log_line(&handle, "daemon is up on 127.0.0.1:4096");
            } else {
                log_line(
                    &handle,
                    &format!("daemon did not answer within 20s; see {}", log_path.display()),
                );
            }
        }
        Err(error) => {
            log_line(&handle, &format!("failed to spawn daemon {}: {error}", odond.display()));
        }
    }
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
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
                        if let Some(mut child) = slot.take() {
                            let _ = child.kill();
                            let _ = child.wait();
                        }
                    }
                }
            }
        });
}
