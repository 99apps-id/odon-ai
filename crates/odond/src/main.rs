use axum::{
    extract::{
        ws::{Message as WsMessage, WebSocket, WebSocketUpgrade},
        Path, Query, State,
    },
    http::{header, Method, StatusCode},
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
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::sync::broadcast;
use tower_http::cors::{Any, CorsLayer};
use tower_http::services::ServeDir;
use tracing::info;

use odon_agent::AgentRuntime;
use odon_channel::{ChannelAdapter, TelegramRelay};
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
    vec![
        AutomationJob {
            id: "daily-standup".to_string(),
            name: "Daily Git & Standup Digest".to_string(),
            description: "Summarize recent commits, pending PRs, and planned tasks every morning".to_string(),
            trigger_type: "cron".to_string(),
            trigger_value: "0 8 * * * (Daily @ 08:00 AM)".to_string(),
            prompt: "Review recent git commits from yesterday, list open branches and summarize what was accomplished.".to_string(),
            model: "claude-sonnet-5-5".to_string(),
            enabled: true,
            last_run: Some("Yesterday at 08:00 AM".to_string()),
            last_status: Some("success".to_string()),
            created_at: "2026-10-15".to_string(),
        },
        AutomationJob {
            id: "repo-health".to_string(),
            name: "Repository Health & Cargo Tests".to_string(),
            description: "Run test suite and check code quality periodically".to_string(),
            trigger_type: "interval".to_string(),
            trigger_value: "Every 4 hours".to_string(),
            prompt: "Check workspace status and verify test suites and linting health.".to_string(),
            model: "gpt-6.1-sol".to_string(),
            enabled: true,
            last_run: Some("2 hours ago".to_string()),
            last_status: Some("success".to_string()),
            created_at: "2026-10-20".to_string(),
        },
        AutomationJob {
            id: "security-audit".to_string(),
            name: "Automated Security & Secret Audit".to_string(),
            description: "Scan code for accidentally committed secrets, dangerous bash calls, and CVEs".to_string(),
            trigger_type: "cron".to_string(),
            trigger_value: "0 0 * * * (Midnight)".to_string(),
            prompt: "Perform a security scan on modified files to ensure no API keys or credentials leaked.".to_string(),
            model: "gemini-4-argon".to_string(),
            enabled: false,
            last_run: None,
            last_status: None,
            created_at: "2026-10-25".to_string(),
        },
    ]
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

            // 8. Telegram Relay
            let tg_token = telegram_token.or_else(|| std::env::var("TELEGRAM_BOT_TOKEN").ok());
            if let Some(token) = tg_token {
                println!("Connecting Telegram Bot Relay...");
                let tg = TelegramRelay::new(Some(token));
                let (in_tx, mut in_rx) = tokio::sync::mpsc::channel(64);
                let _ = tg.start(in_tx).await;

                let rt_clone = runtime.clone();

        tokio::spawn(async move {
                    while let Some(msg) = in_rx.recv().await {
                        println!("Received from Telegram [{}]: {}", msg.sender_id, msg.text);
                        let session_id = msg.session_id.unwrap_or_else(|| format!("tg_{}", msg.sender_id));
                        let cancel = tokio_util::sync::CancellationToken::new();
                        let _ = rt_clone.execute_turn(&session_id, &msg.text, None, cancel).await;
                    }
                });
            }

            let automations = Arc::new(tokio::sync::RwLock::new(load_automations_from_disk()));
            let automation_logs = Arc::new(tokio::sync::RwLock::new(Vec::new()));

            // 9. Build Axum App Router
            let state = AppState {
                runtime,
                storage,
                approval,
                audit,
                tools,
                event_tx,
                workspace_root: workspace,
                oauth_accounts,
                automations,
                automation_logs,
            };

            let cors = CorsLayer::new()
                .allow_origin(Any)
                .allow_methods([Method::GET, Method::POST, Method::DELETE, Method::OPTIONS])
                .allow_headers([header::CONTENT_TYPE, header::AUTHORIZATION, header::ACCEPT]);

            let mut app = Router::new()
                .route("/api/status", get(get_status))
                .route("/api/sessions", get(list_sessions).post(create_session))
                .route("/api/sessions/{id}", get(get_session).delete(delete_session))
                .route("/api/sessions/{id}/messages", get(list_messages).post(send_message))
                .route("/api/sessions/{id}/artifacts", get(list_session_artifacts))
                .route("/api/artifacts/{id}", get(get_artifact))
                .route("/api/tools", get(list_tools))
                .route("/api/approvals", get(list_approvals))
                .route("/api/approvals/{id}", post(resolve_approval))
                .route("/api/approvals/history", get(get_approval_history))
                .route("/api/approvals/policies", get(get_approval_policies))
                .route("/api/approvals/policies/{*key}", axum::routing::delete(delete_approval_policy))
                .route("/api/mcp/servers", get(get_mcp_servers).post(save_mcp_server))
                .route("/api/mcp/servers/{id}", axum::routing::delete(delete_mcp_server))
                .route("/api/mcp/servers/{id}/toggle", post(toggle_mcp_server))
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
                .route("/api/oauth/accounts", get(get_oauth_accounts))
                .route("/api/oauth/login", post(login_oauth))
                .route("/api/oauth/disconnect", post(disconnect_oauth))
                .route("/api/terminal/info", get(get_terminal_info))
                .route("/api/terminal/exec", post(execute_terminal))
                .route("/api/terminal/ws", get(ws_terminal_handler))
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
                // Skills APIs
                .route("/api/skills", get(get_skills_list).post(save_skill_item))
                .route("/api/skills/{name}", axum::routing::delete(delete_skill_item));

            let dist_path = std::path::Path::new("desktop/dist");
            if dist_path.exists() {
                info!("Serving static desktop web application from desktop/dist");
                app = app.fallback_service(ServeDir::new(dist_path));
            }

            let app = app.layer(cors).with_state(state);

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
            "telegram": std::env::var("TELEGRAM_BOT_TOKEN").is_ok(),
            "whatsapp": false
        }
    }))
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

    let session = Session::new(title, mode, model);
    match state.storage.save_session(&session) {
        Ok(_) => (StatusCode::CREATED, Json(json!(session))),
        Err(e) => (StatusCode::INTERNAL_SERVER_ERROR, Json(json!({ "error": e.to_string() }))),
    }
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
struct SendMessageRequest {
    text: String,
    model: Option<String>,
    effort: Option<String>,
}

