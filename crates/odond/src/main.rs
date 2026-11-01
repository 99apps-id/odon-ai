use axum::{
    extract::{Path, State},
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
use serde::Deserialize;
use serde_json::json;
use std::convert::Infallible;
use std::sync::Arc;
use tokio::sync::broadcast;
use tower_http::cors::{Any, CorsLayer};
use tower_http::services::ServeDir;
use tracing::info;

use odon_agent::AgentRuntime;
use odon_channel::{ChannelAdapter, TelegramRelay};
use odon_core::events::create_event_bus;
use odon_core::session::{Session, SessionMode};
use odon_core::AgentEvent;
use odon_provider::{OpenAiCompatibleProvider, SimulatedProvider};
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
    audit: Arc<AuditLogger>,
    tools: Arc<ToolRegistry>,
    event_tx: broadcast::Sender<AgentEvent>,
    workspace_root: String,
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
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

            // 2. Security & Policy
            let policy = SecurityPolicy::default();
            let approval = Arc::new(ApprovalEngine::new(policy));
            let audit = Arc::new(AuditLogger::new());

            // 3. Event Bus
            let (event_tx, _event_rx) = create_event_bus(512);

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
            } else {
                let sim_prov = Arc::new(SimulatedProvider::default());
                runtime.register_provider("default", sim_prov, true).await;
            }

            // Always make simulated provider available as fallback
            let sim_prov = Arc::new(SimulatedProvider::default());
            runtime.register_provider("simulated", sim_prov, false).await;

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
            let state = AppState {
                runtime,
                storage,
                approval,
                tools,
                event_tx,
                workspace_root: workspace,
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
                .route("/api/terminal/exec", post(execute_terminal))
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
        Some("doc") => SessionMode::Documents,
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
}

async fn send_message(
    Path(session_id): Path<String>,
    State(state): State<AppState>,
    Json(req): Json<SendMessageRequest>,
) -> impl IntoResponse {
    let cancel = tokio_util::sync::CancellationToken::new();
    let res = state
        .runtime
        .execute_turn(&session_id, &req.text, req.model.as_deref(), cancel)
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
}

async fn execute_terminal(
    State(state): State<AppState>,
    Json(req): Json<TerminalExecRequest>,
) -> impl IntoResponse {
    let args = json!({ "command": req.command });
    let ctx = odon_tools::ToolContext {
        session_id: "terminal".to_string(),
        workspace_root: state.workspace_root.clone(),
    };
    if let Some(tool) = state.tools.get("shell_exec") {
        match tool.execute(&ctx, args).await {
            Ok(val) => (StatusCode::OK, Json(val)),
            Err(e) => (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(json!({
                    "exit_code": -1,
                    "stdout": "",
                    "stderr": e.to_string(),
                    "success": false
                })),
            ),
        }
    } else {
        (
            StatusCode::NOT_FOUND,
            Json(json!({
                "exit_code": -1,
                "stdout": "",
                "stderr": "shell_exec tool not available".to_string(),
                "success": false
            })),
        )
    }
}

async fn sse_events(
    State(state): State<AppState>,
) -> Sse<impl Stream<Item = std::result::Result<Event, Infallible>>> {
    let mut rx = state.event_tx.subscribe();
    let stream = async_stream::stream! {
        while let Ok(event) = rx.recv().await {
            if let Ok(data) = serde_json::to_string(&event) {
                yield Ok(Event::default().data(data));
            }
        }
    };

    Sse::new(stream).keep_alive(KeepAlive::default())
}
