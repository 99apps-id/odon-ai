#![recursion_limit = "512"]

use axum::{
    extract::Request,
    extract::{
        ws::{Message as WsMessage, WebSocket, WebSocketUpgrade},
        DefaultBodyLimit, Path, Query, State,
    },
    http::{header, HeaderMap, HeaderName, Method, StatusCode},
    middleware::{self, Next},
    response::{
        sse::{Event, KeepAlive, Sse},
        IntoResponse, Json,
    },
    routing::{get, post},
    Router,
};
use chrono::{Datelike, Local, TimeZone, Timelike, Utc};
use clap::{Parser, Subcommand};
use futures_util::{SinkExt, StreamExt};
use portable_pty::{native_pty_system, CommandBuilder, PtySize};
use ring::rand::SecureRandom;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::collections::HashMap;
use std::convert::Infallible;
use std::path::PathBuf;
use std::process::Stdio;
use std::sync::Arc;
// tokio::io
use tokio::sync::broadcast;
use tower_http::cors::{AllowOrigin, CorsLayer};
use tower_http::services::ServeDir;
use tracing::info;

use odon_agent::{AgentRuntime, SteerMessage, SteerQueue};
use odon_channel::{ChannelAdapter, IncomingChannelMessage, TelegramRelay, WhatsAppChannel};
use odon_core::events::create_event_bus;
use odon_core::session::{Session, SessionMode};
use odon_core::{AgentEvent, ArtifactPayload, EventSender, Message, OdonError};
use odon_provider::{
    anthropic::AnthropicProvider,
    host_creds::{discover_antigravity, discover_github_copilot},
    AntigravityProvider, OpenAiCompatibleProvider, SimulatedProvider,
};
use odon_security::{ApprovalDecision, ApprovalEngine, AuditLogger, ExecutionMode, SecurityPolicy};
use odon_storage::StorageEngine;
use odon_tools::{Tool, ToolContext, ToolRegistry};

#[derive(Parser)]
#[command(name = "odond")]
#[command(about = "ODON Headless Daemon - Background agent runtime and API service", long_about = None)]
struct Cli {
    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand)]
enum Commands {
    /// Start the background agent runtime daemon and HTTP API server
    Serve {
        #[arg(short, long, default_value = "127.0.0.1:4096")]
        bind: String,

        #[arg(long)]
        telegram_token: Option<String>,

        #[arg(long, default_value = ".")]
        workspace: String,
    },
    /// Initialize ODON storage schema and configurations
    Init,
    /// Query a running daemon's status and metrics
    Status {
        #[arg(long, default_value = "http://127.0.0.1:4096")]
        url: String,

        #[arg(long)]
        metrics: bool,
    },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AutomationJob {
    pub id: String,
    pub name: String,
    pub description: String,
    pub trigger_type: String,  // "interval", "cron", "on_commit", "manual"
    pub trigger_value: String, // e.g. "Every 2 hours", "0 8 * * *", "15m"
    pub prompt: String,
    pub model: String,
    pub enabled: bool,
    pub last_run: Option<String>,
    pub last_status: Option<String>, // "success", "failed", "running"
    pub created_at: String,
    #[serde(default)]
    pub retry_count: u32,
    #[serde(default)]
    pub retry_delay_secs: u64,
    #[serde(default)]
    pub depends_on: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AutomationLog {
    pub id: String,
    pub automation_id: String,
    pub automation_name: String,
    pub triggered_at: String,
    pub completed_at: Option<String>,
    pub status: String,
    pub output: String,
    #[serde(default)]
    pub attempts: u32,
    #[serde(default)]
    pub error: Option<String>,
}

fn get_automations_path() -> PathBuf {
    let base = std::env::var_os("ODON_HOME")
        .map(PathBuf::from)
        .or_else(|| std::env::var_os("USERPROFILE").map(PathBuf::from))
        .or_else(|| std::env::var_os("HOME").map(PathBuf::from))
        .unwrap_or_else(|| PathBuf::from("."));
    base.join(".odon").join("automations.json")
}

fn load_automations_from_disk() -> Vec<AutomationJob> {
    let path = get_automations_path();
    if let Ok(content) = std::fs::read_to_string(&path) {
        if let Ok(list) = serde_json::from_str::<Vec<AutomationJob>>(&content) {
            return list;
        }
    }
    // No seeded demo jobs: an automation that claims to have run when it never
    // did is worse than an empty state.
    Vec::new()
}

fn get_automation_logs_path() -> PathBuf {
    get_automations_path().with_file_name("automation-logs.json")
}

fn load_automation_logs_from_disk() -> Vec<AutomationLog> {
    if let Ok(content) = std::fs::read_to_string(get_automation_logs_path()) {
        if let Ok(mut list) = serde_json::from_str::<Vec<AutomationLog>>(&content) {
            if list.len() > 100 {
                list.truncate(100);
            }
            return list;
        }
    }
    Vec::new()
}

fn save_automation_logs_to_disk(list: &[AutomationLog]) {
    if let Some(parent) = get_automation_logs_path().parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    if let Ok(content) = serde_json::to_string_pretty(list) {
        let _ = std::fs::write(get_automation_logs_path(), content);
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct WorkspaceConfig {
    default: String,
    allowed: Vec<String>,
}

fn get_workspaces_path() -> PathBuf {
    let base = std::env::var_os("ODON_HOME")
        .map(PathBuf::from)
        .or_else(|| std::env::var_os("USERPROFILE").map(PathBuf::from))
        .or_else(|| std::env::var_os("HOME").map(PathBuf::from))
        .unwrap_or_else(|| PathBuf::from("."));
    base.join(".odon").join("workspaces.json")
}

fn user_home() -> PathBuf {
    std::env::var_os("USERPROFILE")
        .or_else(|| std::env::var_os("HOME"))
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("."))
}

fn expand_home(raw: &str) -> String {
    let trimmed = raw.trim();
    if let Some(rest) = trimmed
        .strip_prefix("~/")
        .or_else(|| trimmed.strip_prefix("~\\"))
    {
        return user_home().join(rest).to_string_lossy().to_string();
    }
    if trimmed == "~" {
        return user_home().to_string_lossy().to_string();
    }
    trimmed.to_string()
}

fn display_path(path: &std::path::Path) -> String {
    let raw = path.to_string_lossy().to_string();
    raw.strip_prefix(r"\\?\").unwrap_or(&raw).to_string()
}

/// Validates a user-selected workspace folder. System directories are refused
/// so a mistaken (or malicious) add cannot expose the whole OS to the agent.
fn canonical_workspace(raw: &str) -> std::result::Result<String, String> {
    if raw.trim().is_empty() {
        return Err("Folder path is empty".to_string());
    }
    let expanded = expand_home(raw);
    let canonical = std::fs::canonicalize(&expanded)
        .map_err(|e| format!("Folder not found or not accessible: {e}"))?;
    if !canonical.is_dir() {
        return Err(format!("Not a folder: {expanded}"));
    }

    let shown = display_path(&canonical);
    let normalized = shown.to_lowercase().replace('\\', "/");
    let trimmed = normalized.trim_end_matches('/');
    let is_drive_root = trimmed.len() == 2 && trimmed.ends_with(':');
    let is_unix_root = trimmed.is_empty() || trimmed == "/";
    let forbidden_prefixes = [
        "c:/windows",
        "c:/program files",
        "c:/program files (x86)",
        "c:/programdata",
        "/etc",
        "/bin",
        "/sbin",
        "/boot",
        "/dev",
        "/proc",
        "/sys",
        "/system",
        "/library",
    ];
    if is_drive_root
        || is_unix_root
        || forbidden_prefixes
            .iter()
            .any(|prefix| trimmed == *prefix || trimmed.starts_with(&format!("{prefix}/")))
    {
        return Err(format!(
            "Refusing to use a system directory as a workspace: {shown}"
        ));
    }
    Ok(shown)
}

fn workspace_allowed(config: &WorkspaceConfig, path: &str) -> bool {
    let target = canonical_workspace(path).unwrap_or_else(|_| path.trim().to_string());
    let mut all = config.allowed.clone();
    all.push(config.default.clone());
    all.iter().any(|candidate| {
        let normalized = canonical_workspace(candidate).unwrap_or_else(|_| candidate.clone());
        normalized.eq_ignore_ascii_case(&target)
    })
}

fn load_workspace_config(default_workspace: &str) -> WorkspaceConfig {
    let default =
        canonical_workspace(default_workspace).unwrap_or_else(|_| expand_home(default_workspace));
    let mut allowed = Vec::new();
    if let Ok(data) = std::fs::read_to_string(get_workspaces_path()) {
        if let Ok(saved) = serde_json::from_str::<WorkspaceConfig>(&data) {
            allowed = saved.allowed;
        }
    }
    if !allowed
        .iter()
        .any(|item| item.eq_ignore_ascii_case(&default))
    {
        allowed.push(default.clone());
    }
    WorkspaceConfig { default, allowed }
}

fn save_workspace_config(config: &WorkspaceConfig) {
    let path = get_workspaces_path();
    if let Some(parent) = path.parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    if let Ok(data) = serde_json::to_string_pretty(config) {
        let _ = std::fs::write(&path, data);
    }
}

async fn workspace_roots(state: &AppState) -> Vec<PathBuf> {
    let config = state.workspaces.read().await;
    let mut roots = vec![PathBuf::from(&config.default)];
    for path in &config.allowed {
        if !roots
            .iter()
            .any(|root| root.to_string_lossy().eq_ignore_ascii_case(path))
        {
            roots.push(PathBuf::from(path));
        }
    }
    roots
}

fn canonical_existing_ancestor(path: &std::path::Path) -> Option<PathBuf> {
    let mut current = path;
    loop {
        if current.exists() {
            return std::fs::canonicalize(current).ok();
        }
        current = current.parent()?;
    }
}

/// Rejects filesystem API access outside the explicitly allowed workspaces.
async fn ensure_allowed_path(
    state: &AppState,
    candidate: &std::path::Path,
) -> std::result::Result<(), String> {
    let canonical = canonical_existing_ancestor(candidate)
        .ok_or_else(|| "Path does not exist or cannot be resolved".to_string())?;
    for root in workspace_roots(state).await {
        if let Ok(canonical_root) = std::fs::canonicalize(&root) {
            if canonical.starts_with(&canonical_root) {
                return Ok(());
            }
        }
    }
    Err("Path is outside the allowed workspaces".to_string())
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct TelegramRelayConfig {
    token: String,
    #[serde(default)]
    allowed_users: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
struct WhatsAppWebConfig {
    #[serde(default)]
    enabled: bool,
    #[serde(default)]
    phone: Option<String>,
    #[serde(default)]
    allowed_numbers: Vec<String>,
    #[serde(default)]
    allow_anyone: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
struct RelayConfig {
    telegram: Option<TelegramRelayConfig>,
    whatsapp_web: Option<WhatsAppWebConfig>,
}

fn get_relay_config_path() -> PathBuf {
    let base = std::env::var_os("ODON_HOME")
        .map(PathBuf::from)
        .or_else(|| std::env::var_os("USERPROFILE").map(PathBuf::from))
        .or_else(|| std::env::var_os("HOME").map(PathBuf::from))
        .unwrap_or_else(|| PathBuf::from("."));
    base.join(".odon").join("relays.json")
}

fn load_relay_config() -> RelayConfig {
    std::fs::read_to_string(get_relay_config_path())
        .ok()
        .and_then(|data| serde_json::from_str(&data).ok())
        .unwrap_or_default()
}

fn save_relay_config(config: &RelayConfig) {
    let path = get_relay_config_path();
    if let Some(parent) = path.parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    if let Ok(data) = serde_json::to_string_pretty(config) {
        let _ = std::fs::write(&path, data);
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let _ = std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o600));
        }
    }
}

struct BridgeChild {
    child: std::sync::Mutex<Option<std::process::Child>>,
}

impl BridgeChild {
    fn new() -> Self {
        Self {
            child: std::sync::Mutex::new(None),
        }
    }
}

impl Drop for BridgeChild {
    fn drop(&mut self) {
        if let Some(mut child) = self.child.lock().unwrap().take() {
            let _ = child.kill();
            let _ = child.wait();
        }
    }
}

/// Shared handle to the bridge process this daemon spawned: its argv and whether
/// it has already exited.
type SpawnedBridge = Arc<std::sync::Mutex<Option<(Vec<String>, bool)>>>;

/// Local manager for the Node/Baileys WhatsApp Web bridge.
#[derive(Clone)]
struct WhatsAppBridge {
    client: reqwest::Client,
    base_url: Arc<std::sync::Mutex<String>>,
    token: String,
    child: Arc<BridgeChild>,
    script: Option<PathBuf>,
    auth_dir: PathBuf,
    daemon_url: String,
    api_token: Option<String>,
    log_path: PathBuf,
    spawned: SpawnedBridge,
}

fn resolve_first_existing(candidates: impl IntoIterator<Item = PathBuf>) -> Option<PathBuf> {
    candidates.into_iter().find(|path| path.exists())
}

impl WhatsAppBridge {
    fn new(bind: &str, api_token: Option<String>) -> Self {
        let port = std::env::var("WA_BRIDGE_PORT")
            .ok()
            .and_then(|value| value.parse::<u16>().ok())
            .unwrap_or(4097);
        let daemon_port = bind
            .rsplit_once(':')
            .and_then(|(_, port)| port.parse::<u16>().ok())
            .unwrap_or(4096);
        let base = std::env::var_os("ODON_HOME")
            .map(PathBuf::from)
            .or_else(|| std::env::var_os("USERPROFILE").map(PathBuf::from))
            .or_else(|| std::env::var_os("HOME").map(PathBuf::from))
            .unwrap_or_else(|| PathBuf::from("."));
        let state_dir = base.join(".odon");
        let script = Self::resolve_script();
        let token_path = state_dir.join("whatsapp-bridge.token");
        let token = std::env::var("WA_BRIDGE_TOKEN")
            .ok()
            .map(|value| value.trim().to_string())
            .filter(|value| !value.is_empty())
            .or_else(|| {
                std::fs::read_to_string(&token_path)
                    .ok()
                    .map(|value| value.trim().to_string())
                    .filter(|value| !value.is_empty())
            })
            .unwrap_or_else(|| {
                let generated = uuid::Uuid::new_v4().to_string();
                if let Some(parent) = token_path.parent() {
                    let _ = std::fs::create_dir_all(parent);
                }
                if std::fs::write(&token_path, &generated).is_ok() {
                    #[cfg(unix)]
                    {
                        use std::os::unix::fs::PermissionsExt;
                        let _ = std::fs::set_permissions(
                            &token_path,
                            std::fs::Permissions::from_mode(0o600),
                        );
                    }
                }
                generated
            });
        Self {
            client: reqwest::Client::new(),
            base_url: Arc::new(std::sync::Mutex::new(format!("http://127.0.0.1:{port}"))),
            token,
            child: Arc::new(BridgeChild::new()),
            script,
            auth_dir: state_dir.join("whatsapp-auth"),
            daemon_url: format!("http://127.0.0.1:{daemon_port}"),
            api_token,
            log_path: state_dir.join("whatsapp-bridge.log"),
            spawned: Arc::new(std::sync::Mutex::new(None)),
        }
    }

    fn resolve_script() -> Option<PathBuf> {
        let mut candidates = Vec::new();

        if let Ok(explicit) = std::env::var("ODON_WHATSAPP_BRIDGE_SCRIPT") {
            candidates.push(PathBuf::from(explicit));
        }

        if let Ok(resource_dir) = std::env::var("ODON_RESOURCE_DIR") {
            let base = PathBuf::from(&resource_dir);
            // Tauri maps the bundle resource to `<resource_dir>/scripts/whatsapp-bridge`;
            // tolerate a direct bridge directory too so a different bundler layout
            // does not turn into "script not found".
            candidates.push(
                base.join("scripts")
                    .join("whatsapp-bridge")
                    .join("index.mjs"),
            );
            candidates.push(base.join("whatsapp-bridge").join("index.mjs"));
            candidates.push(base.join("index.mjs"));
        }

        if let Ok(exe) = std::env::current_exe() {
            if let Some(parent) = exe.parent() {
                candidates.push(
                    parent
                        .join("scripts")
                        .join("whatsapp-bridge")
                        .join("index.mjs"),
                );
                candidates.push(
                    parent
                        .join("resources")
                        .join("scripts")
                        .join("whatsapp-bridge")
                        .join("index.mjs"),
                );
            }
        }

        let cwd = std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."));
        candidates.push(
            cwd.join("scripts")
                .join("whatsapp-bridge")
                .join("index.mjs"),
        );

        resolve_first_existing(candidates)
    }

    fn child_running(&self) -> bool {
        let mut guard = self.child.child.lock().unwrap();
        if let Some(child) = guard.as_mut() {
            match child.try_wait() {
                Ok(None) => return true,
                _ => {
                    *guard = None;
                }
            }
        }
        false
    }

    fn stop(&self) {
        if let Some(mut child) = self.child.child.lock().unwrap().take() {
            let _ = child.kill();
            let _ = child.wait();
        }
        *self.spawned.lock().unwrap() = None;
    }

    fn base_url(&self) -> String {
        self.base_url
            .lock()
            .map(|value| value.clone())
            .unwrap_or_else(|_| "http://127.0.0.1:4097".to_string())
    }

    /// Shared-secret check for the locally spawned bridge. Timing-safe so the
    /// fixed bridge token cannot be guessed byte-by-byte over loopback.
    fn verify_bridge_token(&self, presented: Option<&str>) -> bool {
        let Some(candidate) = presented else {
            return false;
        };
        if self.token.len() != candidate.len() {
            return false;
        }
        let mut diff = 0u8;
        for (x, y) in self.token.bytes().zip(candidate.bytes()) {
            diff |= x ^ y;
        }
        diff == 0
    }

    fn port(&self) -> u16 {
        self.base_url()
            .rsplit(':')
            .next()
            .and_then(|value| value.parse::<u16>().ok())
            .unwrap_or(4097)
    }

    async fn healthy(&self) -> bool {
        self.client
            .get(format!("{}/health", self.base_url()))
            .timeout(std::time::Duration::from_millis(800))
            .send()
            .await
            .map(|response| response.status().is_success())
            .unwrap_or(false)
    }

    /// `/health` is deliberately unauthenticated, so it cannot tell a bridge
    /// started by this daemon from a stale one using an old token.
    async fn authorized(&self) -> bool {
        self.client
            .get(format!("{}/status", self.base_url()))
            .header("x-bridge-token", &self.token)
            .timeout(std::time::Duration::from_millis(800))
            .send()
            .await
            .map(|response| response.status().is_success())
            .unwrap_or(false)
    }

    fn free_bridge_port(start: u16) -> Option<u16> {
        (start..start.saturating_add(50))
            .find(|port| std::net::TcpListener::bind(("127.0.0.1", *port)).is_ok())
    }

    /// Stop an orphaned bridge from an older daemon process. The PID is only
    /// killed after its command line is confirmed to be this repository's
    /// Node bridge, never an unrelated service that happens to own the port.
    fn stop_external_bridge(&self) -> bool {
        let port = self.port();
        #[cfg(target_os = "windows")]
        {
            let Ok(output) = std::process::Command::new("netstat")
                .args(["-ano", "-p", "tcp"])
                .output()
            else {
                return false;
            };
            let text = String::from_utf8_lossy(&output.stdout);
            let Some(pid) = text.lines().find_map(|line| {
                let parts: Vec<&str> = line.split_whitespace().collect();
                if parts.len() >= 5
                    && parts[1].ends_with(&format!(":{port}"))
                    && parts[3].eq_ignore_ascii_case("LISTENING")
                {
                    parts[4].parse::<u32>().ok()
                } else {
                    None
                }
            }) else {
                return false;
            };
            let query =
                format!("(Get-CimInstance Win32_Process -Filter 'ProcessId={pid}').CommandLine");
            let Ok(process) = std::process::Command::new("powershell")
                .args(["-NoProfile", "-NonInteractive", "-Command", &query])
                .output()
            else {
                return false;
            };
            let cmdline = String::from_utf8_lossy(&process.stdout).to_lowercase();
            if !cmdline.contains("whatsapp-bridge") || !cmdline.contains("node") {
                return false;
            }
            return std::process::Command::new("taskkill")
                .args(["/PID", &pid.to_string(), "/F"])
                .status()
                .map(|status| status.success())
                .unwrap_or(false);
        }
        #[cfg(unix)]
        {
            let Ok(output) = std::process::Command::new("lsof")
                .args(["-ti", &format!("tcp:{port}")])
                .output()
            else {
                return false;
            };
            let stdout = String::from_utf8_lossy(&output.stdout).into_owned();
            let Some(pid) = stdout
                .lines()
                .next()
                .map(str::trim)
                .filter(|value| !value.is_empty())
            else {
                return false;
            };
            let Ok(process) = std::process::Command::new("ps")
                .args(["-p", pid, "-o", "command="])
                .output()
            else {
                return false;
            };
            if !String::from_utf8_lossy(&process.stdout).contains("whatsapp-bridge") {
                return false;
            }
            return std::process::Command::new("kill")
                .args(["-TERM", pid])
                .status()
                .map(|status| status.success())
                .unwrap_or(false);
        }
        #[allow(unreachable_code)]
        false
    }

    async fn ensure_started(
        &self,
        allowed_numbers: &[String],
        allow_anyone: bool,
        phone: Option<&str>,
    ) -> Result<(), String> {
        let desired = (allowed_numbers.to_vec(), allow_anyone);
        let unchanged = self
            .spawned
            .lock()
            .unwrap()
            .as_ref()
            .map(|current| current == &desired)
            .unwrap_or(false);

        if self.child_running() && unchanged {
            return Ok(());
        }
        if self.child_running() {
            // Allowlist changed: restart so the bridge sees the new env.
            self.stop();
        }
        // Reuse an externally managed bridge only when it accepts our token.
        // A stale bridge from a previous daemon run keeps the old token; stop
        // it first, then fall back to a free port if it cannot be stopped.
        if self.healthy().await && self.spawned.lock().unwrap().is_none() {
            if self.authorized().await {
                return Ok(());
            }
            // A bridge launched by an older daemon keeps the old token. Stop
            // it (after verifying the process) so only one Baileys instance
            // uses the auth directory.
            if self.stop_external_bridge() {
                for _ in 0..20 {
                    if !self.healthy().await {
                        break;
                    }
                    tokio::time::sleep(std::time::Duration::from_millis(100)).await;
                }
            }
            if self.healthy().await {
                let Some(next_port) = Self::free_bridge_port(self.port().saturating_add(1)) else {
                    return Err(
                        "A WhatsApp bridge is already running with a different auth token, and no free local port is available. Stop the existing bridge process and retry."
                            .to_string(),
                    );
                };
                if let Ok(mut base) = self.base_url.lock() {
                    *base = format!("http://127.0.0.1:{next_port}");
                }
            }
        }
        self.stop();

        let script = self.script.clone().ok_or_else(|| {
            // The bridge ships inside the installed app (Tauri resource
            // `scripts/whatsapp-bridge`). Seeing this on a packaged install means
            // the app predates the bundled bridge, so tell the operator to
            // reinstall instead of hunting for a file that is not there.
            "WhatsApp bridge script not found. Expected scripts/whatsapp-bridge/index.mjs ".to_string()
                + "inside the app resources. Reinstall the desktop app, or set "
                + "ODON_WHATSAPP_BRIDGE_SCRIPT to a bridge checkout for development."
        })?;
        if !script.exists() {
            return Err(format!(
                "WhatsApp bridge script not found at {}. Reinstall the desktop app, or set ODON_WHATSAPP_BRIDGE_SCRIPT to a bridge checkout.",
                script.display()
            ));
        }
        if let Some(parent) = self.log_path.parent() {
            let _ = std::fs::create_dir_all(parent);
        }
        let _ = std::fs::create_dir_all(&self.auth_dir);
        let log = std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(&self.log_path)
            .map_err(|error| format!("Cannot open bridge log: {error}"))?;
        let log_err = log
            .try_clone()
            .map_err(|error| format!("Cannot clone bridge log handle: {error}"))?;

        let mut command = std::process::Command::new("node");
        command
            .arg(&script)
            .current_dir(script.parent().unwrap_or(std::path::Path::new(".")))
            .env("WA_BRIDGE_PORT", self.port().to_string())
            .env("WA_AUTH_DIR", &self.auth_dir)
            .env("WA_BRIDGE_TOKEN", &self.token)
            .env("ODON_DAEMON_URL", &self.daemon_url)
            .env("WA_ALLOWED_NUMBERS", allowed_numbers.join(","))
            .env("WA_ALLOW_ANYONE", if allow_anyone { "1" } else { "0" })
            .env("WA_BRIDGE_LOG_LEVEL", "silent")
            .stdout(std::process::Stdio::from(log))
            .stderr(std::process::Stdio::from(log_err))
            .stdin(std::process::Stdio::null());
        if let Some(api_token) = &self.api_token {
            command.env("ODON_API_TOKEN", api_token);
        }
        if let Some(phone) = phone {
            command.env("WA_PAIRING_PHONE", phone);
        }
        #[cfg(target_os = "windows")]
        {
            use std::os::windows::process::CommandExt;
            command.creation_flags(0x08000000);
        }

        let child = command.spawn().map_err(|error| {
            format!("Failed to start the WhatsApp bridge (Node.js required): {error}")
        })?;
        *self.child.child.lock().unwrap() = Some(child);
        *self.spawned.lock().unwrap() = Some(desired);

        for _ in 0..30 {
            if self.healthy().await {
                return Ok(());
            }
            tokio::time::sleep(std::time::Duration::from_millis(250)).await;
        }
        Err(format!(
            "WhatsApp bridge did not start. Install dependencies with `cd scripts/whatsapp-bridge && npm install`, then retry. Log: {}",
            self.log_path.display()
        ))
    }

    async fn request(
        &self,
        method: reqwest::Method,
        path: &str,
        body: Option<Value>,
    ) -> Result<Value, String> {
        let mut request = self
            .client
            .request(method, format!("{}{}", self.base_url(), path))
            .header("x-bridge-token", &self.token)
            .timeout(std::time::Duration::from_secs(15));
        if let Some(body) = body {
            request = request.json(&body);
        }
        let response = request
            .send()
            .await
            .map_err(|error| format!("WhatsApp bridge unreachable: {error}"))?;
        let status = response.status();
        let value: Value = response.json().await.unwrap_or_else(|_| json!({}));
        if !status.is_success() {
            return Err(value
                .get("error")
                .and_then(|error| error.as_str())
                .unwrap_or("WhatsApp bridge error")
                .to_string());
        }
        Ok(value)
    }

    async fn status(&self) -> Value {
        if !self.healthy().await {
            return json!({ "running": false, "connected": false });
        }
        self.request(reqwest::Method::GET, "/status", None)
            .await
            .unwrap_or_else(|error| json!({ "running": true, "connected": false, "error": error }))
    }

    async fn start(
        &self,
        phone: Option<&str>,
        allowed_numbers: &[String],
        allow_anyone: bool,
    ) -> Result<Value, String> {
        self.ensure_started(allowed_numbers, allow_anyone, phone)
            .await?;
        self.request(
            reqwest::Method::POST,
            "/start",
            Some(json!({ "phone": phone })),
        )
        .await
    }

    async fn logout(&self) -> Result<Value, String> {
        if !self.healthy().await {
            return Ok(json!({ "running": false, "connected": false }));
        }
        self.request(reqwest::Method::POST, "/logout", None).await
    }

    async fn send(&self, to: &str, text: &str) -> Result<(), String> {
        self.request(
            reqwest::Method::POST,
            "/send",
            Some(json!({ "to": to, "text": text })),
        )
        .await
        .map(|_| ())
    }

    async fn send_media(
        &self,
        to: &str,
        file_name: &str,
        mime_type: &str,
        data_base64: &str,
        caption: Option<&str>,
    ) -> Result<(), String> {
        let response = self
            .client
            .post(format!("{}/send-media", self.base_url()))
            .header("x-bridge-token", &self.token)
            .timeout(std::time::Duration::from_secs(120))
            .json(&json!({
                "to": to,
                "file_name": file_name,
                "mime": mime_type,
                "data": data_base64,
                "caption": caption
            }))
            .send()
            .await
            .map_err(|error| format!("WhatsApp bridge media send failed: {error}"))?;
        let status = response.status();
        let body: Value = response.json().await.unwrap_or_else(|_| json!({}));
        if !status.is_success() {
            return Err(body
                .get("error")
                .and_then(Value::as_str)
                .unwrap_or("WhatsApp bridge media send failed")
                .to_string());
        }
        Ok(())
    }
}

fn oauth_base_dir() -> PathBuf {
    std::env::var_os("ODON_HOME")
        .map(PathBuf::from)
        .or_else(|| std::env::var_os("USERPROFILE").map(PathBuf::from))
        .or_else(|| std::env::var_os("HOME").map(PathBuf::from))
        .unwrap_or_else(|| PathBuf::from("."))
}

fn get_oauth_tokens_path() -> PathBuf {
    oauth_base_dir().join(".odon").join("oauth-tokens.enc")
}

fn legacy_oauth_tokens_path() -> PathBuf {
    oauth_base_dir().join(".odon").join("oauth-tokens.json")
}

fn oauth_key_file_path() -> PathBuf {
    oauth_base_dir().join(".odon").join("oauth.key")
}

/// Best-effort 0600 on unix. Windows inherits the profile directory DACL.
fn restrict_file(path: &std::path::Path) {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let _ = std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o600));
    }
    #[cfg(not(unix))]
    {
        let _ = path;
    }
}

/// Magic prefix that marks an encrypted token store.
const OAUTH_MAGIC: &[u8] = b"ODONOAUTH1";
const OAUTH_NONCE_LEN: usize = 12;

/// 32-byte AEAD key for the token store. Prefers `ODON_OAUTH_KEY` so a token
/// file leaked on its own is useless; otherwise falls back to a random key
/// generated once at `~/.odon/oauth.key` so encryption works with no setup.
fn oauth_encryption_key() -> [u8; 32] {
    let ikm: Vec<u8> = match std::env::var_os("ODON_OAUTH_KEY") {
        Some(value) if !value.is_empty() => value.to_string_lossy().into_owned().into_bytes(),
        _ => load_or_create_key_file(),
    };
    let salt = ring::hkdf::Salt::new(ring::hkdf::HKDF_SHA256, b"odon-oauth-store-salt-v1");
    let prk = salt.extract(&ikm);
    let okm = prk
        .expand(&[b"odon-oauth-store-key-v1"], ring::hkdf::HKDF_SHA256)
        .expect("HKDF expand with one info chunk cannot fail");
    let mut key = [0u8; 32];
    okm.fill(&mut key).expect("32-byte HKDF output fits");
    key
}

fn load_or_create_key_file() -> Vec<u8> {
    let path = oauth_key_file_path();
    if let Ok(bytes) = std::fs::read(&path) {
        if bytes.len() >= 32 {
            return bytes;
        }
    }
    let mut key = vec![0u8; 32];
    if ring::rand::SystemRandom::new().fill(&mut key).is_err() {
        tracing::error!("OS random source unavailable; OAuth tokens cannot be encrypted");
        return key;
    }
    if let Some(parent) = path.parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    if std::fs::write(&path, &key).is_ok() {
        restrict_file(&path);
    }
    key
}

fn encrypt_oauth_blob(plaintext: &[u8]) -> Option<Vec<u8>> {
    encrypt_oauth_blob_with_key(&oauth_encryption_key(), plaintext)
}

fn encrypt_oauth_blob_with_key(key: &[u8; 32], plaintext: &[u8]) -> Option<Vec<u8>> {
    let unbound = ring::aead::UnboundKey::new(&ring::aead::CHACHA20_POLY1305, key).ok()?;
    let less_safe = ring::aead::LessSafeKey::new(unbound);
    let mut nonce_bytes = [0u8; OAUTH_NONCE_LEN];
    ring::rand::SystemRandom::new()
        .fill(&mut nonce_bytes)
        .ok()?;
    let nonce = ring::aead::Nonce::assume_unique_for_key(nonce_bytes);
    let mut in_out = plaintext.to_vec();
    less_safe
        .seal_in_place_append_tag(nonce, ring::aead::Aad::empty(), &mut in_out)
        .ok()?;
    let mut blob = Vec::with_capacity(OAUTH_MAGIC.len() + OAUTH_NONCE_LEN + in_out.len());
    blob.extend_from_slice(OAUTH_MAGIC);
    blob.extend_from_slice(&nonce_bytes);
    blob.extend_from_slice(&in_out);
    Some(blob)
}

fn decrypt_oauth_blob(blob: &[u8]) -> Option<Vec<u8>> {
    decrypt_oauth_blob_with_key(&oauth_encryption_key(), blob)
}

fn decrypt_oauth_blob_with_key(key: &[u8; 32], blob: &[u8]) -> Option<Vec<u8>> {
    let rest = blob.strip_prefix(OAUTH_MAGIC)?;
    if rest.len() < OAUTH_NONCE_LEN {
        return None;
    }
    let (nonce_bytes, ciphertext) = rest.split_at(OAUTH_NONCE_LEN);
    let nonce_arr: [u8; OAUTH_NONCE_LEN] = nonce_bytes.try_into().ok()?;
    let unbound = ring::aead::UnboundKey::new(&ring::aead::CHACHA20_POLY1305, key).ok()?;
    let less_safe = ring::aead::LessSafeKey::new(unbound);
    let nonce = ring::aead::Nonce::assume_unique_for_key(nonce_arr);
    let mut in_out = ciphertext.to_vec();
    let plaintext = less_safe
        .open_in_place(nonce, ring::aead::Aad::empty(), &mut in_out)
        .ok()?;
    Some(plaintext.to_vec())
}

fn persist_oauth_tokens(tokens: &HashMap<String, odon_provider::OAuthToken>) -> bool {
    let path = get_oauth_tokens_path();
    if let Some(parent) = path.parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    let Ok(plaintext) = serde_json::to_vec(tokens) else {
        return false;
    };
    let Some(blob) = encrypt_oauth_blob(&plaintext) else {
        tracing::error!("Failed to encrypt OAuth token store; tokens were not saved");
        return false;
    };
    if std::fs::write(&path, blob).is_ok() {
        restrict_file(&path);
        true
    } else {
        false
    }
}

fn load_oauth_tokens() -> HashMap<String, odon_provider::OAuthToken> {
    if let Ok(blob) = std::fs::read(get_oauth_tokens_path()) {
        if let Some(plaintext) = decrypt_oauth_blob(&blob) {
            return serde_json::from_slice(&plaintext).unwrap_or_default();
        }
        // A file without our magic that begins with `{` is a plaintext store.
        if blob.first() == Some(&b'{') {
            return serde_json::from_slice(&blob).unwrap_or_default();
        }
        return HashMap::new();
    }
    // First encrypted load: migrate the legacy plaintext JSON store if present.
    if let Ok(content) = std::fs::read_to_string(legacy_oauth_tokens_path()) {
        if let Ok(tokens) =
            serde_json::from_str::<HashMap<String, odon_provider::OAuthToken>>(&content)
        {
            if !tokens.is_empty() {
                // Only drop the legacy plaintext file once the encrypted
                // store is safely on disk, so a failed migration never
                // destroys the only copy of the tokens.
                if persist_oauth_tokens(&tokens) {
                    let _ = std::fs::remove_file(legacy_oauth_tokens_path());
                }
            }
            return tokens;
        }
    }
    HashMap::new()
}

fn save_oauth_token(catalog_id: &str, token: &odon_provider::OAuthToken) {
    let mut tokens = load_oauth_tokens();
    tokens.insert(catalog_id.to_string(), token.clone());
    persist_oauth_tokens(&tokens);
}

fn remove_oauth_token(catalog_id: &str) {
    let mut tokens = load_oauth_tokens();
    if tokens.remove(catalog_id).is_some() {
        persist_oauth_tokens(&tokens);
    }
}

/// Build the runtime adapter for a persisted OAuth token. Returns None for
/// unknown catalog ids.
fn build_oauth_provider(
    catalog_id: &str,
    token: &odon_provider::OAuthToken,
) -> Option<Arc<dyn odon_provider::ModelProvider>> {
    use odon_provider::{AnthropicProvider, OpenAiCodexProvider};
    match catalog_id {
        "chatgpt" => Some(Arc::new(OpenAiCodexProvider::new(
            token.access_token.clone(),
            token.account_id.clone(),
            None,
        ))),
        "claude-oauth" => {
            let mut headers = reqwest::header::HeaderMap::new();
            headers.insert(
                "anthropic-beta",
                reqwest::header::HeaderValue::from_static(
                    "claude-code-20250219,oauth-2025-04-20,interleaved-thinking-2025-05-14",
                ),
            );
            headers.insert(
                "anthropic-dangerous-direct-browser-access",
                reqwest::header::HeaderValue::from_static("true"),
            );
            headers.insert("x-app", reqwest::header::HeaderValue::from_static("cli"));
            headers.insert(
                reqwest::header::USER_AGENT,
                reqwest::header::HeaderValue::from_static("claude-cli/2.1.280 (external, sdk-cli)"),
            );
            Some(Arc::new(
                AnthropicProvider::with_bearer_auth(
                    token.access_token.clone(),
                    "https://api.anthropic.com/v1",
                )
                .with_label("claude-oauth")
                .with_extra_headers(headers),
            ))
        }
        "antigravity" => Some(Arc::new(odon_provider::AntigravityProvider::new(
            token.access_token.clone(),
            token.account_id.clone(),
            None,
        ))),
        "github-copilot" => Some(Arc::new(OpenAiCompatibleProvider::new(
            "https://api.githubcopilot.com",
            token.access_token.clone(),
            Some("github-copilot"),
        ))),
        "xai-oauth" => Some(Arc::new(OpenAiCompatibleProvider::new(
            "https://api.x.ai/v1",
            token.access_token.clone(),
            Some("xai-oauth"),
        ))),
        _ => None,
    }
}

fn save_automations_to_disk(list: &[AutomationJob]) {
    let path = get_automations_path();
    if let Some(parent) = path.parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    if let Ok(data) = serde_json::to_string_pretty(list) {
        let _ = std::fs::write(&path, data);
    }
}

fn parse_interval(value: &str) -> Option<std::time::Duration> {
    let normalized = value.trim().to_lowercase();
    let normalized = normalized
        .strip_prefix("every ")
        .unwrap_or(normalized.as_str())
        .trim()
        .to_string();
    match normalized.as_str() {
        "hourly" => return Some(std::time::Duration::from_secs(3600)),
        "daily" => return Some(std::time::Duration::from_secs(86_400)),
        "weekly" => return Some(std::time::Duration::from_secs(604_800)),
        _ => {}
    }
    let split = normalized.find(|ch: char| !ch.is_ascii_digit())?;
    let amount = normalized[..split].parse::<u64>().ok()?.max(1);
    let unit = normalized[split..].trim();
    let seconds = if unit.starts_with('s') {
        1
    } else if unit.starts_with('m') {
        60
    } else if unit.starts_with('h') {
        3600
    } else if unit.starts_with('d') {
        86_400
    } else {
        return None;
    };
    Some(std::time::Duration::from_secs(amount * seconds))
}

fn cron_field_matches(field: &str, value: u32, min: u32) -> bool {
    if field == "*" {
        return true;
    }
    field.split(',').any(|part| {
        if let Some(step) = part.strip_prefix("*/") {
            step.parse::<u32>()
                .ok()
                .filter(|step| *step > 0)
                .map(|step| value >= min && (value - min).is_multiple_of(step))
                .unwrap_or(false)
        } else if let Some((start, end)) = part.split_once('-') {
            match (start.parse::<u32>(), end.parse::<u32>()) {
                (Ok(start), Ok(end)) => value >= start && value <= end,
                _ => false,
            }
        } else {
            part.parse::<u32>()
                .map(|expected| {
                    if expected == 7 {
                        value == 0
                    } else {
                        expected == value
                    }
                })
                .unwrap_or(false)
        }
    })
}

fn normalize_cron_expression(value: &str) -> String {
    let fields: Vec<&str> = value.split_whitespace().collect();
    if fields.len() >= 5 {
        fields[..5].join(" ")
    } else {
        value.trim().to_string()
    }
}

fn parse_last_run(value: &str) -> Option<chrono::DateTime<Local>> {
    if let Ok(parsed) = chrono::DateTime::parse_from_rfc3339(value.trim()) {
        return Some(parsed.with_timezone(&Local));
    }
    let lower = value.trim().to_lowercase();
    let now = Local::now();
    if let Some(rest) = lower.strip_suffix(" ago") {
        let mut parts = rest.split_whitespace();
        let amount = parts.next()?.parse::<i64>().ok()?;
        let unit = parts.next().unwrap_or("");
        let seconds = if unit.starts_with("min") {
            60
        } else if unit.starts_with("hour") {
            3600
        } else if unit.starts_with("day") {
            86_400
        } else {
            return None;
        };
        return Some(now - chrono::Duration::seconds(amount * seconds));
    }
    if lower.starts_with("yesterday") {
        let time_part = lower.split(" at ").nth(1)?;
        let time = chrono::NaiveTime::parse_from_str(time_part.trim(), "%I:%M %p").ok()?;
        let date = (now - chrono::Duration::days(1)).date_naive();
        return Local.from_local_datetime(&date.and_time(time)).single();
    }
    None
}

fn cron_matches(expression: &str, now: &chrono::DateTime<Local>) -> bool {
    let expression = normalize_cron_expression(expression);
    let expression = expression.as_str();
    let fields: Vec<&str> = expression.split_whitespace().collect();
    if fields.len() != 5 {
        return false;
    }
    if !cron_field_matches(fields[0], now.minute(), 0)
        || !cron_field_matches(fields[1], now.hour(), 0)
        || !cron_field_matches(fields[3], now.month(), 1)
    {
        return false;
    }
    let dom_matches = cron_field_matches(fields[2], now.day(), 1);
    let dow = now.weekday().num_days_from_sunday();
    let dow_matches = cron_field_matches(fields[4], dow, 0);
    if fields[2] == "*" || fields[4] == "*" {
        dom_matches && dow_matches
    } else {
        dom_matches || dow_matches
    }
}

fn automation_dependencies_satisfied(job: &AutomationJob, jobs: &[AutomationJob]) -> bool {
    job.depends_on.iter().all(|dependency| {
        jobs.iter()
            .find(|candidate| candidate.id == *dependency)
            .and_then(|candidate| candidate.last_status.as_deref())
            == Some("success")
    })
}

fn automation_due(
    job: &AutomationJob,
    jobs: &[AutomationJob],
    now: &chrono::DateTime<Local>,
) -> bool {
    if !job.enabled || job.last_status.as_deref() == Some("running") {
        return false;
    }
    if !automation_dependencies_satisfied(job, jobs) {
        return false;
    }
    let last_run = job.last_run.as_deref().and_then(parse_last_run);
    match job.trigger_type.as_str() {
        "interval" => parse_interval(&job.trigger_value)
            .map(|interval| {
                last_run
                    .map(|last| {
                        now.signed_duration_since(last).num_seconds() >= interval.as_secs() as i64
                    })
                    .unwrap_or(true)
            })
            .unwrap_or(false),
        "cron" => {
            if !cron_matches(&job.trigger_value, now) {
                return false;
            }
            match last_run {
                Some(last) => {
                    last.format("%Y-%m-%d %H:%M").to_string()
                        != now.format("%Y-%m-%d %H:%M").to_string()
                }
                None => true,
            }
        }
        _ => false,
    }
}

async fn run_automation_job(state: AppState, job: AutomationJob) {
    let started = Utc::now().to_rfc3339();
    let max_attempts = job.retry_count.saturating_add(1);
    let mut attempts = 0;
    let mut output = String::new();
    let mut success = false;

    for attempt in 1..=max_attempts {
        attempts = attempt;
        let session_id = format!(
            "auto_{}_{}_{}",
            job.id,
            Utc::now().timestamp_millis(),
            attempt
        );
        let cancel = tokio_util::sync::CancellationToken::new();
        match state
            .runtime
            .execute_turn(&session_id, &job.prompt, Some(&job.model), cancel)
            .await
        {
            Ok(turn) => {
                success = true;
                output = turn.content.text;
                break;
            }
            Err(error) => {
                output = error.to_string();
                if attempt < max_attempts {
                    let delay = job.retry_delay_secs.max(1) * attempt as u64;
                    tokio::time::sleep(std::time::Duration::from_secs(delay)).await;
                }
            }
        }
    }

    let status = if success { "success" } else { "failed" };
    if !success {
        send_automation_failure_alert(&state, &job, &output).await;
    }
    {
        let mut jobs = state.automations.write().await;
        if let Some(existing) = jobs.iter_mut().find(|candidate| candidate.id == job.id) {
            existing.last_run = Some(started.clone());
            existing.last_status = Some(status.to_string());
        }
        save_automations_to_disk(&jobs);
    }
    {
        let mut logs = state.automation_logs.write().await;
        logs.insert(
            0,
            AutomationLog {
                id: uuid::Uuid::new_v4().to_string(),
                automation_id: job.id.clone(),
                automation_name: job.name.clone(),
                triggered_at: started,
                completed_at: Some(Utc::now().to_rfc3339()),
                status: status.to_string(),
                output: output.clone(),
                attempts,
                error: if success { None } else { Some(output) },
            },
        );
        if logs.len() > 100 {
            logs.truncate(100);
        }
        save_automation_logs_to_disk(&logs);
    }
}

/// Candidate locations for the built web UI (`desktop/dist`), in
/// priority order: the ODON_DESKTOP_DIST override, the dev tree under
/// the working directory, and the layouts an installed bundle uses
/// (next to the binary, and the macOS Resources sibling directory).
fn desktop_dist_candidates(
    env_dist: Option<&str>,
    exe_dir: Option<&std::path::Path>,
) -> Vec<std::path::PathBuf> {
    let mut candidates = Vec::new();
    if let Some(custom) = env_dist.filter(|value| !value.trim().is_empty()) {
        candidates.push(std::path::PathBuf::from(custom));
    }
    candidates.push(std::path::PathBuf::from("desktop/dist"));
    if let Some(dir) = exe_dir {
        candidates.push(dir.join("desktop/dist"));
        candidates.push(dir.join("../Resources/desktop/dist"));
    }
    candidates
}

fn relay_mime_for_path(path: &std::path::Path) -> &'static str {
    match path
        .extension()
        .and_then(|value| value.to_str())
        .unwrap_or_default()
        .to_ascii_lowercase()
        .as_str()
    {
        "png" => "image/png",
        "jpg" | "jpeg" => "image/jpeg",
        "webp" => "image/webp",
        "gif" => "image/gif",
        "pdf" => "application/pdf",
        "md" | "markdown" => "text/markdown",
        "txt" => "text/plain",
        "csv" => "text/csv",
        "json" => "application/json",
        "docx" => "application/vnd.openxmlformats-officedocument.wordprocessingml.document",
        "xlsx" => "application/vnd.openxmlformats-officedocument.spreadsheetml.sheet",
        "pptx" => "application/vnd.openxmlformats-officedocument.presentationml.presentation",
        _ => "application/octet-stream",
    }
}

