use clap::{Parser, Subcommand};
use colored::Colorize;
use serde::{Deserialize, Serialize};
use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use tokio_util::sync::CancellationToken;

use odon_agent::AgentRuntime;
use odon_channel::{ChannelAdapter, CliChannel};
use odon_core::events::create_event_bus;
use odon_core::session::{Session, SessionMode};
use odon_provider::{OpenAiCompatibleProvider, SimulatedProvider};
use odon_security::{ApprovalEngine, AuditLogger, RiskTier, SecurityPolicy};
use odon_storage::StorageEngine;
use odon_tools::ToolContext;

#[derive(Parser)]
#[command(name = "odon")]
#[command(about = "ODON CLI - Universal AI assistant and autonomous orchestration network", long_about = None)]
struct Cli {
    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand)]
enum Commands {
    /// Start an interactive chat session with ODON
    Chat {
        #[arg(short, long)]
        session_id: Option<String>,

        #[arg(short, long)]
        model: Option<String>,
    },
    /// Run an autonomous mission or task
    Run {
        prompt: String,

        #[arg(short, long)]
        model: Option<String>,
    },
    /// Inspect and run tools from the local registry
    Tools {
        #[command(subcommand)]
        action: ToolCommands,
    },
    /// Inspect agent sessions and conversation logs
    Agents {
        #[command(subcommand)]
        action: AgentCommands,
    },
    /// Inspect or attach workspace roots (alias: project)
    #[command(alias = "project")]
    Workspace {
        #[command(subcommand)]
        action: WorkspaceCommands,
    },
    /// Inspect or update provider secrets (alias: secrets)
    #[command(alias = "secrets")]
    Config {
        #[command(subcommand)]
        action: ConfigCommands,
    },
    /// Show current ODON system status, relays, and storage
    Status,
}

#[derive(Subcommand)]
enum ConfigCommands {
    /// List provider configuration with masked keys
    List {
        #[arg(long)]
        json: bool,
    },
    /// Set or replace a provider API key
    Set {
        provider_id: String,

        #[arg(long)]
        key: Option<String>,

        #[arg(long)]
        key_file: Option<PathBuf>,

        #[arg(long)]
        base_url: Option<String>,

        #[arg(long)]
        name: Option<String>,

        #[arg(long = "model")]
        models: Vec<String>,

        #[arg(long)]
        default: bool,
    },
    /// Remove a provider configuration
    Delete { provider_id: String },
}

fn daemon_base_url() -> String {
    std::env::var("ODON_DAEMON_URL")
        .unwrap_or_else(|_| "http://127.0.0.1:4096".to_string())
        .trim_end_matches('/')
        .to_string()
}

fn mask_secret(value: &str) -> String {
    if value.len() > 8 {
        format!("{}...{}", &value[..4], &value[value.len() - 4..])
    } else if value.is_empty() {
        String::new()
    } else {
        "********".to_string()
    }
}

async fn daemon_request(
    method: reqwest::Method,
    path: &str,
    body: Option<serde_json::Value>,
) -> anyhow::Result<serde_json::Value> {
    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(8))
        .build()?;
    let mut request = client.request(method, format!("{}{}", daemon_base_url(), path));
    if let Ok(token) = std::env::var("ODON_API_TOKEN") {
        if !token.trim().is_empty() {
            request = request.bearer_auth(token);
        }
    }
    if let Some(body) = body {
        request = request.json(&body);
    }
    let response = request.send().await?;
    let status = response.status();
    let payload: serde_json::Value = response.json().await.unwrap_or_else(|_| serde_json::json!({}));
    if !status.is_success() {
        anyhow::bail!("daemon returned HTTP {status}: {payload}");
    }
    Ok(payload)
}

#[derive(Subcommand)]
enum ToolCommands {
    /// List registered tools
    List {
        #[arg(long)]
        json: bool,

        #[arg(long)]
        filter: Option<String>,
    },
    /// Print a tool's JSON schema
    Schema { name: String },
    /// Execute a tool locally (mutating tools require --yes)
    Run {
        name: String,

        #[arg(long, default_value = "{}")]
        args: String,

        /// Read tool arguments from a JSON file instead of --args
        #[arg(long)]
        args_file: Option<PathBuf>,

        #[arg(long)]
        workspace: Option<String>,

        #[arg(long)]
        yes: bool,
    },
}

