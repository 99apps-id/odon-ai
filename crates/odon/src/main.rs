use clap::{Parser, Subcommand};
use colored::Colorize;
use std::io::{self, Write};
use std::sync::Arc;
use tokio_util::sync::CancellationToken;

use odon_agent::AgentRuntime;
use odon_channel::{ChannelAdapter, CliChannel};
use odon_core::events::create_event_bus;
use odon_core::session::{Session, SessionMode};
use odon_provider::OpenAiCompatibleProvider;
use odon_security::{ApprovalEngine, AuditLogger, SecurityPolicy};
use odon_storage::StorageEngine;
use odon_tools::artifact_gen::{ArtifactCreateCodeTool, ArtifactCreateDocumentTool, ArtifactCreateImageTool};
use odon_tools::fs::{FsListTool, FsReadTool, FsWriteTool};
use odon_tools::git::{GitCommitTool, GitDiffTool, GitStatusTool};
use odon_tools::github::{GitHubGetRepoTool, GitHubListIssuesTool};
use odon_tools::mcp::McpListToolsTool;
use odon_tools::shell::ShellExecTool;
use odon_tools::ToolRegistry;

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
    /// Show current ODON system status, relays, and storage
    Status,
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
    registry.register(Box::new(ArtifactCreateCodeTool));
    registry.register(Box::new(ArtifactCreateDocumentTool));
    registry.register(Box::new(ArtifactCreateImageTool));
    let tools = Arc::new(registry);

    // 5. Agent Runtime
    let workspace = std::env::current_dir()?.to_string_lossy().to_string();
    let runtime = Arc::new(AgentRuntime::new(
        storage.clone(),
        tools,
        approval,
        audit,
        event_tx.clone(),
        workspace,
    ));

    // 6. Register default model provider (BYOK / OpenAI compatible / Ollama)
    let base_url = std::env::var("OPENAI_BASE_URL").unwrap_or_else(|_| "http://localhost:11434/v1".to_string());
    let api_key = std::env::var("OPENAI_API_KEY").unwrap_or_default();
    let default_model = std::env::var("ODON_MODEL").unwrap_or_else(|_| "odon-auto".to_string());

    let provider = Arc::new(OpenAiCompatibleProvider::new(base_url, api_key, Some("openai-compatible")));
    runtime.register_provider("default", provider, true).await;

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
            println!(" • Model:        {}", default_model.cyan());
            println!(" • Relays:       {}", "Telegram Ready | WhatsApp Stub Ready".magenta());
            println!(" • Persistence:  {}", "Durable SQLite State".yellow());
            println!(" • Connectors:   {}", "GitHub | Stdio MCP | Local Filesystem".blue());
            println!(" • Mode:         {}", "ChatGPT Desktop + Codex Workspace (Fluid)".bright_white());
        }
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
