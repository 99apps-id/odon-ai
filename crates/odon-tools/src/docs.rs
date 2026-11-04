use crate::registry::{Tool, ToolContext};
use async_trait::async_trait;
use odon_core::error::{OdonError, Result};
use odon_security::RiskTier;
use reqwest::Client;
use serde_json::{json, Value};
use std::path::{Path, PathBuf};
use std::time::Duration;
use tokio::process::Command;
use tokio::time::timeout;

const COMMAND_TIMEOUT: Duration = Duration::from_secs(300);
const HTTP_TIMEOUT: Duration = Duration::from_secs(60);
const OUTPUT_LIMIT: usize = 64 * 1024;

fn bounded_output(raw: &[u8]) -> (String, bool) {
    if raw.len() <= OUTPUT_LIMIT {
        (String::from_utf8_lossy(raw).to_string(), false)
    } else {
        (
            String::from_utf8_lossy(&raw[..OUTPUT_LIMIT]).to_string(),
            true,
        )
    }
}

async fn run_command(
    program: &str,
    args: &[&str],
    cwd: &str,
    timeout: Duration,
) -> Result<(std::process::Output, String)> {
    let output = tokio::time::timeout(timeout, async {
        Command::new(program)
            .args(args)
            .current_dir(cwd)
            .output()
            .await
    })
    .await
    .map_err(|_| OdonError::Tool {
        tool: "docs".to_string(),
        message: format!("Command timed out after {}s", timeout.as_secs()),
    })??;
    let (stdout, _) = bounded_output(&output.stdout);
    Ok((output, stdout))
}

pub struct OpenApiGenerateTool;

#[async_trait]
impl Tool for OpenApiGenerateTool {
    fn name(&self) -> &str {
        "openapi_generate"
    }
    fn description(&self) -> &str {
        "Generate OpenAPI documentation from project sources"
    }
    fn parameters_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "command": {
                    "type": "string",
                    "description": "Explicit generation command override"
                },
                "timeout_secs": {
                    "type": "integer",
                    "minimum": 1,
                    "maximum": 900,
                    "description": "Optional timeout in seconds"
                }
            }
        })
    }
    fn risk_tier(&self) -> RiskTier {
        RiskTier::Tier2SystemMutating
    }

    async fn execute(&self, ctx: &ToolContext, args: Value) -> Result<Value> {
        let command_override = args
            .get("command")
            .and_then(|v| v.as_str())
            .map(|s| s.to_string());
        let timeout = args
            .get("timeout_secs")
            .and_then(|v| v.as_u64())
            .map(Duration::from_secs)
            .unwrap_or(COMMAND_TIMEOUT);
        let workspace_root = &ctx.workspace_root;
        let command = command_override
            .unwrap_or_else(|| "echo 'OpenAPI generation not configured'".to_string());
        let mut parts = command.split_whitespace();
        let program = parts.next().unwrap_or(&command);
        let args_vec: Vec<&str> = parts.collect();
        let (output, stdout) = run_command(program, &args_vec, workspace_root, timeout).await?;
        let (stderr, stderr_truncated) = bounded_output(&output.stderr);
        Ok(json!({
            "command": command,
            "success": output.status.success(),
            "exit_code": output.status.code(),
            "stdout": stdout,
            "stderr": stderr,
            "stderr_truncated": stderr_truncated
        }))
    }
}

pub struct ArchitectureDiagramTool;

#[async_trait]
impl Tool for ArchitectureDiagramTool {
    fn name(&self) -> &str {
        "architecture_diagram"
    }
    fn description(&self) -> &str {
        "Generate an architecture diagram artifact from repository metadata"
    }
    fn parameters_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "output_path": {
                    "type": "string",
                    "description": "Destination path for the generated diagram"
                },
                "command": {
                    "type": "string",
                    "description": "Explicit generation command override"
                },
                "timeout_secs": {
                    "type": "integer",
                    "minimum": 1,
                    "maximum": 900,
                    "description": "Optional timeout in seconds"
                }
            }
        })
    }
    fn risk_tier(&self) -> RiskTier {
        RiskTier::Tier2SystemMutating
    }

    async fn execute(&self, ctx: &ToolContext, args: Value) -> Result<Value> {
        let output_path = args
            .get("output_path")
            .and_then(|v| v.as_str())
            .unwrap_or("architecture.png");
        let command_override = args
            .get("command")
            .and_then(|v| v.as_str())
            .map(|s| s.to_string());
        let timeout = args
            .get("timeout_secs")
            .and_then(|v| v.as_u64())
            .map(Duration::from_secs)
            .unwrap_or(COMMAND_TIMEOUT);
        let workspace_root = &ctx.workspace_root;
        let command = command_override
            .unwrap_or_else(|| "echo 'Architecture diagram generation not configured'".to_string());
        let mut parts = command.split_whitespace();
        let program = parts.next().unwrap_or(&command);
        let args_vec: Vec<&str> = parts.collect();
        let (output, stdout) = run_command(program, &args_vec, workspace_root, timeout).await?;
        let (stderr, stderr_truncated) = bounded_output(&output.stderr);
        Ok(json!({
            "output_path": output_path,
            "command": command,
            "success": output.status.success(),
            "exit_code": output.status.code(),
            "stdout": stdout,
            "stderr": stderr,
            "stderr_truncated": stderr_truncated
        }))
    }
}

pub struct ChangelogGenerateTool;

#[async_trait]
impl Tool for ChangelogGenerateTool {
    fn name(&self) -> &str {
        "changelog_generate"
    }
    fn description(&self) -> &str {
        "Generate changelog content from git history"
    }
    fn parameters_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "output_path": {
                    "type": "string",
                    "description": "Destination changelog file path"
                },
                "command": {
                    "type": "string",
                    "description": "Explicit command override"
                },
                "timeout_secs": {
                    "type": "integer",
                    "minimum": 1,
                    "maximum": 900,
                    "description": "Optional timeout in seconds"
                }
            }
        })
    }
    fn risk_tier(&self) -> RiskTier {
        RiskTier::Tier2SystemMutating
    }

    async fn execute(&self, ctx: &ToolContext, args: Value) -> Result<Value> {
        let output_path = args
            .get("output_path")
            .and_then(|v| v.as_str())
            .unwrap_or("CHANGELOG.md");
        let command_override = args
            .get("command")
            .and_then(|v| v.as_str())
            .map(|s| s.to_string());
        let timeout = args
            .get("timeout_secs")
            .and_then(|v| v.as_u64())
            .map(Duration::from_secs)
            .unwrap_or(COMMAND_TIMEOUT);
        let workspace_root = &ctx.workspace_root;
        let command =
            command_override.unwrap_or_else(|| "git log --pretty=format:'- %s' -n 20".to_string());
        let mut parts = command.split_whitespace();
        let program = parts.next().unwrap_or(&command);
        let args_vec: Vec<&str> = parts.collect();
        let (output, stdout) = run_command(program, &args_vec, workspace_root, timeout).await?;
        let (stderr, stderr_truncated) = bounded_output(&output.stderr);
        Ok(json!({
            "output_path": output_path,
            "command": command,
            "success": output.status.success(),
            "exit_code": output.status.code(),
            "stdout": stdout,
            "stderr": stderr,
            "stderr_truncated": stderr_truncated
        }))
    }
}
