use async_trait::async_trait;
use odon_core::{OdonError, Result};
use odon_security::RiskTier;
use serde_json::{json, Value};
use std::time::Duration;
use tokio::process::Command;
use tokio::time::timeout;

use crate::registry::{Tool, ToolContext};

const SHELL_OUTPUT_LIMIT: usize = 256 * 1024;

fn clip(raw: &[u8]) -> (String, bool) {
    if raw.len() <= SHELL_OUTPUT_LIMIT {
        (String::from_utf8_lossy(raw).to_string(), false)
    } else {
        (
            String::from_utf8_lossy(&raw[..SHELL_OUTPUT_LIMIT]).to_string(),
            true,
        )
    }
}

pub struct ShellExecTool;

#[async_trait]
impl Tool for ShellExecTool {
    fn name(&self) -> &str {
        "shell_exec"
    }
    fn description(&self) -> &str {
        "Execute a shell command in the workspace (system mutating, requires approval)"
    }
    fn parameters_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "command": { "type": "string", "description": "Command string to run" },
                "cwd": { "type": "string", "description": "Workspace-relative working directory (default: workspace root)" },
                "shell": { "type": "string", "enum": ["auto", "powershell", "cmd", "sh"], "description": "Shell to use on Windows; auto uses PowerShell" },
                "timeout_secs": { "type": "integer", "description": "Execution timeout in seconds (default: 120, max: 900)" }
            },
            "required": ["command"]
        })
    }
    fn risk_tier(&self) -> RiskTier {
        RiskTier::Tier2SystemMutating
    }
    async fn execute(&self, ctx: &ToolContext, args: Value) -> Result<Value> {
        let cmd_str = args
            .get("command")
            .and_then(|c| c.as_str())
            .ok_or_else(|| OdonError::Tool {
                tool: self.name().to_string(),
                message: "Missing 'command' parameter".to_string(),
            })?;
        let timeout_secs = args
            .get("timeout_secs")
            .and_then(Value::as_u64)
            .unwrap_or(120)
            .clamp(1, 900);
        let shell = args
            .get("shell")
            .and_then(Value::as_str)
            .unwrap_or("auto");
        let cwd = match args.get("cwd").and_then(Value::as_str) {
            Some(relative) if !relative.trim().is_empty() => {
                crate::workspace_path::resolve_existing(&ctx.workspace_root, relative)?
            }
            _ => ctx.workspace_root.clone().into(),
        };

        // Defense-in-depth: refuse catastrophic commands even if a caller
        // bypasses ToolRegistry.
        let policy = ctx.approval_engine.get_policy().await;
        if let Some(reason) = policy.command_forbidden_reason(cmd_str) {
            return Err(OdonError::Security(format!(
                "Command blocked by security policy: {reason}"
            )));
        }

        #[cfg(target_os = "windows")]
        let mut cmd = match shell {
            "cmd" => {
                let mut c = Command::new("cmd");
                c.arg("/C").arg(cmd_str);
                c
            }
            "powershell" | "auto" | _ => {
                let mut c = Command::new("powershell");
                c.arg("-NoProfile").arg("-Command").arg(cmd_str);
                c
            }
        };

        #[cfg(not(target_os = "windows"))]
        let mut cmd = {
            let mut c = Command::new("sh");
            c.arg("-c").arg(cmd_str);
            c
        };

        cmd.current_dir(&cwd);

        let started = std::time::Instant::now();
        let mut child = cmd
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::piped())
            .spawn()
            .map_err(|e| OdonError::Tool {
                tool: self.name().to_string(),
                message: format!("Failed to execute command: {e}"),
            })?;
        let stdout_pipe = child.stdout.take();
        let stderr_pipe = child.stderr.take();
        let stdout_task = tokio::spawn(async move {
            let mut buffer = Vec::new();
            if let Some(mut pipe) = stdout_pipe {
                let _ = tokio::io::AsyncReadExt::read_to_end(&mut pipe, &mut buffer).await;
            }
            buffer
        });
        let stderr_task = tokio::spawn(async move {
            let mut buffer = Vec::new();
            if let Some(mut pipe) = stderr_pipe {
                let _ = tokio::io::AsyncReadExt::read_to_end(&mut pipe, &mut buffer).await;
            }
            buffer
        });

        let status = match timeout(Duration::from_secs(timeout_secs), async {
            child.wait().await
        })
        .await
        {
            Ok(result) => result.map_err(|e| OdonError::Tool {
                tool: self.name().to_string(),
                message: format!("Failed to execute command: {e}"),
            })?,
            Err(_) => {
                let _ = child.kill().await;
                return Err(OdonError::Tool {
                    tool: self.name().to_string(),
                    message: format!("Command timed out after {timeout_secs} seconds"),
                });
            }
        };
        let output = std::process::Output {
            status,
            stdout: stdout_task.await.unwrap_or_default(),
            stderr: stderr_task.await.unwrap_or_default(),
        };

        let (stdout, stdout_truncated) = clip(&output.stdout);
        let (stderr, stderr_truncated) = clip(&output.stderr);

        Ok(json!({
            "exit_code": output.status.code().unwrap_or(-1),
            "stdout": stdout,
            "stderr": stderr,
            "success": output.status.success(),
            "duration_ms": started.elapsed().as_millis(),
            "timeout_secs": timeout_secs,
            "cwd": cwd,
            "truncated": stdout_truncated || stderr_truncated
        }))
    }
}

pub struct BashRunTool;

#[async_trait]
impl Tool for BashRunTool {
    fn name(&self) -> &str {
        "bash_run"
    }
    fn description(&self) -> &str {
        "Compatibility wrapper for shell_exec with optional timeout (Termigo compatible)"
    }
    fn parameters_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "command": { "type": "string", "description": "Shell command to run" },
                "timeout_secs": { "type": "integer", "description": "Execution timeout in seconds (default: 120)" }
            },
            "required": ["command"]
        })
    }
    fn risk_tier(&self) -> RiskTier {
        RiskTier::Tier2SystemMutating
    }
    async fn execute(&self, ctx: &ToolContext, args: Value) -> Result<Value> {
        ShellExecTool.execute(ctx, args).await
    }
}

pub struct TerminalExecTool;

#[async_trait]
impl Tool for TerminalExecTool {
    fn name(&self) -> &str {
        "terminal_exec"
    }
    fn description(&self) -> &str {
        "Compatibility wrapper for shell_exec in the active workspace"
    }
    fn parameters_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "command": { "type": "string", "description": "Command to run" }
            },
            "required": ["command"]
        })
    }
    fn risk_tier(&self) -> RiskTier {
        RiskTier::Tier2SystemMutating
    }
    async fn execute(&self, ctx: &ToolContext, args: Value) -> Result<Value> {
        ShellExecTool.execute(ctx, args).await
    }
}
