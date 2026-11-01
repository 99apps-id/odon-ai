use axum::{
    extract::{
        ws::{Message as WsMessage, WebSocket, WebSocketUpgrade},
        Path, State,
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
use serde::Deserialize;
use serde_json::json;
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
use odon_provider::OpenAiCompatibleProvider;
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

            // 6. Register Providers
            let base_url = std::env::var("OPENAI_BASE_URL").unwrap_or_else(|_| "http://localhost:11434/v1".to_string());
            let api_key = std::env::var("OPENAI_API_KEY").unwrap_or_default();

            if !api_key.is_empty() || std::env::var("OPENAI_BASE_URL").is_ok() {
                let openai_prov = Arc::new(OpenAiCompatibleProvider::new(base_url, api_key, Some("openai-compatible")));
                runtime.register_provider("default", openai_prov, true).await;
            }

            // 7. Telegram Relay
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

            // 8. Build Axum App Router
            let oauth_accounts = Arc::new(tokio::sync::RwLock::new(HashMap::new()));
            let state = AppState {
                runtime,
                storage,
                approval,
                audit,
                tools,
                event_tx,
                workspace_root: workspace,
                oauth_accounts,
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
                .route("/api/settings", get(get_settings).post(update_settings))
                .route("/api/models", get(get_models))
                .route("/api/oauth/accounts", get(get_oauth_accounts))
                .route("/api/oauth/login", post(login_oauth))
                .route("/api/oauth/disconnect", post(disconnect_oauth))
                .route("/api/terminal/info", get(get_terminal_info))
                .route("/api/terminal/exec", post(execute_terminal))
                .route("/api/terminal/ws", get(ws_terminal_handler))
                .route("/api/events", get(sse_events));

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
    let dec = if req.decision.eq_ignore_ascii_case("approved") {
        ApprovalDecision::Approved
    } else {
        ApprovalDecision::Rejected {
            reason: req.reason.unwrap_or_else(|| "User rejected action".to_string()),
        }
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
    let openai_configured = std::env::var("OPENAI_API_KEY")
        .map(|value| !value.trim().is_empty())
        .unwrap_or(false);
    let compatible_configured = openai_configured || std::env::var("OPENAI_BASE_URL").is_ok();

    Json(json!({
        "catalog_as_of": "2026-10-31",
        "providers": [
            {
                "id": "chatgpt", "name": "ChatGPT Account", "category": "oauth", "auth_type": "oauth",
                "connected": is_connected("chatgpt"),
                "description": "OpenAI Codex backend using a ChatGPT account",
                "models": [
                    { "id": "gpt-6.1-sol", "name": "GPT-6.1 Sol", "context": 400000, "recommended": true },
                    { "id": "gpt-6-astra", "name": "GPT-6 Astra", "context": 400000 },
                    { "id": "gpt-6-luna", "name": "GPT-6 Luna", "context": 400000 },
                    { "id": "gpt-5.6-sol", "name": "GPT-5.6 Sol", "context": 400000 },
                    { "id": "gpt-5.6-luna", "name": "GPT-5.6 Luna", "context": 400000 },
                    { "id": "gpt-5.6-terra", "name": "GPT-5.6 Terra", "context": 400000 },
                    { "id": "gpt-5.3-codex", "name": "GPT-5.3 Codex", "context": 400000 }
                ]
            },
            {
                "id": "claude-oauth", "name": "Claude Account", "category": "oauth", "auth_type": "oauth",
                "connected": is_connected("claude-oauth"),
                "description": "Anthropic account OAuth with PKCE",
                "models": [
                    { "id": "claude-fable-5-1", "name": "Claude Fable 5.1", "context": 1000000, "recommended": true },
                    { "id": "claude-opus-5-5", "name": "Claude Opus 5.5", "context": 1000000 },
                    { "id": "claude-sonnet-5-5", "name": "Claude Sonnet 5.5", "context": 1000000 },
                    { "id": "claude-haiku-5-5", "name": "Claude Haiku 5.5", "context": 1000000 }
                ]
            },
            {
                "id": "xai-oauth", "name": "xAI Account", "category": "oauth", "auth_type": "oauth",
                "connected": is_connected("xai-oauth"),
                "description": "xAI device authorization",
                "models": [{ "id": "grok-4.7", "name": "Grok 4.7", "context": 2000000, "recommended": true }]
            },
            {
                "id": "github-copilot", "name": "GitHub Copilot", "category": "oauth", "auth_type": "oauth",
                "connected": is_connected("github-copilot"),
                "description": "GitHub device authorization with Copilot token exchange",
                "models": [
                    { "id": "gpt-5.6-sol", "name": "GPT-5.6 Sol", "context": 400000, "recommended": true },
                    { "id": "gpt-5.6-luna", "name": "GPT-5.6 Luna", "context": 400000 },
                    { "id": "gpt-5.6-terra", "name": "GPT-5.6 Terra", "context": 400000 },
                    { "id": "gpt-5.4", "name": "GPT-5.4", "context": 400000 },
                    { "id": "gpt-5.3-codex", "name": "GPT-5.3 Codex", "context": 400000 },
                    { "id": "claude-sonnet-5.5", "name": "Claude Sonnet 5.5", "context": 1000000 },
                    { "id": "gemini-3.8-flash", "name": "Gemini 3.8 Flash", "context": 2000000 },
                    { "id": "grok-4.7", "name": "Grok 4.7", "context": 2000000 }
                ]
            },
            {
                "id": "antigravity", "name": "Google Antigravity", "category": "oauth", "auth_type": "oauth",
                "connected": is_connected("antigravity"),
                "description": "Google Cloud Code Assist OAuth",
                "models": [
                    { "id": "gemini-4-argon", "name": "Gemini 4 Argon", "context": 2000000, "recommended": true },
                    { "id": "gemini-3.8-flash-high", "name": "Gemini 3.8 Flash High", "context": 2000000 },
                    { "id": "claude-opus-5-5", "name": "Claude Opus 5.5", "context": 1000000 }
                ]
            },
            {
                "id": "muse", "name": "Meta Muse Code", "category": "oauth", "auth_type": "oauth",
                "connected": is_connected("muse"),
                "description": "Meta Muse device authorization",
                "models": [{ "id": "muse-spark-1.3", "name": "Muse Spark 1.3", "context": 128000, "recommended": true }]
            },
            {
                "id": "openai", "name": "OpenAI", "category": "byok", "auth_type": "api_key",
                "configured": openai_configured,
                "default_base_url": "https://api.openai.com/v1",
                "description": "Official OpenAI API",
                "models": [
                    { "id": "gpt-6.1-sol", "name": "GPT-6.1 Sol", "context": 400000, "recommended": true },
                    { "id": "gpt-6-astra", "name": "GPT-6 Astra", "context": 400000 },
                    { "id": "gpt-6-luna", "name": "GPT-6 Luna", "context": 400000 },
                    { "id": "gpt-5.6-sol", "name": "GPT-5.6 Sol", "context": 400000 },
                    { "id": "gpt-5.6-luna", "name": "GPT-5.6 Luna", "context": 400000 },
                    { "id": "gpt-5.6-terra", "name": "GPT-5.6 Terra", "context": 400000 }
                ]
            },
            {
                "id": "anthropic", "name": "Anthropic", "category": "byok", "auth_type": "api_key",
                "default_base_url": "https://api.anthropic.com/v1",
                "description": "Official Anthropic API",
                "models": [
                    { "id": "claude-fable-5-1", "name": "Claude Fable 5.1", "context": 1000000, "recommended": true },
                    { "id": "claude-opus-5-5", "name": "Claude Opus 5.5", "context": 1000000 },
                    { "id": "claude-sonnet-5-5", "name": "Claude Sonnet 5.5", "context": 1000000 },
                    { "id": "claude-haiku-5-5", "name": "Claude Haiku 5.5", "context": 1000000 }
                ]
            },
            {
                "id": "google", "name": "Google Gemini", "category": "byok", "auth_type": "api_key",
                "default_base_url": "https://generativelanguage.googleapis.com/v1beta/openai",
                "description": "Official Gemini OpenAI-compatible API",
                "models": [
                    { "id": "gemini-4-argon", "name": "Gemini 4 Argon", "context": 2000000, "recommended": true },
                    { "id": "gemini-3.8-flash", "name": "Gemini 3.8 Flash", "context": 2000000 },
                    { "id": "gemini-3.8-flash-lite", "name": "Gemini 3.8 Flash-Lite", "context": 2000000 }
                ]
            },
            {
                "id": "deepseek", "name": "DeepSeek", "category": "byok", "auth_type": "api_key",
                "default_base_url": "https://api.deepseek.com/v1",
                "description": "DeepSeek V4 API",
                "models": [
                    { "id": "deepseek-v4-pro", "name": "DeepSeek V4 Pro", "context": 1000000, "recommended": true },
                    { "id": "deepseek-flash", "name": "DeepSeek V4.1 Flash", "context": 1000000 }
                ]
            },
            {
                "id": "qwen", "name": "Qwen", "category": "byok", "auth_type": "api_key",
                "default_base_url": "https://dashscope-intl.aliyuncs.com/compatible-mode/v1",
                "description": "Alibaba Model Studio international endpoint",
                "models": [
                    { "id": "qwen3.8-max", "name": "Qwen 3.8 Max", "context": 1000000, "recommended": true },
                    { "id": "qwen3.7-plus", "name": "Qwen 3.7 Plus", "context": 1000000 },
                    { "id": "qwen3.8-flash", "name": "Qwen 3.8 Flash", "context": 1000000 }
                ]
            },
            {
                "id": "mistral", "name": "Mistral", "category": "byok", "auth_type": "api_key",
                "default_base_url": "https://api.mistral.ai/v1",
                "description": "Official Mistral API",
                "models": [
                    { "id": "mistral-large-4", "name": "Mistral Large 4", "context": 1000000, "recommended": true },
                    { "id": "mistral-medium-3.5", "name": "Mistral Medium 3.5", "context": 1000000 },
                    { "id": "mistral-small-4", "name": "Mistral Small 4", "context": 1000000 }
                ]
            },
            {
                "id": "openai-compatible", "name": "OpenAI Compatible", "category": "byok", "auth_type": "api_key",
                "configured": compatible_configured,
                "default_base_url": "http://localhost:11434/v1",
                "description": "Custom OpenAI-compatible endpoint",
                "models": [{ "id": "custom", "name": "Custom model ID", "context": 0, "recommended": true }]
            }
        ]
    }))
}

#[allow(dead_code)]
async fn legacy_models_catalog() -> impl IntoResponse {
    Json(json!({
        "providers": [
            {
                "id": "antigravity",
                "name": "Antigravity (Google / DeepMind)",
                "category": "oauth",
                "auth_type": "oauth",
                "description": "High-throughput intelligence via Antigravity OAuth proxy with hybrid reasoning support",
                "models": [
                    { "id": "claude-3-7-sonnet", "name": "Claude 3.7 Sonnet (Hybrid Reasoning)", "context": 200000, "recommended": true },
                    { "id": "claude-3-5-sonnet-20241022", "name": "Claude 3.5 Sonnet v2", "context": 200000, "recommended": false },
                    { "id": "gemini-2.0-flash", "name": "Gemini 2.0 Flash", "context": 1048576, "recommended": true },
                    { "id": "gemini-1.5-pro", "name": "Gemini 1.5 Pro", "context": 2097152, "recommended": false },
                    { "id": "claude-3-5-haiku", "name": "Claude 3.5 Haiku", "context": 200000, "recommended": false }
                ]
            },
            {
                "id": "codex",
                "name": "OpenAI Codex / ChatGPT",
                "category": "oauth",
                "auth_type": "oauth",
                "description": "Official OpenAI OAuth authorization with specialized coding & reasoning models",
                "models": [
                    { "id": "o3-mini", "name": "o3-mini (Reasoning & Coding)", "context": 200000, "recommended": true },
                    { "id": "o1", "name": "o1 (Advanced Deep Reasoning)", "context": 200000, "recommended": false },
                    { "id": "gpt-4.5-preview", "name": "GPT-4.5 Preview", "context": 128000, "recommended": true },
                    { "id": "gpt-4o", "name": "GPT-4o (Omnimodal Flagship)", "context": 128000, "recommended": true },
                    { "id": "gpt-4o-mini", "name": "GPT-4o Mini (Fast & Light)", "context": 128000, "recommended": false }
                ]
            },
            {
                "id": "claude-oauth",
                "name": "Claude (Anthropic OAuth)",
                "category": "oauth",
                "auth_type": "oauth",
                "description": "Direct Anthropic Claude account authorization via OAuth",
                "models": [
                    { "id": "claude-3-7-sonnet-20250219", "name": "Claude 3.7 Sonnet (Hybrid Reasoning)", "context": 200000, "recommended": true },
                    { "id": "claude-3-5-sonnet-20241022", "name": "Claude 3.5 Sonnet v2", "context": 200000, "recommended": true },
                    { "id": "claude-3-5-haiku-20241022", "name": "Claude 3.5 Haiku", "context": 200000, "recommended": false }
                ]
            },
            {
                "id": "github-copilot",
                "name": "GitHub Copilot",
                "category": "oauth",
                "auth_type": "oauth",
                "description": "GitHub Copilot device authorization with Copilot token exchange",
                "models": [
                    { "id": "copilot-gpt-4o", "name": "GPT-4o (GitHub Copilot)", "context": 128000, "recommended": true },
                    { "id": "copilot-claude-3.5-sonnet", "name": "Claude 3.5 Sonnet (GitHub Copilot)", "context": 200000, "recommended": true },
                    { "id": "copilot-o3-mini", "name": "o3-mini (GitHub Copilot)", "context": 200000, "recommended": false }
                ]
            },
            {
                "id": "xai-oauth",
                "name": "xAI Grok",
                "category": "oauth",
                "auth_type": "oauth",
                "description": "xAI Grok device authorization flow",
                "models": [
                    { "id": "grok-2-latest", "name": "Grok 2 Latest (Flagship)", "context": 131072, "recommended": true },
                    { "id": "grok-beta", "name": "Grok Beta", "context": 131072, "recommended": false }
                ]
            },
            {
                "id": "muse",
                "name": "Meta Muse Code",
                "category": "oauth",
                "auth_type": "oauth",
                "description": "Meta Muse Code developer device authorization and SSE streaming",
                "models": [
                    { "id": "muse-spark-1.3", "name": "Muse Spark 1.3 (Flagship Coding)", "context": 128000, "recommended": true },
                    { "id": "muse-code-v2", "name": "Muse Code v2", "context": 128000, "recommended": false }
                ]
            },
            {
                "id": "deepseek",
                "name": "DeepSeek (BYOK)",
                "category": "byok",
                "auth_type": "api_key",
                "default_base_url": "https://api.deepseek.com/v1",
                "description": "DeepSeek V3 and R1 reasoning models with transparent BYOK pricing",
                "models": [
                    { "id": "deepseek-chat", "name": "DeepSeek-V3 (Chat & Coding)", "context": 64000, "recommended": true },
                    { "id": "deepseek-reasoner", "name": "DeepSeek-R1 (Deep Reasoning)", "context": 64000, "recommended": true },
                    { "id": "deepseek-coder", "name": "DeepSeek Coder", "context": 32000, "recommended": false }
                ]
            },
            {
                "id": "qwen",
                "name": "Qwen Plan / DashScope Global",
                "category": "byok",
                "auth_type": "api_key",
                "default_base_url": "https://dashscope-intl.aliyuncs.com/compatible-mode/v1",
                "description": "Alibaba Cloud international AI gateway: Qwen 2.5 Coder, QwQ Reasoning, and Qwen Max",
                "models": [
                    { "id": "qwen-max-latest", "name": "Qwen Max Latest (Flagship)", "context": 32768, "recommended": true },
                    { "id": "qwen-plus-latest", "name": "Qwen Plus Latest", "context": 131072, "recommended": false },
                    { "id": "qwen-turbo-latest", "name": "Qwen Turbo Latest (Speed)", "context": 131072, "recommended": false },
                    { "id": "qwen-2.5-coder-32b-instruct", "name": "Qwen 2.5 Coder 32B", "context": 131072, "recommended": true },
                    { "id": "qwq-32b-preview", "name": "QwQ 32B Reasoning Preview", "context": 32768, "recommended": false },
                    { "id": "qwen2.5-72b-instruct", "name": "Qwen 2.5 72B Instruct", "context": 131072, "recommended": false }
                ]
            },
            {
                "id": "alibaba",
                "name": "Alibaba Tongyi Qianwen (China)",
                "category": "byok",
                "auth_type": "api_key",
                "default_base_url": "https://dashscope.aliyuncs.com/compatible-mode/v1",
                "description": "Alibaba domestic gateway for high-throughput mainland access",
                "models": [
                    { "id": "qwen-max", "name": "Qwen Max (Domestic)", "context": 32768, "recommended": true },
                    { "id": "qwen-plus", "name": "Qwen Plus (Domestic)", "context": 131072, "recommended": false },
                    { "id": "qwen-turbo", "name": "Qwen Turbo (Domestic)", "context": 131072, "recommended": false }
                ]
            },
            {
                "id": "stepfun",
                "name": "StepFun Plan (阶跃星辰)",
                "category": "byok",
                "auth_type": "api_key",
                "default_base_url": "https://api.stepfun.com/v1",
                "description": "StepFun frontier reasoning, ultra-long 256K context, and multimodal vision models",
                "models": [
                    { "id": "step-2-16k", "name": "Step-2 16K (Multimodal Flagship)", "context": 16384, "recommended": true },
                    { "id": "step-1-32k", "name": "Step-1 32K (Fast)", "context": 32768, "recommended": false },
                    { "id": "step-1-128k", "name": "Step-1 128K (Long Context)", "context": 131072, "recommended": false },
                    { "id": "step-1-256k", "name": "Step-1 256K (Ultra Long Context)", "context": 262144, "recommended": true },
                    { "id": "step-1v-32k", "name": "Step-1V 32K (Vision Multimodal)", "context": 32768, "recommended": false }
                ]
            },
            {
                "id": "openai",
                "name": "OpenAI (Official BYOK)",
                "category": "byok",
                "auth_type": "api_key",
                "default_base_url": "https://api.openai.com/v1",
                "description": "Official OpenAI platform with GPT-4.5, o3-mini, and GPT-4o models",
                "models": [
                    { "id": "gpt-4.5-preview", "name": "GPT-4.5 Preview", "context": 128000, "recommended": true },
                    { "id": "o3-mini", "name": "o3-mini (Reasoning)", "context": 200000, "recommended": true },
                    { "id": "o1", "name": "o1 (Deep Reasoning)", "context": 200000, "recommended": false },
                    { "id": "gpt-4o", "name": "GPT-4o (Omnimodal)", "context": 128000, "recommended": true },
                    { "id": "gpt-4o-mini", "name": "GPT-4o Mini", "context": 128000, "recommended": false }
                ]
            },
            {
                "id": "anthropic",
                "name": "Anthropic (Official BYOK)",
                "category": "byok",
                "auth_type": "api_key",
                "default_base_url": "https://api.anthropic.com/v1",
                "description": "Official Anthropic API with Claude 3.7 Sonnet Hybrid Reasoning",
                "models": [
                    { "id": "claude-3-7-sonnet-20250219", "name": "Claude 3.7 Sonnet (Hybrid Reasoning)", "context": 200000, "recommended": true },
                    { "id": "claude-3-5-sonnet-20241022", "name": "Claude 3.5 Sonnet v2", "context": 200000, "recommended": true },
                    { "id": "claude-3-5-haiku-20241022", "name": "Claude 3.5 Haiku", "context": 200000, "recommended": false }
                ]
            },
            {
                "id": "google",
                "name": "Google Gemini (Official BYOK)",
                "category": "byok",
                "auth_type": "api_key",
                "default_base_url": "https://generativelanguage.googleapis.com/v1beta",
                "description": "Google Gemini API with 1M-2M context and native multimodal reasoning",
                "models": [
                    { "id": "gemini-2.0-flash", "name": "Gemini 2.0 Flash", "context": 1048576, "recommended": true },
                    { "id": "gemini-2.0-flash-thinking-exp", "name": "Gemini 2.0 Flash Thinking", "context": 1048576, "recommended": true },
                    { "id": "gemini-1.5-pro", "name": "Gemini 1.5 Pro", "context": 2097152, "recommended": false }
                ]
            },
            {
                "id": "groq",
                "name": "Groq (Fast Inference)",
                "category": "byok",
                "auth_type": "api_key",
                "default_base_url": "https://api.groq.com/openai/v1",
                "description": "Ultra high-speed LPU inference engine for open-weights models",
                "models": [
                    { "id": "llama-3.3-70b-versatile", "name": "Llama 3.3 70B Versatile", "context": 131072, "recommended": true },
                    { "id": "deepseek-r1-distill-llama-70b", "name": "DeepSeek R1 Distill Llama 70B", "context": 131072, "recommended": true }
                ]
            },
            {
                "id": "mistral",
                "name": "Mistral AI",
                "category": "byok",
                "auth_type": "api_key",
                "default_base_url": "https://api.mistral.ai/v1",
                "description": "Mistral AI models including Codestral and Mistral Large",
                "models": [
                    { "id": "mistral-large-latest", "name": "Mistral Large Latest", "context": 128000, "recommended": true },
                    { "id": "codestral-latest", "name": "Codestral Latest (Coding)", "context": 256000, "recommended": true }
                ]
            },
            {
                "id": "openrouter",
                "name": "OpenRouter",
                "category": "byok",
                "auth_type": "api_key",
                "default_base_url": "https://openrouter.ai/api/v1",
                "description": "Unified gateway across all frontier models and providers",
                "models": [
                    { "id": "anthropic/claude-3.7-sonnet", "name": "Claude 3.7 Sonnet (OpenRouter)", "context": 200000, "recommended": true },
                    { "id": "deepseek/deepseek-r1", "name": "DeepSeek R1 (OpenRouter)", "context": 64000, "recommended": true },
                    { "id": "openai/o3-mini", "name": "o3-mini (OpenRouter)", "context": 200000, "recommended": false }
                ]
            },
            {
                "id": "ollama",
                "name": "Ollama (Local Models)",
                "category": "byok",
                "auth_type": "api_key",
                "default_base_url": "http://localhost:11434/v1",
                "description": "Run open models entirely locally on your hardware without internet",
                "models": [
                    { "id": "llama3.3:latest", "name": "Llama 3.3 70B Local", "context": 32768, "recommended": true },
                    { "id": "qwen2.5-coder:32b", "name": "Qwen 2.5 Coder 32B Local", "context": 32768, "recommended": true },
                    { "id": "deepseek-r1:32b", "name": "DeepSeek R1 32B Local", "context": 32768, "recommended": false }
                ]
            },
            {
                "id": "openai_compatible",
                "name": "OpenAI Compatible (Custom / Gateway)",
                "category": "byok",
                "auth_type": "api_key",
                "default_base_url": "https://api.openai.com/v1",
                "description": "Any OpenAI-compatible gateway: vLLM, LM Studio, LiteLLM, or custom proxies",
                "models": [
                    { "id": "gpt-4o", "name": "GPT-4o", "context": 128000, "recommended": true },
                    { "id": "gpt-4o-mini", "name": "GPT-4o Mini", "context": 128000, "recommended": false },
                    { "id": "o3-mini", "name": "o3-mini", "context": 200000, "recommended": false },
                    { "id": "custom", "name": "Custom Model (Manual input)", "context": 128000, "recommended": false }
                ]
            }
        ]
    }))
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
