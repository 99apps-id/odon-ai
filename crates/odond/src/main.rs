use clap::{Parser, Subcommand};
use std::sync::Arc;
use tracing::info;

use odon_agent::AgentRuntime;
use odon_channel::{ChannelAdapter, TelegramRelay};
use odon_core::events::create_event_bus;
use odon_provider::OpenAiCompatibleProvider;
use odon_security::{ApprovalEngine, AuditLogger, SecurityPolicy};
use odon_storage::StorageEngine;
use odon_tools::fs::{FsListTool, FsReadTool, FsWriteTool};
use odon_tools::git::{GitCommitTool, GitDiffTool, GitStatusTool};
use odon_tools::github::{GitHubGetRepoTool, GitHubListIssuesTool};
use odon_tools::mcp::McpListToolsTool;
use odon_tools::shell::ShellExecTool;
use odon_tools::ToolRegistry;

#[derive(Parser)]
#[command(name = "odond")]
#[command(about = "ODON Headless Daemon - Background agent runtime and relay service", long_about = None)]
struct Cli {
    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand)]
enum Commands {
    /// Start the background agent runtime daemon
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
            info!("Binding daemon on: {}", bind);
            info!("Workspace root: {}", workspace);

            // 1. Storage
            let storage = Arc::new(StorageEngine::default_storage()?);

            // 2. Security & Policy
            let policy = SecurityPolicy::default();
            let approval = Arc::new(ApprovalEngine::new(policy));
            let audit = Arc::new(AuditLogger::new());

            // 3. Event Bus
            let (event_tx, _event_rx) = create_event_bus(256);

            // 4. Tools Registry
            let mut registry = ToolRegistry::new();
            registry.register(Box::new(FsReadTool));
            registry.register(Box::new(FsWriteTool));
            registry.register(Box::new(FsListTool));
            registry.register(Box::new(GitStatusTool));
            registry.register(Box::new(GitDiffTool));
            registry.register(Box::new(GitCommitTool));
            registry.register(Box::new(ShellExecTool));
            registry.register(Box::new(GitHubGetRepoTool::new()));
            registry.register(Box::new(GitHubListIssuesTool::new()));
            registry.register(Box::new(McpListToolsTool));
            let tools = Arc::new(registry);

            // 5. Agent Runtime
            let runtime = Arc::new(AgentRuntime::new(
                storage.clone(),
                tools,
                approval,
                audit,
                event_tx.clone(),
                workspace,
            ));

            // 6. Register default provider (Ollama / Local vLLM / OpenAI compatible)
            let base_url = std::env::var("OPENAI_BASE_URL").unwrap_or_else(|_| "http://localhost:11434/v1".to_string());
            let api_key = std::env::var("OPENAI_API_KEY").unwrap_or_default();
            let provider = Arc::new(OpenAiCompatibleProvider::new(base_url, api_key, Some("openai-compatible")));
            runtime.register_provider("default", provider, true).await;

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

            println!("\nDaemon listening on {} - Press Ctrl+C to terminate.", bind);

            tokio::signal::ctrl_c().await?;
            println!("\nODON Daemon shutting down cleanly.");
        }
    }

    Ok(())
}