fn sanitize_relay_file_name(value: &str, fallback_extension: &str) -> String {
    let mut name: String = value
        .chars()
        .filter(|ch| ch.is_ascii_alphanumeric() || matches!(ch, ' ' | '-' | '_' | '.' | '(' | ')'))
        .collect();
    if name.trim().is_empty() {
        name = "odon-output".to_string();
    }
    if !name.contains('.') {
        name.push_str(fallback_extension);
    }
    name
}

/// Confine relay artifact reads to allowed workspace roots. A hostile
/// artifact payload must not be able to read arbitrary absolute paths
/// (e.g. ~/.ssh/id_rsa) into a Telegram/WhatsApp attachment.
async fn workspace_artifact_path(state: &AppState, candidate: &str) -> Option<PathBuf> {
    let path = PathBuf::from(candidate);
    let path = if path.is_absolute() {
        path
    } else {
        PathBuf::from(&state.workspace_root).join(candidate.trim_start_matches('/'))
    };
    let canonical = std::fs::canonicalize(&path).ok()?;
    for root in workspace_roots(state).await {
        if let Ok(canonical_root) = std::fs::canonicalize(&root) {
            if canonical.starts_with(&canonical_root) && canonical.is_file() {
                return Some(canonical);
            }
        }
    }
    None
}

async fn relay_artifact_file(
    state: &AppState,
    artifact_id: &str,
) -> Option<(String, String, Vec<u8>)> {
    let artifact = state.storage.get_artifact(artifact_id).ok()?;
    match artifact.payload {
        ArtifactPayload::Image {
            file_path,
            preview_url,
            ..
        } => {
            let mut resolved: Option<PathBuf> = None;
            if let Some(candidate) = file_path.as_deref() {
                resolved = workspace_artifact_path(state, candidate).await;
            }
            if resolved.is_none() {
                if let Some(candidate) = preview_url
                    .as_deref()
                    .and_then(|url| url.strip_prefix("/api/files/"))
                {
                    resolved = workspace_artifact_path(state, candidate).await;
                }
            }
            let path = resolved?;
            // Cap relay attachments at 20 MiB.
            const MAX_RELAY_FILE_BYTES: u64 = 20 * 1024 * 1024;
            let metadata = tokio::fs::metadata(&path).await.ok()?;
            if metadata.len() > MAX_RELAY_FILE_BYTES {
                return None;
            }
            let bytes = tokio::fs::read(&path).await.ok()?;
            let mime = relay_mime_for_path(&path);
            Some((
                sanitize_relay_file_name(&artifact.title, ".png"),
                mime.to_string(),
                bytes,
            ))
        }
        ArtifactPayload::Document { markdown, .. } => Some((
            sanitize_relay_file_name(&artifact.title, ".md"),
            "text/markdown".to_string(),
            markdown.into_bytes(),
        )),
        _ => None,
    }
}

async fn collect_relay_attachments(
    state: &AppState,
    turn: &Message,
) -> Vec<(String, String, Vec<u8>)> {
    let mut files = Vec::new();
    for attachment in turn.content.attachments.iter() {
        if let Ok(bytes) = base64::Engine::decode(
            &base64::engine::general_purpose::STANDARD,
            &attachment.data_base64,
        ) {
            files.push((
                sanitize_relay_file_name(&attachment.name, ".bin"),
                attachment.mime_type.clone(),
                bytes,
            ));
        }
    }
    for artifact_id in turn.content.artifact_ids.iter().take(4) {
        if let Some(file) = relay_artifact_file(state, artifact_id).await {
            files.push(file);
        }
    }
    files
}

async fn send_relay_attachments(
    state: &AppState,
    channel: &str,
    sender: &str,
    caption: &str,
    files: &[(String, String, Vec<u8>)],
) {
    for (file_name, mime_type, bytes) in files.iter().take(4) {
        let result = match channel {
            "telegram" => {
                let relay = state.telegram.read().await.clone();
                match relay {
                    Some(relay) => {
                        relay
                            .send_relay_file(
                                sender,
                                file_name,
                                mime_type,
                                bytes.clone(),
                                Some(caption),
                            )
                            .await
                    }
                    None => Ok(()),
                }
            }
            "whatsapp" => {
                let cloud = state.whatsapp_cloud.read().await.clone();
                match cloud {
                    Some(cloud) => {
                        cloud
                            .send_relay_file(
                                sender,
                                file_name,
                                mime_type,
                                bytes.clone(),
                                Some(caption),
                            )
                            .await
                    }
                    None => {
                        let data = base64::Engine::encode(
                            &base64::engine::general_purpose::STANDARD,
                            bytes,
                        );
                        state
                            .whatsapp_bridge
                            .send_media(sender, file_name, mime_type, &data, Some(caption))
                            .await
                            .map_err(OdonError::Channel)
                    }
                }
            }
            _ => Ok(()),
        };
        if let Err(error) = result {
            tracing::warn!("Failed to send relay attachment {file_name}: {error}");
        }
    }
}

fn relay_session_key(channel: &str, sender: &str) -> String {
    format!("{channel}:{sender}")
}

fn relay_channel_for_session(session_id: &str) -> Option<(String, String)> {
    if let Some(sender) = session_id.strip_prefix("tg_") {
        return Some(("telegram".to_string(), sender.to_string()));
    }
    if let Some(sender) = session_id.strip_prefix("wa_") {
        return Some(("whatsapp".to_string(), sender.to_string()));
    }
    None
}

async fn send_relay_text(
    state: &AppState,
    channel: &str,
    sender: &str,
    text: &str,
) -> odon_core::Result<()> {
    match channel {
        "telegram" => {
            let relay = state.telegram.read().await.clone();
            match relay {
                Some(relay) => relay.send_direct_message(sender, text).await,
                None => Ok(()),
            }
        }
        "whatsapp" => {
            let cloud = state.whatsapp_cloud.read().await.clone();
            if let Some(cloud) = cloud {
                cloud.send_direct_message(sender, text).await
            } else {
                state
                    .whatsapp_bridge
                    .send(sender, text)
                    .await
                    .map_err(OdonError::Channel)
            }
        }
        _ => Ok(()),
    }
}

fn format_pending_approvals(requests: &[odon_security::ApprovalRequest]) -> String {
    if requests.is_empty() {
        return "✅ No pending approvals.".to_string();
    }
    let mut lines = vec!["🔒 Pending approvals:".to_string()];
    for request in requests {
        lines.push(format!(
            "- `{}` `{}` (Tier {})\n  {}",
            request.id, request.tool_name, request.risk_tier as u8, request.description
        ));
    }
    lines.push("Use `/approve <id-prefix>` or `/deny <id-prefix>`.".to_string());
    lines.join("\n")
}

async fn resolve_relay_approval(
    state: &AppState,
    session_id: &str,
    id_prefix: &str,
    approved: bool,
) -> String {
    let pending = state.approval.list_pending().await;
    let matches: Vec<_> = pending
        .iter()
        .filter(|request| request.session_id == session_id || session_id.is_empty())
        .filter(|request| request.id.starts_with(id_prefix))
        .collect();
    match matches.len() {
        0 => format!("No matching pending approval for `{id_prefix}`. Use /approvals."),
        1 => {
            let request = matches[0];
            let decision = if approved {
                ApprovalDecision::Approved
            } else {
                ApprovalDecision::Rejected {
                    reason: "Rejected from relay".to_string(),
                }
            };
            let resolved = state.approval.submit_decision(&request.id, decision).await;
            if resolved {
                format!(
                    "{} approval `{}` for `{}`.",
                    if approved {
                        "✅ Approved"
                    } else {
                        "❌ Denied"
                    },
                    request.id,
                    request.tool_name
                )
            } else {
                "Approval was already resolved.".to_string()
            }
        }
        _ => "Multiple approvals match that prefix; use a longer ID prefix.".to_string(),
    }
}

async fn relay_models_text(state: &AppState, session_id: &str) -> String {
    let current = state.relay_model.read().await.clone();
    let mut lines = vec![format!(
        "Current relay model: `{}`",
        current.as_deref().unwrap_or("provider default")
    )];
    let configs = state.storage.list_provider_configs().unwrap_or_default();
    for config in configs {
        let models = serde_json::from_str::<Vec<String>>(&config.models_json).unwrap_or_default();
        if models.is_empty() {
            continue;
        }
        lines.push(format!("- `{}`: {}", config.provider_id, models.join(", ")));
    }
    lines.push("Use `/model provider::model-id` to switch this relay session.".to_string());
    let _ = session_id;
    lines.join("\n")
}

async fn relay_status_text(state: &AppState, session_id: &str) -> String {
    let active = state.active_turns.read().await.contains_key(session_id);
    let pending = state.approval.list_pending().await.len();
    let telegram = state
        .telegram_online
        .load(std::sync::atomic::Ordering::SeqCst);
    let whatsapp = state
        .whatsapp_online
        .load(std::sync::atomic::Ordering::SeqCst);
    format!(
        "✅ ODON online\nSession: `{session_id}`\nActive turn: {}\nPending approvals: {pending}\nTelegram: {}\nWhatsApp: {}",
        if active { "running" } else { "idle" },
        if telegram { "online" } else { "offline" },
        if whatsapp { "online" } else { "offline" }
    )
}

async fn handle_relay_message(state: AppState, message: IncomingChannelMessage) {
    let key = relay_session_key(&message.channel, &message.sender_id);
    let session_id = {
        let overrides = state.relay_sessions.read().await;
        overrides
            .get(&key)
            .cloned()
            .or_else(|| message.session_id.clone())
            .unwrap_or_else(|| format!("{}_{}", message.channel, message.sender_id))
    };
    info!(
        "Relay message from {} [{}] in session {}",
        message.channel, message.sender_id, session_id
    );

    let trimmed = message.text.trim();
    if trimmed.starts_with('/') {
        let mut parts = trimmed.split_whitespace();
        let raw_command = parts.next().unwrap_or_default();
        let command = raw_command
            .split('@')
            .next()
            .unwrap_or_default()
            .to_ascii_lowercase();
        let argument = trimmed
            .split_once(char::is_whitespace)
            .map(|(_, rest)| rest.trim().to_string())
            .unwrap_or_default();

        let reply = match command.as_str() {
            "/start" | "/help" => "🤖 *ODON relay commands*\n\n\
                 /new — new session\n\
                 /models — list configured models\n\
                 /model provider::model — switch model\n\
                 /stop — stop the active turn\n\
                 /steer <text> — steer the active turn\n\
                 /approvals — list pending approvals\n\
                 /approve <id-prefix> — approve\n\
                 /deny <id-prefix> — deny\n\
                 /status — relay status"
                .to_string(),
            "/new" | "/reset" => {
                let model = state.relay_model.read().await.clone();
                let session = Session::new(
                    "Relay session",
                    SessionMode::General,
                    model.as_deref().unwrap_or("odon-auto"),
                );
                let _ = state.storage.save_session(&session);
                state
                    .relay_sessions
                    .write()
                    .await
                    .insert(key, session.id.clone());
                format!("🆕 New ODON session: `{}`", session.id)
            }
            "/models" => relay_models_text(&state, &session_id).await,
            "/model" => {
                if argument.is_empty() {
                    let current = state.relay_model.read().await.clone();
                    format!(
                        "Current relay model: `{}`",
                        current.as_deref().unwrap_or("provider default")
                    )
                } else {
                    *state.relay_model.write().await = Some(argument.clone());
                    format!("Model switched to `{argument}` for this relay.")
                }
            }
            "/status" => relay_status_text(&state, &session_id).await,
            "/stop" => {
                let active = state.active_turns.read().await;
                match active.get(&session_id) {
                    Some(handle) => {
                        handle.cancel.cancel();
                        "🛑 Stop requested.".to_string()
                    }
                    None => "No active turn in this session.".to_string(),
                }
            }
            "/steer" => {
                if argument.is_empty() {
                    "Usage: /steer <guidance>".to_string()
                } else {
                    let active = state.active_turns.read().await;
                    match active.get(&session_id) {
                        Some(handle) => {
                            handle.steer.push(SteerMessage {
                                id: uuid::Uuid::new_v4().to_string(),
                                text: argument.clone(),
                                attachments: message.attachments.clone(),
                                priority: false,
                            });
                            "🧭 Steer queued.".to_string()
                        }
                        None => "No active turn; send the message normally.".to_string(),
                    }
                }
            }
            "/approvals" => {
                let pending = state.approval.list_pending().await;
                let session_pending: Vec<_> = pending
                    .into_iter()
                    .filter(|request| request.session_id == session_id)
                    .collect();
                format_pending_approvals(&session_pending)
            }
            "/approve" | "/deny" => {
                if argument.is_empty() {
                    "Usage: /approve <id-prefix> or /deny <id-prefix>".to_string()
                } else {
                    resolve_relay_approval(&state, &session_id, &argument, command == "/approve")
                        .await
                }
            }
            _ => {
                return;
            }
        };

        let _ = send_relay_text(&state, &message.channel, &message.sender_id, &reply).await;
        return;
    }

    // If a relay turn is already running, ordinary messages become steering
    // guidance instead of preempting the turn.
    {
        let active = state.active_turns.read().await;
        if let Some(handle) = active.get(&session_id) {
            handle.steer.push(SteerMessage {
                id: uuid::Uuid::new_v4().to_string(),
                text: message.text.clone(),
                attachments: message.attachments.clone(),
                priority: false,
            });
            drop(active);
            let _ = send_relay_text(
                &state,
                &message.channel,
                &message.sender_id,
                "🧭 Steer queued for the active turn.",
            )
            .await;
            return;
        }
    }

    let model = state.relay_model.read().await.clone();
    let cancel = tokio_util::sync::CancellationToken::new();
    let steer = SteerQueue::new();
    let turn_id = uuid::Uuid::new_v4().to_string();
    {
        let mut turns = state.active_turns.write().await;
        if let Some(previous) = turns.get(&session_id) {
            previous.cancel.cancel();
        }
        turns.insert(
            session_id.clone(),
            TurnHandle {
                turn_id: turn_id.clone(),
                cancel: cancel.clone(),
                steer: steer.clone(),
            },
        );
    }

    let result = state
        .runtime
        .execute_turn_controlled(
            &session_id,
            &message.text,
            message.attachments.clone(),
            model.as_deref(),
            None,
            cancel,
            Some(steer),
            None,
        )
        .await;
    {
        let mut turns = state.active_turns.write().await;
        retire_turn_handle(&mut turns, &session_id, &turn_id);
    }

    let (reply, attachments) = match result {
        Ok(turn) => {
            let files = collect_relay_attachments(&state, &turn).await;
            let text = if turn.content.text.trim().is_empty() {
                "✅ Done.".to_string()
            } else {
                turn.content.text
            };
            (text, files)
        }
        Err(error) => {
            let text = if error.to_string().contains("cancelled") {
                "🛑 Turn stopped.".to_string()
            } else {
                format!("⚠️ ODON error: {error}")
            };
            (text, Vec::new())
        }
    };
    if let Err(error) = send_relay_text(&state, &message.channel, &message.sender_id, &reply).await
    {
        tracing::error!("Failed to deliver {} reply: {error}", message.channel);
    }
    if !attachments.is_empty() {
        send_relay_attachments(
            &state,
            &message.channel,
            &message.sender_id,
            "Generated by ODON",
            &attachments,
        )
        .await;
    }
}

async fn send_automation_failure_alert(state: &AppState, job: &AutomationJob, error: &str) {
    if std::env::var("ODON_AUTOMATION_ALERTS")
        .map(|value| value == "0")
        .unwrap_or(false)
    {
        return;
    }
    let ctx = ToolContext {
        session_id: format!("automation-alert-{}", job.id),
        workspace_root: state.workspace_root.clone(),
        approval_engine: state.approval.clone(),
        audit_logger: state.audit.clone(),
        storage: Some(state.storage.clone()),
    };
    let message: String = error.chars().take(300).collect();
    let _ = odon_tools::shell_ext::NotificationSendTool
        .execute(
            &ctx,
            json!({
                "title": format!("Automation failed: {}", job.name),
                "message": message
            }),
        )
        .await;
}

async fn automation_scheduler_tick(state: &AppState) {
    let mut jobs = state.automations.write().await;
    // Reload under the same lock used by API saves so a tick cannot overwrite
    // a job that was just created or edited from the UI.
    *jobs = load_automations_from_disk();
    let now = Local::now();
    let due: Vec<AutomationJob> = jobs
        .iter()
        .filter(|job| automation_due(job, &jobs, &now))
        .cloned()
        .collect();
    for job in &due {
        if let Some(existing) = jobs.iter_mut().find(|candidate| candidate.id == job.id) {
            existing.last_run = Some(now.to_rfc3339());
            existing.last_status = Some("running".to_string());
        }
    }
    if !due.is_empty() {
        save_automations_to_disk(&jobs);
    }
    drop(jobs);

    for job in due {
        let state = state.clone();
        tokio::spawn(async move {
            run_automation_job(state, job).await;
        });
    }
}

#[derive(Clone)]
struct TurnHandle {
    turn_id: String,
    cancel: tokio_util::sync::CancellationToken,
    steer: SteerQueue,
}

/// Retire a finished turn's handle, but only if it is still the active one.
///
/// A newer message cancels the running turn and installs its own handle for the
/// same session. Removing unconditionally would then delete that newer handle,
/// so the session would report idle (and reject steering) while a turn is still
/// running. Returns true when a handle was removed.
fn retire_turn_handle(
    turns: &mut std::collections::HashMap<String, TurnHandle>,
    session_id: &str,
    turn_id: &str,
) -> bool {
    let is_current = turns
        .get(session_id)
        .map(|handle| handle.turn_id == turn_id)
        .unwrap_or(false);
    if is_current {
        turns.remove(session_id);
    }
    is_current
}

#[derive(Clone)]
struct AppState {
    runtime: Arc<AgentRuntime>,
    storage: Arc<StorageEngine>,
    approval: Arc<ApprovalEngine>,
    #[allow(dead_code)]
    audit: Arc<AuditLogger>,
    tools: Arc<ToolRegistry>,
    event_tx: EventSender,
    workspace_root: String,
    oauth_accounts: Arc<tokio::sync::RwLock<HashMap<String, serde_json::Value>>>,
    automations: Arc<tokio::sync::RwLock<Vec<AutomationJob>>>,
    automation_logs: Arc<tokio::sync::RwLock<Vec<AutomationLog>>>,
    telegram_online: Arc<std::sync::atomic::AtomicBool>,
    whatsapp_online: Arc<std::sync::atomic::AtomicBool>,
    telegram: Arc<tokio::sync::RwLock<Option<Arc<TelegramRelay>>>>,
    whatsapp_cloud: Arc<tokio::sync::RwLock<Option<Arc<WhatsAppChannel>>>>,
    whatsapp_bridge: WhatsAppBridge,
    relay_config: Arc<tokio::sync::RwLock<RelayConfig>>,
    relay_tx: tokio::sync::mpsc::Sender<IncomingChannelMessage>,
    workspaces: Arc<tokio::sync::RwLock<WorkspaceConfig>>,
    api_token: Option<String>,
    /// Ephemeral per-session token minted for a loopback bind that had no
    /// `ODON_API_TOKEN`. `Some` means it may be handed to a same-origin or
    /// first-party caller; it is never set for an operator-supplied token.
    session_token: Option<String>,
    active_turns: Arc<tokio::sync::RwLock<HashMap<String, TurnHandle>>>,
    relay_sessions: Arc<tokio::sync::RwLock<HashMap<String, String>>>,
    relay_model: Arc<tokio::sync::RwLock<Option<String>>>,
    started_at: std::time::Instant,
}

