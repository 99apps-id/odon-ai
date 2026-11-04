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
                "shell": { "type": "string", "enum": ["auto", "powershell", "cmd", "sh"], "description": "Shell to use: auto picks the platform default (PowerShell on Windows, sh on Linux/macOS); powershell and cmd only exist on Windows" },
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
        let shell = args.get("shell").and_then(Value::as_str).unwrap_or("auto");
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
            _ => {
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
                // Kill the whole tree, not just the shell wrapper. On Windows
                // `powershell.exe` spawns grandchildren that keep the inherited
                // pipes open, so a plain `kill()` leaves `read_to_end` blocked
                // forever and the harness reports a timeout with no output.
                kill_process_tree(&mut child).await;
                // Collect whatever the command already produced before the
                // deadline. Losing it turns a slow build into an opaque error.
                let partial_stdout = collect_partial(stdout_task).await;
                let partial_stderr = collect_partial(stderr_task).await;
                let (stdout, stdout_truncated) = clip(&partial_stdout);
                let (stderr, stderr_truncated) = clip(&partial_stderr);
                return Ok(json!({
                    "exit_code": -1,
                    "stdout": stdout,
                    "stderr": stderr,
                    "success": false,
                    "timed_out": true,
                    "duration_ms": started.elapsed().as_millis(),
                    "timeout_secs": timeout_secs,
                    "cwd": cwd,
                    "truncated": stdout_truncated || stderr_truncated,
                    "note": format!(
                        "Command killed after {timeout_secs}s. Partial output above. \
                         Raise timeout_secs (max 900) for long builds."
                    )
                }));
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

/// Terminate a command together with every process it spawned.
///
/// `Child::kill` only signals the direct child. A shell that launched a build
/// leaves grandchildren behind, holding the stdout/stderr pipes open; reading
/// them then never finishes.
async fn kill_process_tree(child: &mut tokio::process::Child) {
    #[cfg(target_os = "windows")]
    {
        if let Some(pid) = child.id() {
            let _ = tokio::process::Command::new("taskkill")
                .args(["/PID".to_string(), pid.to_string(), "/T".to_string(), "/F".to_string()])
                .stdout(std::process::Stdio::null())
                .stderr(std::process::Stdio::null())
                .status()
                .await;
        }
    }
    let _ = child.kill().await;
}

/// Wait a bounded time for a pipe reader to finish, then keep what it has.
///
/// After a tree kill the readers normally finish at once, but a stubborn
/// grandchild on Windows can keep a handle alive. Never let that hold the
/// turn: give up after a short grace period and report no partial output.
async fn collect_partial(handle: tokio::task::JoinHandle<Vec<u8>>) -> Vec<u8> {
    match tokio::time::timeout(Duration::from_secs(5), handle).await {
        Ok(Ok(buffer)) => buffer,
        _ => Vec::new(),
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

#[cfg(test)]
mod tests {
    use super::*;
    use odon_security::{ApprovalEngine, AuditLogger, SecurityPolicy};
    use std::sync::Arc;

    fn test_context(root: &str) -> crate::registry::ToolContext {
        crate::registry::ToolContext {
            session_id: "shell-test".to_string(),
            workspace_root: root.to_string(),
            approval_engine: Arc::new(ApprovalEngine::new(SecurityPolicy::default())),
            audit_logger: Arc::new(AuditLogger::new()),
            storage: None,
        }
    }

    /// A command that prints, then sleeps past the deadline. The partial output
    /// must survive the timeout instead of being replaced by a bare error.
    #[cfg(target_os = "windows")]
    #[tokio::test]
    async fn timeout_keeps_partial_output_and_returns_promptly() {
        let root = std::env::temp_dir().to_string_lossy().to_string();
        let ctx = test_context(&root);
        let started = std::time::Instant::now();
        let result = ShellExecTool
            .execute(
                &ctx,
                json!({
                    "command": "echo PARTIAL_MARKER && ping -n 20 127.0.0.1 > nul",
                    "shell": "cmd",
                    "timeout_secs": 2
                }),
            )
            .await
            .expect("timeout must be reported as data, not as a hard error");
        let elapsed = started.elapsed();

        assert_eq!(result["timed_out"], json!(true), "timed_out flag missing: {result}");
        assert_eq!(result["success"], json!(false));
        let stdout = result["stdout"].as_str().unwrap_or_default();
        assert!(
            stdout.contains("PARTIAL_MARKER"),
            "partial output was discarded: {stdout:?}"
        );
        assert!(
            elapsed < std::time::Duration::from_secs(20),
            "turn was held for {elapsed:?}; the process tree was not killed"
        );
    }

    /// A command that finishes inside the deadline must not be marked timed out.
    #[tokio::test]
    async fn normal_command_is_not_marked_timed_out() {
        let root = std::env::temp_dir().to_string_lossy().to_string();
        let ctx = test_context(&root);
        #[cfg(target_os = "windows")]
        let args = json!({ "command": "echo OK", "shell": "cmd", "timeout_secs": 30 });
        #[cfg(not(target_os = "windows"))]
        let args = json!({ "command": "echo OK", "timeout_secs": 30 });

        let result = ShellExecTool.execute(&ctx, args).await.expect("echo must succeed");
        assert_eq!(result["timed_out"], serde_json::Value::Null, "unexpected timeout flag: {result}");
        assert_eq!(result["success"], json!(true));
        assert!(result["stdout"].as_str().unwrap_or_default().contains("OK"));
    }
}
