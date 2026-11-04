use crate::registry::{Tool, ToolContext};
use async_trait::async_trait;
use odon_core::error::{OdonError, Result};
use odon_security::RiskTier;
use serde_json::{json, Value};
use std::path::{Path, PathBuf};
use std::time::Duration;
use tokio::process::Command;
use tokio::time::timeout;

const COMMAND_TIMEOUT: Duration = Duration::from_secs(300);
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
        tool: "image".to_string(),
        message: format!("Command timed out after {}s", timeout.as_secs()),
    })??;
    let (stdout, _) = bounded_output(&output.stdout);
    Ok((output, stdout))
}

pub struct DockerImageBuildTool;

#[async_trait]
impl Tool for DockerImageBuildTool {
    fn name(&self) -> &str {
        "docker_image_build"
    }
    fn description(&self) -> &str {
        "Build a Docker image from a Dockerfile with bounded output and timeout"
    }
    fn parameters_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "dockerfile": {
                    "type": "string",
                    "description": "Dockerfile path"
                },
                "tag": {
                    "type": "string",
                    "description": "Image tag"
                },
                "context": {
                    "type": "string",
                    "description": "Build context path"
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
            },
            "required": ["tag"]
        })
    }
    fn risk_tier(&self) -> RiskTier {
        RiskTier::Tier2SystemMutating
    }

    async fn execute(&self, ctx: &ToolContext, args: Value) -> Result<Value> {
        let tag = args
            .get("tag")
            .and_then(|v| v.as_str())
            .ok_or_else(|| OdonError::Tool {
                tool: self.name().to_string(),
                message: "tag is required".to_string(),
            })?;
        let dockerfile = args
            .get("dockerfile")
            .and_then(|v| v.as_str())
            .unwrap_or("Dockerfile");
        let context = args.get("context").and_then(|v| v.as_str()).unwrap_or(".");
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

        let command = if let Some(ref cmd) = command_override {
            cmd.clone()
        } else {
            format!("docker build --file {dockerfile} --tag {tag} {context}")
        };

        let mut parts = command.split_whitespace();
        let program = parts.next().unwrap_or(&command);
        let args_vec: Vec<&str> = parts.collect();
        let (output, stdout) = run_command(program, &args_vec, workspace_root, timeout).await?;
        let (stderr, stderr_truncated) = bounded_output(&output.stderr);
        Ok(json!({
            "tag": tag,
            "command": command,
            "success": output.status.success(),
            "exit_code": output.status.code(),
            "stdout": stdout,
            "stderr": stderr,
            "stderr_truncated": stderr_truncated
        }))
    }
}

pub struct DockerImagePushTool;

#[async_trait]
impl Tool for DockerImagePushTool {
    fn name(&self) -> &str {
        "docker_image_push"
    }
    fn description(&self) -> &str {
        "Push a Docker image to a registry"
    }
    fn parameters_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "tag": {
                    "type": "string",
                    "description": "Image tag to push"
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
            },
            "required": ["tag"]
        })
    }
    fn risk_tier(&self) -> RiskTier {
        RiskTier::Tier2SystemMutating
    }

    async fn execute(&self, ctx: &ToolContext, args: Value) -> Result<Value> {
        let tag = args
            .get("tag")
            .and_then(|v| v.as_str())
            .ok_or_else(|| OdonError::Tool {
                tool: self.name().to_string(),
                message: "tag is required".to_string(),
            })?;
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
        let command = if let Some(ref cmd) = command_override {
            cmd.clone()
        } else {
            format!("docker push {tag}")
        };
        let mut parts = command.split_whitespace();
        let program = parts.next().unwrap_or(&command);
        let args_vec: Vec<&str> = parts.collect();
        let (output, stdout) = run_command(program, &args_vec, workspace_root, timeout).await?;
        let (stderr, stderr_truncated) = bounded_output(&output.stderr);
        Ok(json!({
            "tag": tag,
            "command": command,
            "success": output.status.success(),
            "exit_code": output.status.code(),
            "stdout": stdout,
            "stderr": stderr,
            "stderr_truncated": stderr_truncated
        }))
    }
}