/// Loads `.env` then `.env.local` from the working directory into the process
/// environment. Values already set in the real environment always win, and values
/// are never logged.
fn load_dotenv_files() {
    for name in [".env.local", ".env"] {
        let Ok(text) = std::fs::read_to_string(name) else {
            continue;
        };
        // Tolerate a UTF-8 BOM (Windows editors/PowerShell add one), which would
        // otherwise corrupt the first key.
        let text = text.trim_start_matches('\u{feff}');
        for line in text.lines() {
            let line = line.trim();
            if line.is_empty() || line.starts_with('#') {
                continue;
            }
            let Some((key, value)) = line.split_once('=') else {
                continue;
            };
            let key = key.trim();
            let value = value.trim().trim_matches('"').trim_matches('\'');
            if key.is_empty() || value.is_empty() || std::env::var_os(key).is_some() {
                continue;
            }
            std::env::set_var(key, value);
        }
    }
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    load_dotenv_files();
    tracing_subscriber::fmt()
        .with_env_filter(tracing_subscriber::EnvFilter::from_default_env())
        .init();

    let cli = Cli::parse();

    match cli.command {
        Commands::Status { url, metrics } => {
            let client = reqwest::Client::builder()
                .timeout(std::time::Duration::from_secs(8))
                .build()?;
            let token = std::env::var("ODON_API_TOKEN")
                .ok()
                .filter(|value| !value.trim().is_empty());
            let get_json = |path: &str| {
                let client = client.clone();
                let token = token.clone();
                let endpoint = format!("{}{}", url.trim_end_matches('/'), path);
                async move {
                    let mut request = client.get(endpoint);
                    if let Some(token) = token {
                        request = request.bearer_auth(token);
                    }
                    let response = request.send().await?;
                    if !response.status().is_success() {
                        anyhow::bail!("daemon returned HTTP {}", response.status());
                    }
                    Ok::<Value, anyhow::Error>(response.json().await?)
                }
            };
            let status = get_json("/api/status").await.map_err(|error| {
                anyhow::anyhow!(
                    "Cannot reach odond at {url}: {error}. Start it with `odond serve`."
                )
            })?;
            println!("{}", serde_json::to_string_pretty(&status)?);
            if metrics {
                let metrics = get_json("/api/metrics").await?;
                println!("{}", serde_json::to_string_pretty(&metrics)?);
            }
        }
        Commands::Init => {
            println!("Initializing ODON SQLite storage...");
            let _storage = StorageEngine::default_storage()?;
            println!("Storage initialized successfully at default location.");
        }
        Commands::Serve {
            bind,
            telegram_token,
            workspace,
        } => {
            println!("====================================================");
            println!("   ODON Daemon (odond) - Starting Services");
            println!(
                "   Open Digital Orchestration Network v{}",
                env!("CARGO_PKG_VERSION")
            );
            println!("====================================================");
            info!("Workspace root: {}", workspace);

            // 1. Storage
            let storage = Arc::new(StorageEngine::default_storage()?);

            // 2. Event Bus (created first so approvals can publish on it)
            // Headroom for a burst of token/tool events. A web search or fetch result
            // can make the UI stall while it parses and renders, and a 512-slot ring
            // overflowed during that window: the SSE subscriber lagged and the events
            // it missed could include the turn's terminal event, leaving the UI stuck
            // in "running" with a healthy backend.
            let (event_tx, _event_rx) = create_event_bus(8192);

            // 3. Security & Policy
            let policy = SecurityPolicy::default();
            let approval = Arc::new(ApprovalEngine::new(policy).with_events(event_tx.clone()));
            let audit = Arc::new(AuditLogger::new());

            // 4. Tools Registry
            let tools = Arc::new(odon_tools::build_full_tool_registry());

            // 5. Agent Runtime
            let runtime = Arc::new(AgentRuntime::new(
                storage.clone(),
                tools.clone(),
                approval.clone(),
                audit.clone(),
                event_tx.clone(),
                workspace.clone(),
            ));

            // 6. OAuth and Provider State
            let oauth_accounts = Arc::new(tokio::sync::RwLock::new(HashMap::new()));

            // 7. Initialize Real Providers (Host Discovery, BYOK DB, Env Vars)
            init_providers(&runtime, &storage, &oauth_accounts).await;

            let configured_token = std::env::var("ODON_API_TOKEN")
                .ok()
                .map(|value| value.trim().to_string())
                .filter(|value| !value.is_empty());
            let bind_host = if let Some(rest) = bind.strip_prefix('[') {
                rest.split(']').next().unwrap_or("")
            } else {
                bind.rsplit_once(':')
                    .map(|(host, _)| host)
                    .unwrap_or(bind.as_str())
            };
            let is_loopback = matches!(bind_host, "127.0.0.1" | "localhost" | "::1");
            if !is_loopback && configured_token.is_none() {
                return Err(anyhow::anyhow!(
                    "Refusing to bind {bind} without ODON_API_TOKEN. Set ODON_API_TOKEN to protect a network-exposed daemon, or bind 127.0.0.1."
                ));
            }
            // Only a loopback bind may fall back to a minted token, and only
            // after the refusal above, so a network-exposed daemon can never
            // end up with an ephemeral secret.
            let (api_token, ephemeral_session_token) =
                mint_loopback_session_token(is_loopback, configured_token);
            // The token file is shared with the desktop shell, which reads it
            // to authenticate the UI. It must only be published by a daemon
            // that actually owns the port: writing it before the bind means a
            // second launch (or a failed launch on a busy port) replaces the
            // running daemon's token and every UI request starts failing 401.
            let pending_loopback_token = if ephemeral_session_token {
                api_token.clone()
            } else {
                None
            };
            if api_token.is_some() {
                println!("✔ API token authentication enabled");
            }

            // 8. Channel Relays (Telegram polling + WhatsApp Cloud/Web)
            let (relay_tx, mut relay_rx) =
                tokio::sync::mpsc::channel::<IncomingChannelMessage>(128);

            let mut relay_config = load_relay_config();
            if relay_config.telegram.is_none() {
                let token = telegram_token
                    .clone()
                    .or_else(|| std::env::var("TELEGRAM_BOT_TOKEN").ok())
                    .filter(|value| !value.trim().is_empty());
                if let Some(token) = token {
                    let allowed_users = std::env::var("TELEGRAM_ALLOWED_USERS")
                        .ok()
                        .map(|value| {
                            value
                                .split(',')
                                .map(|item| item.trim().to_string())
                                .filter(|item| !item.is_empty())
                                .collect::<Vec<_>>()
                        })
                        .unwrap_or_default();
                    relay_config.telegram = Some(TelegramRelayConfig {
                        token,
                        allowed_users,
                    });
                }
            }
            let relay_config = Arc::new(tokio::sync::RwLock::new(relay_config));

            let telegram_handle: Arc<tokio::sync::RwLock<Option<Arc<TelegramRelay>>>> =
                Arc::new(tokio::sync::RwLock::new(None));
            let whatsapp_cloud_handle: Arc<tokio::sync::RwLock<Option<Arc<WhatsAppChannel>>>> =
                Arc::new(tokio::sync::RwLock::new(None));
            let whatsapp_bridge = WhatsAppBridge::new(&bind, api_token.clone());

            {
                let config = relay_config.read().await;
                if let Some(telegram) = &config.telegram {
                    println!("Connecting Telegram Bot Relay...");
                    let relay = Arc::new(TelegramRelay::with_allowlist(
                        Some(telegram.token.clone()),
                        telegram.allowed_users.clone(),
                    ));
                    match relay.start(relay_tx.clone()).await {
                        Ok(()) => {
                            *telegram_handle.write().await = Some(relay);
                        }
                        Err(e) => tracing::error!("Telegram relay failed to start: {e}"),
                    }
                }

                let cloud = Arc::new(WhatsAppChannel::from_env());
                if cloud.is_configured() {
                    println!("Starting WhatsApp Cloud API relay...");
                    let _ = cloud.start(relay_tx.clone()).await;
                    *whatsapp_cloud_handle.write().await = Some(cloud);
                }

                if let Some(web) = &config.whatsapp_web {
                    if web.enabled {
                        println!("Restoring WhatsApp Web bridge...");
                        if let Err(e) = whatsapp_bridge
                            .ensure_started(
                                &web.allowed_numbers,
                                web.allow_anyone,
                                web.phone.as_deref(),
                            )
                            .await
                        {
                            tracing::warn!("WhatsApp bridge restore failed: {e}");
                        }
                    }
                }
            }

            let telegram_online = Arc::new(std::sync::atomic::AtomicBool::new(
                telegram_handle
                    .read()
                    .await
                    .as_ref()
                    .map(|relay| relay.is_connected())
                    .unwrap_or(false),
            ));
            let whatsapp_online = Arc::new(std::sync::atomic::AtomicBool::new(
                whatsapp_cloud_handle.read().await.is_some(),
            ));

            // Keep the coarse /api/status relay flag in sync with the bridge.
            {
                let bridge = whatsapp_bridge.clone();
                let cloud_handle = whatsapp_cloud_handle.clone();
                let flag = whatsapp_online.clone();
                tokio::spawn(async move {
                    loop {
                        let cloud_active = cloud_handle.read().await.is_some();
                        // The bridge may have been adopted from a previous
                        // daemon instead of spawned by this process, so always
                        // probe it rather than relying on child ownership.
                        let bridge_connected = bridge
                            .status()
                            .await
                            .get("connected")
                            .and_then(|value| value.as_bool())
                            .unwrap_or(false);
                        flag.store(
                            cloud_active || bridge_connected,
                            std::sync::atomic::Ordering::SeqCst,
                        );
                        tokio::time::sleep(std::time::Duration::from_secs(5)).await;
                    }
                });
            }

            // Relay turn execution is installed after AppState exists so slash
            // commands can use active-turn, approval, model, and session state.

            let mut loaded_automations = load_automations_from_disk();
            for job in &mut loaded_automations {
                if job.trigger_type == "cron" {
                    job.trigger_value = normalize_cron_expression(&job.trigger_value);
                }
                if job.last_status.as_deref() == Some("running") {
                    // A previous daemon exited mid-run; do not leave the job
                    // permanently stuck in "running".
                    job.last_status = Some("failed".to_string());
                }
                if let Some(raw_last_run) = job.last_run.clone() {
                    if let Some(parsed) = parse_last_run(&raw_last_run) {
                        job.last_run = Some(parsed.with_timezone(&Utc).to_rfc3339());
                    } else if job.trigger_type == "interval" {
                        // Never fire a legacy interval job immediately just
                        // because its old timestamp was human-readable.
                        job.last_run = Some(Utc::now().to_rfc3339());
                    }
                }
            }
            save_automations_to_disk(&loaded_automations);
            let automations = Arc::new(tokio::sync::RwLock::new(loaded_automations));
            let automation_logs =
                Arc::new(tokio::sync::RwLock::new(load_automation_logs_from_disk()));

            let workspace_config = load_workspace_config(&workspace);
            let workspace_default = workspace_config.default.clone();

            // 9. Build Axum App Router
            let state = AppState {
                runtime,
                storage,
                approval,
                audit,
                tools,
                event_tx,
                workspace_root: workspace_default,
                oauth_accounts,
                automations,
                automation_logs,
                telegram_online,
                whatsapp_online,
                telegram: telegram_handle,
                whatsapp_cloud: whatsapp_cloud_handle,
                whatsapp_bridge,
                relay_config,
                relay_tx,
                workspaces: Arc::new(tokio::sync::RwLock::new(workspace_config)),
                api_token: api_token.clone(),
                session_token: if ephemeral_session_token {
                    api_token.clone()
                } else {
                    None
                },
                active_turns: Arc::new(tokio::sync::RwLock::new(HashMap::new())),
                relay_sessions: Arc::new(tokio::sync::RwLock::new(HashMap::new())),
                relay_model: Arc::new(tokio::sync::RwLock::new(
                    std::env::var("ODON_RELAY_MODEL")
                        .ok()
                        .map(|value| value.trim().to_string())
                        .filter(|value| !value.is_empty()),
                )),
                started_at: std::time::Instant::now(),
            };

            {
                let scheduler_state = state.clone();
                tokio::spawn(async move {
                    loop {
                        automation_scheduler_tick(&scheduler_state).await;
                        tokio::time::sleep(std::time::Duration::from_secs(20)).await;
                    }
                });
            }

            {
                let relay_state = state.clone();
                tokio::spawn(async move {
                    while let Some(message) = relay_rx.recv().await {
                        handle_relay_message(relay_state.clone(), message).await;
                    }
                });
            }

            {
                let approval_state = state.clone();
                tokio::spawn(async move {
                    let mut events = approval_state.event_tx.subscribe();
                    while let Ok(event) = events.recv().await {
                        if let AgentEvent::ApprovalRequested {
                            session_id,
                            approval_id,
                            tool_name,
                            risk_tier,
                            description,
                            ..
                        } = event.event
                        {
                            let Some((channel, sender)) = relay_channel_for_session(&session_id)
                            else {
                                continue;
                            };
                            let text = format!(
                                "🔒 *Approval required*\n\nID: `{approval_id}`\nTool: `{tool_name}`\nRisk: Tier {risk_tier}\n\n{description}\n\nReply:\n/approve {approval_id}\n/deny {approval_id}"
                            );
                            let _ =
                                send_relay_text(&approval_state, &channel, &sender, &text).await;
                        }
                    }
                });
            }

            // Same-origin requests need no CORS at all. Cross-origin access is
            // limited to the shipped desktop origins, the vite dev server and
            // anything explicitly opted in via ODON_ALLOWED_ORIGINS — never
            // arbitrary localhost ports, which would let any local web page
            // drive the daemon API.
            let allowed_origins = allowed_origins_from_env();
            let cors = CorsLayer::new()
                .allow_origin(AllowOrigin::predicate(move |origin, _| {
                    let Ok(value) = origin.to_str() else {
                        return false;
                    };
                    let lower = value.to_ascii_lowercase();
                    is_first_party_origin(&lower)
                        || allowed_origins.iter().any(|allowed| allowed == &lower)
                }))
                .allow_methods([Method::GET, Method::POST, Method::DELETE, Method::OPTIONS])
                .allow_headers([
                    header::CONTENT_TYPE,
                    header::AUTHORIZATION,
                    header::ACCEPT,
                    HeaderName::from_static("x-odon-token"),
                ])
                .allow_credentials(false);

            let mut app = Router::new()
                .route("/api/status", get(get_status))
                .route("/api/health", get(get_health))
                .route("/api/metrics", get(get_metrics))
                .route("/api/sessions", get(list_sessions).post(create_session))
                .route(
                    "/api/sessions/{id}",
                    get(get_session).delete(delete_session),
                )
                .route(
                    "/api/sessions/{id}/messages",
                    get(list_messages).post(send_message),
                )
                .route("/api/sessions/{id}/cancel", post(cancel_turn))
                .route("/api/sessions/{id}/steer", post(steer_turn))
                .route("/api/sessions/{id}/workspace", post(set_session_workspace))
                .route("/api/sessions/{id}/artifacts", get(list_session_artifacts))
                .route("/api/sessions/{id}/todos", get(get_session_todos))
                .route(
                    "/api/workspaces",
                    get(list_workspaces)
                        .post(add_workspace)
                        .delete(remove_workspace),
                )
                .route("/api/workspaces/browse", get(browse_workspaces))
                .route("/api/artifacts/{id}", get(get_artifact))
                .route("/api/artifacts/{id}/versions", get(list_artifact_versions))
                .route(
                    "/api/artifacts/{id}/versions/{version}",
                    get(get_artifact_version),
                )
                .route(
                    "/api/artifacts/{id}/versions/{version}/restore",
                    post(restore_artifact_version),
                )
                .route("/api/artifacts/{id}/export", get(export_artifact))
                .route("/api/memory", get(list_agent_memories))
                .route(
                    "/api/memory/{*key}",
                    axum::routing::delete(delete_agent_memory),
                )
                .route("/api/tools", get(list_tools))
                .route("/api/approvals", get(list_approvals))
                .route("/api/approvals/{id}", post(resolve_approval))
                .route("/api/approvals/history", get(get_approval_history))
                .route("/api/approvals/policies", get(get_approval_policies))
                .route(
                    "/api/approvals/policies/{*key}",
                    axum::routing::delete(delete_approval_policy),
                )
                .route(
                    "/api/mcp/servers",
                    get(get_mcp_servers).post(save_mcp_server),
                )
                .route(
                    "/api/mcp/servers/{id}",
                    axum::routing::delete(delete_mcp_server),
                )
                .route("/api/mcp/servers/{id}/toggle", post(toggle_mcp_server))
                .route("/api/github/status", get(get_github_status))
                .route("/api/github/user-repos", get(get_github_user_repos))
                .route("/api/github/prs", get(get_github_prs))
                .route(
                    "/api/github/issues",
                    get(get_github_issues).post(create_github_issue),
                )
                .route(
                    "/api/automations",
                    get(get_automations).post(save_automation),
                )
                .route(
                    "/api/automations/{id}",
                    axum::routing::delete(delete_automation),
                )
                .route("/api/automations/{id}/trigger", post(trigger_automation))
                .route("/api/automations/logs", get(get_automation_logs))
                .route("/api/settings", get(get_settings).post(update_settings))
                .route("/api/models", get(get_models))
                .route(
                    "/api/providers/{id}/discover-models",
                    post(discover_provider_models),
                )
                .route("/api/browser/read", get(browser_read_page))
                .route(
                    "/api/browser/sessions",
                    get(browser_sessions_list).post(browser_session_open),
                )
                .route(
                    "/api/browser/sessions/{id}/navigate",
                    post(browser_session_navigate),
                )
                .route(
                    "/api/browser/sessions/{id}/state",
                    get(browser_session_state),
                )
                .route(
                    "/api/browser/sessions/{id}/screenshot",
                    get(browser_session_screenshot),
                )
                .route("/api/browser/sessions/{id}/act", post(browser_session_act))
                .route(
                    "/api/browser/sessions/{id}",
                    axum::routing::delete(browser_session_close),
                )
                .route(
                    "/api/providers/config",
                    get(get_provider_configs).post(save_provider_config),
                )
                .route(
                    "/api/providers/config/{provider_id}",
                    axum::routing::delete(delete_provider_config),
                )
                .route("/api/providers/test", post(test_provider))
                .route("/api/oauth/accounts", get(get_oauth_accounts))
                .route("/api/oauth/login", post(login_oauth))
                .route("/api/oauth/disconnect", post(disconnect_oauth))
                .route("/api/terminal/info", get(get_terminal_info))
                .route("/api/terminal/exec", post(execute_terminal))
                .route("/api/terminal/ws", get(ws_terminal_handler))
                .route(
                    "/api/relays/whatsapp",
                    get(whatsapp_verify_webhook).post(whatsapp_webhook),
                )
                .route(
                    "/api/relays/telegram",
                    get(get_telegram_relay)
                        .post(save_telegram_relay)
                        .delete(disconnect_telegram_relay),
                )
                .route(
                    "/api/relays/whatsapp/bridge/status",
                    get(get_whatsapp_bridge_status),
                )
                .route(
                    "/api/relays/whatsapp/bridge/start",
                    post(start_whatsapp_bridge),
                )
                .route(
                    "/api/relays/whatsapp/bridge/logout",
                    post(logout_whatsapp_bridge),
                )
                .route(
                    "/api/relays/whatsapp/inbound",
                    post(whatsapp_bridge_inbound),
                )
                .route("/api/events", get(sse_events))
                .route("/api/files/{*path}", get(serve_file))
                // File Explorer APIs
                .route("/api/fs/list", get(fs_list_dir))
                .route("/api/fs/read", get(fs_read_file))
                .route("/api/fs/write", post(fs_write_file))
                .route("/api/fs/mkdir", post(fs_mkdir))
                .route("/api/fs/delete", axum::routing::delete(fs_delete_path))
                // SSH Remote APIs
                .route(
                    "/api/ssh/profiles",
                    get(get_ssh_profiles).post(save_ssh_profile),
                )
                .route(
                    "/api/ssh/profiles/{id}",
                    axum::routing::delete(delete_ssh_profile),
                )
                .route("/api/ssh/exec", post(exec_ssh_command))
                .route("/api/ssh/certificates/import", post(import_ssh_certificate))
                // Skills APIs
                .route("/api/skills", get(get_skills_list).post(save_skill_item))
                .route(
                    "/api/skills/{name}",
                    axum::routing::delete(delete_skill_item),
                );

            // The web UI ships in three layouts: the dev tree
            // (desktop/dist under the working directory), an explicit
            // ODON_DESKTOP_DIST override, and next to the daemon binary
            // in installed bundles (including the macOS Resources
            // sibling of the sidecar). First existing candidate wins.
            let exe_dir = std::env::current_exe()
                .ok()
                .and_then(|exe| exe.parent().map(|dir| dir.to_path_buf()));
            let dist_path = desktop_dist_candidates(
                std::env::var("ODON_DESKTOP_DIST").ok().as_deref(),
                exe_dir.as_deref(),
            )
            .into_iter()
            .find(|candidate| candidate.exists());
            if let Some(dist_path) = dist_path {
                info!(
                    "Serving static desktop web application from {}",
                    dist_path.display()
                );
                app = app.fallback_service(ServeDir::new(dist_path));
            }

            let auth_state = state.clone();
            let app = app
                .layer(cors)
                .layer(DefaultBodyLimit::max(36 * 1024 * 1024))
                .with_state(state)
                .layer(middleware::from_fn_with_state(
                    auth_state,
                    require_api_token,
                ));

            let listener = match tokio::net::TcpListener::bind(&bind).await {
                Ok(listener) => listener,
                Err(error) => {
                    // Another daemon owns this port. Leave its token file
                    // untouched and exit with an actionable message instead of
                    // clobbering the credentials the running UI depends on.
                    if error.kind() == std::io::ErrorKind::AddrInUse {
                        anyhow::bail!(
                            "Another ODON daemon is already listening on {bind}. \
                             Close the other instance (or its desktop app) before starting a new one; \
                             the running daemon keeps its API token."
                        );
                    }
                    return Err(error.into());
                }
            };
            // Publish the loopback token only now that this process owns the
            // listener, so the file always matches the daemon the UI talks to.
            if let Some(token) = pending_loopback_token.as_deref() {
                match write_loopback_token_file(token) {
                    Ok(path) => eprintln!("Loopback API token file: {}", path.display()),
                    Err(error) => eprintln!("Could not write the loopback API token file: {error}"),
                }
            }
            println!("\n✔ ODON HTTP & SSE Server listening on: http://{}", bind);
            println!("✔ API ready for Desktop UI and Relays. Press Ctrl+C to stop.\n");

            axum::serve(listener, app).await?;
        }
    }

    Ok(())
}

// ─────────────────────────────────────────────────────────────
// REST & SSE HANDLERS
// ─────────────────────────────────────────────────────────────

// Origin gate ---------------------------------------------------------------
//
// Loopback binding is not access control: any web page the user visits can
// reach http://127.0.0.1:4096 and the browser attaches that page's Origin to
// the request. Origin is therefore the real boundary for local-api hijacking:
// requests with no Origin (native clients, relays, curl) pass, first-party and
// same-origin requests pass, everything else is rejected. This gate is also
// what stops a hostile page from opening a PTY through the terminal WebSocket.

/// First-party origins ODON itself ships: the Tauri shell and the Vite dev
/// server. Compared after trimming, dropping a trailing slash and lowercasing.
fn is_first_party_origin(origin: &str) -> bool {
    let normalized = origin.trim().trim_end_matches('/').to_ascii_lowercase();
    matches!(
        normalized.as_str(),
        "tauri://localhost"
            | "https://tauri.localhost"
            | "http://tauri.localhost"
            | "http://localhost:3000"
            | "http://127.0.0.1:3000"
            | "http://[::1]:3000"
    )
}

/// Host portion of an `authority` (`host[:port]`), borrowed from the input.
/// IPv6 literals keep their brackets (`[::1]:3000` -> `[::1]`).
fn authority_host(authority: &str) -> &str {
    if authority.starts_with('[') {
        return match authority.find(']') {
            Some(end) => &authority[..=end],
            None => authority,
        };
    }
    match authority.rsplit_once(':') {
        Some((host, port)) if !host.contains(':') && port.chars().all(|c| c.is_ascii_digit()) => {
            host
        }
        _ => authority,
    }
}

/// Explicit port of an `authority`, or `None` when it carries none.
fn authority_port(authority: &str) -> Option<u16> {
    if authority.starts_with('[') {
        let end = authority.find(']')?;
        return authority[end + 1..].strip_prefix(':')?.parse().ok();
    }
    match authority.rsplit_once(':') {
        Some((host, port)) if !host.contains(':') => port.parse().ok(),
        _ => None,
    }
}

/// Default port for a URL scheme; 0 when the scheme has none or is unknown.
fn default_port(scheme: &str) -> u16 {
    match scheme {
        "https" => 443,
        "http" => 80,
        _ => 0,
    }
}

/// Any spelling of the loopback interface.
fn is_loopback_host(host: &str) -> bool {
    matches!(host, "localhost" | "127.0.0.1" | "[::1]")
}

/// Reads `ODON_ALLOWED_ORIGINS` as a list of lowercased origin strings.
fn allowed_origins_from_env() -> Vec<String> {
    std::env::var("ODON_ALLOWED_ORIGINS")
        .ok()
        .map(|value| {
            value
                .split(',')
                .map(|item| item.trim().to_ascii_lowercase())
                .filter(|item| !item.is_empty())
                .collect()
        })
        .unwrap_or_default()
}

/// Decides whether a request may drive the API, based on its `Origin`.
///
/// `host` is the request's `Host` header, i.e. the authority the browser
/// actually reached. Allowed: callers that send no Origin (native clients and
/// relays), ODON's own desktop origins, origins opted in through
/// `ODON_ALLOWED_ORIGINS`, and requests whose Origin host is the daemon's own
/// host including every loopback spelling. An opaque `null` origin is always
/// rejected, because that is what sandboxed and `data:` pages send.
fn origin_is_trusted(origin: Option<&str>, host: Option<&str>, extra: &[String]) -> bool {
    let Some(raw_origin) = origin else {
        return true;
    };
    let trimmed = raw_origin.trim();
    if trimmed.is_empty() {
        return true;
    }
    let normalized = trimmed.trim_end_matches('/').to_ascii_lowercase();
    if normalized == "null" {
        return false;
    }
    if is_first_party_origin(&normalized) {
        return true;
    }
    if extra.iter().any(|allowed| allowed == &normalized) {
        return true;
    }
    let Some(bound) = host
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(|value| value.to_ascii_lowercase())
    else {
        return false;
    };
    let Some((scheme, rest)) = normalized.split_once("://") else {
        return false;
    };
    let authority = rest.split('/').next().unwrap_or("");
    if authority.is_empty() {
        return false;
    }
    if authority == bound {
        return true;
    }
    let origin_host = authority_host(authority);
    let bound_host = authority_host(&bound);
    if origin_host.is_empty() || bound_host.is_empty() {
        return false;
    }
    if is_loopback_host(origin_host) && is_loopback_host(bound_host) {
        return true;
    }
    if origin_host == bound_host {
        let default = default_port(scheme);
        return match authority_port(authority) {
            Some(port) => default != 0 && port == default,
            None => default != 0,
        };
    }
    false
}

/// Constant-time string comparison so token guesses cannot use timing oracles.
fn constant_time_eq(a: &str, b: &str) -> bool {
    if a.len() != b.len() {
        return false;
    }
    let mut diff = 0u8;
    for (x, y) in a.bytes().zip(b.bytes()) {
        diff |= x ^ y;
    }
    diff == 0
}

/// Chooses the token the HTTP API will require.
///
/// An operator-supplied `ODON_API_TOKEN` always wins and is persistent: it is
/// never exposed through the API. A loopback bind without one mints a
/// per-session token instead, so a local install works out of the box without
/// a secret the user has to copy around. Any other bind gets no token, because
/// the caller refuses to start in that case.
///
/// Returns the token to enforce and whether it was generated here (ephemeral).
fn mint_loopback_session_token(
    is_loopback: bool,
    configured: Option<String>,
) -> (Option<String>, bool) {
    if let Some(token) = configured {
        return (Some(token), false);
    }
    if !is_loopback {
        return (None, false);
    }
    let token = format!("odon-{}", uuid::Uuid::new_v4().simple());
    eprintln!("No ODON_API_TOKEN set: generated a per-session token for this loopback daemon.");
    (Some(token), true)
}

/// Owner-only file that holds the per-session loopback token.
fn loopback_token_path() -> PathBuf {
    let base = std::env::var_os("ODON_HOME")
        .map(PathBuf::from)
        .or_else(|| std::env::var_os("USERPROFILE").map(PathBuf::from))
        .or_else(|| std::env::var_os("HOME").map(PathBuf::from))
        .unwrap_or_else(|| PathBuf::from("."));
    base.join(".odon").join("api-token")
}

/// Writes the ephemeral session token to `loopback_token_path`, mode 0600 on
/// Unix so no other local user can read it. Never logs the value.
fn write_loopback_token_file(token: &str) -> std::io::Result<PathBuf> {
    let path = loopback_token_path();
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir)?;
    }
    std::fs::write(&path, token.as_bytes())?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mut permissions = std::fs::metadata(&path)?.permissions();
        permissions.set_mode(0o600);
        std::fs::set_permissions(&path, permissions)?;
    }
    Ok(path)
}

#[cfg(test)]
mod origin_tests {
    use super::*;

    fn allow(values: &[&str]) -> Vec<String> {
        values.iter().map(|value| value.to_string()).collect()
    }

    #[test]
    fn requests_without_an_origin_are_allowed() {
        // Native clients, relays and curl send no Origin at all.
        assert!(origin_is_trusted(None, Some("127.0.0.1:4096"), &[]));
        assert!(origin_is_trusted(Some("   "), Some("127.0.0.1:4096"), &[]));
    }

    #[test]
    fn opaque_null_origin_is_rejected() {
        assert!(!origin_is_trusted(
            Some("null"),
            Some("127.0.0.1:4096"),
            &[]
        ));
    }

    #[test]
    fn first_party_origins_are_allowed() {
        assert!(origin_is_trusted(Some("tauri://localhost"), None, &[]));
        assert!(origin_is_trusted(
            Some("https://tauri.localhost"),
            None,
            &[]
        ));
        assert!(origin_is_trusted(Some("http://localhost:3000"), None, &[]));
    }

    #[test]
    fn same_host_origin_is_allowed() {
        assert!(origin_is_trusted(
            Some("http://127.0.0.1:4096"),
            Some("127.0.0.1:4096"),
            &[]
        ));
    }

    #[test]
    fn loopback_aliases_are_allowed() {
        assert!(origin_is_trusted(
            Some("http://localhost:4096"),
            Some("127.0.0.1:4096"),
            &[]
        ));
        assert!(origin_is_trusted(
            Some("http://[::1]:4096"),
            Some("localhost:4096"),
            &[]
        ));
    }

    #[test]
    fn foreign_origin_is_rejected() {
        assert!(!origin_is_trusted(
            Some("https://evil.example"),
            Some("127.0.0.1:4096"),
            &[]
        ));
        assert!(!origin_is_trusted(Some("https://evil.example"), None, &[]));
    }

    #[test]
    fn explicitly_allowed_origin_is_allowed() {
        let extra = allow(&["https://app.example"]);
        assert!(origin_is_trusted(
            Some("https://app.example"),
            Some("127.0.0.1:4096"),
            &extra
        ));
    }

    #[test]
    fn authority_host_strips_ports_and_keeps_ipv6_brackets() {
        assert_eq!(authority_host("127.0.0.1:4096"), "127.0.0.1");
        assert_eq!(authority_host("localhost"), "localhost");
        assert_eq!(authority_host("[::1]:3000"), "[::1]");
        assert_eq!(authority_host("example.com:8443"), "example.com");
    }
}

#[cfg(test)]
mod terminal_env_tests {
    use super::*;

    #[test]
    fn terminal_children_never_inherit_daemon_secrets() {
        for name in TERMINAL_ENVIRONMENT_ALLOWLIST {
            let upper = name.to_ascii_uppercase();
            assert!(!upper.starts_with("ODON_"), "{name} exposes daemon state");
            for marker in ["TOKEN", "SECRET", "API_KEY", "PASSWORD", "CREDENTIAL"] {
                assert!(!upper.contains(marker), "{name} looks like a secret");
            }
        }
    }

    #[test]
    fn terminal_children_still_get_a_usable_shell() {
        // A cleared environment without PATH leaves the user in a shell where
        // no command resolves, which is a silent functional break.
        assert!(TERMINAL_ENVIRONMENT_ALLOWLIST.contains(&"PATH"));
        assert!(TERMINAL_ENVIRONMENT_ALLOWLIST.contains(&"TERM"));
    }
}

#[cfg(test)]
mod token_tests {
    use super::*;

    #[test]
    fn constant_time_eq_accepts_equal_tokens() {
        assert!(constant_time_eq("odon-session", "odon-session"));
    }

    #[test]
    fn constant_time_eq_rejects_different_lengths() {
        assert!(!constant_time_eq("odon", "odon-session"));
    }

    #[test]
    fn constant_time_eq_rejects_different_content() {
        assert!(!constant_time_eq("odon-session", "odon-sessiox"));
    }
}

/// Mandatory bearer-token protection for the HTTP API.
///
/// There is no fall-open mode: the daemon always starts with a token (the
/// operator's `ODON_API_TOKEN`, or the per-session loopback token minted at
/// startup), so an unauthenticated request is always rejected. The WhatsApp
/// webhook routes are exempt because they run their own verify-token and
/// signature checks, and `/api/health` is exempt so a client can discover that
/// a token is required before it has one to send. SSE and the terminal
/// WebSocket may pass the token as a `?token=` query parameter because
/// browsers cannot set headers on those transports.
async fn require_api_token(
    State(state): State<AppState>,
    req: Request,
    next: Next,
) -> axum::response::Response {
    let path = req.uri().path();
    // Only these paths bypass the token check, and only because they either
    // authenticate their caller by other means or must answer before a client
    // can have a token. Everything else is token-gated, always.
    let exempt = !path.starts_with("/api/")
        || path == "/api/relays/whatsapp"
        || path == "/api/relays/whatsapp/inbound"
        || path == "/api/health";
    // CORS preflight is unauthenticated by definition: the browser sends no
    // credentials, so OPTIONS has to short-circuit before the token check.
    let preflight = req.method() == Method::OPTIONS;
    if exempt || preflight {
        return next.run(req).await;
    }

    // Origin gate, applied to every request this middleware actually protects,
    // i.e. right after the exempt early-return above. The WhatsApp relay routes
    // never reach it (they are exempt), which is correct: Meta's servers call
    // them with no Origin header at all.
    let allowed_origins = allowed_origins_from_env();
    let origin = req
        .headers()
        .get(header::ORIGIN)
        .and_then(|value| value.to_str().ok());
    let host = req
        .headers()
        .get(header::HOST)
        .and_then(|value| value.to_str().ok());
    if !origin_is_trusted(origin, host, &allowed_origins) {
        return (StatusCode::FORBIDDEN, "Forbidden origin").into_response();
    }

    // No fall-open mode: startup always yields a token (operator-supplied or
    // minted for loopback), so a missing token here is a bug and must deny.
    let Some(expected) = state.api_token.as_deref() else {
        return (StatusCode::UNAUTHORIZED, "Unauthorized").into_response();
    };

    let bearer_ok = req
        .headers()
        .get(header::AUTHORIZATION)
        .and_then(|value| value.to_str().ok())
        .and_then(|value| value.strip_prefix("Bearer "))
        .map(|value| constant_time_eq(value, expected))
        .unwrap_or(false);
    let header_ok = req
        .headers()
        .get("x-odon-token")
        .and_then(|value| value.to_str().ok())
        .map(|value| constant_time_eq(value, expected))
        .unwrap_or(false);
    // Query-string tokens leak into logs/history/Referer, so they are only
    // accepted on the two browser transports that cannot send headers.
    let query_allowed = path == "/api/events" || path == "/api/terminal/ws";
    let query_ok = query_allowed
        && req
            .uri()
            .query()
            .map(|query| {
                query
                    .split('&')
                    .any(|pair| pair == format!("token={expected}"))
            })
            .unwrap_or(false);

    if bearer_ok || header_ok || query_ok {
        next.run(req).await
    } else {
        (StatusCode::UNAUTHORIZED, "Unauthorized").into_response()
    }
}

async fn get_status(State(state): State<AppState>) -> impl IntoResponse {
    let policy = state.approval.get_policy().await;
    Json(json!({
        "status": "online",
        "version": env!("CARGO_PKG_VERSION"),
        "workspace_root": state.workspace_root,
        "tools_count": state.tools.schemas().len(),
        "execution_mode": format!("{:?}", policy.mode),
        "relays": {
            "desktop_api": true,
            "sse_stream": true,
            "cli": true,
            "telegram": state.telegram_online.load(std::sync::atomic::Ordering::SeqCst),
            "whatsapp": state.whatsapp_online.load(std::sync::atomic::Ordering::SeqCst)
        }
    }))
}

