use async_trait::async_trait;
use odon_core::{OdonError, Result};
use odon_security::RiskTier;
use serde_json::{json, Value};
use tokio::process::Command;

use crate::registry::{Tool, ToolContext};

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
                "command": { "type": "string", "description": "Command string to run" }
            },
            "required": ["command"]
        })
    }
    fn risk_tier(&self) -> RiskTier {
        RiskTier::Tier2SystemMutating
    }
    async fn execute(&self, ctx: &ToolContext, args: Value) -> Result<Value> {
        let cmd_str = args.get("command").and_then(|c| c.as_str()).ok_or_else(|| {
            OdonError::Tool {
                tool: self.name().to_string(),
                message: "Missing 'command' parameter".to_string(),
            }
        })?;

        #[cfg(target_os = "windows")]
        let mut cmd = {
            let mut c = Command::new("powershell");
            c.arg("-NoProfile").arg("-Command").arg(cmd_str);
            c
        };

        #[cfg(not(target_os = "windows"))]
        let mut cmd = {
            let mut c = Command::new("sh");
            c.arg("-c").arg(cmd_str);
            c
        };

        cmd.current_dir(&ctx.workspace_root);

        let output = cmd.output().await.map_err(|e| OdonError::Tool {
            tool: self.name().to_string(),
            message: format!("Failed to execute command: {}", e),
        })?;

        let stdout = String::from_utf8_lossy(&output.stdout).to_string();
        let stderr = String::from_utf8_lossy(&output.stderr).to_string();

        Ok(json!({
            "exit_code": output.status.code().unwrap_or(-1),
            "stdout": stdout,
            "stderr": stderr,
            "success": output.status.success()
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
        "Run a foreground shell command with optional timeout (Termigo compatible)"
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
        "Execute a terminal command directly within the active workspace"
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