async fn send_message(
    Path(session_id): Path<String>,
    State(state): State<AppState>,
    Json(req): Json<SendMessageRequest>,
) -> impl IntoResponse {
    let cancel = tokio_util::sync::CancellationToken::new();
    let res = state
        .runtime
        .execute_turn_with_effort(
            &session_id,
            &req.text,
            req.model.as_deref(),
            req.effort.as_deref(),
            cancel,
        )
        .await;

    match res {
        Ok(msg) => (StatusCode::OK, Json(json!({ "message": msg }))),
        Err(e) => (StatusCode::INTERNAL_SERVER_ERROR, Json(json!({ "error": e.to_string() }))),
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

    if let Err(e) = state.storage.save_provider_config(&pid, &req.api_key, Some(&base_url), is_def) {
        return (StatusCode::INTERNAL_SERVER_ERROR, Json(json!({ "error": e.to_string() })));
    }

    let prov: Arc<dyn odon_provider::ModelProvider> = if pid == "anthropic" {
        Arc::new(AnthropicProvider::with_base_url(&req.api_key, &base_url))
    } else {
        Arc::new(OpenAiCompatibleProvider::new(&base_url, &req.api_key, Some(&pid)))
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
    use odon_provider::{MuseProvider, OpenAiCodexProvider, OpenAiCompatibleProvider};

    let registered: Option<Arc<dyn odon_provider::ModelProvider>> = match catalog_id {
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
            "https://api.githubcopilot.com".to_string(),
            token.access_token.clone(),
            Some("github-copilot"),
        ))),
        "xai-oauth" => Some(Arc::new(OpenAiCompatibleProvider::new(
            "https://api.x.ai/v1".to_string(),
            token.access_token.clone(),
            Some("xai-oauth"),
        ))),
        // No runtime adapter for Anthropic's messages API yet: the account is
        // recorded but is reported as not usable instead of silently falling back.
        _ => None,
    };
    let usable = registered.is_some();
    if let Some(p) = registered {
        state.runtime.register_provider(catalog_id, p, false).await;
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
    let mut accounts = state.oauth_accounts.write().await;
    accounts.remove(&req.provider.to_lowercase());
    StatusCode::OK
}