/// Unauthenticated by design (see `require_api_token`): lets a client discover
/// whether this daemon demands a token before it has one to send. It reports
/// whether a token is required and, for a first-party browser origin only, the
/// ephemeral loopback session token. That is what lets the bundled desktop UI
/// adopt the token without a manual copy/paste.
///
/// The caller must present an explicit trusted `Origin`. A request with no
/// Origin at all is a local tool (curl, a script, another process) and is told
/// only that a token is required: it can read the owner-only token file
/// instead. Without this rule any local process could read the secret from an
/// endpoint that needs no secret.
async fn get_health(headers: HeaderMap, State(state): State<AppState>) -> impl IntoResponse {
    let origin = headers
        .get(header::ORIGIN)
        .and_then(|value| value.to_str().ok());
    let host = headers
        .get(header::HOST)
        .and_then(|value| value.to_str().ok());
    let trusted_origin =
        origin.is_some() && origin_is_trusted(origin, host, &allowed_origins_from_env());
    let session_token = state.session_token.as_deref().filter(|_| trusted_origin);
    Json(json!({
        "status": "online",
        "version": env!("CARGO_PKG_VERSION"),
        "auth_required": state.api_token.is_some(),
        "session_token": session_token,
    }))
}

async fn get_metrics(State(state): State<AppState>) -> impl IntoResponse {
    let sessions = state
        .storage
        .list_sessions()
        .map(|items| items.len())
        .unwrap_or(0);
    let provider_configs = state
        .storage
        .list_provider_configs()
        .map(|items| items.len())
        .unwrap_or(0);
    let pending_approvals = state.approval.list_pending().await.len();
    let audit_records = state.approval.get_audit_history().await.len();
    let automations = state.automations.read().await.len();
    let active_turns = state.active_turns.read().await.len();
    let oauth_accounts = state.oauth_accounts.read().await.len();

    Json(json!({
        "status": "online",
        "version": env!("CARGO_PKG_VERSION"),
        "uptime_seconds": state.started_at.elapsed().as_secs(),
        "workspace_root": state.workspace_root,
        "tools_count": state.tools.schemas().len(),
        "sessions": sessions,
        "provider_configs": provider_configs,
        "oauth_accounts": oauth_accounts,
        "automations": automations,
        "active_turns": active_turns,
        "pending_approvals": pending_approvals,
        "audit_records": audit_records,
        "relays": {
            "telegram": state.telegram_online.load(std::sync::atomic::Ordering::SeqCst),
            "whatsapp": state.whatsapp_online.load(std::sync::atomic::Ordering::SeqCst)
        }
    }))
}

#[derive(Deserialize)]
struct WhatsAppVerifyQuery {
    #[serde(rename = "hub.mode")]
    mode: Option<String>,
    #[serde(rename = "hub.verify_token")]
    token: Option<String>,
    #[serde(rename = "hub.challenge")]
    challenge: Option<String>,
}

/// Meta webhook verification challenge (GET).
async fn whatsapp_verify_webhook(
    State(state): State<AppState>,
    Query(query): Query<WhatsAppVerifyQuery>,
) -> axum::response::Response {
    let expected = {
        let guard = state.whatsapp_cloud.read().await;
        guard
            .as_ref()
            .and_then(|channel| channel.verify_token())
            .map(|token| token.to_string())
    };
    let valid = query.mode.as_deref() == Some("subscribe")
        && expected.is_some()
        && query.token.as_deref() == expected.as_deref();
    if valid {
        (StatusCode::OK, query.challenge.unwrap_or_default()).into_response()
    } else {
        (StatusCode::FORBIDDEN, "Invalid WhatsApp verify token").into_response()
    }
}

/// Inbound WhatsApp Cloud API events (POST). Always answer 200 quickly so Meta
/// does not retry; the actual agent turn runs on the shared relay dispatcher.
async fn whatsapp_webhook(
    State(state): State<AppState>,
    headers: HeaderMap,
    body: String,
) -> axum::response::Response {
    let app_secret = {
        let guard = state.whatsapp_cloud.read().await;
        guard
            .as_ref()
            .and_then(|channel| channel.app_secret())
            .map(|secret| secret.to_string())
    };
    let Some(secret) = app_secret else {
        tracing::error!("Rejected WhatsApp webhook because WHATSAPP_APP_SECRET is not configured");
        return (
            StatusCode::SERVICE_UNAVAILABLE,
            "Webhook signing is not configured",
        )
            .into_response();
    };
    let signature = headers
        .get("x-hub-signature-256")
        .and_then(|value| value.to_str().ok())
        .unwrap_or("");
    if !verify_meta_signature(&secret, body.as_bytes(), signature) {
        tracing::warn!("Rejected WhatsApp webhook with invalid signature");
        return (StatusCode::UNAUTHORIZED, "Invalid signature").into_response();
    }

    let payload: Value = match serde_json::from_str(&body) {
        Ok(value) => value,
        Err(_) => return (StatusCode::BAD_REQUEST, "Invalid JSON").into_response(),
    };

    // Meta expects a 2xx within five seconds. Parsing can download media, so
    // acknowledge immediately and finish the work in the background.
    let whatsapp = state.whatsapp_cloud.read().await.clone();
    if let Some(whatsapp) = whatsapp {
        let relay_tx = state.relay_tx.clone();
        tokio::spawn(async move {
            for message in whatsapp.parse_webhook(&payload).await {
                if relay_tx.send(message).await.is_err() {
                    break;
                }
            }
        });
    }
    (StatusCode::OK, "EVENT_RECEIVED").into_response()
}

fn hex_value(byte: u8) -> Option<u8> {
    match byte {
        b'0'..=b'9' => Some(byte - b'0'),
        b'a'..=b'f' => Some(byte - b'a' + 10),
        b'A'..=b'F' => Some(byte - b'A' + 10),
        _ => None,
    }
}

fn hex_decode(input: &str) -> std::result::Result<Vec<u8>, ()> {
    let bytes = input.as_bytes();
    if !bytes.len().is_multiple_of(2) {
        return Err(());
    }
    let mut out = Vec::with_capacity(bytes.len() / 2);
    let mut index = 0;
    while index < bytes.len() {
        let high = hex_value(bytes[index]).ok_or(())?;
        let low = hex_value(bytes[index + 1]).ok_or(())?;
        out.push((high << 4) | low);
        index += 2;
    }
    Ok(out)
}

/// Validates `X-Hub-Signature-256` from Meta when `WHATSAPP_APP_SECRET` is set.
fn verify_meta_signature(secret: &str, body: &[u8], header: &str) -> bool {
    use hmac::{Hmac, Mac};
    use sha2::Sha256;

    let Some(hex_signature) = header.trim().strip_prefix("sha256=") else {
        return false;
    };
    let Ok(expected) = hex_decode(hex_signature) else {
        return false;
    };
    let Ok(mut mac) = Hmac::<Sha256>::new_from_slice(secret.as_bytes()) else {
        return false;
    };
    mac.update(body);
    mac.verify_slice(&expected).is_ok()
}

async fn list_sessions(State(state): State<AppState>) -> impl IntoResponse {
    match state.storage.list_sessions() {
        Ok(sessions) => (StatusCode::OK, Json(json!(sessions))),
        Err(e) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(json!({ "error": e.to_string() })),
        ),
    }
}

#[derive(Deserialize)]
struct CreateSessionRequest {
    title: Option<String>,
    mode: Option<String>,
    model: Option<String>,
    workspace_root: Option<String>,
}

async fn create_session(
    State(state): State<AppState>,
    Json(req): Json<CreateSessionRequest>,
) -> impl IntoResponse {
    let mode = match req.mode.as_deref() {
        Some("code") => SessionMode::Code,
        Some("create") => SessionMode::Create,
        Some("doc") | Some("documents") => SessionMode::Documents,
        Some("files") => SessionMode::Code,
        Some("agents") => SessionMode::Agents,
        Some("automations") => SessionMode::Automations,
        _ => SessionMode::General,
    };
    let title = req.title.unwrap_or_else(|| "New Conversation".to_string());
    let model = req.model.unwrap_or_else(|| "odon-auto".to_string());

    let workspace_root = match req.workspace_root.filter(|p| !p.trim().is_empty()) {
        Some(raw) => match canonical_workspace(&raw) {
            Ok(path) => {
                let config = state.workspaces.read().await;
                if !workspace_allowed(&config, &path) {
                    return (
                        StatusCode::FORBIDDEN,
                        Json(
                            json!({ "error": "Workspace is not in the allowlist. Add it from the composer first." }),
                        ),
                    );
                }
                Some(path)
            }
            Err(e) => {
                return (StatusCode::BAD_REQUEST, Json(json!({ "error": e })));
            }
        },
        None => None,
    };

    let mut session = Session::new(title, mode, model);
    session.workspace_root = workspace_root;
    match state.storage.save_session(&session) {
        Ok(_) => (StatusCode::CREATED, Json(json!(session))),
        Err(e) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(json!({ "error": e.to_string() })),
        ),
    }
}

#[derive(Deserialize)]
struct SetSessionWorkspaceRequest {
    path: Option<String>,
}

/// Change the working folder for an existing session. Only folders that were
/// explicitly added to the allowlist (or the daemon default) are accepted.
async fn set_session_workspace(
    Path(id): Path<String>,
    State(state): State<AppState>,
    Json(req): Json<SetSessionWorkspaceRequest>,
) -> impl IntoResponse {
    let mut session = match state.storage.get_session(&id) {
        Ok(session) => session,
        Err(e) => {
            return (
                StatusCode::NOT_FOUND,
                Json(json!({ "error": e.to_string() })),
            )
        }
    };

    session.workspace_root = match req.path.filter(|p| !p.trim().is_empty()) {
        Some(raw) => match canonical_workspace(&raw) {
            Ok(path) => {
                let config = state.workspaces.read().await;
                if !workspace_allowed(&config, &path) {
                    return (
                        StatusCode::FORBIDDEN,
                        Json(json!({ "error": "Workspace is not in the allowlist." })),
                    );
                }
                Some(path)
            }
            Err(e) => return (StatusCode::BAD_REQUEST, Json(json!({ "error": e }))),
        },
        None => None,
    };
    session.updated_at = chrono::Utc::now();

    match state.storage.save_session(&session) {
        Ok(_) => (StatusCode::OK, Json(json!(session))),
        Err(e) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(json!({ "error": e.to_string() })),
        ),
    }
}

async fn list_workspaces(State(state): State<AppState>) -> impl IntoResponse {
    let config = state.workspaces.read().await;
    Json(json!({
        "default": config.default,
        "allowed": config.allowed,
    }))
}

#[derive(Deserialize)]
struct AddWorkspaceRequest {
    path: String,
}

async fn add_workspace(
    State(state): State<AppState>,
    Json(req): Json<AddWorkspaceRequest>,
) -> impl IntoResponse {
    let path = match canonical_workspace(&req.path) {
        Ok(path) => path,
        Err(e) => return (StatusCode::BAD_REQUEST, Json(json!({ "error": e }))),
    };
    let mut config = state.workspaces.write().await;
    if !config
        .allowed
        .iter()
        .any(|item| item.eq_ignore_ascii_case(&path))
    {
        config.allowed.push(path.clone());
    }
    save_workspace_config(&config);
    (
        StatusCode::OK,
        Json(json!({ "default": config.default, "allowed": config.allowed, "added": path })),
    )
}

#[derive(Deserialize)]
struct RemoveWorkspaceQuery {
    path: String,
}

async fn remove_workspace(
    State(state): State<AppState>,
    Query(query): Query<RemoveWorkspaceQuery>,
) -> impl IntoResponse {
    let path = canonical_workspace(&query.path).unwrap_or_else(|_| query.path.clone());
    let mut config = state.workspaces.write().await;
    if path.eq_ignore_ascii_case(&config.default) {
        return (
            StatusCode::BAD_REQUEST,
            Json(json!({ "error": "The daemon default workspace cannot be removed." })),
        );
    }
    config
        .allowed
        .retain(|item| !item.eq_ignore_ascii_case(&path));
    save_workspace_config(&config);
    (
        StatusCode::OK,
        Json(json!({ "default": config.default, "allowed": config.allowed })),
    )
}

#[derive(Deserialize)]
struct BrowseWorkspaceQuery {
    path: Option<String>,
}

#[derive(Serialize)]
struct WorkspaceBrowseEntry {
    name: String,
    path: String,
    is_dir: bool,
}

/// Directory-only listing used by the composer's folder picker. It never reads
/// file contents, which keeps the picker from becoming an arbitrary file reader.
async fn browse_workspaces(
    State(state): State<AppState>,
    Query(query): Query<BrowseWorkspaceQuery>,
) -> impl IntoResponse {
    let target = match query.path.filter(|p| !p.trim().is_empty()) {
        Some(raw) => std::path::PathBuf::from(expand_home(&raw)),
        None => {
            let home = user_home();
            if home.is_dir() {
                home
            } else {
                std::path::PathBuf::from(&state.workspace_root)
            }
        }
    };
    let canonical = match std::fs::canonicalize(&target) {
        Ok(path) if path.is_dir() => path,
        _ => {
            return (
                StatusCode::BAD_REQUEST,
                Json(json!({ "error": "Folder not found or not accessible", "entries": [] })),
            );
        }
    };

    let mut entries: Vec<WorkspaceBrowseEntry> = Vec::new();
    if let Ok(read_dir) = std::fs::read_dir(&canonical) {
        for entry in read_dir.flatten().take(500) {
            let path = entry.path();
            if !path.is_dir() {
                continue;
            }
            entries.push(WorkspaceBrowseEntry {
                name: entry.file_name().to_string_lossy().to_string(),
                path: display_path(&path),
                is_dir: true,
            });
        }
    }
    entries.sort_by_key(|entry| entry.name.to_lowercase());

    let parent = canonical.parent().map(display_path);

    // Build a cheap roots list. Do NOT probe every drive letter: on Windows a
    // disconnected mapped/network drive makes `Path::exists()` block for a long
    // time, which used to hang this endpoint.
    let mut roots: Vec<String> = Vec::new();
    let mut push_root = |path: PathBuf| {
        let shown = display_path(&path);
        if !roots.iter().any(|item| item.eq_ignore_ascii_case(&shown)) {
            roots.push(shown);
        }
    };
    let home = user_home();
    if home.is_dir() {
        push_root(home);
    }
    push_root(PathBuf::from(&state.workspace_root));
    // Filesystem root of the currently browsed folder.
    let mut drive_root = canonical.clone();
    while let Some(parent) = drive_root.parent() {
        drive_root = parent.to_path_buf();
    }
    push_root(drive_root);
    if let Ok(system_drive) = std::env::var("SystemDrive") {
        let drive = system_drive.trim_end_matches('\\');
        if !drive.is_empty() {
            push_root(PathBuf::from(format!("{drive}\\")));
        }
    }
    // Also expose the filesystem root of every allowed workspace.
    for workspace in workspace_roots(&state).await {
        let mut root = workspace;
        while let Some(parent) = root.parent() {
            root = parent.to_path_buf();
        }
        push_root(root);
    }

    (
        StatusCode::OK,
        Json(json!({
            "current": display_path(&canonical),
            "parent": parent,
            "roots": roots,
            "entries": entries,
        })),
    )
}

async fn get_session(Path(id): Path<String>, State(state): State<AppState>) -> impl IntoResponse {
    match state.storage.get_session(&id) {
        Ok(session) => {
            let messages = state.storage.list_messages(&id).unwrap_or_default();
            let artifacts = state.storage.list_artifacts(&id).unwrap_or_default();
            // A client that reconnects after the event stream dropped has no way
            // to tell an in-flight turn from a finished one: events it missed are
            // simply gone. Reporting the live turn registry lets the UI restore
            // the "generating" state instead of guessing or staying stuck.
            let running = state.active_turns.read().await.contains_key(&id);
            (
                StatusCode::OK,
                Json(json!({
                    "session": session,
                    "messages": messages,
                    "artifacts": artifacts,
                    "running": running
                })),
            )
        }
        Err(e) => (
            StatusCode::NOT_FOUND,
            Json(json!({ "error": e.to_string() })),
        ),
    }
}

async fn delete_session(
    Path(id): Path<String>,
    State(state): State<AppState>,
) -> impl IntoResponse {
    match state.storage.delete_session(&id) {
        Ok(deleted) => (
            StatusCode::OK,
            Json(json!({ "deleted": deleted, "id": id })),
        ),
        Err(e) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(json!({ "error": e.to_string() })),
        ),
    }
}

async fn list_messages(
    Path(session_id): Path<String>,
    State(state): State<AppState>,
) -> impl IntoResponse {
    match state.storage.list_messages(&session_id) {
        Ok(messages) => (StatusCode::OK, Json(json!(messages))),
        Err(e) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(json!({ "error": e.to_string() })),
        ),
    }
}

#[derive(Deserialize)]
struct AttachmentInput {
    name: String,
    mime_type: String,
    data_base64: String,
}

#[derive(Deserialize)]
struct SendMessageRequest {
    text: String,
    model: Option<String>,
    effort: Option<String>,
    #[serde(default)]
    custom_instructions: Option<String>,
    #[serde(default)]
    plan_mode: Option<bool>,
    #[serde(default)]
    attachments: Vec<AttachmentInput>,
    /// Composer execution mode: `chat`, `auto`, `agent` or `plan`.
    #[serde(default)]
    mode: Option<String>,
}

/// What a composer execution mode actually changes for one turn. Kept as a pure
/// function so the mapping is testable without an HTTP round-trip.
#[derive(Debug, PartialEq)]
struct TurnPlan {
    plan_mode: bool,
    model: Option<String>,
    iteration_cap: Option<usize>,
    instructions: Option<String>,
}

fn resolve_turn_plan(
    mode: &str,
    requested_model: Option<&str>,
    plan_mode: Option<bool>,
    custom_instructions: Option<&str>,
) -> TurnPlan {
    // Plan mode is a per-turn decision, never inherited. Writing it on every turn
    // stops a flag left on by an earlier turn (or set by the agent itself) from
    // silently restricting a later chat turn.
    let plan_mode_enabled = plan_mode.unwrap_or(mode == "plan") || mode == "plan";
    // Auto means "let the provider pick": `odon-auto` resolves to each provider's
    // default model further down.
    let model = if mode == "auto" {
        Some("odon-auto".to_string())
    } else {
        requested_model.map(|m| m.to_string())
    };
    // Every mode uses the configured step budget. Agent mode used to add a
    // hardcoded 200 here, which is now *below* the 500 default and would cut
    // long autonomous work short; the runtime default (or
    // ODON_MAX_TURN_ITERATIONS) governs instead.
    let iteration_cap: Option<usize> = None;
    let mode_note = match mode {
        "plan" => Some(
            "Execution mode: PLAN. Investigate with read-only tools, then present a concrete plan with the todo list. Do not modify files until plan mode is left.",
        ),
        "agent" => Some(
            "Execution mode: AGENT. Work autonomously to completion. Break the work into todo items, and delegate independent side tasks with run_subagent rather than interleaving them yourself.",
        ),
        "auto" => Some(
            "Execution mode: AUTO. Use the provider's default model and choose the cheapest sufficient approach for each step.",
        ),
        _ => None,
    };
    let instructions = match (custom_instructions, mode_note) {
        (Some(custom), Some(note)) => Some(format!("{custom}\n\n{note}")),
        (Some(custom), None) => Some(custom.to_string()),
        (None, Some(note)) => Some(note.to_string()),
        (None, None) => None,
    };
    TurnPlan {
        plan_mode: plan_mode_enabled,
        model,
        iteration_cap,
        instructions,
    }
}

async fn send_message(
    Path(session_id): Path<String>,
    State(state): State<AppState>,
    Json(req): Json<SendMessageRequest>,
) -> impl IntoResponse {
    let mode = req
        .mode
        .as_deref()
        .unwrap_or("chat")
        .trim()
        .to_ascii_lowercase();
    let turn = resolve_turn_plan(
        &mode,
        req.model.as_deref(),
        req.plan_mode,
        req.custom_instructions.as_deref(),
    );
    let _ = odon_tools::harness::set_plan_mode(&state.storage, &session_id, turn.plan_mode);
    let requested_model = turn.model.clone();
    let iteration_cap = turn.iteration_cap;
    let turn_instructions = turn.instructions.clone();
    let cancel = tokio_util::sync::CancellationToken::new();
    let steer = SteerQueue::new();
    let turn_id = uuid::Uuid::new_v4().to_string();
    let attachments: Vec<odon_core::MessageAttachment> = req
        .attachments
        .into_iter()
        .map(|a| odon_core::MessageAttachment {
            name: a.name,
            mime_type: a.mime_type,
            data_base64: a.data_base64,
        })
        .collect();

    {
        let mut turns = state.active_turns.write().await;
        // Only one live turn per session: preempt an older one instead of
        // interleaving their writes.
        if let Some(previous) = turns.get(&session_id) {
            previous.cancel.cancel();
        }
        turns.insert(
            session_id.clone(),
            TurnHandle {
                turn_id: turn_id.clone(),
                cancel: cancel.clone(),
                steer: steer.clone(),
            },
        );
    }

    let res = state
        .runtime
        .execute_turn_with_cap(
            &session_id,
            &req.text,
            attachments,
            requested_model.as_deref(),
            req.effort.as_deref(),
            cancel,
            Some(steer),
            turn_instructions.as_deref(),
            iteration_cap,
        )
        .await;

    {
        let mut turns = state.active_turns.write().await;
        retire_turn_handle(&mut turns, &session_id, &turn_id);
    }

    match res {
        Ok(msg) => (StatusCode::OK, Json(json!({ "message": msg }))),
        Err(e) => {
            // Keep the transport body clean: the UI renders this directly.
            let message = match e {
                odon_core::OdonError::Provider { message, .. } => message,
                other => other.to_string(),
            };
            // A missing model is a configuration mistake the operator can fix in
            // Settings, not a gateway outage. Reporting 502 made the UI say
            // "failed to contact the AI model", which sent people to check API
            // keys and connectivity instead of picking a model.
            let status = if message.starts_with("No model selected")
                || message.starts_with("The configured model ID is empty")
            {
                StatusCode::BAD_REQUEST
            } else {
                StatusCode::BAD_GATEWAY
            };
            (status, Json(json!({ "error": message })))
        }
    }
}

async fn cancel_turn(
    Path(session_id): Path<String>,
    State(state): State<AppState>,
) -> impl IntoResponse {
    let turns = state.active_turns.read().await;
    if let Some(handle) = turns.get(&session_id) {
        handle.cancel.cancel();
        (
            StatusCode::OK,
            Json(json!({ "cancelled": true, "session_id": session_id })),
        )
    } else {
        (
            StatusCode::OK,
            Json(json!({ "cancelled": false, "session_id": session_id })),
        )
    }
}

#[derive(Deserialize)]
struct SteerRequest {
    text: String,
    #[serde(default)]
    attachments: Vec<AttachmentInput>,
    #[serde(default)]
    priority: bool,
    #[serde(default)]
    steer_id: Option<String>,
}

async fn steer_turn(
    Path(session_id): Path<String>,
    State(state): State<AppState>,
    Json(req): Json<SteerRequest>,
) -> impl IntoResponse {
    let text = req.text.trim().to_string();
    if text.is_empty() {
        return (
            StatusCode::BAD_REQUEST,
            Json(json!({ "error": "Steer text is empty" })),
        );
    }
    let attachments: Vec<odon_core::MessageAttachment> = req
        .attachments
        .into_iter()
        .map(|a| odon_core::MessageAttachment {
            name: a.name,
            mime_type: a.mime_type,
            data_base64: a.data_base64,
        })
        .collect();
    let steer_id = req
        .steer_id
        .unwrap_or_else(|| uuid::Uuid::new_v4().to_string());

    let turns = state.active_turns.read().await;
    if let Some(handle) = turns.get(&session_id) {
        handle.steer.push(SteerMessage {
            id: steer_id.clone(),
            text,
            attachments,
            priority: req.priority,
        });
        (
            StatusCode::OK,
            Json(json!({
                "queued": true,
                "steer_id": steer_id,
                "priority": req.priority,
            })),
        )
    } else {
        // The turn already finished; the UI falls back to a normal message.
        (
            StatusCode::OK,
            Json(json!({ "queued": false, "steer_id": steer_id })),
        )
    }
}

/// Truncate a string on a UTF-8 char boundary (String::truncate panics
/// when the byte index lands inside a multi-byte character).
fn truncate_on_char_boundary(value: &mut String, max_bytes: usize) {
    if value.len() <= max_bytes {
        return;
    }
    let mut end = max_bytes;
    while end > 0 && !value.is_char_boundary(end) {
        end -= 1;
    }
    value.truncate(end);
}

fn mask_secret(secret: &str) -> String {
    let chars: Vec<char> = secret.chars().collect();
    if chars.len() > 8 {
        // Char-based masking: byte slicing panics on multi-byte UTF-8.
        let head: String = chars[..4].iter().collect();
        let tail: String = chars[chars.len() - 4..].iter().collect();
        format!("{}...{}", head, tail)
    } else if secret.is_empty() {
        String::new()
    } else {
        "********".to_string()
    }
}

async fn get_telegram_relay(State(state): State<AppState>) -> impl IntoResponse {
    let config = state.relay_config.read().await;
    let online = state
        .telegram_online
        .load(std::sync::atomic::Ordering::SeqCst);
    match &config.telegram {
        Some(telegram) => Json(json!({
            "configured": true,
            "online": online,
            "token_masked": mask_secret(&telegram.token),
            "allowed_users": telegram.allowed_users,
        })),
        None => Json(json!({
            "configured": false,
            "online": false,
            "token_masked": "",
            "allowed_users": [],
        })),
    }
}

#[derive(Deserialize)]
struct TelegramRelayRequest {
    token: String,
    #[serde(default)]
    allowed_users: Vec<String>,
}

async fn save_telegram_relay(
    State(state): State<AppState>,
    Json(req): Json<TelegramRelayRequest>,
) -> impl IntoResponse {
    let token = req.token.trim().to_string();
    if token.is_empty() {
        return (
            StatusCode::BAD_REQUEST,
            Json(json!({ "error": "Telegram bot token is required" })),
        );
    }
    let allowed_users: Vec<String> = req
        .allowed_users
        .into_iter()
        .map(|item| item.trim().trim_start_matches('@').to_lowercase())
        .filter(|item| !item.is_empty())
        .collect();

    if let Some(previous) = state.telegram.write().await.take() {
        previous.stop();
    }
    let relay = Arc::new(TelegramRelay::with_allowlist(
        Some(token.clone()),
        allowed_users.clone(),
    ));
    if let Err(error) = relay.start(state.relay_tx.clone()).await {
        state
            .telegram_online
            .store(false, std::sync::atomic::Ordering::SeqCst);
        return (
            StatusCode::BAD_GATEWAY,
            Json(json!({ "error": error.to_string() })),
        );
    }
    *state.telegram.write().await = Some(relay);
    state
        .telegram_online
        .store(true, std::sync::atomic::Ordering::SeqCst);

    let mut config = state.relay_config.write().await;
    config.telegram = Some(TelegramRelayConfig {
        token: token.clone(),
        allowed_users: allowed_users.clone(),
    });
    save_relay_config(&config);

    (
        StatusCode::OK,
        Json(json!({
            "configured": true,
            "online": true,
            "token_masked": mask_secret(&token),
            "allowed_users": allowed_users,
        })),
    )
}

async fn disconnect_telegram_relay(State(state): State<AppState>) -> impl IntoResponse {
    if let Some(previous) = state.telegram.write().await.take() {
        previous.stop();
    }
    state
        .telegram_online
        .store(false, std::sync::atomic::Ordering::SeqCst);
    let mut config = state.relay_config.write().await;
    config.telegram = None;
    save_relay_config(&config);
    Json(json!({ "configured": false, "online": false }))
}

async fn get_whatsapp_bridge_status(State(state): State<AppState>) -> impl IntoResponse {
    Json(state.whatsapp_bridge.status().await)
}

#[derive(Deserialize)]
struct WhatsAppBridgeStartRequest {
    #[serde(default)]
    phone: Option<String>,
    #[serde(default)]
    allowed_numbers: Vec<String>,
    #[serde(default)]
    allow_anyone: bool,
}

async fn start_whatsapp_bridge(
    State(state): State<AppState>,
    Json(req): Json<WhatsAppBridgeStartRequest>,
) -> impl IntoResponse {
    let (allowed_numbers, allow_anyone) = {
        let config = state.relay_config.read().await;
        match &config.whatsapp_web {
            Some(web) => (
                if req.allowed_numbers.is_empty() {
                    web.allowed_numbers.clone()
                } else {
                    req.allowed_numbers.clone()
                },
                req.allow_anyone || web.allow_anyone,
            ),
            None => (req.allowed_numbers.clone(), req.allow_anyone),
        }
    };
    let phone = req
        .phone
        .map(|value| value.trim().replace(|c: char| !c.is_ascii_digit(), ""))
        .filter(|value| !value.is_empty());

    // A new pairing phone requires a fresh bridge process so it can request a
    // pairing code via WA_PAIRING_PHONE; an existing connected session is left
    // untouched when the phone is unchanged.
    let phone_changed = {
        let config = state.relay_config.read().await;
        config
            .whatsapp_web
            .as_ref()
            .and_then(|web| web.phone.clone())
            != phone
    };
    if phone.is_some() && phone_changed {
        state.whatsapp_bridge.stop();
    }

    match state
        .whatsapp_bridge
        .start(phone.as_deref(), &allowed_numbers, allow_anyone)
        .await
    {
        Ok(status) => {
            let mut config = state.relay_config.write().await;
            config.whatsapp_web = Some(WhatsAppWebConfig {
                enabled: true,
                phone,
                allowed_numbers,
                allow_anyone,
            });
            save_relay_config(&config);
            (StatusCode::OK, Json(status))
        }
        Err(error) => (StatusCode::BAD_GATEWAY, Json(json!({ "error": error }))),
    }
}

async fn logout_whatsapp_bridge(State(state): State<AppState>) -> impl IntoResponse {
    let result = state.whatsapp_bridge.logout().await;
    state
        .whatsapp_online
        .store(false, std::sync::atomic::Ordering::SeqCst);
    let mut config = state.relay_config.write().await;
    if let Some(web) = config.whatsapp_web.as_mut() {
        web.enabled = false;
    }
    save_relay_config(&config);
    match result {
        Ok(status) => (StatusCode::OK, Json(status)),
        Err(error) => (StatusCode::BAD_GATEWAY, Json(json!({ "error": error }))),
    }
}

#[derive(Deserialize)]
struct WhatsAppBridgeInbound {
    from: String,
    text: String,
    #[serde(default)]
    attachments: Vec<odon_core::MessageAttachment>,
    #[serde(default)]
    #[allow(dead_code)]
    id: Option<String>,
    #[serde(default)]
    #[allow(dead_code)]
    push_name: Option<String>,
}

async fn whatsapp_bridge_inbound(
    State(state): State<AppState>,
    headers: axum::http::HeaderMap,
    Json(req): Json<WhatsAppBridgeInbound>,
) -> impl IntoResponse {
    let presented = headers
        .get("x-bridge-token")
        .and_then(|value| value.to_str().ok());
    if !state.whatsapp_bridge.verify_bridge_token(presented) {
        return (
            StatusCode::UNAUTHORIZED,
            Json(json!({ "error": "Invalid bridge token" })),
        );
    }
    let digits: String = req
        .from
        .split('@')
        .next()
        .unwrap_or("")
        .chars()
        .filter(|c| c.is_ascii_digit())
        .collect();
    {
        let config = state.relay_config.read().await;
        let Some(web) = &config.whatsapp_web else {
            return (
                StatusCode::FORBIDDEN,
                Json(json!({ "error": "WhatsApp Web relay is not configured" })),
            );
        };
        if !web.allow_anyone && !web.allowed_numbers.iter().any(|number| number == &digits) {
            return (
                StatusCode::FORBIDDEN,
                Json(json!({ "error": "Sender is not in WHATSAPP_ALLOWED_NUMBERS" })),
            );
        }
    }
    let text = req.text.trim().to_string();
    if text.is_empty() && req.attachments.is_empty() {
        return (
            StatusCode::BAD_REQUEST,
            Json(json!({ "error": "Empty message" })),
        );
    }
    let text = if text.is_empty() {
        "[WhatsApp media message]".to_string()
    } else {
        text
    };
    let message = IncomingChannelMessage {
        channel: "whatsapp".to_string(),
        sender_id: req.from.clone(),
        session_id: Some(format!("wa_{}", req.from)),
        text,
        attachments: req.attachments,
    };
    let _ = state.relay_tx.send(message).await;
    (StatusCode::OK, Json(json!({ "accepted": true })))
}

#[derive(Deserialize)]
struct TestProviderRequest {
    provider_id: String,
    #[serde(default)]
    model: Option<String>,
}

/// Sends a minimal request straight to a provider so the UI can distinguish an
/// auth problem, a bad Base URL, an unknown model, and an upstream 5xx.
async fn test_provider(
    State(state): State<AppState>,
    Json(req): Json<TestProviderRequest>,
) -> impl IntoResponse {
    use odon_provider::{ChatRequest, StreamEvent};

    let provider = match state.runtime.get_provider(Some(&req.provider_id)).await {
        Some(p) => p,
        None => {
            return Json(json!({
                "ok": false,
                "provider": req.provider_id,
                "error": "Provider is not configured. Save an API key or sign in first."
            }));
        }
    };

    let model = req
        .model
        .filter(|m| !m.trim().is_empty())
        .unwrap_or_else(|| "odon-auto".to_string());
    let request = ChatRequest {
        messages: vec![odon_core::Message::new_user(
            "provider-test",
            "Reply with the single word: OK",
        )],
        model: model.clone(),
        system_prompt: Some("You are a connection test. Reply briefly.".to_string()),
        tools: Vec::new(),
        temperature: Some(0.0),
        max_tokens: Some(24),
        effort: None,
    };

    let (tx, mut rx) = tokio::sync::mpsc::channel(64);
    let cancel = tokio_util::sync::CancellationToken::new();
    let started = std::time::Instant::now();
    let prov = provider.clone();
    let task = tokio::spawn(async move { prov.stream_chat(request, tx, cancel).await });

    let collect = async {
        let mut text = String::new();
        while let Some(evt) = rx.recv().await {
            match evt {
                StreamEvent::TextDelta(d) => text.push_str(&d),
                StreamEvent::Error(e) => return Err(e),
                StreamEvent::Done => break,
                _ => {}
            }
        }
        Ok::<String, String>(text)
    };

    let outcome = tokio::time::timeout(std::time::Duration::from_secs(45), collect).await;
    let latency_ms = started.elapsed().as_millis() as u64;

    match outcome {
        Ok(Ok(text)) => {
            let _ = task.await;
            Json(json!({
                "ok": true,
                "provider": req.provider_id,
                "model": model,
                "latency_ms": latency_ms,
                "sample": text.chars().take(400).collect::<String>(),
            }))
        }
        Ok(Err(err)) => {
            let _ = task.await;
            Json(json!({
                "ok": false,
                "provider": req.provider_id,
                "model": model,
                "latency_ms": latency_ms,
                "error": err,
            }))
        }
        Err(_) => {
            task.abort();
            Json(json!({
                "ok": false,
                "provider": req.provider_id,
                "model": model,
                "latency_ms": latency_ms,
                "error": "Timed out after 45s. The endpoint is reachable but did not stream a response.",
            }))
        }
    }
}

