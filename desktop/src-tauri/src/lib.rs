use std::io::Write;
use std::net::TcpStream;
use std::path::PathBuf;
use std::process::{Child, Command, Stdio};
use std::sync::Mutex;
use std::time::{Duration, Instant};

use tauri::{Manager, RunEvent};
use tauri_plugin_dialog::DialogExt;

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
fn start_daemon(app: &tauri::AppHandle) -> Result<(), String> {
    let handle = app.clone();
    if daemon_stably_reachable() {
        log_line(&handle, "daemon already answering on 127.0.0.1:4096; not spawning");
        return Ok(());
    }
    let Some(odond) = sidecar_path() else {
        let message = "odond sidecar not found next to the app executable; start `odond serve` manually";
        log_line(&handle, message);
        return Err(message.to_string());
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
    // Tell the daemon where bundled resources live (the WhatsApp bridge script
    // and its node_modules). Without this the daemon only guesses from the exe
    // location and reports "WhatsApp bridge script not found" in the bundled
    // app even though the resource shipped.
    let resource_dir = app.path().resource_dir().ok();
    let mut command = Command::new(&odond);
    command
        .args(["serve", "--bind", DAEMON_ADDR, "--workspace", &workspace])
        .stdout(stdout)
        .stderr(stderr);
    if let Some(dir) = resource_dir.as_ref() {
        command.env("ODON_RESOURCE_DIR", dir);
    }
    match command.spawn() {
        Ok(child) => {
            if let Some(state) = app.try_state::<DaemonChild>() {
                if let Ok(mut slot) = state.0.lock() {
                    *slot = Some(child);
                }
            }
            if wait_for_daemon() {
                log_line(&handle, "daemon is up on 127.0.0.1:4096");
                Ok(())
            } else {
                let message =
                    format!("daemon did not answer within 20s; see {}", log_path.display());
                log_line(&handle, &message);
                Err(message)
            }
        }
        Err(error) => {
            let message = format!("failed to spawn daemon {}: {error}", odond.display());
            log_line(&handle, &message);
            Err(message)
        }
    }
}

/// Startup path: bring the daemon up and leave the outcome in the log.
fn spawn_daemon(app: &tauri::App) {
    let _ = start_daemon(app.handle());
}

/// Stops a daemon this shell owns, if any, and waits for the port to
/// close so an immediately following start binds cleanly.
fn stop_owned_daemon(app: &tauri::AppHandle) {
    if let Some(state) = app.try_state::<DaemonChild>() {
        if let Ok(mut slot) = state.0.lock() {
            if let Some(mut child) = slot.take() {
                let _ = child.kill();
                let _ = child.wait();
            }
        }
    }
    let deadline = Instant::now() + Duration::from_secs(5);
    while Instant::now() < deadline && daemon_reachable() {
        std::thread::sleep(Duration::from_millis(100));
    }
}

/// Where the daemon leaves the loopback token it generated for this
/// session. The daemon writes it so local relays can authenticate; the
/// desktop shell reads the same file instead of demanding that the
/// operator paste a token into Settings.
fn daemon_token_path(app: &tauri::AppHandle) -> Option<PathBuf> {
    let home = app.path().home_dir().ok()?;
    Some(home.join(".odon").join("api-token"))
}

fn read_token_file(path: &std::path::Path) -> Option<String> {
    let raw = std::fs::read_to_string(path).ok()?;
    let token = raw.trim().to_string();
    (!token.is_empty()).then_some(token)
}

/// Returns the daemon's loopback token, waiting briefly for the file
/// because the shell may be asked before the freshly spawned daemon has
/// finished writing it.
#[tauri::command]
async fn daemon_token(app: tauri::AppHandle) -> Result<Option<String>, String> {
    tauri::async_runtime::spawn_blocking(move || {
        let Some(path) = daemon_token_path(&app) else {
            return Ok(None);
        };
        if let Some(token) = read_token_file(&path) {
            return Ok(Some(token));
        }
        let deadline = Instant::now() + Duration::from_secs(10);
        while Instant::now() < deadline {
            if let Some(token) = read_token_file(&path) {
                return Ok(Some(token));
            }
            std::thread::sleep(Duration::from_millis(100));
        }
        Ok(None)
    })
    .await
    .map_err(|error| format!("token task failed: {error}"))?
}

/// Native folder picker. Returns the chosen path, or None when the
/// dialog was dismissed.
#[tauri::command]
async fn pick_folder(app: tauri::AppHandle) -> Result<Option<String>, String> {
    // The dialog is shown on the main thread and answers from there, so the
    // wait must not occupy the async runtime. A blocking wait on a worker
    // thread keeps the event loop free to deliver the callback; awaiting an
    // in-process channel on the command future can strand the picker.
    let (tx, rx) = std::sync::mpsc::channel();
    app.dialog().file().pick_folder(move |path| {
        let _ = tx.send(path.map(|p| p.to_string()));
    });
    tauri::async_runtime::spawn_blocking(move || {
        rx.recv_timeout(Duration::from_secs(600))
            .map_err(|_| "the folder dialog was dismissed without an answer".to_string())
    })
    .await
    .map_err(|e| format!("folder dialog task failed: {e}"))?
}

/// Stops the daemon the shell started and starts a fresh one. Refuses
/// with an explanation when the answering daemon belongs to someone
/// else, so the shell never kills a process it does not own.
#[tauri::command]
async fn restart_daemon(app: tauri::AppHandle) -> Result<String, String> {
    tauri::async_runtime::spawn_blocking(move || {
        stop_owned_daemon(&app);
        if daemon_reachable() {
            return Err(
                "a daemon is still answering on 127.0.0.1:4096 and was not started by this app; stop it manually"
                    .to_string(),
            );
        }
        start_daemon(&app)?;
        if wait_for_daemon() {
            Ok("daemon restarted and answering on 127.0.0.1:4096".to_string())
        } else {
            Err("daemon did not answer within 20s; see the odon-desktop log".to_string())
        }
    })
    .await
    .map_err(|error| format!("restart task failed: {error}"))?
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_dialog::init())
        .manage(DaemonChild(Mutex::new(None)))
        .invoke_handler(tauri::generate_handler![
            pick_folder,
            restart_daemon,
            daemon_token
        ])
        .setup(|app| {
            spawn_daemon(app);
            Ok(())
        })
        .build(tauri::generate_context!())
        .expect("error while building the ODON desktop application")
        .run(|app_handle, event| {
            if let RunEvent::Exit = event {
                stop_owned_daemon(app_handle);
            }
        });
}