#[derive(Subcommand)]
enum AgentCommands {
    /// Run a one-shot agent task
    Run {
        prompt: String,

        #[arg(short, long)]
        model: Option<String>,
    },
    /// List persisted sessions
    List {
        #[arg(long)]
        json: bool,
    },
    /// Print the most recent messages of a session
    Logs {
        session_id: String,

        #[arg(long, default_value_t = 50)]
        limit: usize,
    },
}

#[derive(Subcommand)]
enum WorkspaceCommands {
    /// List configured workspace roots
    List {
        #[arg(long)]
        json: bool,
    },
    /// Add an existing folder to the workspace allowlist
    Attach { path: String },
    /// Create a folder (if needed) and add it to the workspace allowlist
    Init { path: Option<String> },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct LocalWorkspaceConfig {
    default: String,
    allowed: Vec<String>,
}

fn workspace_config_path() -> PathBuf {
    let base = std::env::var_os("ODON_HOME")
        .map(PathBuf::from)
        .or_else(|| std::env::var_os("USERPROFILE").map(PathBuf::from))
        .or_else(|| std::env::var_os("HOME").map(PathBuf::from))
        .unwrap_or_else(|| PathBuf::from("."));
    base.join(".odon").join("workspaces.json")
}

fn load_local_workspaces() -> LocalWorkspaceConfig {
    if let Ok(data) = std::fs::read_to_string(workspace_config_path()) {
        if let Ok(config) = serde_json::from_str::<LocalWorkspaceConfig>(&data) {
            return config;
        }
    }
    let current = std::env::current_dir()
        .map(|path| path.to_string_lossy().to_string())
        .unwrap_or_else(|_| ".".to_string());
    LocalWorkspaceConfig {
        default: current.clone(),
        allowed: vec![current],
    }
}

fn save_local_workspaces(config: &LocalWorkspaceConfig) -> anyhow::Result<()> {
    let path = workspace_config_path();
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    std::fs::write(path, serde_json::to_vec_pretty(config)?)?;
    Ok(())
}

fn attach_workspace(path: &Path) -> anyhow::Result<PathBuf> {
    let canonical = std::fs::canonicalize(path)
        .map_err(|error| anyhow::anyhow!("Cannot open workspace {}: {error}", path.display()))?;
    let mut config = load_local_workspaces();
    let value = canonical.to_string_lossy().to_string();
    if !config
        .allowed
        .iter()
        .any(|item| item.eq_ignore_ascii_case(&value))
    {
        config.allowed.push(value.clone());
    }
    config.default = value.clone();
    save_local_workspaces(&config)?;
    Ok(canonical)
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let cli = Cli::parse();

    // 1. Storage
    let storage = Arc::new(StorageEngine::default_storage()?);

    // 2. Security
    let policy = SecurityPolicy::default();
    let approval = Arc::new(ApprovalEngine::new(policy));
    let audit = Arc::new(AuditLogger::new());

    // 3. Event Bus
    let (event_tx, mut event_rx) = create_event_bus(128);

    // 4. Tools
    let tools = Arc::new(odon_tools::build_full_tool_registry());

    // 5. Agent Runtime
    let workspace = std::env::current_dir()?.to_string_lossy().to_string();
    let runtime = Arc::new(AgentRuntime::new(
        storage.clone(),
        tools.clone(),
        approval.clone(),
        audit.clone(),
        event_tx.clone(),
        workspace,
    ));

    // 6. Register model providers (BYOK / OpenAI / Ollama / Fallback)
    let base_url = std::env::var("OPENAI_BASE_URL").unwrap_or_else(|_| "http://localhost:11434/v1".to_string());
    let api_key = std::env::var("OPENAI_API_KEY").unwrap_or_default();
    let default_model = std::env::var("ODON_MODEL").unwrap_or_else(|_| "odon-auto".to_string());

    if !api_key.is_empty() || std::env::var("OPENAI_BASE_URL").is_ok() {
        let provider = Arc::new(OpenAiCompatibleProvider::new(base_url, api_key, Some("openai-compatible")));
        runtime.register_provider("default", provider, true).await;
    } else {
        let sim_provider = Arc::new(SimulatedProvider::default());
        runtime.register_provider("default", sim_provider, true).await;
    }

    let cli_channel = CliChannel::new();

    // Spawn event printer
    tokio::spawn(async move {
        while let Ok(evt) = event_rx.recv().await {
            let _ = cli_channel.broadcast_event(&evt).await;
        }
    });

    match cli.command {
        Commands::Status => {
            println!("{}", "====================================================".bold());
            println!("   ODON (Open Digital Orchestration Network)");
            println!("   Version: {}", env!("CARGO_PKG_VERSION").green());
            println!("{}", "====================================================".bold());
            let tool_count = tools.schemas().len();
            let session_count = storage
                .list_sessions()
                .map(|sessions| sessions.len())
                .unwrap_or(0);
            println!(" • Model:        {}", default_model.cyan());
            println!(" • Tools:        {}", tool_count.to_string().green());
            println!(" • Sessions:     {}", session_count.to_string().green());
            println!(" • Relays:       {}", "Telegram | WhatsApp relays".magenta());
            println!(" • Persistence:  {}", "Durable SQLite State".yellow());
            println!(" • Connectors:   {}", "GitHub | Stdio MCP | Local Filesystem".blue());
            println!(" • Mode:         {}", "ChatGPT Desktop + Codex Workspace (Fluid)".bright_white());
            println!(" • Daemon:       run `odond status --metrics` for live metrics");
        }
        Commands::Tools { action } => match action {
            ToolCommands::List { json, filter } => {
                let mut schemas = tools.schemas();
                schemas.sort_by(|a, b| a.name.cmp(&b.name));
                if let Some(filter) = filter {
                    let needle = filter.to_lowercase();
                    schemas.retain(|schema| {
                        schema.name.to_lowercase().contains(&needle)
                            || schema.description.to_lowercase().contains(&needle)
                    });
                }
                if json {
                    println!("{}", serde_json::to_string_pretty(&schemas)?);
                } else {
                    for schema in schemas {
                        println!(
                            "{:<28} tier{}  {}",
                            schema.name, schema.risk_tier, schema.description
                        );
                    }
                }
            }
            ToolCommands::Schema { name } => {
                let schema = tools
                    .schemas()
                    .into_iter()
                    .find(|schema| schema.name.eq_ignore_ascii_case(&name))
                    .ok_or_else(|| anyhow::anyhow!("Tool '{name}' not found"))?;
                println!("{}", serde_json::to_string_pretty(&schema)?);
            }
            ToolCommands::Run {
                name,
                args,
                args_file,
                workspace,
                yes,
            } => {
                let args_text = match args_file {
                    Some(path) => std::fs::read_to_string(&path).map_err(|error| {
                        anyhow::anyhow!("Cannot read {}: {error}", path.display())
                    })?,
                    None => args,
                };
                let args: serde_json::Value = serde_json::from_str(&args_text)
                    .map_err(|error| anyhow::anyhow!("Invalid tool arguments JSON: {error}"))?;
                let risk = approval.check_risk(&name).await;
                if risk > RiskTier::Tier0ReadOnly && !yes {
                    return Err(anyhow::anyhow!(
                        "Tool '{name}' requires explicit approval ({risk:?}). Re-run with --yes."
                    ));
                }
                if yes {
                    approval.pre_approve(&name, &args).await;
                }
                let root = workspace.unwrap_or_else(|| {
                    std::env::current_dir()
                        .map(|path| path.to_string_lossy().to_string())
                        .unwrap_or_else(|_| ".".to_string())
                });
                let ctx = ToolContext {
                    session_id: "cli_tool".to_string(),
                    workspace_root: root,
                    approval_engine: approval.clone(),
                    audit_logger: audit.clone(),
                    storage: Some(storage.clone()),
                };
                let result = tools.execute_tool(&name, &ctx, args).await?;
                println!("{}", serde_json::to_string_pretty(&result)?);
            }
        },
        Commands::Agents { action } => match action {
            AgentCommands::Run { prompt, model } => {
                let session = Session::new(
                    "Agent Run",
                    SessionMode::General,
                    model.as_deref().unwrap_or(&default_model),
                );
                let _ = storage.save_session(&session);
                let cancel = CancellationToken::new();
                runtime
                    .execute_turn(&session.id, &prompt, model.as_deref(), cancel)
                    .await?;
                println!("Session: {}", session.id);
            }
            AgentCommands::List { json } => {
                let sessions = storage.list_sessions()?;
                if json {
                    println!("{}", serde_json::to_string_pretty(&sessions)?);
                } else {
                    for session in sessions {
                        println!(
                            "{}  {:<12} {:<10} {}",
                            session.id,
                            format!("{:?}", session.mode).to_lowercase(),
                            format!("{:?}", session.status).to_lowercase(),
                            session.title
                        );
                    }
                }
            }
            AgentCommands::Logs { session_id, limit } => {
                let mut messages = storage.list_messages(&session_id)?;
                if messages.len() > limit {
                    messages = messages.split_off(messages.len() - limit);
                }
                for message in messages {
                    println!(
                        "[{}] {} {}",
                        message.created_at.to_rfc3339(),
                        format!("{:?}", message.role).to_lowercase(),
                        message.content.text
                    );
                    if let Some(thought) = &message.content.thought {
                        if !thought.trim().is_empty() {
                            println!("  thought: {thought}");
                        }
                    }
                    for tool in &message.content.tool_calls {
                        println!("  tool: {} {}", tool.tool_name, tool.arguments);
                    }
                }
            }
        },
        Commands::Workspace { action } => match action {
            WorkspaceCommands::List { json } => {
                let config = load_local_workspaces();
                if json {
                    println!("{}", serde_json::to_string_pretty(&config)?);
                } else {
                    println!("default: {}", config.default);
                    for path in config.allowed {
                        println!("  - {path}");
                    }
                }
            }
            WorkspaceCommands::Attach { path } => {
                let canonical = attach_workspace(Path::new(&path))?;
                println!("Attached {}", canonical.display());
                println!("Restart odond to load the updated workspace allowlist.");
            }
            WorkspaceCommands::Init { path } => {
                let target = path.unwrap_or_else(|| ".".to_string());
                std::fs::create_dir_all(&target)?;
                let canonical = attach_workspace(Path::new(&target))?;
                println!("Initialized workspace {}", canonical.display());
                println!("Restart odond to load the updated workspace allowlist.");
            }
        },
        Commands::Config { action } => match action {
            ConfigCommands::List { json } => {
                let providers = match daemon_request(
                    reqwest::Method::GET,
                    "/api/providers/config",
                    None,
                )
                .await
                {
                    Ok(value) => value,
                    Err(_) => {
                        let configs = storage.list_provider_configs()?;
                        serde_json::json!(configs
                            .into_iter()
                            .map(|config| serde_json::json!({
                                "provider_id": config.provider_id,
                                "configured": true,
                                "masked_key": mask_secret(&config.api_key),
                                "base_url": config.base_url,
                                "is_default": config.is_default,
                                "name": config.name,
                                "models": serde_json::from_str::<Vec<String>>(&config.models_json).unwrap_or_default(),
                                "updated_at": config.updated_at,
                            }))
                            .collect::<Vec<_>>())
                    }
                };
                let settings = daemon_request(
                    reqwest::Method::GET,
                    "/api/settings",
                    None,
                )
                .await
                .ok();
                if json {
                    println!(
                        "{}",
                        serde_json::to_string_pretty(&serde_json::json!({
                            "providers": providers,
                            "settings": settings
                        }))?
                    );
                } else {
                    if let Some(items) = providers.as_array() {
                        for item in items {
                            println!(
                                "{}  key={}  base={}  models={}",
                                item.get("provider_id").and_then(|v| v.as_str()).unwrap_or("-"),
                                item.get("masked_key").and_then(|v| v.as_str()).unwrap_or("-"),
                                item.get("base_url").and_then(|v| v.as_str()).unwrap_or("-"),
                                item.get("models")
                                    .and_then(|v| v.as_array())
                                    .map(|models| models.len())
                                    .unwrap_or(0)
                            );
                        }
                    } else {
                        println!("{}", serde_json::to_string_pretty(&providers)?);
                    }
                    if let Some(settings) = settings {
                        println!("settings: {}", serde_json::to_string(&settings)?);
                    }
                }
            }
            ConfigCommands::Set {
                provider_id,
                key,
                key_file,
                base_url,
                name,
                models,
                default,
            } => {
                let api_key = match key_file {
                    Some(path) => std::fs::read_to_string(&path)
                        .map_err(|error| anyhow::anyhow!("Cannot read {}: {error}", path.display()))?
                        .trim()
                        .to_string(),
                    None => match key {
                        Some(value) => value.trim().to_string(),
                        None => {
                            print!("API key for {provider_id}: ");
                            io::stdout().flush()?;
                            let mut value = String::new();
                            io::stdin().read_line(&mut value)?;
                            value.trim().to_string()
                        }
                    },
                };
                if api_key.is_empty() {
                    return Err(anyhow::anyhow!("API key is required"));
                }
                let mut body = serde_json::json!({
                    "provider_id": provider_id.clone(),
                    "api_key": api_key.clone(),
                    "base_url": base_url.clone(),
                    "is_default": default,
                    "name": name.clone()
                });
                if !models.is_empty() {
                    body["models"] = serde_json::json!(models.clone());
                }
                match daemon_request(reqwest::Method::POST, "/api/providers/config", Some(body)).await {
                    Ok(_) => println!("Saved {provider_id} through the running daemon."),
                    Err(error) => {
                        let existing = storage.get_provider_config(&provider_id).ok().flatten();
                        let models_json = if models.is_empty() {
                            existing
                                .as_ref()
                                .map(|config| config.models_json.clone())
                                .unwrap_or_else(|| "[]".to_string())
                        } else {
                            serde_json::to_string(&models)?
                        };
                        storage.save_provider_config(
                            &provider_id,
                            &api_key,
                            base_url.as_deref(),
                            default,
                            name.as_deref()
                                .or_else(|| existing.as_ref().and_then(|config| config.name.as_deref())),
                            &models_json,
                        )?;
                        println!(
                            "Saved {provider_id} to local storage ({error}). Restart odond to load it."
                        );
                    }
                }
            }
            ConfigCommands::Delete { provider_id } => {
                match daemon_request(
                    reqwest::Method::DELETE,
                    &format!("/api/providers/config/{provider_id}"),
                    None,
                )
                .await
                {
                    Ok(_) => println!("Deleted {provider_id} through the running daemon."),
                    Err(error) => {
                        let deleted = storage.delete_provider_config(&provider_id)?;
                        println!(
                            "{} {provider_id} in local storage ({error}).",
                            if deleted { "Deleted" } else { "No configuration for" }
                        );
                    }
                }
            }
        },
        Commands::Run { prompt, model } => {
            println!("{} {}", "▶ Running mission:".bold().green(), prompt);
            let session = Session::new("Mission Run", SessionMode::General, model.as_deref().unwrap_or(&default_model));
            let _ = storage.save_session(&session);
            let cancel = CancellationToken::new();

            let _ = runtime.execute_turn(&session.id, &prompt, model.as_deref(), cancel).await;
            println!("\n{}", "✔ Mission completed.".bold().green());
        }
        Commands::Chat { session_id, model } => {
            let sid = session_id.unwrap_or_else(|| {
                let s = Session::new("New Chat", SessionMode::General, model.as_deref().unwrap_or(&default_model));
                let _ = storage.save_session(&s);
                s.id
            });

            println!("{}", "====================================================".bold());
            println!("   ODON Chat - What can ODON do for you?");
            println!("   Code, create, automate, and more.");
            println!("   Type 'exit' to quit.");
            println!("{}", "====================================================".bold());

            loop {
                print!("\n{} ", "You ▶".bold().blue());
                io::stdout().flush()?;

                let mut input = String::new();
                io::stdin().read_line(&mut input)?;
                let trimmed = input.trim();

                if trimmed.is_empty() {
                    continue;
                }
                if trimmed.eq_ignore_ascii_case("exit") || trimmed.eq_ignore_ascii_case("quit") {
                    println!("{}", "Goodbye!".green());
                    break;
                }

                print!("\n{} ", "ODON ▶".bold().purple());
                io::stdout().flush()?;

                let cancel = CancellationToken::new();
                if let Err(e) = runtime.execute_turn(&sid, trimmed, model.as_deref(), cancel).await {
                    eprintln!("\n{}: {}", "Turn error".red(), e);
                }
                println!();
            }
        }
    }

    Ok(())
}