async fn list_session_artifacts(
    Path(session_id): Path<String>,
    State(state): State<AppState>,
) -> impl IntoResponse {
    match state.storage.list_artifacts(&session_id) {
        Ok(artifacts) => (StatusCode::OK, Json(json!(artifacts))),
        Err(e) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(json!({ "error": e.to_string() })),
        ),
    }
}

async fn get_session_todos(
    Path(session_id): Path<String>,
    State(state): State<AppState>,
) -> impl IntoResponse {
    match state.storage.get_todos(&session_id) {
        Ok(items) => (StatusCode::OK, Json(json!({ "items": items }))),
        Err(e) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(json!({ "error": e.to_string() })),
        ),
    }
}

async fn get_artifact(Path(id): Path<String>, State(state): State<AppState>) -> impl IntoResponse {
    match state.storage.get_artifact(&id) {
        Ok(artifact) => (StatusCode::OK, Json(json!(artifact))),
        Err(e) => (
            StatusCode::NOT_FOUND,
            Json(json!({ "error": e.to_string() })),
        ),
    }
}

async fn list_artifact_versions(
    Path(id): Path<String>,
    State(state): State<AppState>,
) -> impl IntoResponse {
    match state.storage.list_artifact_versions(&id) {
        Ok(versions) => (StatusCode::OK, Json(json!(versions))),
        Err(error) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(json!({ "error": error.to_string() })),
        ),
    }
}

async fn get_artifact_version(
    Path((id, version)): Path<(String, i64)>,
    State(state): State<AppState>,
) -> impl IntoResponse {
    match state.storage.get_artifact_version(&id, version) {
        Ok(item) => (StatusCode::OK, Json(json!(item))),
        Err(error) => (
            StatusCode::NOT_FOUND,
            Json(json!({ "error": error.to_string() })),
        ),
    }
}

async fn restore_artifact_version(
    Path((id, version)): Path<(String, i64)>,
    State(state): State<AppState>,
) -> impl IntoResponse {
    match state.storage.restore_artifact_version(&id, version) {
        Ok(artifact) => (StatusCode::OK, Json(json!(artifact))),
        Err(error) => (
            StatusCode::NOT_FOUND,
            Json(json!({ "error": error.to_string() })),
        ),
    }
}

#[derive(Deserialize)]
struct ArtifactExportQuery {
    #[serde(default)]
    format: Option<String>,
}

/// Escape HTML special chars for safely interpolating into markup.
fn html_escape(value: &str) -> String {
    value
        .replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
        .replace('\'', "&#39;")
}

fn artifact_export_file_name(title: &str, extension: &str) -> String {
    let safe: String = title
        .chars()
        .map(|ch| {
            if ch.is_ascii_alphanumeric() || matches!(ch, ' ' | '-' | '_' | '.') {
                ch
            } else {
                '-'
            }
        })
        .collect::<String>()
        .trim()
        .replace(' ', "-")
        .trim_matches('-')
        .to_string();
    let base = if safe.is_empty() {
        "artifact".to_string()
    } else {
        safe
    };
    format!("{base}.{extension}")
}

fn artifact_export_content(
    artifact: &odon_core::Artifact,
    format: &str,
) -> Option<(String, String)> {
    match format {
        "json" => Some((
            serde_json::to_string_pretty(artifact).ok()?,
            "application/json".to_string(),
        )),
        "md" | "markdown" => match &artifact.payload {
            ArtifactPayload::Document { markdown, .. } => Some((
                markdown.clone(),
                "text/markdown".to_string(),
            )),
            ArtifactPayload::Code { files, .. } => {
                let content = files
                    .iter()
                    .map(|file| format!("## {}\n\n```\n{}\n```", file.path, file.content))
                    .collect::<Vec<_>>()
                    .join("\n\n");
                Some((content, "text/markdown".to_string()))
            }
            _ => None,
        },
        "html" => match &artifact.payload {
            ArtifactPayload::Document { markdown, .. } => Some((
                format!(
                    "<!doctype html><meta charset=\"utf-8\"><title>{}</title><pre style=\"white-space:pre-wrap;font-family:system-ui\">{}</pre>",
                    html_escape(&artifact.title),
                    html_escape(markdown)
                ),
                "text/html".to_string(),
            )),
            _ => None,
        },
        _ => None,
    }
}

async fn export_artifact(
    Path(id): Path<String>,
    Query(query): Query<ArtifactExportQuery>,
    State(state): State<AppState>,
) -> impl IntoResponse {
    let format = query
        .format
        .unwrap_or_else(|| "json".to_string())
        .to_ascii_lowercase();
    let Ok(artifact) = state.storage.get_artifact(&id) else {
        return (
            StatusCode::NOT_FOUND,
            [(header::CONTENT_TYPE, "application/json")],
            json!({ "error": "Artifact not found" })
                .to_string()
                .into_bytes(),
        )
            .into_response();
    };
    let Some((content, mime)) = artifact_export_content(&artifact, &format) else {
        return (
            StatusCode::BAD_REQUEST,
            [(header::CONTENT_TYPE, "application/json")],
            json!({ "error": format!("Unsupported export format '{format}'") })
                .to_string()
                .into_bytes(),
        )
            .into_response();
    };
    let extension = if mime == "application/json" {
        "json"
    } else if mime == "text/html" {
        "html"
    } else {
        "md"
    };
    let file_name = artifact_export_file_name(&artifact.title, extension);
    (
        StatusCode::OK,
        [
            (header::CONTENT_TYPE, mime),
            (
                header::CONTENT_DISPOSITION,
                format!("attachment; filename=\"{file_name}\""),
            ),
        ],
        content.into_bytes(),
    )
        .into_response()
}

async fn list_tools(State(state): State<AppState>) -> impl IntoResponse {
    let schemas = state.tools.schemas();
    Json(json!(schemas))
}

async fn list_agent_memories(State(state): State<AppState>) -> impl IntoResponse {
    let mut memories = state.storage.list_memories(None).unwrap_or_default();
    // Plan-mode and todo state live in the same table but are implementation
    // details, not user-facing long-term memory.
    memories.retain(|item| item.category != "plan_mode" && item.category != "todos");
    Json(json!(memories))
}

async fn delete_agent_memory(
    Path(key): Path<String>,
    State(state): State<AppState>,
) -> impl IntoResponse {
    let deleted = state.storage.delete_memory(&key).unwrap_or(false);
    Json(json!({ "deleted": deleted, "key": key }))
}

async fn list_approvals(State(state): State<AppState>) -> impl IntoResponse {
    let pending = state.approval.list_pending().await;
    Json(json!(pending))
}

#[derive(Deserialize)]
struct ResolveApprovalRequest {
    decision: String, // "approved" or "rejected"
    reason: Option<String>,
}

async fn resolve_approval(
    Path(id): Path<String>,
    State(state): State<AppState>,
    Json(req): Json<ResolveApprovalRequest>,
) -> impl IntoResponse {
    let dec = match req.decision.to_ascii_lowercase().as_str() {
        "approved" | "approve_once" => ApprovalDecision::Approved,
        "approved_always" | "approve_always" => ApprovalDecision::ApprovedAlways,
        "approved_all" | "approve_all" | "allow_all" => ApprovalDecision::ApprovedAll,
        _ => ApprovalDecision::Rejected {
            reason: req
                .reason
                .unwrap_or_else(|| "User rejected action".to_string()),
        },
    };

    let resolved = state.approval.submit_decision(&id, dec).await;
    Json(json!({ "resolved": resolved, "id": id }))
}

async fn get_settings(State(state): State<AppState>) -> impl IntoResponse {
    let policy = state.approval.get_policy().await;
    Json(json!({
        "execution_mode": format!("{:?}", policy.mode),
        "workspace_root": state.workspace_root,
    }))
}

#[derive(Deserialize)]
struct UpdateSettingsRequest {
    mode: Option<String>, // "autonomous", "supervised", "locked"
}

async fn update_settings(
    State(state): State<AppState>,
    Json(req): Json<UpdateSettingsRequest>,
) -> impl IntoResponse {
    if let Some(m) = req.mode {
        let exec_mode = match m.to_lowercase().as_str() {
            "autonomous" => ExecutionMode::Autonomous,
            "supervised" => ExecutionMode::Supervised,
            "locked" => ExecutionMode::Locked,
            other => {
                return (
                    StatusCode::BAD_REQUEST,
                    Json(json!({ "error": format!("Unknown execution mode: {other}") })),
                )
                    .into_response();
            }
        };
        state.approval.update_mode(exec_mode).await;
    }
    let policy = state.approval.get_policy().await;
    Json(json!({ "execution_mode": format!("{:?}", policy.mode) })).into_response()
}

#[derive(Deserialize)]
struct TerminalExecRequest {
    command: String,
    cwd: Option<String>,
}

async fn get_terminal_info(State(state): State<AppState>) -> impl IntoResponse {
    let username = std::env::var("USERNAME")
        .or_else(|_| std::env::var("USER"))
        .unwrap_or_else(|_| "developer".to_string());

    let shell_name = if cfg!(target_os = "windows") {
        "PowerShell"
    } else {
        "bash"
    };

    Json(json!({
        "shell": shell_name,
        "os": std::env::consts::OS,
        "arch": std::env::consts::ARCH,
        "default_cwd": state.workspace_root,
        "user": username,
    }))
}

async fn execute_terminal(
    State(state): State<AppState>,
    Json(req): Json<TerminalExecRequest>,
) -> impl IntoResponse {
    let working_dir = req.cwd.unwrap_or_else(|| state.workspace_root.clone());
    let working_path = std::path::Path::new(&working_dir);
    let final_dir = if working_path.exists() {
        working_dir
    } else {
        state.workspace_root.clone()
    };

    let command_str = req.command.trim();
    if command_str.is_empty() {
        return (
            StatusCode::OK,
            Json(json!({
                "exit_code": 0,
                "stdout": "",
                "stderr": "",
                "cwd": final_dir,
                "success": true,
            })),
        );
    }

    let policy = state.approval.get_policy().await;
    if let Some(reason) = policy.command_forbidden_reason(command_str) {
        return (
            StatusCode::FORBIDDEN,
            Json(json!({
                "exit_code": 1,
                "stdout": "",
                "stderr": format!("Command blocked by security policy: {reason}"),
                "cwd": final_dir,
                "success": false,
            })),
        );
    }

    let session_id = "terminal";
    let parameters = json!({"command": command_str, "cwd": final_dir});
    let description = format!("Run terminal command: {command_str}");
    match state
        .approval
        .evaluate_and_request(session_id, "terminal_exec", &parameters, &description)
        .await
    {
        Ok(odon_security::ApprovalDecision::Approved)
        | Ok(odon_security::ApprovalDecision::ApprovedAlways)
        | Ok(odon_security::ApprovalDecision::ApprovedAll) => {}
        Ok(odon_security::ApprovalDecision::Rejected { reason }) => {
            return (
                StatusCode::FORBIDDEN,
                Json(json!({
                    "exit_code": 1,
                    "stdout": "",
                    "stderr": format!("Command execution denied: {reason}"),
                    "cwd": final_dir,
                    "success": false,
                })),
            );
        }
        Ok(odon_security::ApprovalDecision::Expired) | Err(_) => {
            return (
                StatusCode::FORBIDDEN,
                Json(json!({
                    "exit_code": 1,
                    "stdout": "",
                    "stderr": "Command execution approval unavailable or timed out",
                    "cwd": final_dir,
                    "success": false,
                })),
            );
        }
    }

    // Run the command directly instead of interpolating it into a larger
    // shell script. The endpoint is a terminal by design, but this removes a
    // second layer of shell parsing and `}`/newline breakout from the wrapper.
    #[cfg(target_os = "windows")]
    let mut cmd = {
        let mut c = tokio::process::Command::new("powershell.exe");
        c.args([
            "-NoProfile",
            "-NonInteractive",
            "-ExecutionPolicy",
            "Bypass",
            "-Command",
            command_str,
        ]);
        c
    };

    #[cfg(not(target_os = "windows"))]
    let mut cmd = {
        let mut c = tokio::process::Command::new("sh");
        c.args(["-c", command_str]);
        c
    };

    cmd.current_dir(&final_dir);
    cmd.stdout(Stdio::piped());
    cmd.stderr(Stdio::piped());
    sanitize_child_environment(&mut cmd);

    let execution =
        match tokio::time::timeout(std::time::Duration::from_secs(120), cmd.output()).await {
            Ok(result) => result,
            Err(_) => {
                return (
                    StatusCode::GATEWAY_TIMEOUT,
                    Json(json!({
                        "exit_code": -1,
                        "stdout": "",
                        "stderr": "Process execution timed out after 120 seconds",
                        "success": false
                    })),
                );
            }
        };
    let output = match execution {
        Ok(output) => output,
        Err(e) => {
            return (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(json!({
                    "exit_code": 1,
                    "stdout": "",
                    "stderr": format!("Process execution failed: {}", e),
                    "success": false
                })),
            );
        }
    };
    let full_stdout = String::from_utf8_lossy(&output.stdout).to_string();
    let stderr = String::from_utf8_lossy(&output.stderr).to_string();
    const MAX_OUTPUT_BYTES: usize = 1_048_576;
    if full_stdout.len() > MAX_OUTPUT_BYTES || stderr.len() > MAX_OUTPUT_BYTES {
        let mut truncated = false;
        let mut capped_stdout = full_stdout;
        let mut capped_stderr = stderr;
        if capped_stdout.len() > MAX_OUTPUT_BYTES {
            truncate_on_char_boundary(&mut capped_stdout, MAX_OUTPUT_BYTES);
            truncated = true;
        }
        if capped_stderr.len() > MAX_OUTPUT_BYTES {
            truncate_on_char_boundary(&mut capped_stderr, MAX_OUTPUT_BYTES);
            truncated = true;
        }
        if truncated {
            capped_stderr.push_str("\n[ODON] output capped at 1 MB");
        }
        let exit_code = output
            .status
            .code()
            .unwrap_or(if output.status.success() { 0 } else { 1 });

        return (
            StatusCode::OK,
            Json(json!({

                "exit_code": exit_code,

                "stdout": capped_stdout,

                "stderr": capped_stderr,

                "success": output.status.success()

            })),
        );
    }
    let exit_code = output
        .status
        .code()
        .unwrap_or(if output.status.success() { 0 } else { 1 });

    let mut updated_cwd = final_dir;
    let mut cleaned_stdout = full_stdout;
    if let Some(pos) = cleaned_stdout.rfind("__ODON_CWD__:") {
        let cwd_part = cleaned_stdout[pos + 13..].trim();
        if cwd_part.starts_with("C:\\") || cwd_part.starts_with("/") {
            updated_cwd = cwd_part.trim_end_matches("__ODON_CWD__").to_string();
            cleaned_stdout.truncate(pos);
            if cleaned_stdout.ends_with("\r\n") {
                cleaned_stdout.truncate(cleaned_stdout.len() - 2);
            } else if cleaned_stdout.ends_with('\n') {
                cleaned_stdout.truncate(cleaned_stdout.len() - 1);
            }
        }
    }

    (
        StatusCode::OK,
        Json(json!({
            "exit_code": exit_code,
            "stdout": cleaned_stdout,
            "stderr": stderr,
            "cwd": updated_cwd,
            "success": exit_code == 0,
        })),
    )
}

#[derive(Deserialize)]
struct OAuthLoginRequest {
    provider: String,
}

#[derive(Deserialize)]
struct OAuthDisconnectRequest {
    provider: String,
}

fn default_base_url_for(provider: &str) -> String {
    match provider {
        "openai" => "https://api.openai.com/v1".to_string(),
        "google" => "https://generativelanguage.googleapis.com/v1beta/openai".to_string(),
        "deepseek" => "https://api.deepseek.com/v1".to_string(),
        "anthropic" => "https://api.anthropic.com/v1".to_string(),
        "qwen" => "https://dashscope-intl.aliyuncs.com/compatible-mode/v1".to_string(),
        "alibaba" => "https://dashscope.aliyuncs.com/compatible-mode/v1".to_string(),
        "stepfun" => "https://api.stepfun.com/v1".to_string(),
        "mistral" => "https://api.mistral.ai/v1".to_string(),
        "openrouter" => "https://openrouter.ai/api/v1".to_string(),
        "groq" => "https://api.groq.com/openai/v1".to_string(),
        "ollama" => "http://localhost:11434/v1".to_string(),
        // Meta Model API is OpenAI-compatible and serves Muse on /v1, so a Muse
        // BYOK config without an explicit URL must not fall through to Ollama.
        "muse" => "https://api.meta.ai/v1".to_string(),
        _ => "http://localhost:11434/v1".to_string(),
    }
}

#[allow(unused_assignments)]
async fn init_providers(
    runtime: &Arc<AgentRuntime>,
    storage: &Arc<StorageEngine>,
    oauth_accounts: &Arc<tokio::sync::RwLock<HashMap<String, serde_json::Value>>>,
) {
    info!("Initializing AI Model Providers (Strict Real Providers Mode - No Mock Fallback)...");

    #[allow(unused_assignments)]
    let mut has_default = false;

    // A. Auto-discover GitHub Copilot from host credentials
    if let Some(token) = discover_github_copilot().await {
        info!("✔ GitHub Copilot host authentication discovered and verified");
        let prov = Arc::new(OpenAiCompatibleProvider::new(
            "https://api.githubcopilot.com".to_string(),
            token.access_token.clone(),
            Some("github-copilot"),
        ));
        runtime
            .register_provider("github-copilot", prov.clone(), true)
            .await;
        runtime.register_provider("default", prov, true).await;
        has_default = true;

        oauth_accounts.write().await.insert(
            "github-copilot".to_string(),
            json!({
                "provider": "github-copilot",
                "email": "GitHub Copilot Host",
                "plan": "GitHub Copilot Active",
                "account_id": null,
                "connected": true,
                "usable": true,
            }),
        );
    }

    // B. Auto-discover Google Antigravity from host credentials
    if let Some(token) = discover_antigravity().await {
        info!("✔ Google Antigravity host authentication discovered and refreshed");
        let prov = Arc::new(AntigravityProvider::new(
            token.access_token.clone(),
            token.account_id.clone(),
            None,
        ));
        runtime
            .register_provider("antigravity", prov.clone(), !has_default)
            .await;
        if !has_default {
            runtime.register_provider("default", prov, true).await;
            has_default = true;
        }

        oauth_accounts.write().await.insert(
            "antigravity".to_string(),
            json!({
                "provider": "antigravity",
                "email": token.email.unwrap_or_else(|| "Google Cloud Code".to_string()),
                "plan": "Antigravity",
                "account_id": token.account_id,
                "connected": true,
                "usable": true,
            }),
        );
    }

    // B2. Restore OAuth providers saved from previous sign-ins.
    for (catalog_id, token) in load_oauth_tokens() {
        if runtime.get_provider(Some(&catalog_id)).await.is_some() {
            continue;
        }
        if let Some(prov) = build_oauth_provider(&catalog_id, &token) {
            info!("Restoring persisted OAuth provider: {}", catalog_id);
            runtime
                .register_provider(&catalog_id, prov.clone(), !has_default)
                .await;
            if !has_default {
                runtime.register_provider("default", prov, true).await;
                has_default = true;
            }
            let account = token.account();
            oauth_accounts.write().await.insert(
                catalog_id.clone(),
                json!({
                    "provider": catalog_id,
                    "email": account.email,
                    "plan": account.plan,
                    "account_id": account.account_id,
                    "connected": true,
                    "usable": true,
                }),
            );
        }
    }

    // C. Load saved BYOK configs from SQLite database
    if let Ok(configs) = storage.list_provider_configs() {
        for c in configs {
            let pid = c.provider_id.to_lowercase();
            let base_url = c.base_url.unwrap_or_else(|| default_base_url_for(&pid));
            info!("Loading saved BYOK provider: {} (url: {})", pid, base_url);

            let prov: Arc<dyn odon_provider::ModelProvider> = if pid == "anthropic" {
                Arc::new(AnthropicProvider::with_base_url(&c.api_key, &base_url))
            } else {
                Arc::new(OpenAiCompatibleProvider::new(
                    &base_url,
                    &c.api_key,
                    Some(&pid),
                ))
            };

            let is_def = c.is_default || !has_default;
            runtime.register_provider(&pid, prov.clone(), is_def).await;
            if is_def {
                runtime.register_provider("default", prov, true).await;
                has_default = true;
            }
        }
    }

    let _ = has_default;

    // D. Environment variables fallback
    if let Ok(key) = std::env::var("OPENAI_API_KEY") {
        if !key.trim().is_empty() {
            let base_url = std::env::var("OPENAI_BASE_URL")
                .unwrap_or_else(|_| "https://api.openai.com/v1".to_string());
            let prov = Arc::new(OpenAiCompatibleProvider::new(base_url, key, Some("openai")));
            runtime
                .register_provider("openai", prov.clone(), !has_default)
                .await;
            if !has_default {
                runtime.register_provider("default", prov, true).await;
                has_default = true;
            }
        }
    }

    if let Ok(key) = std::env::var("ANTHROPIC_API_KEY") {
        if !key.trim().is_empty() {
            let prov = Arc::new(AnthropicProvider::new(key));
            runtime
                .register_provider("anthropic", prov.clone(), !has_default)
                .await;
            if !has_default {
                runtime.register_provider("default", prov, true).await;
                has_default = true;
            }
        }
    }

    if let Ok(key) = std::env::var("DEEPSEEK_API_KEY") {
        if !key.trim().is_empty() {
            let prov = Arc::new(OpenAiCompatibleProvider::new_deepseek(key));
            runtime
                .register_provider("deepseek", prov.clone(), !has_default)
                .await;
            if !has_default {
                runtime.register_provider("default", prov, true).await;
                has_default = true;
            }
        }
    }

    // Only register simulated under its explicit key for offline testing, NEVER as default
    runtime
        .register_provider(
            "odon-simulated",
            Arc::new(SimulatedProvider::default()),
            false,
        )
        .await;
}

#[derive(Deserialize)]
struct SaveProviderConfigRequest {
    provider_id: String,
    api_key: String,
    base_url: Option<String>,
    is_default: Option<bool>,
    #[serde(default)]
    name: Option<String>,
    #[serde(default)]
    models: Option<Vec<String>>,
}

async fn get_provider_configs(State(state): State<AppState>) -> impl IntoResponse {
    let configs = state.storage.list_provider_configs().unwrap_or_default();
    let mut result = Vec::new();
    for c in configs {
        let masked = mask_secret(&c.api_key);
        result.push(json!({
            "provider_id": c.provider_id,
            "configured": true,
            "masked_key": masked,
            "base_url": c.base_url,
            "is_default": c.is_default,
            "name": c.name,
            "models": serde_json::from_str::<Vec<String>>(&c.models_json).unwrap_or_default(),
            "updated_at": c.updated_at,
        }));
    }
    Json(result)
}

async fn save_provider_config(
    State(state): State<AppState>,
    Json(req): Json<SaveProviderConfigRequest>,
) -> impl IntoResponse {
    let pid = req.provider_id.to_lowercase();
    let is_def = req.is_default.unwrap_or(false);
    let base_url = req
        .base_url
        .clone()
        .unwrap_or_else(|| default_base_url_for(&pid));

    let existing = state.storage.get_provider_config(&pid).ok().flatten();

    // Leaving the key field blank in the UI means "keep the stored key"; it
    // must never wipe or replace a configured credential with an empty string.
    // The same applies to name/models: older Save Key callers omit them.
    let api_key = if req.api_key.trim().is_empty() {
        existing
            .as_ref()
            .map(|c| c.api_key.clone())
            .unwrap_or_default()
    } else {
        req.api_key.clone()
    };
    let name = req
        .name
        .clone()
        .or_else(|| existing.as_ref().and_then(|c| c.name.clone()));
    let models_json = match &req.models {
        Some(models) => serde_json::to_string(models).unwrap_or_else(|_| "[]".to_string()),
        None => existing
            .as_ref()
            .map(|c| c.models_json.clone())
            .unwrap_or_else(|| "[]".to_string()),
    };

    if let Err(e) = state.storage.save_provider_config(
        &pid,
        &api_key,
        Some(&base_url),
        is_def,
        name.as_deref(),
        &models_json,
    ) {
        return (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(json!({ "error": e.to_string() })),
        );
    }

    let prov: Arc<dyn odon_provider::ModelProvider> = if pid == "anthropic" {
        Arc::new(AnthropicProvider::with_base_url(&api_key, &base_url))
    } else {
        Arc::new(OpenAiCompatibleProvider::new(
            &base_url,
            &api_key,
            Some(&pid),
        ))
    };

    state
        .runtime
        .register_provider(&pid, prov.clone(), is_def)
        .await;
    if is_def {
        state.runtime.register_provider("default", prov, true).await;
    }

    (
        StatusCode::OK,
        Json(json!({ "status": "saved", "provider": pid, "is_default": is_def })),
    )
}

async fn delete_provider_config(
    Path(provider_id): Path<String>,
    State(state): State<AppState>,
) -> impl IntoResponse {
    let _ = state.storage.delete_provider_config(&provider_id);
    state.runtime.unregister_provider(&provider_id).await;
    StatusCode::OK
}

async fn get_oauth_accounts(State(state): State<AppState>) -> impl IntoResponse {
    let accounts = state.oauth_accounts.read().await;
    let list: Vec<serde_json::Value> = accounts.values().cloned().collect();
    Json(list)
}

async fn login_oauth(
    State(state): State<AppState>,
    Json(req): Json<OAuthLoginRequest>,
) -> impl IntoResponse {
    use odon_provider::{LoginResponse, OAuthProvider, OAuthService};

    let provider: OAuthProvider = match req.provider.parse() {
        Ok(p) => p,
        Err(_) => {
            return (
                StatusCode::NOT_IMPLEMENTED,
                Json(json!({ "error": format!(
                    "OAuth for '{}' is not available: it needs a registered client that is not configured.",
                    req.provider
                ) })),
            );
        }
    };

    let service = match OAuthService::new() {
        Ok(s) => s,
        Err(e) => {
            return (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(json!({ "error": e.to_string() })),
            );
        }
    };
    let pending = match service.begin_login(provider).await {
        Ok(p) => p,
        Err(e) => {
            return (
                StatusCode::BAD_GATEWAY,
                Json(json!({ "error": e.to_string() })),
            )
        }
    };
    let response = pending.response().clone();

    // Finish the exchange in the background; the UI polls /api/oauth/accounts.
    let catalog_id = oauth_catalog_id(provider);
    let st = state.clone();
    tokio::spawn(async move {
        match pending.complete().await {
            Ok(token) => register_oauth_provider(&st, catalog_id, token).await,
            Err(e) => tracing::warn!("OAuth login for {catalog_id} failed: {e}"),
        }
    });

    let body = match response {
        LoginResponse::Browser {
            authorization_url,
            redirect_uri,
            ..
        } => json!({
            "status": "pending_browser",
            "auth_url": authorization_url,
            "redirect_uri": redirect_uri,
        }),
        LoginResponse::Device {
            user_code,
            verification_uri,
            expires_in,
            interval,
            ..
        } => json!({
            "status": "pending_device",
            "user_code": user_code,
            "verification_uri": verification_uri,
            "expires_in": expires_in,
            "interval": interval,
        }),
    };
    (StatusCode::OK, Json(body))
}

fn oauth_catalog_id(provider: odon_provider::OAuthProvider) -> &'static str {
    use odon_provider::OAuthProvider::*;
    match provider {
        OpenAiCodex => "chatgpt",
        ClaudeOauth => "claude-oauth",
        Antigravity => "antigravity",
        XaiOauth => "xai-oauth",
        GithubCopilot => "github-copilot",
    }
}

async fn register_oauth_provider(
    state: &AppState,
    catalog_id: &'static str,
    token: odon_provider::OAuthToken,
) {
    let registered = build_oauth_provider(catalog_id, &token);
    let usable = registered.is_some();
    if let Some(p) = registered {
        state.runtime.register_provider(catalog_id, p, false).await;
    }
    // Persist every successful login. A provider without a runtime adapter is
    // still a valid account and must not disappear when odond restarts.
    save_oauth_token(catalog_id, &token);

    let account = token.account();
    state.oauth_accounts.write().await.insert(
        catalog_id.to_string(),
        json!({
            "provider": catalog_id,
            "email": account.email,
            "plan": account.plan,
            "account_id": account.account_id,
            "connected": true,
            "usable": usable,
        }),
    );
}

async fn disconnect_oauth(
    State(state): State<AppState>,
    Json(req): Json<OAuthDisconnectRequest>,
) -> impl IntoResponse {
    let key = req.provider.to_lowercase();
    let mut accounts = state.oauth_accounts.write().await;
    accounts.remove(&key);
    drop(accounts);
    remove_oauth_token(&key);
    StatusCode::OK
}

#[derive(Deserialize, Debug, Default)]
struct TerminalWsQuery {
    #[serde(default)]
    shell: Option<String>,
    #[serde(default)]
    cwd: Option<String>,
    #[serde(default)]
    ssh_profile_id: Option<String>,
    #[serde(default)]
    cols: Option<u16>,
    #[serde(default)]
    rows: Option<u16>,
}

async fn ws_terminal_handler(
    headers: HeaderMap,
    ws: WebSocketUpgrade,
    axum::extract::Query(query): axum::extract::Query<TerminalWsQuery>,
    State(state): State<AppState>,
) -> impl IntoResponse {
    // Origin is the real gate here: CORS does not apply to WebSocket
    // handshakes, and the API token may ride in the query string, so a foreign
    // page must be rejected before the upgrade - otherwise any web page could
    // open a PTY on this machine.
    let origin = headers
        .get(header::ORIGIN)
        .and_then(|value| value.to_str().ok());
    let host = headers
        .get(header::HOST)
        .and_then(|value| value.to_str().ok());
    if !origin_is_trusted(origin, host, &allowed_origins_from_env()) {
        return (
            StatusCode::FORBIDDEN,
            Json(json!({ "error": "Forbidden origin" })),
        )
            .into_response();
    }
    // Interactive shells cannot be pre-approved per keystroke, so the upgrade
    // itself is the gate: confinement to the workspace and, on Unix, an
    // explicit shell allowlist. API-token middleware still applies first.
    if query.ssh_profile_id.is_none() {
        let cwd = query.cwd.as_deref().unwrap_or(&state.workspace_root);
        let candidate = std::path::Path::new(cwd);
        let abs = if candidate.is_absolute() {
            candidate.to_path_buf()
        } else {
            std::path::Path::new(&state.workspace_root).join(candidate)
        };
        if let Err(reason) = ensure_allowed_path(&state, &abs).await {
            return (StatusCode::FORBIDDEN, Json(json!({ "error": reason }))).into_response();
        }
        #[cfg(not(target_os = "windows"))]
        {
            let shell = query.shell.as_deref().unwrap_or("bash");
            if !["bash", "sh", "zsh", "fish", "dash"].contains(&shell) {
                return (
                    StatusCode::BAD_REQUEST,
                    Json(json!({ "error": "Unsupported shell" })),
                )
                    .into_response();
            }
        }
    }
    let workspace = state.workspace_root.clone();
    ws.on_upgrade(move |socket| handle_terminal_socket(socket, workspace, query))
}

/// The only environment a terminal child inherits.
///
/// Everything else the daemon holds - `ODON_API_TOKEN`, provider API keys,
/// relay and bridge tokens, database URLs - is withheld, because a shell can
/// `printenv` whatever it was given and tool output can carry it back to the
/// model. Interactive shells and one-shot commands share this list so the two
/// spawn paths cannot drift apart.
const TERMINAL_ENVIRONMENT_ALLOWLIST: &[&str] = &[
    "PATH",
    "HOME",
    "USER",
    "LOGNAME",
    "SHELL",
    "TERM",
    "LANG",
    "LC_ALL",
    "TMPDIR",
    "TEMP",
    "TMP",
    "SystemRoot",
    "SystemDrive",
    "ComSpec",
    "PATHEXT",
    "USERPROFILE",
    "HOMEDRIVE",
    "HOMEPATH",
    "USERNAME",
    "APPDATA",
    "LOCALAPPDATA",
    "PSModulePath",
];

fn terminal_environment() -> impl Iterator<Item = (&'static str, std::ffi::OsString)> {
    TERMINAL_ENVIRONMENT_ALLOWLIST
        .iter()
        .filter_map(|name| std::env::var_os(name).map(|value| (*name, value)))
}

fn sanitize_pty_environment(command: &mut CommandBuilder) {
    command.env_clear();
    for (name, value) in terminal_environment() {
        command.env(name, value);
    }
}

fn sanitize_child_environment(command: &mut tokio::process::Command) {
    command.env_clear();
    for (name, value) in terminal_environment() {
        command.env(name, value);
    }
}