async fn ws_terminal_handler(
    ws: WebSocketUpgrade,
    State(state): State<AppState>,
) -> impl IntoResponse {
    let workspace = state.workspace_root.clone();
    ws.on_upgrade(move |socket| handle_terminal_socket(socket, workspace))
}

async fn handle_terminal_socket(socket: WebSocket, workspace: String) {
    #[cfg(target_os = "windows")]
    let mut cmd = tokio::process::Command::new("powershell.exe");
    #[cfg(target_os = "windows")]
    cmd.args(["-NoLogo", "-NoExit", "-Command", "-"]);

    #[cfg(not(target_os = "windows"))]
    let mut cmd = tokio::process::Command::new("sh");

    cmd.current_dir(&workspace);
    cmd.stdin(Stdio::piped());
    cmd.stdout(Stdio::piped());
    cmd.stderr(Stdio::piped());

    let mut child = match cmd.spawn() {
        Ok(c) => c,
        Err(e) => {
            let (mut ws_tx, _) = socket.split();
            let _ = ws_tx
                .send(WsMessage::Text(format!("Failed to spawn shell: {}\r\n", e).into()))
                .await;
            return;
        }
    };

    let mut stdin = match child.stdin.take() {
        Some(s) => s,
        None => return,
    };
    let mut stdout = match child.stdout.take() {
        Some(s) => s,
        None => return,
    };
    let mut stderr = match child.stderr.take() {
        Some(s) => s,
        None => return,
    };

    let (mut ws_tx, mut ws_rx) = socket.split();
    let (tx, mut rx) = tokio::sync::mpsc::channel::<String>(100);

    let ws_send_task = tokio::spawn(async move {
        while let Some(msg) = rx.recv().await {
            if ws_tx.send(WsMessage::Text(msg.into())).await.is_err() {
                break;
            }
        }
    });

    let tx_stdout = tx.clone();
    let stdout_task = tokio::spawn(async move {
        let mut buf = [0u8; 1024];
        loop {
            match stdout.read(&mut buf).await {
                Ok(0) => break,
                Ok(n) => {
                    let s = String::from_utf8_lossy(&buf[..n]).to_string();
                    if tx_stdout.send(s).await.is_err() {
                        break;
                    }
                }
                Err(_) => break,
            }
        }
    });

    let tx_stderr = tx.clone();
    let stderr_task = tokio::spawn(async move {
        let mut buf = [0u8; 1024];
        loop {
            match stderr.read(&mut buf).await {
                Ok(0) => break,
                Ok(n) => {
                    let s = String::from_utf8_lossy(&buf[..n]).to_string();
                    if tx_stderr.send(s).await.is_err() {
                        break;
                    }
                }
                Err(_) => break,
            }
        }
    });

    while let Some(Ok(msg)) = ws_rx.next().await {
        match msg {
            WsMessage::Text(text) => {
                let mut data = text.as_bytes().to_vec();
                if !data.ends_with(b"\n") {
                    data.push(b'\n');
                }
                if stdin.write_all(&data).await.is_err() {
                    break;
                }
            }
            WsMessage::Binary(bin) => {
                if stdin.write_all(&bin).await.is_err() {
                    break;
                }
            }
            WsMessage::Close(_) => break,
            _ => {}
        }
    }

    let _ = child.kill().await;
    stdout_task.abort();
    stderr_task.abort();
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
            "ollama" => true,
            "openai-compatible" | "openai_compatible" => std::env::var("OPENAI_BASE_URL").is_ok(),
            _ => false,
        }
    };

    Json(json!({
        "catalog_as_of": "2026-10-31",
        "providers": [
            {
                "id": "github-copilot", "name": "GitHub Copilot", "category": "oauth", "auth_type": "oauth",
                "connected": is_connected("github-copilot"),
                "description": "Active GitHub Copilot session with frontier GPT-6 Astra and Claude 5.5 models",
                "models": [
                    { "id": "gpt-6-astra", "name": "GPT-6 Astra (GitHub Copilot)", "context": 1050000, "recommended": true },
                    { "id": "claude-sonnet-5-5", "name": "Claude Sonnet 5.5 (GitHub Copilot)", "context": 1000000, "recommended": true },
                    { "id": "gpt-6.1-sol", "name": "GPT-6.1 Sol (GitHub Copilot)", "context": 1050000, "recommended": false },
                    { "id": "claude-haiku-5-5", "name": "Claude Haiku 5.5 (GitHub Copilot)", "context": 1000000, "recommended": false }
                ]
            },
            {
                "id": "antigravity", "name": "Google Antigravity", "category": "oauth", "auth_type": "oauth",
                "connected": is_connected("antigravity"),
                "description": "Google Cloud Code Assist & Antigravity OAuth with Gemini 4 and Claude 5.5 reasoning",
                "models": [
                    { "id": "gemini-4-argon", "name": "Gemini 4 Argon (Flagship)", "context": 1000000, "recommended": true },
                    { "id": "gemini-3.8-flash", "name": "Gemini 3.8 Flash (Agentic & Coding)", "context": 1000000, "recommended": true },
                    { "id": "gemini-3.7-flash", "name": "Gemini 3.7 Flash", "context": 1000000, "recommended": false },
                    { "id": "claude-sonnet-5-5", "name": "Claude Sonnet 5.5 (Antigravity)", "context": 1000000, "recommended": true },
                    { "id": "gemini-3.8-live", "name": "Gemini 3.8 Live (Real-Time)", "context": 128000, "recommended": false }
                ]
            },
            {
                "id": "chatgpt", "name": "ChatGPT / OpenAI Codex", "category": "oauth", "auth_type": "oauth",
                "connected": is_connected("chatgpt"),
                "description": "Official OpenAI OAuth authorization with GPT-6 frontier reasoning models",
                "models": [
                    { "id": "gpt-6-astra", "name": "GPT-6 Astra (Flagship Frontier)", "context": 1050000, "recommended": true },
                    { "id": "gpt-6.1-sol", "name": "GPT-6.1 Sol (Production Reasoning)", "context": 1050000, "recommended": true },
                    { "id": "gpt-6.1-terra", "name": "GPT-6.1 Terra (Visual & Agentic)", "context": 1050000, "recommended": false },
                    { "id": "gpt-6-luna", "name": "GPT-6 Luna (Fast & High Throughput)", "context": 1050000, "recommended": false }
                ]
            },
            {
                "id": "claude-oauth", "name": "Claude (Anthropic OAuth)", "category": "oauth", "auth_type": "oauth",
                "connected": is_connected("claude-oauth"),
                "description": "Direct Anthropic Claude account authorization with Claude 5.5 architecture",
                "models": [
                    { "id": "claude-sonnet-5-5", "name": "Claude Sonnet 5.5 (Flagship Balance)", "context": 1000000, "recommended": true },
                    { "id": "claude-opus-5-5", "name": "Claude Opus 5.5 (Deep Agentic Coding)", "context": 1000000, "recommended": true },
                    { "id": "claude-haiku-5-5", "name": "Claude Haiku 5.5 (Low Latency)", "context": 1000000, "recommended": false },
                    { "id": "claude-fable-5-1", "name": "Claude Fable 5.1 (Complex Reasoning)", "context": 1000000, "recommended": false }
                ]
            },
            {
                "id": "xai-oauth", "name": "xAI Grok", "category": "oauth", "auth_type": "oauth",
                "connected": is_connected("xai-oauth"),
                "description": "xAI Grok device authorization with Grok 4.7 frontier reasoning",
                "models": [
                    { "id": "grok-4.7", "name": "Grok 4.7 (Flagship Frontier)", "context": 500000, "recommended": true },
                    { "id": "grok-4.6", "name": "Grok 4.6 (High Context)", "context": 500000, "recommended": false },
                    { "id": "grok-4.20-reasoning", "name": "Grok 4.20 Reasoning (Deep Math & Logic)", "context": 256000, "recommended": true }
                ]
            },
            {
                "id": "muse", "name": "Meta Muse Code", "category": "oauth", "auth_type": "oauth",
                "connected": is_connected("muse"),
                "description": "Meta Muse Code developer device authorization and SSE streaming",
                "models": [
                    { "id": "muse-spark-2.0", "name": "Muse Spark 2.0 (Flagship Coding)", "context": 256000, "recommended": true },
                    { "id": "muse-code-v3", "name": "Muse Code v3", "context": 128000, "recommended": false }
                ]
            },
            {
                "id": "openai", "name": "OpenAI (Official BYOK)", "category": "byok", "auth_type": "api_key",
                "configured": is_byok("openai"),
                "default_base_url": "https://api.openai.com/v1",
                "description": "Official OpenAI platform with GPT-6 Astra, Sol, Terra, and Luna models",
                "models": [
                    { "id": "gpt-6-astra", "name": "GPT-6 Astra (Flagship Frontier)", "context": 1050000, "recommended": true },
                    { "id": "gpt-6.1-sol", "name": "GPT-6.1 Sol (Production Reasoning)", "context": 1050000, "recommended": true },
                    { "id": "gpt-6.1-terra", "name": "GPT-6.1 Terra (Visual & Agentic)", "context": 1050000, "recommended": false },
                    { "id": "gpt-6-luna", "name": "GPT-6 Luna (Fast & High Throughput)", "context": 1050000, "recommended": false }
                ]
            },
            {
                "id": "anthropic", "name": "Anthropic (Official BYOK)", "category": "byok", "auth_type": "api_key",
                "configured": is_byok("anthropic"),
                "default_base_url": "https://api.anthropic.com/v1",
                "description": "Official Anthropic API with Claude 5.5 adaptive thinking models",
                "models": [
                    { "id": "claude-sonnet-5-5", "name": "Claude Sonnet 5.5 (Flagship Balance)", "context": 1000000, "recommended": true },
                    { "id": "claude-opus-5-5", "name": "Claude Opus 5.5 (Deep Agentic Coding)", "context": 1000000, "recommended": true },
                    { "id": "claude-haiku-5-5", "name": "Claude Haiku 5.5 (Low Latency)", "context": 1000000, "recommended": false },
                    { "id": "claude-fable-5-1", "name": "Claude Fable 5.1 (Complex Reasoning)", "context": 1000000, "recommended": false }
                ]
            },
            {
                "id": "google", "name": "Google Gemini (Official BYOK)", "category": "byok", "auth_type": "api_key",
                "configured": is_byok("google"),
                "default_base_url": "https://generativelanguage.googleapis.com/v1beta/openai",
                "description": "Google Gemini API with 1M context and native multimodal reasoning",
                "models": [
                    { "id": "gemini-4-argon", "name": "Gemini 4 Argon (Flagship)", "context": 1000000, "recommended": true },
                    { "id": "gemini-3.8-flash", "name": "Gemini 3.8 Flash (Agentic & Coding)", "context": 1000000, "recommended": true },
                    { "id": "gemini-3.7-flash", "name": "Gemini 3.7 Flash", "context": 1000000, "recommended": false },
                    { "id": "gemini-3.8-live", "name": "Gemini 3.8 Live (Real-Time)", "context": 128000, "recommended": false }
                ]
            },
            {
                "id": "deepseek", "name": "DeepSeek (BYOK)", "category": "byok", "auth_type": "api_key",
                "configured": is_byok("deepseek"),
                "default_base_url": "https://api.deepseek.com/v1",
                "description": "DeepSeek V4.1 Flash and Pro reasoning models with 1M context",
                "models": [
                    { "id": "deepseek-flash", "name": "DeepSeek-V4.1-Flash (General & Agent)", "context": 1000000, "recommended": true },
                    { "id": "deepseek-v4-pro", "name": "DeepSeek-V4-Pro (Deep Reasoning & Coding)", "context": 1000000, "recommended": true }
                ]
            },
            {
                "id": "qwen", "name": "Qwen Plan / DashScope Global", "category": "byok", "auth_type": "api_key",
                "configured": is_byok("qwen"),
                "default_base_url": "https://dashscope-intl.aliyuncs.com/compatible-mode/v1",
                "description": "Alibaba Cloud international AI gateway: Qwen 3.5, Qwen Max, and Qwen 2.5 Coder",
                "models": [
                    { "id": "qwen3.5-plus", "name": "Qwen 3.5 Plus (Flagship)", "context": 1000000, "recommended": true },
                    { "id": "qwen-max", "name": "Qwen Max (Complex Reasoning)", "context": 131072, "recommended": true },
                    { "id": "qwen-2.5-coder-32b-instruct", "name": "Qwen 2.5 Coder 32B", "context": 131072, "recommended": true },
                    { "id": "qwq-32b-preview", "name": "QwQ 32B Reasoning Preview", "context": 32768, "recommended": false }
                ]
            },
            {
                "id": "mistral", "name": "Mistral AI", "category": "byok", "auth_type": "api_key",
                "configured": is_byok("mistral"),
                "default_base_url": "https://api.mistral.ai/v1",
                "description": "Mistral AI models including Mistral Large 4, Medium 3.5, and Codestral",
                "models": [
                    { "id": "mistral-large-4", "name": "Mistral Large 4 (Multimodal Flagship)", "context": 128000, "recommended": true },
                    { "id": "mistral-medium-3.5", "name": "Mistral Medium 3.5 (Agentic & Coding)", "context": 128000, "recommended": true },
                    { "id": "mistral-small-4", "name": "Mistral Small 4", "context": 128000, "recommended": false },
                    { "id": "codestral-latest", "name": "Codestral Latest (Code Specialist)", "context": 256000, "recommended": true },
                    { "id": "ministral-8b", "name": "Ministral 8B (Fast & Light)", "context": 128000, "recommended": false }
                ]
            },
            {
                "id": "groq", "name": "Groq (Fast Inference)", "category": "byok", "auth_type": "api_key",
                "configured": is_byok("groq"),
                "default_base_url": "https://api.groq.com/openai/v1",
                "description": "Ultra high-speed LPU inference engine for open-weights Llama 4 models",
                "models": [
                    { "id": "llama-4-scout", "name": "Llama 4 Scout (10M Context)", "context": 10000000, "recommended": true },
                    { "id": "llama-3.3-70b-versatile", "name": "Llama 3.3 70B Versatile", "context": 131072, "recommended": true },
                    { "id": "llama-3.1-8b-instant", "name": "Llama 3.1 8B Instant", "context": 131072, "recommended": false }
                ]
            },
            {
                "id": "openrouter", "name": "OpenRouter", "category": "byok", "auth_type": "api_key",
                "configured": is_byok("openrouter"),
                "default_base_url": "https://openrouter.ai/api/v1",
                "description": "Unified gateway across all frontier models and providers",
                "models": [
                    { "id": "openai/gpt-6-astra", "name": "GPT-6 Astra (OpenRouter)", "context": 1050000, "recommended": true },
                    { "id": "anthropic/claude-sonnet-5-5", "name": "Claude Sonnet 5.5 (OpenRouter)", "context": 1000000, "recommended": true },
                    { "id": "deepseek/deepseek-flash", "name": "DeepSeek Flash (OpenRouter)", "context": 1000000, "recommended": true },
                    { "id": "google/gemini-4-argon", "name": "Gemini 4 Argon (OpenRouter)", "context": 1000000, "recommended": false }
                ]
            },
            {
                "id": "ollama", "name": "Ollama (Local Models)", "category": "byok", "auth_type": "api_key",
                "configured": is_byok("ollama"),
                "default_base_url": "http://localhost:11434/v1",
                "description": "Run open models entirely locally on your hardware without internet",
                "models": [
                    { "id": "llama4:scout", "name": "Llama 4 Scout Local (10M Context)", "context": 10000000, "recommended": true },
                    { "id": "llama4:maverick", "name": "Llama 4 Maverick Local", "context": 1000000, "recommended": true },
                    { "id": "llama3.3", "name": "Llama 3.3 70B Local", "context": 32768, "recommended": false },
                    { "id": "deepseek-flash", "name": "DeepSeek-V4.1 Local", "context": 64000, "recommended": false }
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
    let full = std::path::Path::new(&state.workspace_root).join(&clean);
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

async fn get_github_prs(
    Query(q): Query<GitHubQuery>,
) -> impl IntoResponse {
    let owner = q.owner.unwrap_or_else(|| "99apps-id".to_string());
    let repo = q.repo.unwrap_or_else(|| "odon-ai".to_string());
    let client = reqwest::Client::new();
    let url = format!("https://api.github.com/repos/{owner}/{repo}/pulls?per_page=20");
    let mut req = client.get(&url).header("User-Agent", "odon-desktop");
    if let Ok(token) = std::env::var("GITHUB_TOKEN") {
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
    Query(q): Query<GitHubQuery>,
) -> impl IntoResponse {
    let owner = q.owner.unwrap_or_else(|| "99apps-id".to_string());
    let repo = q.repo.unwrap_or_else(|| "odon-ai".to_string());
    let st = q.state.unwrap_or_else(|| "open".to_string());
    let client = reqwest::Client::new();
    let url = format!("https://api.github.com/repos/{owner}/{repo}/issues?state={st}&per_page=20");
    let mut req = client.get(&url).header("User-Agent", "odon-desktop");
    if let Ok(token) = std::env::var("GITHUB_TOKEN") {
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
    Json(req): Json<CreateIssuePayload>,
) -> impl IntoResponse {
    let owner = req.owner.unwrap_or_else(|| "99apps-id".to_string());
    let repo = req.repo.unwrap_or_else(|| "odon-ai".to_string());
    let client = reqwest::Client::new();
    let url = format!("https://api.github.com/repos/{owner}/{repo}/issues");
    let mut r = client.post(&url).header("User-Agent", "odon-desktop");
    if let Ok(token) = std::env::var("GITHUB_TOKEN") {
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

#[derive(Serialize, Deserialize, Clone)]
struct SshProfile {
    id: String,
    alias: String,
    host: String,
    port: u16,
    user: String,
    key_path: Option<String>,
    auth_type: String,
    description: Option<String>,
    created_at: String,
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
    vec![
        SshProfile {
            id: "vps-naracitra".to_string(),
            alias: "vps-naracitra".to_string(),
            host: "103.126.116.111".to_string(),
            port: 2233,
            user: "naracitrasolusindo".to_string(),
            key_path: Some("~/.ssh/Nara.pem".to_string()),
            auth_type: "key".to_string(),
            description: Some("Ubuntu 24.04 Production VPS (Naracitra)".to_string()),
            created_at: "2026-10-19T00:00:00Z".to_string(),
        },
        SshProfile {
            id: "laptop-farka1in".to_string(),
            alias: "laptop-farka1in".to_string(),
            host: "100.68.30.91".to_string(),
            port: 22,
            user: "iwan".to_string(),
            key_path: None,
            auth_type: "key".to_string(),
            description: Some("Tailscale Windows 10 Home Workstation".to_string()),
            created_at: "2026-10-19T00:00:00Z".to_string(),
        },
    ]
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
    let mut profiles = load_ssh_profiles();
    if let Some(idx) = profiles.iter().position(|p| p.id == profile.id) {
        profiles[idx] = profile;
    } else {
        profiles.push(profile);
    }
    save_ssh_profiles_to_disk(&profiles);
    Json(json!({ "success": true, "profiles": profiles }))
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
    Json(req): Json<SshExecPayload>,
) -> impl IntoResponse {
    let profiles = load_ssh_profiles();
    let profile = req.profile_id.as_ref().and_then(|pid| profiles.iter().find(|p| &p.id == pid || &p.alias == pid));

    let host = req.host.as_deref().or_else(|| profile.map(|p| p.host.as_str())).unwrap_or("localhost");
    let port = req.port.or_else(|| profile.map(|p| p.port)).unwrap_or(22);
    let user = req.user.as_deref().or_else(|| profile.map(|p| p.user.as_str())).unwrap_or("root");
    let key = req.key_path.as_deref().or_else(|| profile.and_then(|p| p.key_path.as_deref()));

    let mut cmd = tokio::process::Command::new("ssh");
    cmd.arg("-p").arg(port.to_string());
    cmd.arg("-o").arg("StrictHostKeyChecking=no");
    cmd.arg("-o").arg("ConnectTimeout=10");

    if let Some(k) = key {
        let expanded = if k.starts_with('~') {
            if let Some(home) = std::env::var_os("USERPROFILE").or_else(|| std::env::var_os("HOME")) {
                let home_str = home.to_string_lossy().to_string();
                k.replacen('~', &home_str, 1)
            } else {
                k.to_string()
            }
        } else {
            k.to_string()
        };
        cmd.arg("-i").arg(expanded);
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

async fn save_skill_item(
    Json(payload): Json<SaveSkillPayload>,
) -> impl IntoResponse {
    let dir = get_skills_dir();
    let _ = std::fs::create_dir_all(&dir);
    let filename = format!("{}.md", payload.name.trim().replace(' ', "-").to_lowercase());
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
    let filename = format!("{}.md", name);
    let path = dir.join(filename);

    match std::fs::remove_file(&path) {
        Ok(_) => Json(json!({ "success": true })),
        Err(e) => Json(json!({ "success": false, "error": e.to_string() })),
    }
}
