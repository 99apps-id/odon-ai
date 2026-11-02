use async_trait::async_trait;
use odon_core::{AgentEvent, Result};
use tokio::sync::mpsc;

use crate::adapter::{ChannelAdapter, IncomingChannelMessage};

pub struct CliChannel;

impl Default for CliChannel {
    fn default() -> Self {
        Self::new()
    }
}

impl CliChannel {
    pub fn new() -> Self {
        Self
    }
}

#[async_trait]
impl ChannelAdapter for CliChannel {
    fn channel_name(&self) -> &str {
        "cli"
    }

    async fn start(&self, _incoming_tx: mpsc::Sender<IncomingChannelMessage>) -> Result<()> {
        // CLI interactive loop is controlled directly by the `odon chat` subcommand
        Ok(())
    }

    async fn broadcast_event(&self, event: &AgentEvent) -> Result<()> {
        match event {
            AgentEvent::TextDelta { delta, .. } => {
                print!("{}", delta);
                use std::io::Write;
                let _ = std::io::stdout().flush();
            }
            AgentEvent::ThoughtDelta { delta, .. } => {
                // Show thinking faintly in italic/dim if terminal supports it
                print!("\x1b[2m{}\x1b[0m", delta);
                use std::io::Write;
                let _ = std::io::stdout().flush();
            }
            AgentEvent::ToolCallStarted { tool_name, .. } => {
                println!("\n\x1b[36m⚙ Calling tool: {}\x1b[0m", tool_name);
            }
            AgentEvent::ToolCallFinished { is_error, .. } => {
                if *is_error {
                    println!("\x1b[31m✖ Tool failed\x1b[0m");
                } else {
                    println!("\x1b[32m✔ Tool completed\x1b[0m");
                }
            }
            AgentEvent::ArtifactCreated { artifact, .. } => {
                println!(
                    "\n\x1b[35m✨ Artifact Created: {} [{:?}]\x1b[0m - {}",
                    artifact.title, artifact.kind, artifact.summary
                );
            }
            AgentEvent::ApprovalRequested {
                tool_name,
                description,
                ..
            } => {
                println!(
                    "\n\x1b[33m⚠ Approval required for: {} ({})\x1b[0m",
                    tool_name, description
                );
            }
            AgentEvent::Error { message, .. } => {
                eprintln!("\n\x1b[31mError: {}\x1b[0m", message);
            }
            _ => {}
        }
        Ok(())
    }

    async fn send_direct_message(&self, _recipient: &str, content: &str) -> Result<()> {
        println!("{}", content);
        Ok(())
    }
}