async fn handle_terminal_socket(socket: WebSocket, workspace: String, query: TerminalWsQuery) {
    let cols = query.cols.unwrap_or(100).max(10);
    let rows = query.rows.unwrap_or(28).max(5);

    let pty_system = native_pty_system();
    let pair = match pty_system.openpty(PtySize {
        rows,
        cols,
        pixel_width: 0,
        pixel_height: 0,
    }) {
        Ok(p) => p,
        Err(e) => {
            let (mut ws_tx, _) = socket.split();
            let _ = ws_tx
                .send(WsMessage::Text(
                    format!("Failed to open native PTY: {}\r\n", e).into(),
                ))
                .await;
            return;
        }
    };

    let mut cmd = if let Some(pid) = &query.ssh_profile_id {
        let profiles = load_ssh_profiles();
        let profile = profiles
            .into_iter()
            .find(|p| &p.id == pid || &p.alias == pid);
        if let Some(p) = profile {
            let mut c = CommandBuilder::new("ssh");
            c.arg("-tt");
            c.arg("-o");
            c.arg("StrictHostKeyChecking=accept-new");
            c.arg("-o");
            c.arg("ConnectTimeout=15");
            c.arg("-p");
            c.arg(p.port.to_string());
            if let Err(e) = validate_ssh_target(&p.host, &p.user) {
                let (mut ws_tx, _) = socket.split();
                let _ = ws_tx
                    .send(WsMessage::Text(
                        format!("Invalid SSH target: {e}\r\n").into(),
                    ))
                    .await;
                return;
            }
            if let Some(k) = &p.key_path {
                match validate_ssh_key_path(k) {
                    Ok(path) => {
                        c.arg("-i");
                        c.arg(path);
                    }
                    Err(e) => {
                        let (mut ws_tx, _) = socket.split();
                        let _ = ws_tx
                            .send(WsMessage::Text(format!("SSH key error: {e}\r\n").into()))
                            .await;
                        return;
                    }
                }
            }
            c.arg(format!("{}@{}", p.user, p.host));
            c
        } else {
            let (mut ws_tx, _) = socket.split();
            let _ = ws_tx
                .send(WsMessage::Text(
                    format!("SSH Profile not found: {}\r\n", pid).into(),
                ))
                .await;
            return;
        }
    } else {
        let working_dir = query.cwd.as_deref().unwrap_or(&workspace);
        #[cfg(target_os = "windows")]
        let c = {
            let shell_type = query.shell.as_deref().unwrap_or("powershell");
            let mut cb = if shell_type.eq_ignore_ascii_case("cmd") {
                CommandBuilder::new("cmd.exe")
            } else if shell_type.eq_ignore_ascii_case("wsl")
                || shell_type.eq_ignore_ascii_case("bash")
            {
                CommandBuilder::new("wsl.exe")
            } else {
                let mut p = CommandBuilder::new("powershell.exe");
                p.arg("-NoLogo");
                p
            };
            cb.cwd(working_dir);
            cb
        };
        #[cfg(not(target_os = "windows"))]
        let c = {
            let shell_type = query.shell.as_deref().unwrap_or("bash");
            let mut cb = CommandBuilder::new(shell_type);
            cb.cwd(working_dir);
            cb
        };
        c
    };
    sanitize_pty_environment(&mut cmd);

    let mut child = match pair.slave.spawn_command(cmd) {
        Ok(ch) => ch,
        Err(e) => {
            let (mut ws_tx, _) = socket.split();
            let _ = ws_tx
                .send(WsMessage::Text(
                    format!("Failed to spawn process in PTY: {}\r\n", e).into(),
                ))
                .await;
            return;
        }
    };

    let mut reader = match pair.master.try_clone_reader() {
        Ok(r) => r,
        Err(e) => {
            let (mut ws_tx, _) = socket.split();
            let _ = ws_tx
                .send(WsMessage::Text(
                    format!("Failed to clone PTY reader: {}\r\n", e).into(),
                ))
                .await;
            return;
        }
    };

    let mut writer = match pair.master.take_writer() {
        Ok(w) => w,
        Err(e) => {
            let (mut ws_tx, _) = socket.split();
            let _ = ws_tx
                .send(WsMessage::Text(
                    format!("Failed to get PTY writer: {}\r\n", e).into(),
                ))
                .await;
            return;
        }
    };

    let master_ref = std::sync::Arc::new(std::sync::Mutex::new(pair.master));

    let (mut ws_tx, mut ws_rx) = socket.split();
    let (pty_out_tx, mut pty_out_rx) = tokio::sync::mpsc::channel::<Vec<u8>>(200);

    // Thread 1: Reader from PTY -> channel
    let _pty_read_thread = std::thread::spawn(move || {
        let mut buf = [0u8; 4096];
        loop {
            match reader.read(&mut buf) {
                Ok(0) => break,
                Ok(n) => {
                    if pty_out_tx.blocking_send(buf[..n].to_vec()).is_err() {
                        break;
                    }
                }
                Err(_) => break,
            }
        }
    });

    // Task 2: Channel -> WebSocket client
    let ws_send_task = tokio::spawn(async move {
        while let Some(bytes) = pty_out_rx.recv().await {
            if ws_tx.send(WsMessage::Binary(bytes.into())).await.is_err() {
                break;
            }
        }
    });

    // Thread 3: Channel -> Writer into PTY
    let (pty_in_tx, mut pty_in_rx) = tokio::sync::mpsc::channel::<Vec<u8>>(200);
    let _pty_write_thread = std::thread::spawn(move || {
        while let Some(bytes) = pty_in_rx.blocking_recv() {
            if writer.write_all(&bytes).is_err() {
                break;
            }
            let _ = writer.flush();
        }
    });

    // Loop: Inbound WebSocket messages from xterm.js
    let master_clone = master_ref.clone();
    while let Some(Ok(msg)) = ws_rx.next().await {
        match msg {
            WsMessage::Text(text) => {
                // Check if it is a resize event: {"type":"resize","cols":120,"rows":30}
                if text.starts_with('{') && text.contains("resize") {
                    if let Ok(v) = serde_json::from_str::<serde_json::Value>(&text) {
                        if v.get("type").and_then(|t| t.as_str()) == Some("resize") {
                            let c = v.get("cols").and_then(|c| c.as_u64()).unwrap_or(80) as u16;
                            let r = v.get("rows").and_then(|r| r.as_u64()).unwrap_or(24) as u16;
                            if let Ok(m) = master_clone.lock() {
                                let _ = m.resize(PtySize {
                                    rows: r,
                                    cols: c,
                                    pixel_width: 0,
                                    pixel_height: 0,
                                });
                            }
                            continue;
                        }
                    }
                }
                if pty_in_tx.send(text.as_bytes().to_vec()).await.is_err() {
                    break;
                }
            }
            WsMessage::Binary(bin) => {
                if pty_in_tx.send(bin.to_vec()).await.is_err() {
                    break;
                }
            }
            WsMessage::Close(_) => break,
            _ => {}
        }
    }

    drop(pty_in_tx);
    let _ = child.kill();
    let _ = child.wait();
    ws_send_task.abort();
    let _ = ws_send_task.await;
    drop(master_ref);
}

/// Model ids the saved gateway config makes selectable.
///
/// The static `openai-compatible` catalogue entry ships with an empty list, so
/// this is the only place the operator's own model ids can surface. Falling back
/// to `OPENAI_MODEL` keeps an env-only gateway usable.
fn gateway_model_ids(byok_configs: &[odon_storage::ProviderConfigRecord]) -> Vec<String> {
    let mut model_ids: Vec<String> = byok_configs
        .iter()
        .find(|c| {
            c.provider_id.eq_ignore_ascii_case("openai-compatible")
                || c.provider_id.eq_ignore_ascii_case("openai_compatible")
        })
        .and_then(|c| serde_json::from_str::<Vec<String>>(&c.models_json).ok())
        .unwrap_or_default()
        .into_iter()
        .map(|id| id.trim().to_string())
        .filter(|id| !id.is_empty())
        .collect();

    if model_ids.is_empty() {
        if let Ok(model) = std::env::var("OPENAI_MODEL") {
            if !model.trim().is_empty() {
                model_ids.push(model.trim().to_string());
            }
        }
    }

    model_ids
}

/// Ask a BYOK/gateway endpoint which models it actually serves.
///
/// A gateway with a saved key but an empty `models_json` is only half
/// configured: the catalogue then exposes `models: []`, the model picker has
/// nothing to select, and every send fails with "no model selected". The key
/// alone cannot tell us the model ids, so this asks the endpoint and remembers
/// the answer.
async fn discover_provider_models(
    Path(pid): Path<String>,
    State(state): State<AppState>,
) -> impl IntoResponse {
    let pid = pid.to_lowercase();
    let Some(config) = state.storage.get_provider_config(&pid).ok().flatten() else {
        return (
            StatusCode::NOT_FOUND,
            Json(json!({ "error": format!("Provider '{pid}' is not configured") })),
        );
    };
    if config.api_key.trim().is_empty() {
        return (
            StatusCode::BAD_REQUEST,
            Json(json!({ "error": format!("Provider '{pid}' has no API key") })),
        );
    }
    let base = config
        .base_url
        .clone()
        .unwrap_or_else(|| default_base_url_for(&pid));
    let url = format!("{}/models", base.trim_end_matches('/'));

    let client = match reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(15))
        .build()
    {
        Ok(client) => client,
        Err(error) => {
            return (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(json!({ "error": error.to_string() })),
            )
        }
    };
    let response = match client.get(&url).bearer_auth(&config.api_key).send().await {
        Ok(response) => response,
        Err(error) => {
            return (
                StatusCode::BAD_GATEWAY,
                Json(json!({ "error": format!("Could not reach {url}: {error}") })),
            )
        }
    };
    if !response.status().is_success() {
        let status = response.status();
        return (
            StatusCode::BAD_GATEWAY,
            Json(json!({ "error": format!("{url} returned HTTP {status}") })),
        );
    }
    let body: Value = match response.json().await {
        Ok(body) => body,
        Err(error) => {
            return (
                StatusCode::BAD_GATEWAY,
                Json(json!({ "error": format!("{url} did not return JSON: {error}") })),
            )
        }
    };

    // OpenAI-compatible gateways answer {data:[{id}]}; Ollama answers
    // {models:[{name}]}. Accept either so the caller never has to care which.
    let mut models: Vec<String> = Vec::new();
    for entry in body
        .get("data")
        .and_then(Value::as_array)
        .map(Vec::as_slice)
        .unwrap_or_default()
    {
        if let Some(id) = entry.get("id").and_then(Value::as_str) {
            models.push(id.to_string());
        }
    }
    for entry in body
        .get("models")
        .and_then(Value::as_array)
        .map(Vec::as_slice)
        .unwrap_or_default()
    {
        if let Some(id) = entry
            .get("name")
            .or_else(|| entry.get("id"))
            .and_then(Value::as_str)
        {
            models.push(id.to_string());
        }
    }
    models.sort();
    models.dedup();

    if models.is_empty() {
        return (
            StatusCode::BAD_GATEWAY,
            Json(json!({ "error": format!("{url} listed no models") })),
        );
    }

    let models_json = serde_json::to_string(&models).unwrap_or_else(|_| "[]".to_string());
    if let Err(error) = state.storage.save_provider_config(
        &pid,
        &config.api_key,
        Some(&base),
        config.is_default,
        config.name.as_deref(),
        &models_json,
    ) {
        return (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(json!({ "error": error.to_string() })),
        );
    }

    (
        StatusCode::OK,
        Json(json!({ "provider_id": pid, "models": models })),
    )
}

async fn get_models(State(state): State<AppState>) -> impl IntoResponse {
    let connected = state.oauth_accounts.read().await;
    let is_connected = |provider: &str| connected.contains_key(provider);
    let byok_configs = state.storage.list_provider_configs().unwrap_or_default();
    let is_byok = |pid: &str| -> bool {
        byok_configs
            .iter()
            .any(|c| c.provider_id.eq_ignore_ascii_case(pid))
            || match pid {
                "openai" => std::env::var("OPENAI_API_KEY")
                    .map(|v| !v.trim().is_empty())
                    .unwrap_or(false),
                "anthropic" => std::env::var("ANTHROPIC_API_KEY")
                    .map(|v| !v.trim().is_empty())
                    .unwrap_or(false),
                "deepseek" => std::env::var("DEEPSEEK_API_KEY")
                    .map(|v| !v.trim().is_empty())
                    .unwrap_or(false),
                "google" => std::env::var("GEMINI_API_KEY")
                    .or_else(|_| std::env::var("GOOGLE_API_KEY"))
                    .map(|v| !v.trim().is_empty())
                    .unwrap_or(false),
                "mistral" => std::env::var("MISTRAL_API_KEY")
                    .map(|v| !v.trim().is_empty())
                    .unwrap_or(false),
                "groq" => std::env::var("GROQ_API_KEY")
                    .map(|v| !v.trim().is_empty())
                    .unwrap_or(false),
                "openrouter" => std::env::var("OPENROUTER_API_KEY")
                    .map(|v| !v.trim().is_empty())
                    .unwrap_or(false),
                "qwen" => std::env::var("DASHSCOPE_API_KEY")
                    .or_else(|_| std::env::var("QWEN_API_KEY"))
                    .map(|v| !v.trim().is_empty())
                    .unwrap_or(false),
                "ollama" => std::env::var("OLLAMA_BASE_URL").is_ok(),
                "openai-compatible" | "openai_compatible" => {
                    std::env::var("OPENAI_BASE_URL").is_ok()
                }
                _ => false,
            }
    };

    // The Copilot catalogue is account-specific: the live /models endpoint
    // marks models disabled by plan/policy, and those must not be offered
    // because selecting one returns a 400 "modelnotsupported".
    let copilot_catalog = match odon_provider::copilot::copilot_models().await {
        Some(models) if !models.is_empty() => {
            let preferred = [
                "gpt-6.1-sol",
                "claude-sonnet-5.5",
                "gpt-5.3-codex",
                "gemini-3.8-flash",
            ];
            let mut entries: Vec<serde_json::Value> = models
                .into_iter()
                .map(|model| {
                    json!({
                        "id": model.id,
                        "name": model.name,
                        "context": model.context,
                        "recommended": preferred.contains(&model.id.as_str()),
                    })
                })
                .collect();
            entries.sort_by_key(|entry| !entry["recommended"].as_bool().unwrap_or(false));
            serde_json::Value::Array(entries)
        }
        _ => json!([
            { "id": "gpt-6.1-sol", "name": "GPT-6.1 Sol", "context": 400000, "recommended": true },
            { "id": "claude-sonnet-5.5", "name": "Claude Sonnet 5.5", "context": 328000, "recommended": true },
            { "id": "gpt-5.3-codex", "name": "GPT-5.3-Codex", "context": 400000, "recommended": true },
            { "id": "gemini-3.8-flash", "name": "Gemini 3.8 Flash", "context": 265536, "recommended": true },
            { "id": "gpt-6.1-terra", "name": "GPT-6.1 Terra", "context": 400000, "recommended": false },
            { "id": "gpt-6-astra", "name": "GPT-6 Astra", "context": 400000, "recommended": false },
            { "id": "gpt-6-sol", "name": "GPT-6 Sol", "context": 400000, "recommended": false },
            { "id": "gpt-6-luna", "name": "GPT-6 Luna", "context": 400000, "recommended": false },
            { "id": "gpt-5.6-sol", "name": "GPT-5.6 Sol", "context": 400000, "recommended": false },
            { "id": "gpt-5.6-terra", "name": "GPT-5.6 Terra", "context": 400000, "recommended": false },
            { "id": "gpt-5.6-luna", "name": "GPT-5.6 Luna", "context": 328000, "recommended": false },
            { "id": "claude-sonnet-5", "name": "Claude Sonnet 5", "context": 264000, "recommended": false },
            { "id": "claude-opus-5.5", "name": "Claude Opus 5.5", "context": 328000, "recommended": false },
            { "id": "claude-opus-5", "name": "Claude Opus 5", "context": 264000, "recommended": false },
            { "id": "claude-opus-4.8", "name": "Claude Opus 4.8", "context": 264000, "recommended": false },
            { "id": "claude-opus-4.8-fast", "name": "Claude Opus 4.8 (fast mode)", "context": 264000, "recommended": false },
            { "id": "claude-haiku-5.5", "name": "Claude Haiku 5.5", "context": 200000, "recommended": false },
            { "id": "claude-haiku-4.5", "name": "Claude Haiku 4.5", "context": 200000, "recommended": false },
            { "id": "claude-fable-5.1", "name": "Claude Fable 5.1", "context": 264000, "recommended": false },
            { "id": "claude-fable-5", "name": "Claude Fable 5", "context": 264000, "recommended": false },
            { "id": "gemini-4-argon", "name": "Gemini 4 Argon", "context": 265536, "recommended": false },
            { "id": "grok-4.7", "name": "Grok 4.7", "context": 328000, "recommended": false },
            { "id": "grok-4.6", "name": "Grok 4.6", "context": 328000, "recommended": false },
            { "id": "kimi-k3", "name": "Kimi K3", "context": 1048576, "recommended": false },
            { "id": "mai-code-1.1-flash", "name": "MAI-Code-1.1-Flash", "context": 256000, "recommended": false }
        ]),
    };

    // The Codex backend publishes an account-specific catalogue; old slugs
    // (gpt-5-codex, gpt-5, gpt-4.1) now return 400/404.
    let codex_catalog = match odon_provider::codex_catalog::codex_models().await {
        Some(models) if !models.is_empty() => {
            let preferred = ["gpt-6.1-sol", "gpt-6-astra", "gpt-6.1-terra"];
            let mut entries: Vec<serde_json::Value> = models
                .into_iter()
                .map(|model| {
                    json!({
                        "id": model.id,
                        "name": model.name,
                        "context": model.context,
                        "recommended": preferred.contains(&model.id.as_str()),
                    })
                })
                .collect();
            entries.sort_by_key(|entry| !entry["recommended"].as_bool().unwrap_or(false));
            serde_json::Value::Array(entries)
        }
        _ => json!([
            { "id": "gpt-6.1-sol", "name": "GPT-6.1 Sol", "context": 500000, "recommended": true },
            { "id": "gpt-6-astra", "name": "GPT-6 Astra", "context": 272000, "recommended": true },
            { "id": "gpt-6.1-terra", "name": "GPT-6.1 Terra", "context": 500000, "recommended": true },
            { "id": "gpt-6-sol", "name": "GPT-6 Sol", "context": 272000, "recommended": false },
            { "id": "gpt-6-luna", "name": "GPT-6 Luna", "context": 272000, "recommended": false },
            { "id": "gpt-5.6-sol", "name": "GPT-5.6 Sol", "context": 272000, "recommended": false },
            { "id": "gpt-5.6-terra", "name": "GPT-5.6 Terra", "context": 272000, "recommended": false },
            { "id": "gpt-5.6-luna", "name": "GPT-5.6 Luna", "context": 272000, "recommended": false }
        ]),
    };

    let mut payload = json!({
        "catalog_as_of": "2026-11-02",
        "providers": [
            {
                "id": "github-copilot", "name": "GitHub Copilot", "category": "oauth", "auth_type": "oauth",
                "connected": is_connected("github-copilot"),
                "description": "GitHub Copilot subscription models (live model-picker list; retired models removed)",
                // Sources:
                // https://docs.github.com/en/copilot/reference/ai-models/supported-models
                // https://api.githubcopilot.com/models (model_picker_enabled=true, type=chat)
                "models": copilot_catalog
            },
            {
                "id": "antigravity", "name": "Google Antigravity", "category": "oauth", "auth_type": "oauth",
                "connected": is_connected("antigravity"),
                "description": "Google Cloud Code Assist & Antigravity OAuth (Gemini and Claude)",
                // Wire ids mirror the working Antigravity/Cloud Code client.
                "models": [
                    { "id": "gemini-4-argon", "name": "Gemini 4 Argon", "context": 2000000, "recommended": true },
                    { "id": "gemini-3.1-pro", "name": "Gemini 3.1 Pro", "context": 1048576, "recommended": true },
                    { "id": "gemini-3.8-flash-medium", "name": "Gemini 3.8 Flash", "context": 1048576, "recommended": true },
                    { "id": "gemini-3.8-flash-high", "name": "Gemini 3.8 Flash High", "context": 1048576, "recommended": false },
                    { "id": "claude-opus-5-5", "name": "Claude Opus 5.5", "context": 1000000, "recommended": false },
                    { "id": "claude-sonnet-5-5", "name": "Claude Sonnet 5.5", "context": 1000000, "recommended": false },
                    { "id": "claude-sonnet-4-6", "name": "Claude Sonnet 4.6", "context": 1000000, "recommended": false }
                ]
            },
            {
                "id": "chatgpt", "name": "ChatGPT / OpenAI Codex", "category": "oauth", "auth_type": "oauth",
                "connected": is_connected("chatgpt"),
                "description": "Official ChatGPT account authorization through the Codex backend",
                "models": codex_catalog
            },
            {
                "id": "claude-oauth", "name": "Claude (Anthropic OAuth)", "category": "oauth", "auth_type": "oauth",
                "connected": is_connected("claude-oauth"),
                "description": "Direct Anthropic Claude account authorization",
                "models": [
                    { "id": "claude-fable-5-1", "name": "Claude Fable 5.1", "context": 1000000, "recommended": true },
                    { "id": "claude-opus-5-5", "name": "Claude Opus 5.5", "context": 1000000, "recommended": true },
                    { "id": "claude-sonnet-5-5", "name": "Claude Sonnet 5.5", "context": 1000000, "recommended": true },
                    { "id": "claude-haiku-5-5", "name": "Claude Haiku 5.5", "context": 1000000, "recommended": false },
                    { "id": "claude-opus-5", "name": "Claude Opus 5", "context": 1000000, "recommended": false },
                    { "id": "claude-sonnet-5", "name": "Claude Sonnet 5", "context": 1000000, "recommended": false },
                    { "id": "claude-sonnet-4-6", "name": "Claude Sonnet 4.6", "context": 1000000, "recommended": false },
                    { "id": "claude-haiku-4-5", "name": "Claude Haiku 4.5", "context": 200000, "recommended": false },
                    { "id": "claude-fable-5", "name": "Claude Fable 5", "context": 1000000, "recommended": false }
                ]
            },
            {
                "id": "xai-oauth", "name": "xAI Grok", "category": "oauth", "auth_type": "oauth",
                "connected": is_connected("xai-oauth"),
                "description": "xAI Grok device authorization",
                "models": [
                    { "id": "grok-4.7", "name": "Grok 4.7", "context": 500000, "recommended": true },
                    { "id": "grok-4.6", "name": "Grok 4.6", "context": 500000, "recommended": false },
                    { "id": "grok-4.5", "name": "Grok 4.5", "context": 500000, "recommended": false },
                    { "id": "grok-build-0.1", "name": "Grok Build 0.1", "context": 256000, "recommended": false }
                ]
            },
            {
                "id": "muse", "name": "Meta Muse (Model API)", "category": "byok", "auth_type": "api_key",
                "configured": is_byok("muse"),
                "default_base_url": "https://api.meta.ai/v1",
                "description": "Meta Model API pay-as-you-go key (LLM|numeric_id|secret). OpenAI- and Anthropic-compatible.",
                "models": [
                    { "id": "muse-wonder-1.0", "name": "Muse Wonder 1.0", "context": 1048576, "recommended": true },
                    { "id": "muse-spark-1.3", "name": "Muse Spark 1.3", "context": 1048576, "recommended": true },
                    { "id": "muse-spark-1.3-contributor", "name": "Muse Spark 1.3 Contributor", "context": 1048576, "recommended": false },
                    { "id": "muse-spark-1.2", "name": "Muse Spark 1.2", "context": 1048576, "recommended": false },
                    { "id": "muse-spark-1.2-contributor", "name": "Muse Spark 1.2 Contributor", "context": 1048576, "recommended": false },
                    { "id": "muse-spark-1.1", "name": "Muse Spark 1.1", "context": 1048576, "recommended": false }
                ]
            },
            {
                "id": "openai", "name": "OpenAI (Official BYOK)", "category": "byok", "auth_type": "api_key",
                "configured": is_byok("openai"),
                "default_base_url": "https://api.openai.com/v1",
                "description": "Official OpenAI API models",
                "models": [
                    { "id": "gpt-6-astra", "name": "GPT-6 Astra", "context": 1050000, "recommended": true },
                    { "id": "gpt-6.1-sol", "name": "GPT-6.1 Sol", "context": 1050000, "recommended": true },
                    { "id": "gpt-6.1-terra", "name": "GPT-6.1 Terra", "context": 1050000, "recommended": true },
                    { "id": "gpt-6-luna", "name": "GPT-6 Luna", "context": 1050000, "recommended": false },
                    { "id": "gpt-6-sol", "name": "GPT-6 Sol", "context": 1050000, "recommended": false },
                    { "id": "gpt-5.6-sol", "name": "GPT-5.6 Sol", "context": 1050000, "recommended": false },
                    { "id": "gpt-5.6-terra", "name": "GPT-5.6 Terra", "context": 1050000, "recommended": false },
                    { "id": "gpt-5.6-luna", "name": "GPT-5.6 Luna", "context": 1050000, "recommended": false },
                    { "id": "gpt-5.5", "name": "GPT-5.5", "context": 1050000, "recommended": false },
                    { "id": "gpt-5.4", "name": "GPT-5.4", "context": 1050000, "recommended": false },
                    { "id": "gpt-5.4-mini", "name": "GPT-5.4 mini", "context": 400000, "recommended": false },
                    { "id": "gpt-5.3-codex", "name": "GPT-5.3 Codex", "context": 1000000, "recommended": false }
                ]
            },
            {
                "id": "anthropic", "name": "Anthropic (Official BYOK)", "category": "byok", "auth_type": "api_key",
                "configured": is_byok("anthropic"),
                "default_base_url": "https://api.anthropic.com/v1",
                "description": "Official Anthropic API with Claude models",
                "models": [
                    { "id": "claude-fable-5-1", "name": "Claude Fable 5.1", "context": 1000000, "recommended": true },
                    { "id": "claude-opus-5-5", "name": "Claude Opus 5.5", "context": 1000000, "recommended": true },
                    { "id": "claude-sonnet-5-5", "name": "Claude Sonnet 5.5", "context": 1000000, "recommended": true },
                    { "id": "claude-haiku-5-5", "name": "Claude Haiku 5.5", "context": 1000000, "recommended": false },
                    { "id": "claude-opus-5", "name": "Claude Opus 5", "context": 1000000, "recommended": false },
                    { "id": "claude-sonnet-5", "name": "Claude Sonnet 5", "context": 1000000, "recommended": false },
                    { "id": "claude-sonnet-4-6", "name": "Claude Sonnet 4.6", "context": 1000000, "recommended": false },
                    { "id": "claude-haiku-4-5", "name": "Claude Haiku 4.5", "context": 200000, "recommended": false },
                    { "id": "claude-fable-5", "name": "Claude Fable 5", "context": 1000000, "recommended": false }
                ]
            },
            {
                "id": "google", "name": "Google Gemini (Official BYOK)", "category": "byok", "auth_type": "api_key",
                "configured": is_byok("google"),
                "default_base_url": "https://generativelanguage.googleapis.com/v1beta/openai",
                "description": "Google Gemini API with 1M context and native multimodal reasoning",
                "models": [
                    { "id": "gemini-4-argon", "name": "Gemini 4 Argon", "context": 1000000, "recommended": true },
                    { "id": "gemini-3.8-flash", "name": "Gemini 3.8 Flash", "context": 1000000, "recommended": true },
                    { "id": "gemini-3.1-pro-preview", "name": "Gemini 3.1 Pro", "context": 1000000, "recommended": true },
                    { "id": "gemini-3.6-flash", "name": "Gemini 3.6 Flash", "context": 1000000, "recommended": false },
                    { "id": "gemini-3.5-flash-lite", "name": "Gemini 3.5 Flash-Lite", "context": 1000000, "recommended": false },
                    { "id": "gemini-3.1-flash-lite", "name": "Gemini 3.1 Flash-Lite", "context": 1000000, "recommended": false },
                    { "id": "gemini-3-flash-preview", "name": "Gemini 3 Flash (Preview)", "context": 1000000, "recommended": false }
                ]
            },
            {
                "id": "deepseek", "name": "DeepSeek (BYOK)", "category": "byok", "auth_type": "api_key",
                "configured": is_byok("deepseek"),
                "default_base_url": "https://api.deepseek.com/v1",
                "description": "DeepSeek V4 Pro and V4.1 Flash (1M context)",
                "models": [
                    { "id": "deepseek-v4-pro", "name": "DeepSeek V4 Pro", "context": 1000000, "recommended": true },
                    { "id": "deepseek-flash", "name": "DeepSeek V4.1 Flash", "context": 1000000, "recommended": true }
                ]
            },
            {
                "id": "qwen", "name": "Qwen Plan / DashScope Global", "category": "byok", "auth_type": "api_key",
                "configured": is_byok("qwen"),
                "default_base_url": "https://dashscope-intl.aliyuncs.com/compatible-mode/v1",
                "description": "Alibaba Cloud international AI gateway (Qwen3.8 Max, Plus, Flash, 27B)",
                "models": [
                    { "id": "qwen3.8-max", "name": "Qwen3.8 Max", "context": 1000000, "recommended": true },
                    { "id": "qwen3.8-flash", "name": "Qwen3.8 Flash", "context": 1000000, "recommended": true },
                    { "id": "qwen3.8-27b", "name": "Qwen3.8 27B", "context": 1000000, "recommended": true },
                    { "id": "qwen3.7-plus", "name": "Qwen3.7 Plus", "context": 1000000, "recommended": false }
                ]
            },
            {
                "id": "mistral", "name": "Mistral AI", "category": "byok", "auth_type": "api_key",
                "configured": is_byok("mistral"),
                "default_base_url": "https://api.mistral.ai/v1",
                "description": "Mistral AI models including Large 4, Small 4, Codestral, and Ministral 3",
                "models": [
                    { "id": "mistral-large-latest", "name": "Mistral Large 4", "context": 1000000, "recommended": true },
                    { "id": "mistral-small-latest", "name": "Mistral Small 4", "context": 256000, "recommended": true },
                    { "id": "codestral-latest", "name": "Codestral 25.08", "context": 128000, "recommended": true },
                    { "id": "ministral-3-8b-25-12", "name": "Ministral 3 8B", "context": 256000, "recommended": false }
                ]
            },
            {
                "id": "groq", "name": "Groq (Fast Inference)", "category": "byok", "auth_type": "api_key",
                "configured": is_byok("groq"),
                "default_base_url": "https://api.groq.com/openai/v1",
                "description": "Ultra high-speed LPU inference for open-weights models",
                "models": [
                    { "id": "openai/gpt-oss-120b", "name": "GPT-OSS 120B (Groq)", "context": 131072, "recommended": true },
                    { "id": "openai/gpt-oss-20b", "name": "GPT-OSS 20B (Groq)", "context": 131072, "recommended": true },
                    { "id": "qwen/qwen3.8-27b", "name": "Qwen3.8 27B (Groq)", "context": 131072, "recommended": true },
                    { "id": "llama-3.3-70b-versatile", "name": "Llama 3.3 70B (Groq)", "context": 131072, "recommended": false },
                    { "id": "llama-3.1-8b-instant", "name": "Llama 3.1 8B (Groq)", "context": 131072, "recommended": false }
                ]
            },
            {
                "id": "openrouter", "name": "OpenRouter", "category": "byok", "auth_type": "api_key",
                "configured": is_byok("openrouter"),
                "default_base_url": "https://openrouter.ai/api/v1",
                "description": "Unified gateway across all frontier models and providers",
                "models": [
                    { "id": "openrouter/auto", "name": "OpenRouter Auto", "context": 2000000, "recommended": true },
                    { "id": "anthropic/claude-opus-5.5", "name": "Claude Opus 5.5 (OpenRouter)", "context": 1000000, "recommended": true },
                    { "id": "openai/gpt-6-astra", "name": "GPT-6 Astra (OpenRouter)", "context": 1050000, "recommended": true },
                    { "id": "moonshotai/kimi-k3.1-pro", "name": "Kimi K3.1 Pro (OpenRouter)", "context": 1048576, "recommended": false },
                    { "id": "z-ai/glm-5.3", "name": "GLM 5.3 (OpenRouter)", "context": 1048576, "recommended": false },
                    { "id": "qwen/qwen3.8-max-prime", "name": "Qwen3.8 Max Prime (OpenRouter)", "context": 1000000, "recommended": false },
                    { "id": "deepseek/deepseek-v4-pro", "name": "DeepSeek V4 Pro (OpenRouter)", "context": 1048576, "recommended": false }
                ]
            },
            {
                "id": "ollama", "name": "Ollama (Local Models)", "category": "byok", "auth_type": "api_key",
                "configured": is_byok("ollama"),
                "default_base_url": "http://localhost:11434/v1",
                "description": "Run open models entirely locally on your hardware without internet",
                "models": [
                    { "id": "qwen3.8:27b", "name": "Qwen3.8 27B", "context": 131072, "recommended": true },
                    { "id": "qwen3-coder:30b", "name": "Qwen3 Coder 30B", "context": 131072, "recommended": true },
                    { "id": "gpt-oss:20b", "name": "GPT-OSS 20B", "context": 131072, "recommended": true },
                    { "id": "glm-4.7-flash:latest", "name": "GLM 4.7 Flash", "context": 131072, "recommended": false },
                    { "id": "deepseek-v4-flash:latest", "name": "DeepSeek V4 Flash", "context": 131072, "recommended": false },
                    { "id": "qwen2.5-coder:latest", "name": "Qwen2.5 Coder", "context": 32768, "recommended": false },
                    { "id": "llama3.2:latest", "name": "Llama 3.2", "context": 131072, "recommended": false }
                ]
            },
            {
                "id": "openai-compatible", "name": "OpenAI Compatible (Custom / Gateway)", "category": "byok", "auth_type": "api_key",
                "configured": is_byok("openai-compatible"),
                "default_base_url": "http://localhost:11434/v1",
                "description": "Any OpenAI-compatible gateway: vLLM, LM Studio, LiteLLM, or custom proxies",
                "models": []
            }
        ]
    });

    // Every saved endpoint with a non-catalog id is a first-class provider:
    // its own key, Base URL, model list, and selectable provider id.
    const STATIC_PROVIDER_IDS: &[&str] = &[
        "github-copilot",
        "antigravity",
        "chatgpt",
        "claude-oauth",
        "xai-oauth",
        "muse",
        "openai",
        "anthropic",
        "google",
        "deepseek",
        "qwen",
        "mistral",
        "groq",
        "openrouter",
        "ollama",
        "openai-compatible",
        "openai_compatible",
        "default",
        "odon-simulated",
    ];
    if let Some(list) = payload.get_mut("providers").and_then(|v| v.as_array_mut()) {
        for c in byok_configs.iter() {
            if STATIC_PROVIDER_IDS
                .iter()
                .any(|id| id.eq_ignore_ascii_case(&c.provider_id))
            {
                continue;
            }
            let model_ids: Vec<String> =
                serde_json::from_str::<Vec<String>>(&c.models_json).unwrap_or_default();
            let models: Vec<serde_json::Value> = model_ids
                .into_iter()
                .map(|id| {
                    json!({
                        "id": id,
                        "name": id,
                        "context": 128000,
                        "recommended": false
                    })
                })
                .collect();
            list.push(json!({
                "id": c.provider_id,
                "name": c.name.clone().unwrap_or_else(|| c.provider_id.clone()),
                "category": "byok",
                "auth_type": "api_key",
                "configured": true,
                "is_custom": true,
                "default_base_url": c.base_url,
                "description": "Custom OpenAI-compatible endpoint",
                "models": models
            }));
        }

        // The static `openai-compatible` entry ships with an empty model list
        // because it is a gateway: the real models live in the saved provider
        // config. Without this the UI offers a provider that has nothing to
        // select, so a turn goes out with an empty model id and dies with
        // "No model selected" right after the daemon starts.
        for entry in list.iter_mut() {
            let is_gateway = entry
                .get("id")
                .and_then(|v| v.as_str())
                .map(|id| {
                    id.eq_ignore_ascii_case("openai-compatible")
                        || id.eq_ignore_ascii_case("openai_compatible")
                })
                .unwrap_or(false);
            if !is_gateway {
                continue;
            }

            let config = byok_configs.iter().find(|c| {
                c.provider_id.eq_ignore_ascii_case("openai-compatible")
                    || c.provider_id.eq_ignore_ascii_case("openai_compatible")
            });

            let model_ids = gateway_model_ids(&byok_configs);

            if let Some(obj) = entry.as_object_mut() {
                obj.insert(
                    "models".to_string(),
                    json!(model_ids
                        .into_iter()
                        .map(|id| json!({
                            "id": id,
                            "name": id,
                            "context": 128000,
                            "recommended": false
                        }))
                        .collect::<Vec<_>>()),
                );
                if let Some(c) = config {
                    obj.insert("default_base_url".to_string(), json!(c.base_url));
                    obj.insert(
                        "configured".to_string(),
                        json!(!c.api_key.trim().is_empty() || c.base_url.is_some()),
                    );
                }
            }
        }
    }

    Json(payload)
}

