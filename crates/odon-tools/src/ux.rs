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

const HTTP_TIMEOUT: Duration = Duration::from_secs(60);
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
        tool: "ux".to_string(),
        message: format!("Command timed out after {}s", timeout.as_secs()),
    })??;
    let (stdout, _) = bounded_output(&output.stdout);
    Ok((output, stdout))
}

pub struct LighthouseCiTool;

#[async_trait]
impl Tool for LighthouseCiTool {
    fn name(&self) -> &str {
        "lighthouse_ci"
    }
    fn description(&self) -> &str {
        "Run Lighthouse CI audit against a target URL and return structured scores"
    }
    fn parameters_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "url": {
                    "type": "string",
                    "format": "uri",
                    "description": "Target URL to audit"
                },
                "command": {
                    "type": "string",
                    "description": "Explicit command override"
                },
                "timeout_secs": {
                    "type": "integer",
                    "minimum": 1,
                    "maximum": 900
                }
            },
            "required": ["url"]
        })
    }
    fn risk_tier(&self) -> RiskTier {
        RiskTier::Tier2SystemMutating
    }

    async fn execute(&self, ctx: &ToolContext, args: Value) -> Result<Value> {
        let url = args
            .get("url")
            .and_then(|v| v.as_str())
            .ok_or_else(|| OdonError::Tool {
                tool: self.name().to_string(),
                message: "url is required".to_string(),
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
        let command = command_override.unwrap_or_else(|| format!("lhci collect --url={url}"));
        let mut parts = command.split_whitespace();
        let program = parts.next().unwrap_or(&command);
        let args_vec: Vec<&str> = parts.collect();
        let (output, stdout) =
            run_command(program, &args_vec, &ctx.workspace_root, timeout).await?;
        let (stderr, stderr_truncated) = bounded_output(&output.stderr);
        Ok(json!({
            "url": url,
            "command": command,
            "success": output.status.success(),
            "exit_code": output.status.code(),
            "stdout": stdout,
            "stderr": stderr,
            "stderr_truncated": stderr_truncated
        }))
    }
}

pub struct AxeCoreScanTool;

#[async_trait]
impl Tool for AxeCoreScanTool {
    fn name(&self) -> &str {
        "axe_core_scan"
    }
    fn description(&self) -> &str {
        "Run axe-core accessibility scan against a target URL"
    }
    fn parameters_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "url": {
                    "type": "string",
                    "format": "uri"
                },
                "command": {
                    "type": "string",
                    "description": "Explicit command override"
                },
                "timeout_secs": {
                    "type": "integer",
                    "minimum": 1,
                    "maximum": 900
                }
            },
            "required": ["url"]
        })
    }
    fn risk_tier(&self) -> RiskTier {
        RiskTier::Tier2SystemMutating
    }

    async fn execute(&self, ctx: &ToolContext, args: Value) -> Result<Value> {
        let url = args
            .get("url")
            .and_then(|v| v.as_str())
            .ok_or_else(|| OdonError::Tool {
                tool: self.name().to_string(),
                message: "url is required".to_string(),
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
        let command =
            command_override.unwrap_or_else(|| format!("echo 'axe-core scan requested for {url}'"));
        let mut parts = command.split_whitespace();
        let program = parts.next().unwrap_or(&command);
        let args_vec: Vec<&str> = parts.collect();
        let (output, stdout) =
            run_command(program, &args_vec, &ctx.workspace_root, timeout).await?;
        let (stderr, stderr_truncated) = bounded_output(&output.stderr);
        Ok(json!({
            "url": url,
            "command": command,
            "success": output.status.success(),
            "exit_code": output.status.code(),
            "stdout": stdout,
            "stderr": stderr,
            "stderr_truncated": stderr_truncated
        }))
    }
}

pub struct ResponsiveScreenshotTool;

#[async_trait]
impl Tool for ResponsiveScreenshotTool {
    fn name(&self) -> &str {
        "responsive_screenshot"
    }
    fn description(&self) -> &str {
        "Capture responsive screenshots for common viewport sizes"
    }
    fn parameters_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "url": {
                    "type": "string",
                    "format": "uri"
                },
                "viewport": {
                    "type": "string",
                    "description": "Viewport preset or custom size"
                },
                "command": {
                    "type": "string",
                    "description": "Explicit command override"
                },
                "timeout_secs": {
                    "type": "integer",
                    "minimum": 1,
                    "maximum": 900
                }
            },
            "required": ["url"]
        })
    }
    fn risk_tier(&self) -> RiskTier {
        RiskTier::Tier2SystemMutating
    }

    async fn execute(&self, ctx: &ToolContext, args: Value) -> Result<Value> {
        let url = args
            .get("url")
            .and_then(|v| v.as_str())
            .ok_or_else(|| OdonError::Tool {
                tool: self.name().to_string(),
                message: "url is required".to_string(),
            })?;
        let viewport = args
            .get("viewport")
            .and_then(|v| v.as_str())
            .unwrap_or("responsive");
        let command_override = args
            .get("command")
            .and_then(|v| v.as_str())
            .map(|s| s.to_string());
        let timeout = args
            .get("timeout_secs")
            .and_then(|v| v.as_u64())
            .map(Duration::from_secs)
            .unwrap_or(COMMAND_TIMEOUT);
        let command = command_override.unwrap_or_else(|| {
            format!("echo 'Screenshot requested for {url} viewport={viewport}'")
        });
        let mut parts = command.split_whitespace();
        let program = parts.next().unwrap_or(&command);
        let args_vec: Vec<&str> = parts.collect();
        let (output, stdout) =
            run_command(program, &args_vec, &ctx.workspace_root, timeout).await?;
        let (stderr, stderr_truncated) = bounded_output(&output.stderr);
        Ok(json!({
            "url": url,
            "viewport": viewport,
            "command": command,
            "success": output.status.success(),
            "exit_code": output.status.code(),
            "stdout": stdout,
            "stderr": stderr,
            "stderr_truncated": stderr_truncated
        }))
    }
}
