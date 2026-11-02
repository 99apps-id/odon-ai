#![recursion_limit = "512"]

use portable_pty::{native_pty_system, CommandBuilder, PtySize};
use axum::{
    extract::{
        ws::{Message as WsMessage, WebSocket, WebSocketUpgrade},
        Path, Query, State,
    },
    extract::Request,
    http::{header, HeaderMap, HeaderName, Method, StatusCode},
    middleware::{self, Next},
    response::{
        sse::{Event, KeepAlive, Sse},
        IntoResponse, Json,
    },
    routing::{get, post},
    Router,
};
use clap::{Parser, Subcommand};
use futures_util::stream::Stream;
use futures_util::{SinkExt, StreamExt};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::path::PathBuf;
use std::collections::HashMap;
use std::convert::Infallible;
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
use odon_core::AgentEvent;
use odon_provider::{
    anthropic::AnthropicProvider,
    host_creds::{discover_antigravity, discover_github_copilot},
    AntigravityProvider, OpenAiCompatibleProvider, SimulatedProvider,
};
use odon_security::{ApprovalDecision, ApprovalEngine, AuditLogger, ExecutionMode, SecurityPolicy};
use odon_storage::StorageEngine;
use odon_tools::ToolRegistry;

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
}


#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AutomationJob {
    pub id: String,
    pub name: String,
    pub description: String,
    pub trigger_type: String, // "interval", "cron", "on_commit", "manual"
    pub trigger_value: String, // e.g. "Every 2 hours", "0 8 * * *", "15m"
    pub prompt: String,
    pub model: String,
    pub enabled: bool,
    pub last_run: Option<String>,
    pub last_status: Option<String>, // "success", "failed", "running"
    pub created_at: String,
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
    if let Some(rest) = trimmed.strip_prefix("~/").or_else(|| trimmed.strip_prefix("~\\")) {
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
    let default = canonical_workspace(default_workspace)
        .unwrap_or_else(|_| expand_home(default_workspace));
    let mut allowed = Vec::new();
    if let Ok(data) = std::fs::read_to_string(get_workspaces_path()) {
        if let Ok(saved) = serde_json::from_str::<WorkspaceConfig>(&data) {
            allowed = saved.allowed;
        }
    }
    if !allowed.iter().any(|item| item.eq_ignore_ascii_case(&default)) {
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

/// Local manager for the Node/Baileys WhatsApp Web bridge.
#[derive(Clone)]
struct WhatsAppBridge {
    client: reqwest::Client,
    base_url: String,
    token: String,
    child: Arc<BridgeChild>,
    script: Option<PathBuf>,
    auth_dir: PathBuf,
    daemon_url: String,
    api_token: Option<String>,
    log_path: PathBuf,
    spawned: Arc<std::sync::Mutex<Option<(Vec<String>, bool)>>>,
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
        let token = std::env::var("WA_BRIDGE_TOKEN")
            .ok()
            .filter(|value| !value.trim().is_empty())
            .unwrap_or_else(|| uuid::Uuid::new_v4().to_string());
        Self {
            client: reqwest::Client::new(),
            base_url: format!("http://127.0.0.1:{port}"),
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
        if let Ok(explicit) = std::env::var("ODON_WHATSAPP_BRIDGE_SCRIPT") {
            let path = PathBuf::from(explicit);
            if path.exists() {
                return Some(path);
            }
        }
        let cwd = std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."));
        [
            "scripts/whatsapp-bridge/index.mjs",
            "../scripts/whatsapp-bridge/index.mjs",
            "../../scripts/whatsapp-bridge/index.mjs",
        ]
        .iter()
        .map(|relative| cwd.join(relative))
        .find(|candidate| candidate.exists())
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

    async fn healthy(&self) -> bool {
        self.client
            .get(format!("{}/health", self.base_url))
            .timeout(std::time::Duration::from_millis(800))
            .send()
            .await
            .map(|response| response.status().is_success())
            .unwrap_or(false)
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
        // An externally managed bridge that is already healthy can be reused.
        if self.healthy().await && self.spawned.lock().unwrap().is_none() {
            return Ok(());
        }
        self.stop();

        let script = self.script.clone().ok_or_else(|| {
            "WhatsApp bridge script not found. Expected scripts/whatsapp-bridge/index.mjs".to_string()
        })?;
        if !script.exists() {
            return Err(format!(
                "WhatsApp bridge script not found at {}",
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
            .env("WA_BRIDGE_PORT", self.base_url.rsplit(':').next().unwrap_or("4097"))
            .env("WA_AUTH_DIR", &self.auth_dir)
            .env("WA_BRIDGE_TOKEN", &self.token)
            .env("ODON_DAEMON_URL", &self.daemon_url)
            .env(
                "WA_ALLOWED_NUMBERS",
                allowed_numbers.join(","),
            )
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

        let child = command
            .spawn()
            .map_err(|error| format!("Failed to start the WhatsApp bridge (Node.js required): {error}"))?;
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
            .request(method, format!("{}{}", self.base_url, path))
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
}

fn get_oauth_tokens_path() -> PathBuf {
    let base = std::env::var_os("ODON_HOME")
        .map(PathBuf::from)
        .or_else(|| std::env::var_os("USERPROFILE").map(PathBuf::from))
        .or_else(|| std::env::var_os("HOME").map(PathBuf::from))
        .unwrap_or_else(|| PathBuf::from("."));
    base.join(".odon").join("oauth-tokens.json")
}

fn load_oauth_tokens() -> HashMap<String, odon_provider::OAuthToken> {
    let path = get_oauth_tokens_path();
    std::fs::read_to_string(&path)
        .ok()
        .and_then(|content| serde_json::from_str(&content).ok())
        .unwrap_or_default()
}

fn save_oauth_token(catalog_id: &str, token: &odon_provider::OAuthToken) {
    let mut tokens = load_oauth_tokens();
    tokens.insert(catalog_id.to_string(), token.clone());
    let path = get_oauth_tokens_path();
    if let Some(parent) = path.parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    if let Ok(data) = serde_json::to_string_pretty(&tokens) {
        if std::fs::write(&path, data).is_ok() {
            #[cfg(unix)]
            {
                use std::os::unix::fs::PermissionsExt;
                let _ = std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o600));
            }
        }
    }
}

fn remove_oauth_token(catalog_id: &str) {
    let mut tokens = load_oauth_tokens();
    if tokens.remove(catalog_id).is_some() {
        if let Ok(data) = serde_json::to_string_pretty(&tokens) {
            let path = get_oauth_tokens_path();
            let _ = std::fs::write(&path, data);
        }
    }
}

/// Build the runtime adapter for a persisted OAuth token. Returns None for
/// unknown catalog ids.
fn build_oauth_provider(
    catalog_id: &str,
    token: &odon_provider::OAuthToken,
) -> Option<Arc<dyn odon_provider::ModelProvider>> {
    use odon_provider::{MuseProvider, OpenAiCodexProvider};
    match catalog_id {
        "chatgpt" => Some(Arc::new(OpenAiCodexProvider::new(
            token.access_token.clone(),
            token.account_id.clone(),
            None,
        ))),
        "muse" => Some(Arc::new(MuseProvider::new(token.access_token.clone(), None))),
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

#[derive(Clone)]
struct TurnHandle {
    turn_id: String,
    cancel: tokio_util::sync::CancellationToken,
    steer: SteerQueue,
}

#[derive(Clone)]
struct AppState {
    runtime: Arc<AgentRuntime>,
    storage: Arc<StorageEngine>,
    approval: Arc<ApprovalEngine>,
    #[allow(dead_code)]
    audit: Arc<AuditLogger>,
    tools: Arc<ToolRegistry>,
    event_tx: broadcast::Sender<AgentEvent>,
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
    active_turns: Arc<tokio::sync::RwLock<HashMap<String, TurnHandle>>>,
}

/// Loads `.env` then `.env.local` from the working directory into the process
/// environment. Values already set in the real environment always win, and values
/// are never logged.
fn load_dotenv_files() {
    for name in [".env.local", ".env"] {
        let Ok(text) = std::fs::read_to_string(name) else { continue };
        // Tolerate a UTF-8 BOM (Windows editors/PowerShell add one), which would
        // otherwise corrupt the first key.
        let text = text.trim_start_matches('\u{feff}');
        for line in text.lines() {
            let line = line.trim();
            if line.is_empty() || line.starts_with('#') {
                continue;
            }
            let Some((key, value)) = line.split_once('=') else { continue };
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
            println!("   Open Digital Orchestration Network v{}", env!("CARGO_PKG_VERSION"));
            println!("====================================================");
            info!("Workspace root: {}", workspace);

            // 1. Storage
            let storage = Arc::new(StorageEngine::default_storage()?);

            // 2. Event Bus (created first so approvals can publish on it)
            let (event_tx, _event_rx) = create_event_bus(512);

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

            let api_token = std::env::var("ODON_API_TOKEN")
                .ok()
                .map(|value| value.trim().to_string())
                .filter(|value| !value.is_empty());
            let bind_host = if let Some(rest) = bind.strip_prefix('[') {
                rest.split(']').next().unwrap_or("")
            } else {
                bind.rsplit_once(':').map(|(host, _)| host).unwrap_or(bind.as_str())
            };
            let is_loopback = matches!(bind_host, "127.0.0.1" | "localhost" | "::1");
            if !is_loopback && api_token.is_none() {
                return Err(anyhow::anyhow!(
                    "Refusing to bind {bind} without ODON_API_TOKEN. Set ODON_API_TOKEN to protect a network-exposed daemon, or bind 127.0.0.1."
                ));
            }
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
                        let bridge_connected = if bridge.child_running() {
                            bridge
                                .status()
                                .await
                                .get("connected")
                                .and_then(|value| value.as_bool())
                                .unwrap_or(false)
                        } else {
                            false
                        };
                        flag.store(
                            cloud_active || bridge_connected,
                            std::sync::atomic::Ordering::SeqCst,
                        );
                        tokio::time::sleep(std::time::Duration::from_secs(5)).await;
                    }
                });
            }

            // Single dispatcher shared by every inbound channel: run one agent
            // turn and deliver the final answer back on the originating channel.
            {
                let rt_relay = runtime.clone();
                let tg_handle = telegram_handle.clone();
                let wa_cloud_handle = whatsapp_cloud_handle.clone();
                let bridge = whatsapp_bridge.clone();
                let relay_model = std::env::var("ODON_RELAY_MODEL").ok();
                tokio::spawn(async move {
                    while let Some(msg) = relay_rx.recv().await {
                        let session_id = msg
                            .session_id
                            .clone()
                            .unwrap_or_else(|| format!("{}_{}", msg.channel, msg.sender_id));
                        info!(
                            "Relay turn from {} [{}] in session {}",
                            msg.channel, msg.sender_id, session_id
                        );
                        let cancel = tokio_util::sync::CancellationToken::new();
                        let result = rt_relay
                            .execute_turn_with_attachments(
                                &session_id,
                                &msg.text,
                                msg.attachments.clone(),
                                relay_model.as_deref(),
                                None,
                                cancel,
                            )
                            .await;
                        let reply = match result {
                            Ok(turn) => {
                                if turn.content.text.trim().is_empty() {
                                    "✅ Done.".to_string()
                                } else {
                                    turn.content.text
                                }
                            }
                            Err(e) => format!("⚠️ ODON error: {e}"),
                        };
                        let delivered: std::result::Result<(), String> = match msg.channel.as_str() {
                            "telegram" => {
                                let relay = tg_handle.read().await.clone();
                                match relay {
                                    Some(relay) => relay
                                        .send_direct_message(&msg.sender_id, &reply)
                                        .await
                                        .map_err(|e| e.to_string()),
                                    None => Ok(()),
                                }
                            }
                            "whatsapp" => {
                                let cloud = wa_cloud_handle.read().await.clone();
                                if let Some(cloud) = cloud {
                                    cloud
                                        .send_direct_message(&msg.sender_id, &reply)
                                        .await
                                        .map_err(|e| e.to_string())
                                } else {
                                    bridge.send(&msg.sender_id, &reply).await
                                }
                            }
                            _ => Ok(()),
                        };
                        if let Err(e) = delivered {
                            tracing::error!("Failed to deliver {} reply: {e}", msg.channel);
                        }
                    }
                });
            }

            let automations = Arc::new(tokio::sync::RwLock::new(load_automations_from_disk()));
            let automation_logs = Arc::new(tokio::sync::RwLock::new(Vec::new()));

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
                active_turns: Arc::new(tokio::sync::RwLock::new(HashMap::new())),
            };

            let allowed_origins: Vec<String> = std::env::var("ODON_ALLOWED_ORIGINS")
                .ok()
                .map(|value| {
                    value
                        .split(',')
                        .map(|item| item.trim().to_ascii_lowercase())
                        .filter(|item| !item.is_empty())
                        .collect()
                })
                .unwrap_or_default();
            let cors = CorsLayer::new()
                .allow_origin(AllowOrigin::predicate(move |origin, _| {
                    let Ok(value) = origin.to_str() else {
                        return false;
                    };
                    let lower = value.to_ascii_lowercase();
                    let local = lower == "http://localhost"
                        || lower.starts_with("http://localhost:")
                        || lower == "http://127.0.0.1"
                        || lower.starts_with("http://127.0.0.1:")
                        || lower == "http://[::1]"
                        || lower.starts_with("http://[::1]:")
                        || lower == "tauri://localhost"
                        || lower == "https://tauri.localhost"
                        || lower == "http://tauri.localhost";
                    local || allowed_origins.iter().any(|allowed| allowed == &lower)
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
                .route("/api/sessions", get(list_sessions).post(create_session))
                .route("/api/sessions/{id}", get(get_session).delete(delete_session))
                .route("/api/sessions/{id}/messages", get(list_messages).post(send_message))
                .route("/api/sessions/{id}/cancel", post(cancel_turn))
                .route("/api/sessions/{id}/steer", post(steer_turn))
                .route("/api/sessions/{id}/workspace", post(set_session_workspace))
                .route("/api/sessions/{id}/artifacts", get(list_session_artifacts))
                .route("/api/workspaces", get(list_workspaces).post(add_workspace).delete(remove_workspace))
                .route("/api/workspaces/browse", get(browse_workspaces))
                .route("/api/artifacts/{id}", get(get_artifact))
                .route("/api/memory", get(list_agent_memories))
                .route("/api/memory/{*key}", axum::routing::delete(delete_agent_memory))
                .route("/api/tools", get(list_tools))
                .route("/api/approvals", get(list_approvals))
                .route("/api/approvals/{id}", post(resolve_approval))
                .route("/api/approvals/history", get(get_approval_history))
                .route("/api/approvals/policies", get(get_approval_policies))
                .route("/api/approvals/policies/{*key}", axum::routing::delete(delete_approval_policy))
                .route("/api/mcp/servers", get(get_mcp_servers).post(save_mcp_server))
                .route("/api/mcp/servers/{id}", axum::routing::delete(delete_mcp_server))
                .route("/api/mcp/servers/{id}/toggle", post(toggle_mcp_server))
                .route("/api/github/status", get(get_github_status))
                .route("/api/github/user-repos", get(get_github_user_repos))
                .route("/api/github/prs", get(get_github_prs))
                .route("/api/github/issues", get(get_github_issues).post(create_github_issue))
                .route("/api/automations", get(get_automations).post(save_automation))
                .route("/api/automations/{id}", axum::routing::delete(delete_automation))
                .route("/api/automations/{id}/trigger", post(trigger_automation))
                .route("/api/automations/logs", get(get_automation_logs))
                .route("/api/settings", get(get_settings).post(update_settings))
                .route("/api/models", get(get_models))
                .route("/api/providers/config", get(get_provider_configs).post(save_provider_config))
                .route("/api/providers/config/{provider_id}", axum::routing::delete(delete_provider_config))
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
                .route("/api/ssh/profiles", get(get_ssh_profiles).post(save_ssh_profile))
                .route("/api/ssh/profiles/{id}", axum::routing::delete(delete_ssh_profile))
                .route("/api/ssh/exec", post(exec_ssh_command))
                .route("/api/ssh/certificates/import", post(import_ssh_certificate))
                // Skills APIs
                .route("/api/skills", get(get_skills_list).post(save_skill_item))
                .route("/api/skills/{name}", axum::routing::delete(delete_skill_item));

            let dist_path = std::path::Path::new("desktop/dist");
            if dist_path.exists() {
                info!("Serving static desktop web application from desktop/dist");
                app = app.fallback_service(ServeDir::new(dist_path));
            }

            let auth_state = state.clone();
            let app = app
                .layer(cors)
                .with_state(state)
                .layer(middleware::from_fn_with_state(auth_state, require_api_token));

            let listener = tokio::net::TcpListener::bind(&bind).await?;
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

/// Optional bearer-token protection for the HTTP API. Enabled by setting
/// `ODON_API_TOKEN`; the WhatsApp webhook is exempt because it has its own
/// verify-token/signature checks. SSE and the terminal WebSocket may pass the
/// token as a `?token=` query parameter because browsers cannot set headers on
/// those transports.
async fn require_api_token(
    State(state): State<AppState>,
    req: Request,
    next: Next,
) -> axum::response::Response {
    let Some(expected) = state.api_token.as_deref() else {
        return next.run(req).await;
    };
    let path = req.uri().path();
    if !path.starts_with("/api/")
        || path == "/api/relays/whatsapp"
        || req.method() == Method::OPTIONS
    {
        return next.run(req).await;
    }

    let bearer_ok = req
        .headers()
        .get(header::AUTHORIZATION)
        .and_then(|value| value.to_str().ok())
        .map(|value| value == format!("Bearer {expected}"))
        .unwrap_or(false);
    let header_ok = req
        .headers()
        .get("x-odon-token")
        .and_then(|value| value.to_str().ok())
        .map(|value| value == expected)
        .unwrap_or(false);
    let query_ok = req
        .uri()
        .query()
        .map(|query| query.split('&').any(|pair| pair == format!("token={expected}")))
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
    if let Some(secret) = app_secret {
        let signature = headers
            .get("x-hub-signature-256")
            .and_then(|value| value.to_str().ok())
            .unwrap_or("");
        if !verify_meta_signature(&secret, body.as_bytes(), signature) {
            tracing::warn!("Rejected WhatsApp webhook with invalid signature");
            return (StatusCode::UNAUTHORIZED, "Invalid signature").into_response();
        }
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
    if bytes.len() % 2 != 0 {
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
        Err(e) => (StatusCode::INTERNAL_SERVER_ERROR, Json(json!({ "error": e.to_string() }))),
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
                        Json(json!({ "error": "Workspace is not in the allowlist. Add it from the composer first." })),
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
        Err(e) => (StatusCode::INTERNAL_SERVER_ERROR, Json(json!({ "error": e.to_string() }))),
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
        Err(e) => return (StatusCode::NOT_FOUND, Json(json!({ "error": e.to_string() }))),
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
        Err(e) => (StatusCode::INTERNAL_SERVER_ERROR, Json(json!({ "error": e.to_string() }))),
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
    if !config.allowed.iter().any(|item| item.eq_ignore_ascii_case(&path)) {
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
    entries.sort_by(|a, b| a.name.to_lowercase().cmp(&b.name.to_lowercase()));

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
        let mut root = PathBuf::from(workspace);
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

async fn get_session(
    Path(id): Path<String>,
    State(state): State<AppState>,
) -> impl IntoResponse {
    match state.storage.get_session(&id) {
        Ok(session) => {
            let messages = state.storage.list_messages(&id).unwrap_or_default();
            let artifacts = state.storage.list_artifacts(&id).unwrap_or_default();
            (StatusCode::OK, Json(json!({
                "session": session,
                "messages": messages,
                "artifacts": artifacts
            })))
        }
        Err(e) => (StatusCode::NOT_FOUND, Json(json!({ "error": e.to_string() }))),
    }
}

async fn delete_session(
    Path(id): Path<String>,
    State(state): State<AppState>,
) -> impl IntoResponse {
    match state.storage.delete_session(&id) {
        Ok(deleted) => (StatusCode::OK, Json(json!({ "deleted": deleted, "id": id }))),
        Err(e) => (StatusCode::INTERNAL_SERVER_ERROR, Json(json!({ "error": e.to_string() }))),
    }
}

async fn list_messages(
    Path(session_id): Path<String>,
    State(state): State<AppState>,
) -> impl IntoResponse {
    match state.storage.list_messages(&session_id) {
        Ok(messages) => (StatusCode::OK, Json(json!(messages))),
        Err(e) => (StatusCode::INTERNAL_SERVER_ERROR, Json(json!({ "error": e.to_string() }))),
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
    attachments: Vec<AttachmentInput>,
}

async fn send_message(
    Path(session_id): Path<String>,
    State(state): State<AppState>,
    Json(req): Json<SendMessageRequest>,
) -> impl IntoResponse {
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
        .execute_turn_controlled(
            &session_id,
            &req.text,
            attachments,
            req.model.as_deref(),
            req.effort.as_deref(),
            cancel,
            Some(steer),
        )
        .await;

    {
        let mut turns = state.active_turns.write().await;
        if turns
            .get(&session_id)
            .map(|handle| handle.turn_id == turn_id)
            .unwrap_or(false)
        {
            turns.remove(&session_id);
        }
    }

    match res {
        Ok(msg) => (StatusCode::OK, Json(json!({ "message": msg }))),
        Err(e) => {
            // Keep the transport body clean: the UI renders this directly.
            let message = match e {
                odon_core::OdonError::Provider { message, .. } => message,
                other => other.to_string(),
            };
            (StatusCode::BAD_GATEWAY, Json(json!({ "error": message })))
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

fn mask_secret(secret: &str) -> String {
    if secret.len() > 8 {
        format!(
            "{}...{}",
            &secret[..4],
            &secret[secret.len() - 4..]
        )
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
        Err(error) => (
            StatusCode::BAD_GATEWAY,
            Json(json!({ "error": error })),
        ),
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
    #[allow(dead_code)]
    id: Option<String>,
    #[serde(default)]
    #[allow(dead_code)]
    push_name: Option<String>,
}

async fn whatsapp_bridge_inbound(
    State(state): State<AppState>,
    Json(req): Json<WhatsAppBridgeInbound>,
) -> impl IntoResponse {
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
        if !web.allow_anyone
            && !web
                .allowed_numbers
                .iter()
                .any(|number| number == &digits)
        {
            return (
                StatusCode::FORBIDDEN,
                Json(json!({ "error": "Sender is not in WHATSAPP_ALLOWED_NUMBERS" })),
            );
        }
    }
    let text = req.text.trim().to_string();
    if text.is_empty() {
        return (
            StatusCode::BAD_REQUEST,
            Json(json!({ "error": "Empty message" })),
        );
    }
    let message = IncomingChannelMessage {
        channel: "whatsapp".to_string(),
        sender_id: req.from.clone(),
        session_id: Some(format!("wa_{}", req.from)),
        text,
        attachments: Vec::new(),
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
        Err(e) => (StatusCode::INTERNAL_SERVER_ERROR, Json(json!({ "error": e.to_string() }))),
    }
}

async fn get_artifact(
    Path(id): Path<String>,
    State(state): State<AppState>,
) -> impl IntoResponse {
    match state.storage.get_artifact(&id) {
        Ok(artifact) => (StatusCode::OK, Json(json!(artifact))),
        Err(e) => (StatusCode::NOT_FOUND, Json(json!({ "error": e.to_string() }))),
    }
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
        _ => ApprovalDecision::Rejected {
            reason: req.reason.unwrap_or_else(|| "User rejected action".to_string()),
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
            "supervised" => ExecutionMode::Supervised,
            "locked" => ExecutionMode::Locked,
            _ => ExecutionMode::Autonomous,
        };
        state.approval.update_mode(exec_mode).await;
    }
    let policy = state.approval.get_policy().await;
    Json(json!({ "execution_mode": format!("{:?}", policy.mode) }))
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
        | Ok(odon_security::ApprovalDecision::ApprovedAlways) => {}
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

    #[cfg(target_os = "windows")]
    let mut cmd = {
        let script = format!(
            "[Console]::OutputEncoding = [System.Text.UTF8Encoding]::new(); [Console]::InputEncoding = [System.Text.UTF8Encoding]::new(); Set-Location -LiteralPath '{}'; try {{ & {{ {} }} }} catch {{ Write-Error $_ }}; $code = if ($LASTEXITCODE -ne $null -and $LASTEXITCODE -ne 0) {{ $LASTEXITCODE }} elseif ($?) {{ 0 }} else {{ 1 }}; Write-Host \"`n__ODON_CWD__:$($ExecutionContext.SessionState.Path.CurrentLocation.Path)\"; exit $code",
            final_dir.replace('\'', "''"),
            command_str
        );
        let mut c = tokio::process::Command::new("powershell.exe");
        c.args(["-NoProfile", "-NonInteractive", "-ExecutionPolicy", "Bypass", "-Command", &script]);
        c
    };

    #[cfg(not(target_os = "windows"))]
    let mut cmd = {
        let script = format!(
            "cd '{}' && {} && printf \"\\n__ODON_CWD__:%s\\n\" \"$(pwd)\"",
            final_dir.replace('\'', "'\\''"),
            command_str
        );
        let mut c = tokio::process::Command::new("sh");
        c.args(["-c", &script]);
        c
    };

    cmd.current_dir(&final_dir);
    cmd.stdout(Stdio::piped());
    cmd.stderr(Stdio::piped());

    match cmd.output().await {
        Ok(output) => {
            let full_stdout = String::from_utf8_lossy(&output.stdout).to_string();
            let stderr = String::from_utf8_lossy(&output.stderr).to_string();
            let exit_code = output.status.code().unwrap_or(if output.status.success() { 0 } else { 1 });

            let mut updated_cwd = final_dir;
            let mut cleaned_stdout = full_stdout;
            if let Some(pos) = cleaned_stdout.rfind("__ODON_CWD__:") {
                let cwd_part = cleaned_stdout[pos + 13..].trim();
                if let Some(first_line) = cwd_part.lines().next() {
                    let extracted = first_line.trim().to_string();
                    if !extracted.is_empty() && std::path::Path::new(&extracted).exists() {
                        updated_cwd = extracted;
                    }
                }
                cleaned_stdout.truncate(pos);
                if cleaned_stdout.ends_with("\r\n") {
                    cleaned_stdout.truncate(cleaned_stdout.len() - 2);
                } else if cleaned_stdout.ends_with('\n') {
                    cleaned_stdout.truncate(cleaned_stdout.len() - 1);
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
        Err(e) => (
            StatusCode::OK,
            Json(json!({
                "exit_code": -1,
                "stdout": "",
                "stderr": format!("Process execution failed: {}", e),
                "cwd": final_dir,
                "success": false,
            })),
        ),
    }
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
        runtime.register_provider("github-copilot", prov.clone(), true).await;
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
        runtime.register_provider("antigravity", prov.clone(), !has_default).await;
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
            runtime.register_provider(&catalog_id, prov.clone(), !has_default).await;
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
                Arc::new(OpenAiCompatibleProvider::new(&base_url, &c.api_key, Some(&pid)))
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
            let base_url = std::env::var("OPENAI_BASE_URL").unwrap_or_else(|_| "https://api.openai.com/v1".to_string());
            let prov = Arc::new(OpenAiCompatibleProvider::new(base_url, key, Some("openai")));
            runtime.register_provider("openai", prov.clone(), !has_default).await;
            if !has_default {
                runtime.register_provider("default", prov, true).await;
                has_default = true;
            }
        }
    }

    if let Ok(key) = std::env::var("ANTHROPIC_API_KEY") {
        if !key.trim().is_empty() {
            let prov = Arc::new(AnthropicProvider::new(key));
            runtime.register_provider("anthropic", prov.clone(), !has_default).await;
            if !has_default {
                runtime.register_provider("default", prov, true).await;
                has_default = true;
            }
        }
    }

    if let Ok(key) = std::env::var("DEEPSEEK_API_KEY") {
        if !key.trim().is_empty() {
            let prov = Arc::new(OpenAiCompatibleProvider::new_deepseek(key));
            runtime.register_provider("deepseek", prov.clone(), !has_default).await;
            if !has_default {
                runtime.register_provider("default", prov, true).await;
                has_default = true;
            }
        }
    }

    // Only register simulated under its explicit key for offline testing, NEVER as default
    runtime.register_provider("odon-simulated", Arc::new(SimulatedProvider::default()), false).await;
}

#[derive(Deserialize)]
struct SaveProviderConfigRequest {
    provider_id: String,
    api_key: String,
    base_url: Option<String>,
    is_default: Option<bool>,
}

async fn get_provider_configs(State(state): State<AppState>) -> impl IntoResponse {
    let configs = state.storage.list_provider_configs().unwrap_or_default();
    let mut result = Vec::new();
    for c in configs {
        let masked = if c.api_key.len() > 8 {
            format!("{}...{}", &c.api_key[..4], &c.api_key[c.api_key.len() - 4..])
        } else {
            "********".to_string()
        };
        result.push(json!({
            "provider_id": c.provider_id,
            "configured": true,
            "masked_key": masked,
            "base_url": c.base_url,
            "is_default": c.is_default,
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
    let base_url = req.base_url.clone().unwrap_or_else(|| default_base_url_for(&pid));

    // Leaving the key field blank in the UI means "keep the stored key"; it
    // must never wipe or replace a configured credential with an empty string.
    let api_key = if req.api_key.trim().is_empty() {
        state
            .storage
            .get_provider_config(&pid)
            .ok()
            .flatten()
            .map(|c| c.api_key)
            .unwrap_or_default()
    } else {
        req.api_key.clone()
    };

    if let Err(e) = state.storage.save_provider_config(&pid, &api_key, Some(&base_url), is_def) {
        return (StatusCode::INTERNAL_SERVER_ERROR, Json(json!({ "error": e.to_string() })));
    }

    let prov: Arc<dyn odon_provider::ModelProvider> = if pid == "anthropic" {
        Arc::new(AnthropicProvider::with_base_url(&api_key, &base_url))
    } else {
        Arc::new(OpenAiCompatibleProvider::new(&base_url, &api_key, Some(&pid)))
    };

    state.runtime.register_provider(&pid, prov.clone(), is_def).await;
    if is_def {
        state.runtime.register_provider("default", prov, true).await;
    }

    (StatusCode::OK, Json(json!({ "status": "saved", "provider": pid, "is_default": is_def })))
}

async fn delete_provider_config(
    Path(provider_id): Path<String>,
    State(state): State<AppState>,
) -> impl IntoResponse {
    let _ = state.storage.delete_provider_config(&provider_id);
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
            return (StatusCode::INTERNAL_SERVER_ERROR, Json(json!({ "error": e.to_string() })));
        }
    };
    let pending = match service.begin_login(provider).await {
        Ok(p) => p,
        Err(e) => return (StatusCode::BAD_GATEWAY, Json(json!({ "error": e.to_string() }))),
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
        LoginResponse::Browser { authorization_url, redirect_uri, .. } => json!({
            "status": "pending_browser",
            "auth_url": authorization_url,
            "redirect_uri": redirect_uri,
        }),
        LoginResponse::Device { user_code, verification_uri, expires_in, interval, .. } => json!({
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
        Muse => "muse",
    }
}

async fn register_oauth_provider(state: &AppState, catalog_id: &'static str, token: odon_provider::OAuthToken) {
    // No runtime adapter for Anthropic's messages API yet: the account is
    // recorded but is reported as not usable instead of silently falling back.
    let registered = build_oauth_provider(catalog_id, &token);
    let usable = registered.is_some();
    if let Some(p) = registered {
        state.runtime.register_provider(catalog_id, p, false).await;
        // Persist the login so it survives a daemon restart.
        save_oauth_token(catalog_id, &token);
    }

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
    ws: WebSocketUpgrade,
    axum::extract::Query(query): axum::extract::Query<TerminalWsQuery>,
    State(state): State<AppState>,
) -> impl IntoResponse {
    let workspace = state.workspace_root.clone();
    ws.on_upgrade(move |socket| handle_terminal_socket(socket, workspace, query))
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
            let _ = ws_tx.send(WsMessage::Text(format!("Failed to open native PTY: {}\r\n", e).into())).await;
            return;
        }
    };

    let cmd = if let Some(pid) = &query.ssh_profile_id {
        let profiles = load_ssh_profiles();
        let profile = profiles.into_iter().find(|p| &p.id == pid || &p.alias == pid);
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
                let _ = ws_tx.send(WsMessage::Text(format!("Invalid SSH target: {e}\r\n").into())).await;
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
                        let _ = ws_tx.send(WsMessage::Text(format!("SSH key error: {e}\r\n").into())).await;
                        return;
                    }
                }
            }
            c.arg(format!("{}@{}", p.user, p.host));
            c
        } else {
            let (mut ws_tx, _) = socket.split();
            let _ = ws_tx.send(WsMessage::Text(format!("SSH Profile not found: {}\r\n", pid).into())).await;
            return;
        }
    } else {
        let working_dir = query.cwd.as_deref().unwrap_or(&workspace);
        #[cfg(target_os = "windows")]
        let c = {
            let shell_type = query.shell.as_deref().unwrap_or("powershell");
            let mut cb = if shell_type.eq_ignore_ascii_case("cmd") {
                CommandBuilder::new("cmd.exe")
            } else if shell_type.eq_ignore_ascii_case("wsl") || shell_type.eq_ignore_ascii_case("bash") {
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

    let mut child = match pair.slave.spawn_command(cmd) {
        Ok(ch) => ch,
        Err(e) => {
            let (mut ws_tx, _) = socket.split();
            let _ = ws_tx.send(WsMessage::Text(format!("Failed to spawn process in PTY: {}\r\n", e).into())).await;
            return;
        }
    };

    let mut reader = match pair.master.try_clone_reader() {
        Ok(r) => r,
        Err(e) => {
            let (mut ws_tx, _) = socket.split();
            let _ = ws_tx.send(WsMessage::Text(format!("Failed to clone PTY reader: {}\r\n", e).into())).await;
            return;
        }
    };

    let mut writer = match pair.master.take_writer() {
        Ok(w) => w,
        Err(e) => {
            let (mut ws_tx, _) = socket.split();
            let _ = ws_tx.send(WsMessage::Text(format!("Failed to get PTY writer: {}\r\n", e).into())).await;
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
                                let _ = m.resize(PtySize { rows: r, cols: c, pixel_width: 0, pixel_height: 0 });
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

    let _ = child.kill();
    ws_send_task.abort();
}

async fn get_models(State(state): State<AppState>) -> impl IntoResponse {
    let connected = state.oauth_accounts.read().await;
    let is_connected = |provider: &str| connected.contains_key(provider);
    let byok_configs = state.storage.list_provider_configs().unwrap_or_default();
    let is_byok = |pid: &str| -> bool {
        byok_configs.iter().any(|c| c.provider_id.eq_ignore_ascii_case(pid)) ||
        match pid {
            "openai" => std::env::var("OPENAI_API_KEY").map(|v| !v.trim().is_empty()).unwrap_or(false),
            "anthropic" => std::env::var("ANTHROPIC_API_KEY").map(|v| !v.trim().is_empty()).unwrap_or(false),
            "deepseek" => std::env::var("DEEPSEEK_API_KEY").map(|v| !v.trim().is_empty()).unwrap_or(false),
            "google" => std::env::var("GEMINI_API_KEY").or_else(|_| std::env::var("GOOGLE_API_KEY")).map(|v| !v.trim().is_empty()).unwrap_or(false),
            "mistral" => std::env::var("MISTRAL_API_KEY").map(|v| !v.trim().is_empty()).unwrap_or(false),
            "groq" => std::env::var("GROQ_API_KEY").map(|v| !v.trim().is_empty()).unwrap_or(false),
            "openrouter" => std::env::var("OPENROUTER_API_KEY").map(|v| !v.trim().is_empty()).unwrap_or(false),
            "qwen" => std::env::var("DASHSCOPE_API_KEY").or_else(|_| std::env::var("QWEN_API_KEY")).map(|v| !v.trim().is_empty()).unwrap_or(false),
            "ollama" => std::env::var("OLLAMA_BASE_URL").is_ok(),
            "openai-compatible" | "openai_compatible" => std::env::var("OPENAI_BASE_URL").is_ok(),
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

    Json(json!({
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
                "id": "muse", "name": "Meta Muse Code", "category": "oauth", "auth_type": "oauth",
                "connected": is_connected("muse"),
                "description": "Meta Muse Code developer device authorization and SSE streaming",
                "models": [
                    { "id": "muse-spark-1.3", "name": "Muse Spark 1.3", "context": 1000000, "recommended": true }
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
    }))
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
        ).into_response();
    }
    // Serve only from an allowed workspace, and verify the canonical target so
    // a symlink inside the workspace cannot escape it.
    let mut served: Option<PathBuf> = None;
    for root in workspace_roots(&state).await {
        let candidate = root.join(&clean);
        if let (Ok(canonical_root), Ok(canonical)) =
            (std::fs::canonicalize(&root), std::fs::canonicalize(&candidate))
        {
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
            ).into_response();
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
            (
                StatusCode::OK,
                [
                    (header::CONTENT_TYPE, mime),
                    (header::CACHE_CONTROL, "public, max-age=3600"),
                ],
                bytes,
            ).into_response()
        }
        Err(_) => (
            StatusCode::NOT_FOUND,
            [(header::CONTENT_TYPE, "text/plain")],
            "File not found".as_bytes().to_vec(),
        ).into_response(),
    }
}

async fn sse_events(
    State(state): State<AppState>,
) -> Sse<impl Stream<Item = std::result::Result<Event, Infallible>>> {
    let mut rx = state.event_tx.subscribe();
    let stream = async_stream::stream! {
        loop {
            match rx.recv().await {
                Ok(event) => {
                    if let Ok(data) = serde_json::to_string(&event) {
                        yield Ok(Event::default().data(data));
                    }
                }
                // A slow client missed events: keep streaming instead of dropping it.
                Err(broadcast::error::RecvError::Lagged(_)) => continue,
                Err(broadcast::error::RecvError::Closed) => break,
            }
        }
    };

    Sse::new(stream).keep_alive(KeepAlive::default())
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

async fn get_mcp_servers() -> impl IntoResponse {
    let servers = odon_tools::mcp::load_mcp_servers();
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
    Json(json!({ "success": true, "servers": servers }))
}

async fn delete_mcp_server(
    Path(id): Path<String>,
) -> impl IntoResponse {
    let mut servers = odon_tools::mcp::load_mcp_servers();
    servers.retain(|s| s.id != id);
    let _ = odon_tools::mcp::save_mcp_servers(&servers);
    Json(json!({ "success": true, "deleted": id }))
}

async fn toggle_mcp_server(
    Path(id): Path<String>,
) -> impl IntoResponse {
    let mut servers = odon_tools::mcp::load_mcp_servers();
    if let Some(s) = servers.iter_mut().find(|s| s.id == id) {
        s.enabled = !s.enabled;
    }
    let _ = odon_tools::mcp::save_mcp_servers(&servers);
    Json(json!({ "success": true, "servers": servers }))
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

async fn get_github_status(
    State(state): State<AppState>,
) -> impl IntoResponse {
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
        .args(["repo", "list", "--limit", "40", "--json", "name,owner,isPrivate,description"])
        .output()
        .await
    {
        if output.status.success() {
            if let Ok(repos) = serde_json::from_slice::<Value>(&output.stdout) {
                if let Some(arr) = repos.as_array() {
                    let mut list = Vec::new();
                    for item in arr {
                        let name = item.get("name").and_then(|n| n.as_str()).unwrap_or("");
                        let owner = item.get("owner").and_then(|o| o.get("login")).and_then(|l| l.as_str()).unwrap_or("");
                        let desc = item.get("description").and_then(|d| d.as_str()).unwrap_or("");
                        let is_priv = item.get("isPrivate").and_then(|p| p.as_bool()).unwrap_or(false);
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
                            let owner = item.get("owner").and_then(|o| o.get("login")).and_then(|l| l.as_str()).unwrap_or("");
                            let desc = item.get("description").and_then(|d| d.as_str()).unwrap_or("");
                            let is_priv = item.get("private").and_then(|p| p.as_bool()).unwrap_or(false);
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
        Err(e) => (StatusCode::INTERNAL_SERVER_ERROR, Json(json!({ "error": e.to_string() }))),
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
        Err(e) => (StatusCode::INTERNAL_SERVER_ERROR, Json(json!({ "error": e.to_string() }))),
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

    let owner = req.owner.filter(|o| !o.trim().is_empty()).unwrap_or(ws_owner);
    let repo = req.repo.filter(|r| !r.trim().is_empty()).unwrap_or(ws_repo);

    if owner.is_empty() || repo.is_empty() {
        return (StatusCode::BAD_REQUEST, Json(json!({ "error": "Owner and repo are required" })));
    }

    let client = reqwest::Client::new();
    let url = format!("https://api.github.com/repos/{owner}/{repo}/issues");
    let mut r = client.post(&url).header("User-Agent", "odon-desktop");
    if let Some(token) = get_github_auth_token().await {
        r = r.header("Authorization", format!("Bearer {}", token));
    }
    match r.json(&json!({ "title": req.title, "body": req.body.unwrap_or_default() })).send().await {
        Ok(resp) => {
            let data: Value = resp.json().await.unwrap_or(json!({ "status": "created" }));
            (StatusCode::OK, Json(data))
        }
        Err(e) => (StatusCode::INTERNAL_SERVER_ERROR, Json(json!({ "error": e.to_string() }))),
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
        let session_id = format!("auto_{}_{}", job.id, chrono::Utc::now().timestamp());
        let rt = state.runtime.clone();
        let logs = state.automation_logs.clone();
        let job_name = job.name.clone();
        let prompt = job.prompt.clone();
        let model = job.model.clone();

        let run_session_id = session_id.clone();
        tokio::spawn(async move {
            let start = chrono::Utc::now().to_rfc3339();
            let cancel = tokio_util::sync::CancellationToken::new();
            let result = rt.execute_turn(&run_session_id, &prompt, Some(&model), cancel).await;
            let end = chrono::Utc::now().to_rfc3339();
            let (status, output) = match result {
                Ok(turn) => ("success".to_string(), turn.content.text),
                Err(e) => ("failed".to_string(), e.to_string()),
            };

            let mut log_list = logs.write().await;
            log_list.insert(0, AutomationLog {
                id: uuid::Uuid::new_v4().to_string(),
                automation_id: id,
                automation_name: job_name,
                triggered_at: start,
                completed_at: Some(end),
                status,
                output,
            });
            if log_list.len() > 100 {
                log_list.truncate(100);
            }
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
                let name = p.file_name().unwrap_or_default().to_string_lossy().to_string();
                let size = if is_dir { 0 } else { entry.metadata().map(|m| m.len()).unwrap_or(0) };
                let ext = p.extension().map(|e| e.to_string_lossy().to_string());
                let modified = entry.metadata().ok().and_then(|m| m.modified().ok()).map(|t| {
                    chrono::DateTime::<chrono::Utc>::from(t).to_rfc3339()
                });
                let rel = p.strip_prefix(&state.workspace_root)
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

#[derive(Deserialize)]
struct FsWritePayload {
    path: String,
    content: String,
}

async fn fs_write_file(
    State(state): State<AppState>,
    Json(payload): Json<FsWritePayload>,
) -> impl IntoResponse {
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
        if workspace_roots(&state)
            .await
            .iter()
            .any(|root| std::fs::canonicalize(root).map(|r| r == canonical).unwrap_or(false))
        {
            return Json(json!({ "success": false, "error": "Refusing to delete a workspace root" }));
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
        for item in extra.split(',').map(str::trim).filter(|item| !item.is_empty()) {
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
    let allow_any = std::env::var("ODON_ALLOW_ANY_SSH_KEY")
        .map(|v| matches!(v.trim().to_ascii_lowercase().as_str(), "1" | "true" | "yes"))
        .unwrap_or(false);
    if allow_any {
        return Ok(display_path(&canonical));
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
    let valid_host = !host.is_empty()
        && !host.starts_with('-')
        && host
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '-' | ':' | '_' | '[' | ']'));
    if !valid_host {
        return Err(format!("Invalid SSH host: {host}"));
    }
    let valid_user = !user.is_empty()
        && !user.starts_with('-')
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

async fn import_ssh_certificate(
    Json(payload): Json<SshCertImportPayload>,
) -> impl IntoResponse {
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

async fn save_ssh_profile(
    Json(profile): Json<SshProfile>,
) -> impl IntoResponse {
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

async fn delete_ssh_profile(
    Path(id): Path<String>,
) -> impl IntoResponse {
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
    let profile = req.profile_id.as_ref().and_then(|pid| profiles.iter().find(|p| &p.id == pid || &p.alias == pid));

    let host = req.host.as_deref().or_else(|| profile.map(|p| p.host.as_str())).unwrap_or("localhost");
    let port = req.port.or_else(|| profile.map(|p| p.port)).unwrap_or(22);
    let user = req.user.as_deref().or_else(|| profile.map(|p| p.user.as_str())).unwrap_or("root");
    let key = req.key_path.as_deref().or_else(|| profile.and_then(|p| p.key_path.as_deref()));

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
        | Ok(odon_security::ApprovalDecision::ApprovedAlways) => {}
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

    match cmd.output().await {
        Ok(output) => {
            let stdout = String::from_utf8_lossy(&output.stdout).to_string();
            let stderr = String::from_utf8_lossy(&output.stderr).to_string();
            Json(json!({
                "exit_code": output.status.code().unwrap_or(-1),
                "stdout": stdout,
                "stderr": stderr,
                "success": output.status.success()
            }))
        }
        Err(e) => Json(json!({
            "exit_code": 1,
            "stdout": "",
            "stderr": format!("Failed to execute ssh: {}", e),
            "success": false
        })),
    }
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
            if p.extension().map_or(false, |ext| ext == "md") {
                let name = p.file_stem().unwrap_or_default().to_string_lossy().to_string();
                let content = std::fs::read_to_string(&p).unwrap_or_default();
                let first_line = content.lines().next().unwrap_or("").trim_start_matches('#').trim().to_string();
                let title = if first_line.is_empty() { name.clone() } else { first_line };
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

async fn save_skill_item(
    Json(payload): Json<SaveSkillPayload>,
) -> impl IntoResponse {
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

async fn delete_skill_item(
    Path(name): Path<String>,
) -> impl IntoResponse {
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
    fn skill_slugs_cannot_traverse() {
        assert_eq!(safe_skill_slug("../../etc/passwd"), "etc-passwd");
        assert_eq!(safe_skill_slug("My Skill"), "my-skill");
        assert_eq!(safe_skill_slug(""), "skill");
    }
}