#[derive(Deserialize)]
struct BrowserReadQuery {
    url: String,
}

fn inject_preview_base(html: &str, base_url: &reqwest::Url) -> String {
    let lowered = html.to_ascii_lowercase();
    if lowered.contains("<base") {
        return html.to_string();
    }
    let tag = format!(r#"<base href="{}">"#, base_url);
    if let Some(position) = lowered.find("<head") {
        if let Some(tag_end) = html[position..].find('>') {
            let insert_at = position + tag_end + 1;
            let mut output = String::with_capacity(html.len() + tag.len());
            output.push_str(&html[..insert_at]);
            output.push_str(&tag);
            output.push_str(&html[insert_at..]);
            return output;
        }
    }
    format!("{tag}{html}")
}

// ── Live browser bridge ─────────────────────────────────────────────────────
//
// These routes expose the exact Chromium sessions the agent tools drive, so the
// browser panel shows what the agent sees and the user can click in it.

fn bridge_error(e: odon_core::error::OdonError) -> axum::response::Response {
    (
        StatusCode::BAD_REQUEST,
        Json(json!({ "error": e.to_string() })),
    )
        .into_response()
}

async fn browser_sessions_list(State(state): State<AppState>) -> impl IntoResponse {
    match odon_tools::browser_bridge::list_sessions(&state.workspace_root).await {
        Ok(v) => Json(v).into_response(),
        Err(e) => bridge_error(e),
    }
}

#[derive(Deserialize)]
struct BrowserOpenRequest {
    #[serde(default)]
    url: String,
}

async fn browser_session_open(
    State(state): State<AppState>,
    Json(req): Json<BrowserOpenRequest>,
) -> impl IntoResponse {
    match odon_tools::browser_bridge::open_session(&state.workspace_root, &req.url).await {
        Ok(v) => Json(v).into_response(),
        Err(e) => bridge_error(e),
    }
}

#[derive(Deserialize)]
struct BrowserNavigateRequest {
    url: String,
    #[serde(default)]
    timeout_ms: Option<u64>,
}

async fn browser_session_navigate(
    Path(id): Path<String>,
    State(state): State<AppState>,
    Json(req): Json<BrowserNavigateRequest>,
) -> impl IntoResponse {
    match odon_tools::browser_bridge::navigate(
        &state.workspace_root,
        &id,
        &req.url,
        req.timeout_ms.unwrap_or(10_000),
    )
    .await
    {
        Ok(v) => Json(v).into_response(),
        Err(e) => bridge_error(e),
    }
}

async fn browser_session_state(
    Path(id): Path<String>,
    State(state): State<AppState>,
) -> impl IntoResponse {
    let _ = state;
    match odon_tools::browser_bridge::page_state(&id).await {
        Ok(v) => Json(v).into_response(),
        Err(e) => bridge_error(e),
    }
}

async fn browser_session_screenshot(
    Path(id): Path<String>,
    State(state): State<AppState>,
) -> impl IntoResponse {
    let _ = state;
    match odon_tools::browser_bridge::screenshot(&id).await {
        Ok(png) => Json(json!({ "session_id": id, "png_base64": png })).into_response(),
        Err(e) => bridge_error(e),
    }
}

#[derive(Deserialize)]
struct BrowserActRequest {
    action: String,
    #[serde(default)]
    x: Option<f64>,
    #[serde(default)]
    y: Option<f64>,
    #[serde(default)]
    text: Option<String>,
}

async fn browser_session_act(
    Path(id): Path<String>,
    State(state): State<AppState>,
    Json(req): Json<BrowserActRequest>,
) -> impl IntoResponse {
    let _ = state;
    let result = match req.action.as_str() {
        "click" => match (req.x, req.y) {
            (Some(x), Some(y)) => odon_tools::browser_bridge::click(&id, x, y).await,
            _ => Err(odon_core::error::OdonError::Tool {
                tool: "browser".into(),
                message: "click requires x and y".into(),
            }),
        },
        "type" => match req.text {
            Some(text) => odon_tools::browser_bridge::type_text(&id, &text).await,
            None => Err(odon_core::error::OdonError::Tool {
                tool: "browser".into(),
                message: "type requires text".into(),
            }),
        },
        other => Err(odon_core::error::OdonError::Tool {
            tool: "browser".into(),
            message: format!("unsupported action '{other}'"),
        }),
    };
    match result {
        Ok(()) => Json(json!({ "ok": true })).into_response(),
        Err(e) => bridge_error(e),
    }
}

async fn browser_session_close(
    Path(id): Path<String>,
    State(state): State<AppState>,
) -> impl IntoResponse {
    let _ = state;
    match odon_tools::browser_bridge::close(&id).await {
        Ok(()) => Json(json!({ "ok": true })).into_response(),
        Err(e) => bridge_error(e),
    }
}

/// Read a page for the in-app browser reader. The request goes through the
/// same SSRF guard as agent web tools, so a preview URL cannot probe localhost
/// or private networks.
async fn browser_read_page(Query(query): Query<BrowserReadQuery>) -> impl IntoResponse {
    let parsed = match reqwest::Url::parse(query.url.trim()) {
        Ok(url) => url,
        Err(e) => {
            return (
                StatusCode::BAD_REQUEST,
                Json(json!({ "error": format!("Invalid URL: {e}") })),
            )
                .into_response();
        }
    };
    // The preview panel is a local developer tool, so loopback dev servers are
    // allowed. Private LAN ranges still go through the SSRF guard.
    let is_loopback = parsed
        .host_str()
        .map(|host| {
            host.eq_ignore_ascii_case("localhost")
                || host == "::1"
                || host == "[::1]"
                || host.starts_with("127.")
        })
        .unwrap_or(false);
    let validation = if is_loopback {
        odon_tools::net_guard::validate_url_allow_loopback(&query.url).await
    } else {
        odon_tools::net_guard::validate_url(&query.url).await
    };
    let validated = match validation {
        Ok(target) => target,
        Err(e) => {
            return (
                StatusCode::BAD_REQUEST,
                Json(json!({ "error": e.to_string() })),
            )
                .into_response();
        }
    };

    let client = match validated.client(
        reqwest::Client::builder()
            .timeout(std::time::Duration::from_secs(20))
            .user_agent("Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/130.0.0.0 Safari/537.36"),
    ) {
        Ok(client) => client,
        Err(e) => {
            return (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(json!({ "error": format!("HTTP client error: {e}") })),
            )
                .into_response();
        }
    };

    let resp = match client.get(validated.as_str()).send().await {
        Ok(resp) => resp,
        Err(e) => {
            return (
                StatusCode::BAD_GATEWAY,
                Json(json!({ "error": format!("Failed to fetch page: {e}") })),
            )
                .into_response();
        }
    };
    let status = resp.status().as_u16();
    let final_url = resp.url().clone();
    let content_type = resp
        .headers()
        .get(header::CONTENT_TYPE)
        .and_then(|value| value.to_str().ok())
        .unwrap_or("text/html")
        .to_string();
    let bytes = resp.bytes().await.unwrap_or_default();
    let capped = &bytes[..bytes.len().min(2_000_000)];
    let body = String::from_utf8_lossy(capped);

    let title = regex::Regex::new(r#"(?i)<title[^>]*>([^<]+)</title>"#)
        .ok()
        .and_then(|re| re.captures(&body))
        .and_then(|caps| caps.get(1))
        .map(|m| m.as_str().trim().to_string())
        .unwrap_or_default();

    let is_html = content_type.to_ascii_lowercase().contains("text/html")
        || body
            .trim_start()
            .to_ascii_lowercase()
            .starts_with("<!doctype html")
        || body.trim_start().to_ascii_lowercase().starts_with("<html");
    let preview_html = if is_html {
        inject_preview_base(&body, &final_url)
    } else if content_type.to_ascii_lowercase().contains("json") {
        let escaped = body
            .replace('&', "&amp;")
            .replace('<', "&lt;")
            .replace('>', "&gt;");
        format!(
            "<!doctype html><html><head><meta charset=\"utf-8\"><base href=\"{}\"><title>{}</title></head><body style=\"margin:0;background:#fff;color:#111;font-family:ui-monospace,SFMono-Regular,Menlo,monospace\"><pre style=\"white-space:pre-wrap;padding:16px\">{}</pre></body></html>",
            html_escape(final_url.as_str()),
            html_escape(&title),
            escaped
        )
    } else {
        format!(
            "<!doctype html><html><head><meta charset=\"utf-8\"><base href=\"{}\"><title>{}</title></head><body style=\"margin:0;background:#fff;color:#111;font-family:system-ui\"><div style=\"padding:16px\">This URL returned <strong>{}</strong> content, which the built-in preview cannot render. <a href=\"{}\" target=\"_blank\" rel=\"noreferrer\">Open it directly</a>.</div></body></html>",
            html_escape(final_url.as_str()),
            html_escape(&title),
            html_escape(&content_type),
            html_escape(final_url.as_str())
        )
    };

    let mut text = odon_tools::web::clean_html_snippet(&body);
    if text.chars().count() > 120_000 {
        text = text.chars().take(120_000).collect();
        text.push_str("\n... [truncated]");
    }

    let links: Vec<String> = regex::Regex::new(r#"(?i)href=["']([^"']+)["']"#)
        .ok()
        .map(|re| {
            let mut links = Vec::new();
            for caps in re.captures_iter(&body).take(100) {
                if let Some(href) = caps.get(1) {
                    if let Ok(url) = validated.url().join(href.as_str().trim()) {
                        let link = url.to_string();
                        if !links.contains(&link) {
                            links.push(link);
                        }
                    }
                }
            }
            links
        })
        .unwrap_or_default();

    (
        StatusCode::OK,
        Json(json!({
            "url": final_url.to_string(),
            "status": status,
            "title": title,
            "content_type": content_type,
            "html": preview_html,
            "text": text,
            "links": links,
        })),
    )
        .into_response()
}

async fn serve_file(
    Path(rel_path): Path<String>,
    State(state): State<AppState>,
) -> impl IntoResponse {
    let clean = rel_path.trim_start_matches('/').replace('\\', "/");
    if clean.contains("..") {
        return (
            StatusCode::BAD_REQUEST,
            [(header::CONTENT_TYPE, "text/plain")],
            "Invalid path".as_bytes().to_vec(),
        )
            .into_response();
    }
    // Serve only from an allowed workspace, and verify the canonical target so
    // a symlink inside the workspace cannot escape it.
    let mut served: Option<PathBuf> = None;
    for root in workspace_roots(&state).await {
        let candidate = root.join(&clean);
        if let (Ok(canonical_root), Ok(canonical)) = (
            std::fs::canonicalize(&root),
            std::fs::canonicalize(&candidate),
        ) {
            if canonical.starts_with(&canonical_root) && canonical.is_file() {
                served = Some(canonical);
                break;
            }
        }
    }
    let full = match served {
        Some(path) => path,
        None => {
            return (
                StatusCode::NOT_FOUND,
                [(header::CONTENT_TYPE, "text/plain")],
                "File not found".as_bytes().to_vec(),
            )
                .into_response();
        }
    };
    match tokio::fs::read(&full).await {
        Ok(bytes) => {
            let mime = if clean.ends_with(".png") {
                "image/png"
            } else if clean.ends_with(".jpg") || clean.ends_with(".jpeg") {
                "image/jpeg"
            } else if clean.ends_with(".svg") {
                "image/svg+xml"
            } else if clean.ends_with(".webp") {
                "image/webp"
            } else if clean.ends_with(".json") {
                "application/json"
            } else if clean.ends_with(".txt") || clean.ends_with(".md") {
                "text/plain; charset=utf-8"
            } else {
                "application/octet-stream"
            };
            // SVG can carry script; serve it as an attachment so it never
            // executes in the daemon origin. Never let shared caches store
            // workspace file contents.
            if clean.ends_with(".svg") {
                (
                    StatusCode::OK,
                    [
                        (header::CONTENT_TYPE, mime),
                        (header::CACHE_CONTROL, "private, no-store"),
                        (header::CONTENT_DISPOSITION, "attachment"),
                    ],
                    bytes,
                )
                    .into_response()
            } else {
                (
                    StatusCode::OK,
                    [
                        (header::CONTENT_TYPE, mime),
                        (header::CACHE_CONTROL, "private, no-store"),
                    ],
                    bytes,
                )
                    .into_response()
            }
        }
        Err(_) => (
            StatusCode::NOT_FOUND,
            [(header::CONTENT_TYPE, "text/plain")],
            "File not found".as_bytes().to_vec(),
        )
            .into_response(),
    }
}

async fn sse_events(
    headers: HeaderMap,
    Query(params): Query<HashMap<String, String>>,
    State(state): State<AppState>,
) -> axum::response::Response {
    // Origin is the real gate here: EventSource cannot set headers, so the
    // token may arrive as a query parameter and a foreign page could otherwise
    // subscribe to every event this daemon emits.
    let origin = headers
        .get(header::ORIGIN)
        .and_then(|value| value.to_str().ok());
    let host = headers
        .get(header::HOST)
        .and_then(|value| value.to_str().ok());
    if !origin_is_trusted(origin, host, &allowed_origins_from_env()) {
        return (
            StatusCode::FORBIDDEN,
            Json(json!({ "error": "Forbidden origin" })),
        )
            .into_response();
    }
    let requested_session_id = params.get("session_id").cloned();
    let mut rx = state.event_tx.subscribe();
    let stream = async_stream::stream! {
        loop {
            match rx.recv().await {
                // Items arrive enveloped: a `session_id` (present whenever the
                // event was published inside a turn) beside the flattened
                // event, so the wire JSON keeps its flat `type` / `payload`
                // shape and only gains the routing id.
                Ok(envelope) => {
                    if let Some(required) = &requested_session_id {
                        if envelope.session_id.as_deref() != Some(required.as_str()) {
                            continue;
                        }
                    }
                    if let Ok(data) = serde_json::to_string(&envelope) {
                        yield Ok::<Event, Infallible>(Event::default().data(data));
                    }
                }
                // A slow client missed events. Skipping silently used to hide the
                // reason a turn appeared to hang, so log the gap; the enlarged bus
                // above makes reaching this branch rare.
                Err(broadcast::error::RecvError::Lagged(missed)) => {
                    tracing::warn!(
                        missed,
                        "SSE subscriber lagged and dropped events; client may need to resync"
                    );
                    continue;
                }
                Err(broadcast::error::RecvError::Closed) => break,
            }
        }
    };

    Sse::new(stream)
        .keep_alive(
            KeepAlive::new()
                .interval(std::time::Duration::from_secs(5))
                .text("keep-alive"),
        )
        .into_response()
}

async fn get_approval_history(State(state): State<AppState>) -> impl IntoResponse {
    let history = state.approval.get_audit_history().await;
    Json(json!(history))
}

async fn get_approval_policies(State(state): State<AppState>) -> impl IntoResponse {
    let policies = state.approval.get_always_allowed().await;
    Json(json!(policies))
}

async fn delete_approval_policy(
    Path(key): Path<String>,
    State(state): State<AppState>,
) -> impl IntoResponse {
    let removed = state.approval.revoke_allowed(&key).await;
    Json(json!({ "removed": removed, "key": key }))
}

/// Redact stdio-server env secrets before returning MCP configs to clients.
fn redact_mcp_env(
    servers: Vec<odon_tools::mcp::McpServerConfig>,
) -> Vec<odon_tools::mcp::McpServerConfig> {
    servers
        .into_iter()
        .map(|mut s| {
            if let Some(env) = s.env.as_mut() {
                for value in env.values_mut() {
                    *value = "***".to_string();
                }
            }
            s
        })
        .collect()
}

async fn get_mcp_servers() -> impl IntoResponse {
    let servers = redact_mcp_env(odon_tools::mcp::load_mcp_servers());
    Json(json!(servers))
}

async fn save_mcp_server(
    Json(server): Json<odon_tools::mcp::McpServerConfig>,
) -> impl IntoResponse {
    let mut servers = odon_tools::mcp::load_mcp_servers();
    if let Some(pos) = servers.iter().position(|s| s.id == server.id) {
        servers[pos] = server;
    } else {
        servers.push(server);
    }
    let _ = odon_tools::mcp::save_mcp_servers(&servers);
    Json(json!({ "success": true, "servers": redact_mcp_env(servers) }))
}

async fn delete_mcp_server(Path(id): Path<String>) -> impl IntoResponse {
    let mut servers = odon_tools::mcp::load_mcp_servers();
    servers.retain(|s| s.id != id);
    let _ = odon_tools::mcp::save_mcp_servers(&servers);
    Json(json!({ "success": true, "deleted": id }))
}

async fn toggle_mcp_server(Path(id): Path<String>) -> impl IntoResponse {
    let mut servers = odon_tools::mcp::load_mcp_servers();
    if let Some(s) = servers.iter_mut().find(|s| s.id == id) {
        s.enabled = !s.enabled;
    }
    let _ = odon_tools::mcp::save_mcp_servers(&servers);
    Json(json!({ "success": true, "servers": redact_mcp_env(servers) }))
}

#[derive(Deserialize)]
struct GitHubQuery {
    owner: Option<String>,
    repo: Option<String>,
    state: Option<String>,
}

async fn get_github_auth_token() -> Option<String> {
    if let Ok(tok) = std::env::var("GITHUB_TOKEN") {
        if !tok.trim().is_empty() {
            return Some(tok.trim().to_string());
        }
    }
    if let Ok(output) = tokio::process::Command::new("gh")
        .args(["auth", "token"])
        .output()
        .await
    {
        if output.status.success() {
            let tok = String::from_utf8_lossy(&output.stdout).trim().to_string();
            if !tok.is_empty() {
                return Some(tok);
            }
        }
    }
    None
}

fn parse_github_repo_url(url: &str) -> Option<(String, String)> {
    let clean = url.trim().trim_end_matches(".git");
    if let Some(idx) = clean.find("github.com/") {
        let part = &clean[idx + 11..];
        let mut split = part.split('/');
        let owner = split.next()?.trim();
        let repo = split.next()?.trim();
        if !owner.is_empty() && !repo.is_empty() {
            return Some((owner.to_string(), repo.to_string()));
        }
    }
    if let Some(idx) = clean.find("github.com:") {
        let part = &clean[idx + 11..];
        let mut split = part.split('/');
        let owner = split.next()?.trim();
        let repo = split.next()?.trim();
        if !owner.is_empty() && !repo.is_empty() {
            return Some((owner.to_string(), repo.to_string()));
        }
    }
    None
}

fn detect_workspace_github_repo(workspace_root: &str) -> Option<(String, String)> {
    let output = std::process::Command::new("git")
        .args(["remote", "get-url", "origin"])
        .current_dir(workspace_root)
        .output()
        .ok()?;
    if !output.status.success() {
        return None;
    }
    let url = String::from_utf8_lossy(&output.stdout).trim().to_string();
    parse_github_repo_url(&url)
}

async fn get_github_status(State(state): State<AppState>) -> impl IntoResponse {
    let token_opt = get_github_auth_token().await;
    let ws_repo = detect_workspace_github_repo(&state.workspace_root);

    if let Some(token) = token_opt {
        let client = reqwest::Client::new();
        let res = client
            .get("https://api.github.com/user")
            .header("User-Agent", "odon-desktop")
            .header("Authorization", format!("Bearer {}", token))
            .send()
            .await;

        if let Ok(resp) = res {
            if resp.status().is_success() {
                if let Ok(user) = resp.json::<Value>().await {
                    return Json(json!({
                        "authenticated": true,
                        "auth_source": "gh_auth_token",
                        "user": {
                            "login": user.get("login").and_then(|l| l.as_str()).unwrap_or(""),
                            "name": user.get("name").and_then(|n| n.as_str()).unwrap_or(""),
                            "avatar_url": user.get("avatar_url").and_then(|a| a.as_str()).unwrap_or(""),
                            "bio": user.get("bio").and_then(|b| b.as_str()).unwrap_or("")
                        },
                        "workspace_repo": ws_repo.map(|(o, r)| json!({ "owner": o, "repo": r }))
                    }));
                }
            }
        }
    }

    Json(json!({
        "authenticated": false,
        "auth_source": null,
        "user": null,
        "workspace_repo": ws_repo.map(|(o, r)| json!({ "owner": o, "repo": r }))
    }))
}

async fn get_github_user_repos() -> impl IntoResponse {
    if let Ok(output) = tokio::process::Command::new("gh")
        .args([
            "repo",
            "list",
            "--limit",
            "40",
            "--json",
            "name,owner,isPrivate,description",
        ])
        .output()
        .await
    {
        if output.status.success() {
            if let Ok(repos) = serde_json::from_slice::<Value>(&output.stdout) {
                if let Some(arr) = repos.as_array() {
                    let mut list = Vec::new();
                    for item in arr {
                        let name = item.get("name").and_then(|n| n.as_str()).unwrap_or("");
                        let owner = item
                            .get("owner")
                            .and_then(|o| o.get("login"))
                            .and_then(|l| l.as_str())
                            .unwrap_or("");
                        let desc = item
                            .get("description")
                            .and_then(|d| d.as_str())
                            .unwrap_or("");
                        let is_priv = item
                            .get("isPrivate")
                            .and_then(|p| p.as_bool())
                            .unwrap_or(false);
                        list.push(json!({
                            "name": name,
                            "owner": owner,
                            "full_name": format!("{}/{}", owner, name),
                            "is_private": is_priv,
                            "description": desc
                        }));
                    }
                    return Json(json!({ "repos": list }));
                }
            }
        }
    }

    if let Some(token) = get_github_auth_token().await {
        let client = reqwest::Client::new();
        if let Ok(resp) = client
            .get("https://api.github.com/user/repos?per_page=40&sort=updated")
            .header("User-Agent", "odon-desktop")
            .header("Authorization", format!("Bearer {}", token))
            .send()
            .await
        {
            if resp.status().is_success() {
                if let Ok(items) = resp.json::<Value>().await {
                    if let Some(arr) = items.as_array() {
                        let mut list = Vec::new();
                        for item in arr {
                            let name = item.get("name").and_then(|n| n.as_str()).unwrap_or("");
                            let owner = item
                                .get("owner")
                                .and_then(|o| o.get("login"))
                                .and_then(|l| l.as_str())
                                .unwrap_or("");
                            let desc = item
                                .get("description")
                                .and_then(|d| d.as_str())
                                .unwrap_or("");
                            let is_priv = item
                                .get("private")
                                .and_then(|p| p.as_bool())
                                .unwrap_or(false);
                            list.push(json!({
                                "name": name,
                                "owner": owner,
                                "full_name": format!("{}/{}", owner, name),
                                "is_private": is_priv,
                                "description": desc
                            }));
                        }
                        return Json(json!({ "repos": list }));
                    }
                }
            }
        }
    }

    Json(json!({ "repos": [] }))
}

async fn get_github_prs(
    State(state): State<AppState>,
    Query(q): Query<GitHubQuery>,
) -> impl IntoResponse {
    let (ws_owner, ws_repo) = detect_workspace_github_repo(&state.workspace_root)
        .unwrap_or_else(|| ("".to_string(), "".to_string()));

    let owner = q.owner.filter(|o| !o.trim().is_empty()).unwrap_or(ws_owner);
    let repo = q.repo.filter(|r| !r.trim().is_empty()).unwrap_or(ws_repo);

    if owner.is_empty() || repo.is_empty() {
        return (StatusCode::OK, Json(json!([])));
    }

    let client = reqwest::Client::new();
    let url = format!("https://api.github.com/repos/{owner}/{repo}/pulls?per_page=20");
    let mut req = client.get(&url).header("User-Agent", "odon-desktop");
    if let Some(token) = get_github_auth_token().await {
        req = req.header("Authorization", format!("Bearer {}", token));
    }
    match req.send().await {
        Ok(resp) => {
            let data: Value = resp.json().await.unwrap_or(json!([]));
            (StatusCode::OK, Json(data))
        }
        Err(e) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(json!({ "error": e.to_string() })),
        ),
    }
}

async fn get_github_issues(
    State(state): State<AppState>,
    Query(q): Query<GitHubQuery>,
) -> impl IntoResponse {
    let (ws_owner, ws_repo) = detect_workspace_github_repo(&state.workspace_root)
        .unwrap_or_else(|| ("".to_string(), "".to_string()));

    let owner = q.owner.filter(|o| !o.trim().is_empty()).unwrap_or(ws_owner);
    let repo = q.repo.filter(|r| !r.trim().is_empty()).unwrap_or(ws_repo);

    if owner.is_empty() || repo.is_empty() {
        return (StatusCode::OK, Json(json!([])));
    }

    let st = q.state.unwrap_or_else(|| "open".to_string());
    let client = reqwest::Client::new();
    let url = format!("https://api.github.com/repos/{owner}/{repo}/issues?state={st}&per_page=20");
    let mut req = client.get(&url).header("User-Agent", "odon-desktop");
    if let Some(token) = get_github_auth_token().await {
        req = req.header("Authorization", format!("Bearer {}", token));
    }
    match req.send().await {
        Ok(resp) => {
            let data: Value = resp.json().await.unwrap_or(json!([]));
            (StatusCode::OK, Json(data))
        }
        Err(e) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(json!({ "error": e.to_string() })),
        ),
    }
}

#[derive(Deserialize)]
struct CreateIssuePayload {
    owner: Option<String>,
    repo: Option<String>,
    title: String,
    body: Option<String>,
}

async fn create_github_issue(
    State(state): State<AppState>,
    Json(req): Json<CreateIssuePayload>,
) -> impl IntoResponse {
    let (ws_owner, ws_repo) = detect_workspace_github_repo(&state.workspace_root)
        .unwrap_or_else(|| ("".to_string(), "".to_string()));

    let owner = req
        .owner
        .filter(|o| !o.trim().is_empty())
        .unwrap_or(ws_owner);
    let repo = req.repo.filter(|r| !r.trim().is_empty()).unwrap_or(ws_repo);

    if owner.is_empty() || repo.is_empty() {
        return (
            StatusCode::BAD_REQUEST,
            Json(json!({ "error": "Owner and repo are required" })),
        );
    }

    let client = reqwest::Client::new();
    let url = format!("https://api.github.com/repos/{owner}/{repo}/issues");
    let mut r = client.post(&url).header("User-Agent", "odon-desktop");
    if let Some(token) = get_github_auth_token().await {
        r = r.header("Authorization", format!("Bearer {}", token));
    }
    match r
        .json(&json!({ "title": req.title, "body": req.body.unwrap_or_default() }))
        .send()
        .await
    {
        Ok(resp) => {
            let data: Value = resp.json().await.unwrap_or(json!({ "status": "created" }));
            (StatusCode::OK, Json(data))
        }
        Err(e) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(json!({ "error": e.to_string() })),
        ),
    }
}

async fn get_automations(State(state): State<AppState>) -> impl IntoResponse {
    let list = state.automations.read().await.clone();
    Json(json!(list))
}

async fn save_automation(
    State(state): State<AppState>,
    Json(job): Json<AutomationJob>,
) -> impl IntoResponse {
    let mut list = state.automations.write().await;
    if let Some(pos) = list.iter().position(|j| j.id == job.id) {
        list[pos] = job.clone();
    } else {
        list.push(job.clone());
    }
    save_automations_to_disk(&list);
    Json(json!({ "success": true, "job": job }))
}

async fn delete_automation(
    Path(id): Path<String>,
    State(state): State<AppState>,
) -> impl IntoResponse {
    let mut list = state.automations.write().await;
    list.retain(|j| j.id != id);
    save_automations_to_disk(&list);
    Json(json!({ "success": true, "deleted": id }))
}

async fn trigger_automation(
    Path(id): Path<String>,
    State(state): State<AppState>,
) -> impl IntoResponse {
    let job_opt = {
        let list = state.automations.read().await;
        list.iter().find(|j| j.id == id).cloned()
    };

    if let Some(job) = job_opt {
        if job.last_status.as_deref() == Some("running") {
            return Json(json!({ "success": false, "error": "Automation is already running" }));
        }
        let session_id = format!("auto_{}_{}", job.id, chrono::Utc::now().timestamp());
        {
            let mut list = state.automations.write().await;
            if let Some(existing) = list.iter_mut().find(|candidate| candidate.id == job.id) {
                existing.last_run = Some(Utc::now().to_rfc3339());
                existing.last_status = Some("running".to_string());
            }
            save_automations_to_disk(&list);
        }
        let state_clone = state.clone();
        tokio::spawn(async move {
            run_automation_job(state_clone, job).await;
        });

        Json(json!({ "success": true, "status": "triggered", "session_id": session_id }))
    } else {
        Json(json!({ "success": false, "error": "Job not found" }))
    }
}

async fn get_automation_logs(State(state): State<AppState>) -> impl IntoResponse {
    let logs = state.automation_logs.read().await.clone();
    Json(json!(logs))
}

// ─────────────────────────────────────────────────────────────
// FILE EXPLORER HANDLERS
// ─────────────────────────────────────────────────────────────

#[derive(Deserialize)]
struct FsListQuery {
    path: Option<String>,
}

#[derive(Serialize)]
struct FsEntryItem {
    name: String,
    path: String,
    is_dir: bool,
    size: u64,
    modified_at: Option<String>,
    extension: Option<String>,
}

async fn fs_list_dir(
    State(state): State<AppState>,
    Query(q): Query<FsListQuery>,
) -> impl IntoResponse {
    let target = q.path.unwrap_or_else(|| ".".to_string());
    let path = std::path::Path::new(&target);

    let abs = if path.is_absolute() {
        path.to_path_buf()
    } else {
        std::path::Path::new(&state.workspace_root).join(path)
    };

    if let Err(e) = ensure_allowed_path(&state, &abs).await {
        return Json(json!({ "error": e, "entries": [] }));
    }

    let read_res = std::fs::read_dir(&abs);
    match read_res {
        Ok(entries) => {
            let mut list = Vec::new();
            for entry in entries.flatten() {
                let p = entry.path();
                let is_dir = p.is_dir();
                let name = p
                    .file_name()
                    .unwrap_or_default()
                    .to_string_lossy()
                    .to_string();
                let size = if is_dir {
                    0
                } else {
                    entry.metadata().map(|m| m.len()).unwrap_or(0)
                };
                let ext = p.extension().map(|e| e.to_string_lossy().to_string());
                let modified = entry
                    .metadata()
                    .ok()
                    .and_then(|m| m.modified().ok())
                    .map(|t| chrono::DateTime::<chrono::Utc>::from(t).to_rfc3339());
                let rel = p
                    .strip_prefix(&state.workspace_root)
                    .unwrap_or(&p)
                    .to_string_lossy()
                    .replace('\\', "/");

                list.push(FsEntryItem {
                    name,
                    path: rel,
                    is_dir,
                    size,
                    modified_at: modified,
                    extension: ext,
                });
            }

            // Sort directories first, then alphabetical
            list.sort_by(|a, b| {
                if a.is_dir != b.is_dir {
                    b.is_dir.cmp(&a.is_dir)
                } else {
                    a.name.to_lowercase().cmp(&b.name.to_lowercase())
                }
            });

            Json(json!({
                "current_path": abs.to_string_lossy().replace('\\', "/"),
                "workspace_root": state.workspace_root,
                "entries": list
            }))
        }
        Err(e) => Json(json!({
            "error": format!("Cannot open path: {}", e),
            "entries": []
        })),
    }
}

#[derive(Deserialize)]
struct FsReadQuery {
    path: String,
}

async fn fs_read_file(
    State(state): State<AppState>,
    Query(q): Query<FsReadQuery>,
) -> impl IntoResponse {
    let p = std::path::Path::new(&q.path);
    let abs = if p.is_absolute() {
        p.to_path_buf()
    } else {
        std::path::Path::new(&state.workspace_root).join(p)
    };
    if let Err(e) = ensure_allowed_path(&state, &abs).await {
        return Json(json!({ "error": e, "path": q.path }));
    }

    match std::fs::read(&abs) {
        Ok(bytes) => {
            let size = bytes.len() as u64;
            match String::from_utf8(bytes.clone()) {
                Ok(content) => Json(json!({
                    "path": q.path,
                    "name": abs.file_name().unwrap_or_default().to_string_lossy(),
                    "size": size,
                    "is_text": true,
                    "content": content
                })),
                Err(_) => {
                    use base64::Engine;
                    let b64 = base64::engine::general_purpose::STANDARD.encode(&bytes);
                    Json(json!({
                        "path": q.path,
                        "name": abs.file_name().unwrap_or_default().to_string_lossy(),
                        "size": size,
                        "is_text": false,
                        "base64": b64
                    }))
                }
            }
        }
        Err(e) => Json(json!({ "error": format!("Failed to read file: {}", e) })),
    }
}

async fn evaluate_http_action(
    state: &AppState,
    tool_name: &str,
    parameters: &Value,
    description: &str,
) -> Result<(), Value> {
    match state
        .approval
        .evaluate_and_request("desktop-api", tool_name, parameters, description)
        .await
    {
        Ok(
            ApprovalDecision::Approved
            | ApprovalDecision::ApprovedAlways
            | ApprovalDecision::ApprovedAll,
        ) => Ok(()),
        Ok(ApprovalDecision::Rejected { reason }) => {
            Err(json!({ "success": false, "error": reason, "code": "approval_rejected" }))
        }
        Ok(ApprovalDecision::Expired) => Err(json!({
            "success": false,
            "error": "Approval request expired",
            "code": "approval_expired"
        })),
        Err(error) => Err(json!({
            "success": false,
            "error": error.to_string(),
            "code": "approval_error"
        })),
    }
}

#[derive(Deserialize)]
struct FsWritePayload {
    path: String,
    content: String,
}

async fn fs_write_file(
    State(state): State<AppState>,
    Json(payload): Json<FsWritePayload>,
) -> impl IntoResponse {
    if let Err(error) = evaluate_http_action(
        &state,
        "fs_write",
        &json!({ "path": payload.path }),
        &format!("Write file {}", payload.path),
    )
    .await
    {
        return Json(error);
    }

    let p = std::path::Path::new(&payload.path);
    let abs = if p.is_absolute() {
        p.to_path_buf()
    } else {
        std::path::Path::new(&state.workspace_root).join(p)
    };
    if let Err(e) = ensure_allowed_path(&state, &abs).await {
        return Json(json!({ "success": false, "error": e, "path": payload.path }));
    }

    if let Some(parent) = abs.parent() {
        let _ = std::fs::create_dir_all(parent);
    }

    match std::fs::write(&abs, payload.content.as_bytes()) {
        Ok(_) => Json(json!({ "success": true, "path": payload.path })),
        Err(e) => Json(json!({ "success": false, "error": e.to_string() })),
    }
}

#[derive(Deserialize)]
struct FsMkdirPayload {
    path: String,
}

async fn fs_mkdir(
    State(state): State<AppState>,
    Json(payload): Json<FsMkdirPayload>,
) -> impl IntoResponse {
    if let Err(error) = evaluate_http_action(
        &state,
        "fs_mkdir",
        &json!({ "path": payload.path }),
        &format!("Create directory {}", payload.path),
    )
    .await
    {
        return Json(error);
    }

    let p = std::path::Path::new(&payload.path);
    let abs = if p.is_absolute() {
        p.to_path_buf()
    } else {
        std::path::Path::new(&state.workspace_root).join(p)
    };
    if let Err(e) = ensure_allowed_path(&state, &abs).await {
        return Json(json!({ "success": false, "error": e, "path": payload.path }));
    }

    match std::fs::create_dir_all(&abs) {
        Ok(_) => Json(json!({ "success": true, "path": payload.path })),
        Err(e) => Json(json!({ "success": false, "error": e.to_string() })),
    }
}

#[derive(Deserialize)]
struct FsDeleteQuery {
    path: String,
}

async fn fs_delete_path(
    State(state): State<AppState>,
    Query(q): Query<FsDeleteQuery>,
) -> impl IntoResponse {
    if let Err(error) = evaluate_http_action(
        &state,
        "fs_delete",
        &json!({ "path": q.path }),
        &format!("Delete path {}", q.path),
    )
    .await
    {
        return Json(error);
    }

    let p = std::path::Path::new(&q.path);
    let abs = if p.is_absolute() {
        p.to_path_buf()
    } else {
        std::path::Path::new(&state.workspace_root).join(p)
    };
    if let Err(e) = ensure_allowed_path(&state, &abs).await {
        return Json(json!({ "success": false, "error": e, "path": q.path }));
    }
    // Refuse to delete a workspace root itself.
    if let Ok(canonical) = std::fs::canonicalize(&abs) {
        if workspace_roots(&state).await.iter().any(|root| {
            std::fs::canonicalize(root)
                .map(|r| r == canonical)
                .unwrap_or(false)
        }) {
            return Json(
                json!({ "success": false, "error": "Refusing to delete a workspace root" }),
            );
        }
    }

    let res = if abs.is_dir() {
        std::fs::remove_dir_all(&abs)
    } else {
        std::fs::remove_file(&abs)
    };

    match res {
        Ok(_) => Json(json!({ "success": true, "path": q.path })),
        Err(e) => Json(json!({ "success": false, "error": e.to_string() })),
    }
}

// ─────────────────────────────────────────────────────────────
// SSH REMOTE PROFILE & EXEC HANDLERS
// ─────────────────────────────────────────────────────────────

fn ssh_key_roots() -> Vec<PathBuf> {
    let mut roots = vec![user_home().join(".ssh")];
    let state_home = std::env::var_os("ODON_HOME")
        .map(PathBuf::from)
        .unwrap_or_else(user_home);
    roots.push(state_home.join(".odon").join("ssh-keys"));
    if let Ok(extra) = std::env::var("ODON_SSH_KEY_DIRS") {
        for item in extra
            .split(',')
            .map(str::trim)
            .filter(|item| !item.is_empty())
        {
            roots.push(PathBuf::from(expand_home(item)));
        }
    }
    roots
}

/// SSH `-i` arguments may only reference keys inside the dedicated key folders.
/// This prevents a key-path parameter from pointing at arbitrary local files
/// (credential discovery / path traversal).
fn validate_ssh_key_path(raw: &str) -> std::result::Result<String, String> {
    let expanded = expand_home(raw);
    let canonical = std::fs::canonicalize(&expanded)
        .map_err(|e| format!("SSH key not found or not accessible: {e}"))?;
    if !canonical.is_file() {
        return Err(format!("SSH key is not a file: {}", expanded));
    }
    let allowed = ssh_key_roots()
        .into_iter()
        .filter_map(|root| std::fs::canonicalize(root).ok())
        .any(|root| canonical.starts_with(&root));
    if !allowed {
        return Err(format!(
            "SSH key must live in ~/.ssh, ~/.odon/ssh-keys, or ODON_SSH_KEY_DIRS: {}",
            display_path(&canonical)
        ));
    }
    Ok(display_path(&canonical))
}

fn validate_ssh_target(host: &str, user: &str) -> std::result::Result<(), String> {
    let host = host.trim();
    let valid_host =
        if host.is_empty() || host.starts_with('-') || host.chars().any(char::is_whitespace) {
            false
        } else if let Some(inner) = host.strip_prefix('[').and_then(|h| h.strip_suffix(']')) {
            inner.parse::<std::net::Ipv6Addr>().is_ok()
        } else if host.parse::<std::net::IpAddr>().is_ok() {
            true
        } else {
            host.len() <= 253
                && !host.starts_with('.')
                && !host.ends_with('.')
                && host.split('.').all(|label| {
                    !label.is_empty()
                        && label.len() <= 63
                        && !label.starts_with('-')
                        && !label.ends_with('-')
                        && label
                            .chars()
                            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '-' | '_'))
                })
        };
    if !valid_host {
        return Err(format!(
            "Invalid SSH host: {host}. Use a DNS name, IPv4, or bracketed IPv6 address; use punycode for IDN."
        ));
    }
    let valid_user = !user.is_empty()
        && !user.starts_with('-')
        && user.len() <= 64
        && user
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '-' | '_'));
    if !valid_user {
        return Err(format!("Invalid SSH user: {user}"));
    }
    Ok(())
}

fn safe_key_filename(raw: &str) -> String {
    let name = std::path::Path::new(raw)
        .file_name()
        .map(|f| f.to_string_lossy().to_string())
        .unwrap_or_default()
        .replace(['/', '\\', ':', '*', '?', '"', '<', '>', '|'], "_");
    if name.is_empty() {
        format!("key-{}.pem", uuid::Uuid::new_v4())
    } else {
        name
    }
}

#[derive(Serialize, Deserialize, Clone)]
struct SshProfile {
    id: String,
    alias: String,
    host: String,
    port: u16,
    user: String,
    #[serde(default)]
    key_path: Option<String>,
    #[serde(default)]
    certificate_filename: Option<String>,
    #[serde(default)]
    certificate_data: Option<String>,
    auth_type: String,
    #[serde(default)]
    description: Option<String>,
    created_at: String,
}

#[derive(Deserialize)]
struct SshCertImportPayload {
    filename: String,
    content: String,
    #[serde(default)]
    #[allow(dead_code)]
    profile_id: Option<String>,
}

async fn import_ssh_certificate(Json(payload): Json<SshCertImportPayload>) -> impl IntoResponse {
    let base = std::env::var_os("ODON_HOME")
        .map(PathBuf::from)
        .or_else(|| std::env::var_os("USERPROFILE").map(PathBuf::from))
        .or_else(|| std::env::var_os("HOME").map(PathBuf::from))
        .unwrap_or_else(|| PathBuf::from("."));
    let cert_dir = base.join(".odon").join("ssh-keys");
    let _ = std::fs::create_dir_all(&cert_dir);

    if payload.content.trim().is_empty() {
        return (
            StatusCode::BAD_REQUEST,
            Json(json!({ "success": false, "error": "Key content is empty" })),
        );
    }
    if payload.content.len() > 1024 * 1024 {
        return (
            StatusCode::BAD_REQUEST,
            Json(json!({ "success": false, "error": "Key content is too large (max 1 MB)" })),
        );
    }

    let clean_name = safe_key_filename(&payload.filename);
    let target_path = cert_dir.join(&clean_name);

    if let Err(e) = std::fs::write(&target_path, &payload.content) {
        return (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(json!({ "success": false, "error": e.to_string() })),
        );
    }

    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let _ = std::fs::set_permissions(&target_path, std::fs::Permissions::from_mode(0o600));
    }

    let saved_path = target_path.to_string_lossy().to_string();
    (
        StatusCode::OK,
        Json(json!({
            "success": true,
            "saved_path": saved_path,
            "filename": clean_name
        })),
    )
}

fn get_ssh_profiles_path() -> PathBuf {
    let base = std::env::var_os("ODON_HOME")
        .map(PathBuf::from)
        .or_else(|| std::env::var_os("USERPROFILE").map(PathBuf::from))
        .or_else(|| std::env::var_os("HOME").map(PathBuf::from))
        .unwrap_or_else(|| PathBuf::from("."));
    base.join(".odon").join("ssh-profiles.json")
}

fn load_ssh_profiles() -> Vec<SshProfile> {
    let path = get_ssh_profiles_path();
    if let Ok(data) = std::fs::read_to_string(&path) {
        if let Ok(profiles) = serde_json::from_str::<Vec<SshProfile>>(&data) {
            return profiles;
        }
    }
    // Default to empty list; user-managed via Settings or SSH tab
    Vec::new()
}

fn save_ssh_profiles_to_disk(profiles: &[SshProfile]) {
    let path = get_ssh_profiles_path();
    if let Some(parent) = path.parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    if let Ok(data) = serde_json::to_string_pretty(profiles) {
        let _ = std::fs::write(&path, data);
    }
}

async fn get_ssh_profiles() -> impl IntoResponse {
    let profiles = load_ssh_profiles();
    Json(json!({ "profiles": profiles }))
}

async fn save_ssh_profile(Json(profile): Json<SshProfile>) -> impl IntoResponse {
    if let Err(e) = validate_ssh_target(&profile.host, &profile.user) {
        return (
            StatusCode::BAD_REQUEST,
            Json(json!({ "success": false, "error": e })),
        );
    }

    let mut profile = profile;
    // Never persist private key material inside the profile JSON. If a client
    // sends certificate_data, move it to the protected key directory and keep
    // only the resulting path in the profile.
    if let Some(data) = profile.certificate_data.take() {
        if !data.trim().is_empty() {
            let base = std::env::var_os("ODON_HOME")
                .map(PathBuf::from)
                .unwrap_or_else(user_home);
            let cert_dir = base.join(".odon").join("ssh-keys");
            let _ = std::fs::create_dir_all(&cert_dir);
            let name = safe_key_filename(
                profile
                    .certificate_filename
                    .as_deref()
                    .unwrap_or("profile-key.pem"),
            );
            let target = cert_dir.join(&name);
            if let Err(e) = std::fs::write(&target, data.as_bytes()) {
                return (
                    StatusCode::INTERNAL_SERVER_ERROR,
                    Json(json!({ "success": false, "error": e.to_string() })),
                );
            }
            #[cfg(unix)]
            {
                use std::os::unix::fs::PermissionsExt;
                let _ = std::fs::set_permissions(&target, std::fs::Permissions::from_mode(0o600));
            }
            profile.key_path = Some(display_path(&target));
            profile.certificate_filename = Some(name);
        }
    }

    if let Some(key) = profile.key_path.clone() {
        match validate_ssh_key_path(&key) {
            Ok(path) => profile.key_path = Some(path),
            Err(e) => {
                return (
                    StatusCode::BAD_REQUEST,
                    Json(json!({ "success": false, "error": e })),
                );
            }
        }
    }

    let mut profiles = load_ssh_profiles();
    if let Some(idx) = profiles.iter().position(|p| p.id == profile.id) {
        profiles[idx] = profile;
    } else {
        profiles.push(profile);
    }
    save_ssh_profiles_to_disk(&profiles);
    (
        StatusCode::OK,
        Json(json!({ "success": true, "profiles": profiles })),
    )
}

async fn delete_ssh_profile(Path(id): Path<String>) -> impl IntoResponse {
    let mut profiles = load_ssh_profiles();
    profiles.retain(|p| p.id != id);
    save_ssh_profiles_to_disk(&profiles);
    Json(json!({ "success": true, "profiles": profiles }))
}

#[derive(Deserialize)]
struct SshExecPayload {
    profile_id: Option<String>,
    host: Option<String>,
    port: Option<u16>,
    user: Option<String>,
    key_path: Option<String>,
    command: String,
}

async fn exec_ssh_command(
    State(state): State<AppState>,
    Json(req): Json<SshExecPayload>,
) -> impl IntoResponse {
    let profiles = load_ssh_profiles();
    let profile = req
        .profile_id
        .as_ref()
        .and_then(|pid| profiles.iter().find(|p| &p.id == pid || &p.alias == pid));

    let host = req
        .host
        .as_deref()
        .or_else(|| profile.map(|p| p.host.as_str()))
        .unwrap_or("localhost");
    let port = req.port.or_else(|| profile.map(|p| p.port)).unwrap_or(22);
    let user = req
        .user
        .as_deref()
        .or_else(|| profile.map(|p| p.user.as_str()))
        .unwrap_or("root");
    let key = req
        .key_path
        .as_deref()
        .or_else(|| profile.and_then(|p| p.key_path.as_deref()));

    if let Err(e) = validate_ssh_target(host, user) {
        return Json(json!({
            "exit_code": 1,
            "stdout": "",
            "stderr": e,
            "success": false
        }));
    }

    // Remote commands are still commands: apply the same catastrophic-command
    // policy used by local shell tools.
    let policy = state.approval.get_policy().await;
    if let Some(reason) = policy.command_forbidden_reason(&req.command) {
        return Json(json!({
            "exit_code": 1,
            "stdout": "",
            "stderr": format!("Command blocked by security policy: {reason}"),
            "success": false
        }));
    }

    let session_id = req.profile_id.as_deref().unwrap_or("ssh");
    let command_text = req.command.clone();
    let parameters = json!({"command": req.command, "host": host, "port": port, "user": user});
    let description = format!("Run SSH command on {user}@{host}: {command_text}");
    match state
        .approval
        .evaluate_and_request(session_id, "ssh_exec", &parameters, &description)
        .await
    {
        Ok(odon_security::ApprovalDecision::Approved)
        | Ok(odon_security::ApprovalDecision::ApprovedAlways)
        | Ok(odon_security::ApprovalDecision::ApprovedAll) => {}
        Ok(odon_security::ApprovalDecision::Rejected { reason }) => {
            return Json(json!({
                "exit_code": 1,
                "stdout": "",
                "stderr": format!("SSH command execution denied: {reason}"),
                "success": false
            }));
        }
        Ok(odon_security::ApprovalDecision::Expired) | Err(_) => {
            return Json(json!({
                "exit_code": 1,
                "stdout": "",
                "stderr": "SSH command approval unavailable or timed out",
                "success": false
            }));
        }
    }

    let mut cmd = tokio::process::Command::new("ssh");
    cmd.arg("-p").arg(port.to_string());
    cmd.arg("-o").arg("StrictHostKeyChecking=accept-new");
    cmd.arg("-o").arg("ConnectTimeout=10");

    if let Some(k) = key {
        match validate_ssh_key_path(k) {
            Ok(path) => {
                cmd.arg("-i").arg(path);
            }
            Err(e) => {
                return Json(json!({
                    "exit_code": 1,
                    "stdout": "",
                    "stderr": e,
                    "success": false
                }));
            }
        }
    }

    cmd.arg(format!("{user}@{host}"));
    cmd.arg(&req.command);

    let execution =
        match tokio::time::timeout(std::time::Duration::from_secs(120), cmd.output()).await {
            Ok(result) => result,
            Err(_) => {
                return Json(json!( {
                    "exit_code": -1,
                    "stdout": "",
                    "stderr": "SSH command timed out after 120 seconds",
                    "success": false
                }));
            }
        };
    let output = match execution {
        Ok(output) => output,
        Err(e) => {
            return Json(json!( {
                "exit_code": 1,
                "stdout": "",
                "stderr": format!("Failed to execute ssh: {}", e),
                "success": false
            }));
        }
    };
    let stdout = String::from_utf8_lossy(&output.stdout).to_string();
    let stderr = String::from_utf8_lossy(&output.stderr).to_string();
    const MAX_OUTPUT_BYTES: usize = 1_048_576;
    let mut capped_stdout = stdout;
    let mut capped_stderr = stderr;
    let mut truncated = false;
    if capped_stdout.len() > MAX_OUTPUT_BYTES {
        truncate_on_char_boundary(&mut capped_stdout, MAX_OUTPUT_BYTES);
        truncated = true;
    }
    if capped_stderr.len() > MAX_OUTPUT_BYTES {
        truncate_on_char_boundary(&mut capped_stderr, MAX_OUTPUT_BYTES);
        truncated = true;
    }
    if truncated {
        capped_stderr.push_str("\n[ODON] output capped at 1 MB");
    }
    Json(json!( {
        "exit_code": output.status.code().unwrap_or(-1),
        "stdout": capped_stdout,
        "stderr": capped_stderr,
        "success": output.status.success()
    }))
}

// ─────────────────────────────────────────────────────────────
// SKILLS HANDLERS (.odon/skills)
// ─────────────────────────────────────────────────────────────

fn get_skills_dir() -> PathBuf {
    let base = std::env::var_os("ODON_HOME")
        .map(PathBuf::from)
        .or_else(|| std::env::var_os("USERPROFILE").map(PathBuf::from))
        .or_else(|| std::env::var_os("HOME").map(PathBuf::from))
        .unwrap_or_else(|| PathBuf::from("."));
    base.join(".odon").join("skills")
}

#[derive(Serialize)]
struct SkillItemInfo {
    name: String,
    title: String,
    description: String,
    path: String,
    content: String,
}

async fn get_skills_list() -> impl IntoResponse {
    let dir = get_skills_dir();
    let mut skills = Vec::new();

    if let Ok(entries) = std::fs::read_dir(&dir) {
        for entry in entries.flatten() {
            let p = entry.path();
            if p.extension().is_some_and(|ext| ext == "md") {
                let name = p
                    .file_stem()
                    .unwrap_or_default()
                    .to_string_lossy()
                    .to_string();
                let content = std::fs::read_to_string(&p).unwrap_or_default();
                let first_line = content
                    .lines()
                    .next()
                    .unwrap_or("")
                    .trim_start_matches('#')
                    .trim()
                    .to_string();
                let title = if first_line.is_empty() {
                    name.clone()
                } else {
                    first_line
                };
                let desc = content.lines().nth(1).unwrap_or("").trim().to_string();

                skills.push(SkillItemInfo {
                    name,
                    title,
                    description: desc,
                    path: p.to_string_lossy().to_string(),
                    content,
                });
            }
        }
    }

    Json(json!({ "skills": skills }))
}

#[derive(Deserialize)]
struct SaveSkillPayload {
    name: String,
    content: String,
}

fn safe_skill_slug(raw: &str) -> String {
    let slug: String = raw
        .trim()
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || c == '-' || c == '_' {
                c.to_ascii_lowercase()
            } else {
                '-'
            }
        })
        .collect();
    let slug = slug.trim_matches('-');
    if slug.is_empty() {
        "skill".to_string()
    } else {
        slug.to_string()
    }
}

async fn save_skill_item(Json(payload): Json<SaveSkillPayload>) -> impl IntoResponse {
    if payload.content.len() > 1024 * 1024 {
        return Json(json!({ "success": false, "error": "Skill content is too large (max 1 MB)" }));
    }
    let dir = get_skills_dir();
    let _ = std::fs::create_dir_all(&dir);
    let filename = format!("{}.md", safe_skill_slug(&payload.name));
    let path = dir.join(filename);

    match std::fs::write(&path, payload.content.as_bytes()) {
        Ok(_) => Json(json!({ "success": true, "path": path.to_string_lossy() })),
        Err(e) => Json(json!({ "success": false, "error": e.to_string() })),
    }
}

async fn delete_skill_item(Path(name): Path<String>) -> impl IntoResponse {
    let dir = get_skills_dir();
    let filename = format!("{}.md", safe_skill_slug(&name));
    let path = dir.join(filename);

    match std::fs::remove_file(&path) {
        Ok(_) => Json(json!({ "success": true })),
        Err(e) => Json(json!({ "success": false, "error": e.to_string() })),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::TimeZone;

    #[test]
    fn composer_modes_change_the_turn_not_just_the_placeholder() {
        // Chat is the baseline: no plan gate, no model override, default step cap.
        let chat = resolve_turn_plan("chat", Some("gpt-5.6-luna"), None, None);
        assert!(!chat.plan_mode);
        assert_eq!(chat.model.as_deref(), Some("gpt-5.6-luna"));
        assert_eq!(chat.iteration_cap, None);
        assert_eq!(chat.instructions, None);

        // Auto routes to the provider default instead of the pinned model.
        let auto = resolve_turn_plan("auto", Some("gpt-5.6-luna"), None, None);
        assert_eq!(auto.model.as_deref(), Some("odon-auto"));
        assert!(!auto.plan_mode);
        assert!(auto.instructions.unwrap().contains("AUTO"));

        // Agent authorises delegation and leaves the step budget to the
        // runtime default (500, or ODON_MAX_TURN_ITERATIONS) instead of
        // lowering it with a hardcoded cap.
        let agent = resolve_turn_plan("agent", Some("gpt-5.6-luna"), None, None);
        assert_eq!(agent.iteration_cap, None);
        assert!(!agent.plan_mode);
        assert!(agent.instructions.unwrap().contains("run_subagent"));

        // Plan gates mutating tools even when the client forgot the flag.
        let plan = resolve_turn_plan("plan", Some("gpt-5.6-luna"), None, None);
        assert!(plan.plan_mode);

        // A stale `plan_mode: true` from an earlier turn must not leak into chat.
        let chat_after_plan = resolve_turn_plan("chat", None, Some(false), None);
        assert!(!chat_after_plan.plan_mode);

        // Custom instructions are preserved and mode guidance is appended.
        let custom = resolve_turn_plan("agent", None, None, Some("Answer in Indonesian"));
        let text = custom.instructions.unwrap();
        assert!(text.starts_with("Answer in Indonesian"));
        assert!(text.contains("AGENT"));
    }

    #[test]
    fn automation_intervals_and_cron_match() {
        assert_eq!(parse_interval("15m").unwrap().as_secs(), 900);
        assert_eq!(parse_interval("Every 2 hours").unwrap().as_secs(), 7200);
        assert_eq!(parse_interval("daily").unwrap().as_secs(), 86_400);
        assert!(parse_interval("nonsense").is_none());

        let at = Local.with_ymd_and_hms(2026, 11, 2, 8, 0, 0).unwrap();
        assert!(cron_matches("0 8 * * *", &at));
        assert!(!cron_matches("0 9 * * *", &at));
        assert!(cron_matches("*/15 * * * *", &at));
        assert_eq!(
            normalize_cron_expression("0 8 * * * (Daily @ 08:00 AM)"),
            "0 8 * * *"
        );
        assert!(parse_last_run("2 hours ago").is_some());
        assert!(parse_last_run("Yesterday at 08:00 AM").is_some());
    }

    #[test]
    fn desktop_dist_candidates_cover_dev_override_and_bundle_layouts() {
        let exe_dir = std::path::Path::new("/opt/odon/bin");
        let candidates = desktop_dist_candidates(Some("/custom/dist"), Some(exe_dir));
        assert_eq!(candidates[0], std::path::PathBuf::from("/custom/dist"));
        assert_eq!(candidates[1], std::path::PathBuf::from("desktop/dist"));
        assert_eq!(
            candidates[2],
            std::path::PathBuf::from("/opt/odon/bin/desktop/dist")
        );
        assert_eq!(
            candidates[3],
            std::path::PathBuf::from("/opt/odon/bin/../Resources/desktop/dist")
        );
        // A blank override is ignored rather than shadowing the dev path.
        let candidates = desktop_dist_candidates(Some("  "), None);
        assert_eq!(candidates[0], std::path::PathBuf::from("desktop/dist"));
    }

    #[test]
    fn workspace_validator_accepts_temp_and_rejects_system_roots() {
        let temp = std::env::temp_dir();
        let temp_str = temp.to_string_lossy().to_string();
        assert!(canonical_workspace(&temp_str).is_ok());
        assert!(canonical_workspace("").is_err());
        assert!(canonical_workspace("definitely-not-a-real-folder-odon").is_err());
        #[cfg(windows)]
        {
            assert!(canonical_workspace("C:\\").is_err());
            assert!(canonical_workspace("C:\\Windows").is_err());
        }
        #[cfg(not(windows))]
        {
            assert!(canonical_workspace("/").is_err());
            assert!(canonical_workspace("/etc").is_err());
        }
    }

    #[test]
    fn workspace_allowlist_matches_default() {
        let temp = canonical_workspace(&std::env::temp_dir().to_string_lossy()).unwrap();
        let config = WorkspaceConfig {
            default: temp.clone(),
            allowed: vec![temp.clone()],
        };
        assert!(workspace_allowed(&config, &temp));
        #[cfg(windows)]
        assert!(!workspace_allowed(&config, "C:\\Windows"));
        #[cfg(not(windows))]
        assert!(!workspace_allowed(&config, "/etc"));
    }

    #[test]
    fn ssh_target_validation_accepts_dns_ipv4_ipv6_and_punycode() {
        assert!(validate_ssh_target("example.com", "root").is_ok());
        assert!(validate_ssh_target("sub.example.co.uk", "deploy_user").is_ok());
        assert!(validate_ssh_target("xn--bcher-kva.example", "root").is_ok());
        assert!(validate_ssh_target("192.168.1.10", "root").is_ok());
        assert!(validate_ssh_target("2001:db8::1", "root").is_ok());
        assert!(validate_ssh_target("[2001:db8::1]", "root").is_ok());
        assert!(validate_ssh_target("my_host.local", "root").is_ok());
    }

    #[test]
    fn ssh_target_validation_rejects_injection_and_bad_hosts() {
        for host in [
            "",
            "-oProxyCommand=calc.exe",
            "host;rm -rf /",
            "bad host",
            "example..com",
            ".example.com",
            "example.com.",
            "münchen.de",
            "a:b",
        ] {
            assert!(
                validate_ssh_target(host, "root").is_err(),
                "host should be rejected: {host}"
            );
        }
        for user in ["", "-oProxyCommand=x", "root;rm", "root user", "üser"] {
            assert!(
                validate_ssh_target("example.com", user).is_err(),
                "user should be rejected: {user}"
            );
        }
    }

    #[test]
    fn safe_key_filename_strips_paths() {
        assert_eq!(safe_key_filename("../../id_rsa"), "id_rsa");
        assert_eq!(safe_key_filename("id_ed25519"), "id_ed25519");
        let generated = safe_key_filename("");
        assert!(generated.starts_with("key-"));
        assert!(generated.ends_with(".pem"));
    }

    #[test]
    fn skill_slugs_cannot_traverse() {
        assert_eq!(safe_skill_slug("../../etc/passwd"), "etc-passwd");
        assert_eq!(safe_skill_slug("My Skill"), "my-skill");
        assert_eq!(safe_skill_slug(""), "skill");
    }

    #[test]
    fn claude_oauth_token_builds_a_usable_anthropic_provider() {
        let token = odon_provider::OAuthToken {
            provider: "claude-oauth".to_string(),
            access_token: "test-access-token".to_string(),
            refresh_token: Some("test-refresh-token".to_string()),
            expires_at: None,
            account_id: None,
            id_token: None,
            email: None,
            plan: Some("Claude OAuth".to_string()),
        };
        let provider = build_oauth_provider("claude-oauth", &token)
            .expect("Claude OAuth must register a runtime provider");
        assert_eq!(provider.provider_name(), "claude-oauth");
    }

    #[test]
    fn muse_byok_defaults_to_the_meta_model_api() {
        // Muse is a BYOK provider built from a saved config. When the user does
        // not type a base URL, the fallback must be Meta's Model API; an unknown
        // id otherwise falls through to the Ollama URL and silently points the
        // provider at localhost.
        assert_eq!(default_base_url_for("muse"), "https://api.meta.ai/v1");
        assert_ne!(
            default_base_url_for("muse"),
            default_base_url_for("some-unknown-provider")
        );
    }

    #[test]
    fn resolve_first_existing_returns_first_match() {
        let dir = std::env::temp_dir().join("odond-resolve-test");
        let _ = std::fs::create_dir_all(&dir);
        let first = dir.join("a.mjs");
        let second = dir.join("b.mjs");
        std::fs::write(&first, "x").unwrap();
        std::fs::write(&second, "y").unwrap();

        assert_eq!(
            resolve_first_existing(vec![second.clone(), first.clone()]),
            Some(second)
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn resolve_first_existing_returns_none_when_missing() {
        assert!(resolve_first_existing(vec![
            PathBuf::from("/missing/a.mjs"),
            PathBuf::from("/missing/b.mjs"),
        ])
        .is_none());
    }

    fn handle(turn_id: &str) -> TurnHandle {
        TurnHandle {
            turn_id: turn_id.to_string(),
            cancel: tokio_util::sync::CancellationToken::new(),
            steer: SteerQueue::new(),
        }
    }

    #[test]
    fn retiring_a_finished_turn_keeps_the_newer_turns_handle() {
        // Turn A is running, then a newer message cancels it and installs B.
        let mut turns = std::collections::HashMap::new();
        turns.insert("s1".to_string(), handle("A"));
        turns.insert("s1".to_string(), handle("B"));

        // A now finishes. It must not delete B's handle, otherwise the session
        // reports idle (and rejects steering) while B is still running.
        assert!(!retire_turn_handle(&mut turns, "s1", "A"));
        assert_eq!(
            turns.get("s1").map(|h| h.turn_id.as_str()),
            Some("B"),
            "the newer turn's handle was removed by the finished turn"
        );
    }

    #[test]
    fn retiring_the_current_turn_removes_its_handle() {
        let mut turns = std::collections::HashMap::new();
        turns.insert("s1".to_string(), handle("B"));

        assert!(retire_turn_handle(&mut turns, "s1", "B"));
        assert!(turns.get("s1").is_none());

        // Retiring twice is harmless.
        assert!(!retire_turn_handle(&mut turns, "s1", "B"));
    }
}

#[cfg(test)]
mod oauth_store_tests {
    use super::*;

    const KEY: [u8; 32] = [7u8; 32];

    #[test]
    fn oauth_blob_round_trips_through_the_aead() {
        let plaintext = br#"{"chatgpt":{"access_token":"super-secret-value"}}"#;
        let blob = encrypt_oauth_blob_with_key(&KEY, plaintext).expect("encrypt");
        // The ciphertext must carry our magic header and must not leak the payload.
        assert!(blob.starts_with(OAUTH_MAGIC));
        assert!(
            !blob
                .windows(plaintext.len())
                .any(|window| window == plaintext),
            "ciphertext must not contain the plaintext token"
        );
        let recovered = decrypt_oauth_blob_with_key(&KEY, &blob).expect("decrypt");
        assert_eq!(recovered, plaintext);
    }

    #[test]
    fn oauth_blob_rejects_a_wrong_key() {
        let blob = encrypt_oauth_blob_with_key(&KEY, b"tokens").expect("encrypt");
        assert!(decrypt_oauth_blob_with_key(&[9u8; 32], &blob).is_none());
    }

    #[test]
    fn oauth_blob_rejects_tampering() {
        let mut blob = encrypt_oauth_blob_with_key(&KEY, b"tokens").expect("encrypt");
        let last = blob.len() - 1;
        blob[last] ^= 0xFF;
        assert!(decrypt_oauth_blob_with_key(&KEY, &blob).is_none());
    }

    #[test]
    fn oauth_blob_ignores_non_magic_input() {
        // A legacy plaintext JSON store has no magic header and is not decryptable.
        assert!(decrypt_oauth_blob_with_key(&KEY, b"{\"x\":1}").is_none());
    }
}

#[cfg(test)]
mod gateway_catalogue_tests {
    use super::*;
    use odon_storage::ProviderConfigRecord;

    fn config(models_json: &str) -> ProviderConfigRecord {
        ProviderConfigRecord {
            provider_id: "openai-compatible".to_string(),
            api_key: "k".to_string(),
            base_url: Some("http://localhost:11434/v1".to_string()),
            is_default: true,
            name: None,
            models_json: models_json.to_string(),
            updated_at: String::new(),
        }
    }

    #[test]
    fn gateway_models_come_from_the_saved_config() {
        // Regression: the static catalogue entry reported `models: []`, so a
        // saved gateway had nothing to select and every turn failed with
        // "No model selected" after the daemon started.
        let ids = gateway_model_ids(&[config("[\"gw-model-a\",\" gw-model-b \"]")]);
        assert_eq!(ids, vec!["gw-model-a".to_string(), "gw-model-b".to_string()]);
    }

    #[test]
    fn gateway_models_ignore_blank_and_broken_entries() {
        assert!(gateway_model_ids(&[config("[]")]).is_empty());
        assert!(gateway_model_ids(&[config("not json")]).is_empty());
        assert!(gateway_model_ids(&[config("[\"  \"]")]).is_empty());
        assert!(gateway_model_ids(&[]).is_empty());
    }

    #[test]
    fn gateway_models_ignore_other_providers() {
        let mut other = config("[\"nope\"]");
        other.provider_id = "deepseek".to_string();
        assert!(gateway_model_ids(&[other]).is_empty());
    }
}
