use crate::registry::{Tool, ToolContext};
use crate::shell_ext::{background_tasks, BackgroundTask, MAX_BACKGROUND_BUFFER, RUNNING_EXIT_CODE};
use async_trait::async_trait;
use odon_core::error::{OdonError, Result};
use odon_security::RiskTier;
use serde_json::{json, Value};
use std::sync::atomic::Ordering;
use std::sync::Arc;
use tokio::io::AsyncReadExt;

pub struct DevServerStartTool;

#[async_trait]
impl Tool for DevServerStartTool {
    fn name(&self) -> &str { "dev_server_start" }
    fn description(&self) -> &str {
        "Start a dev server command in the background and return a task id you can use with dev_server_logs and dev_server_stop"
    }
    fn parameters_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "command": { "type": "string", "description": "Dev server command, for example 'pnpm dev' or 'npm run dev'" },
                "task_name": { "type": "string", "description": "Optional human readable name for this dev server" }
            },
            "required": ["command"]
        })
    }
    fn risk_tier(&self) -> RiskTier { RiskTier::Tier2SystemMutating }

    async fn execute(&self, ctx: &ToolContext, args: Value) -> Result<Value> {
        let cmd_str = args.get("command").and_then(|c| c.as_str()).unwrap_or("");
        if cmd_str.trim().is_empty() {
            return Err(OdonError::Tool {
                tool: self.name().to_string(),
                message: "Missing 'command' argument".to_string(),
            });
        }

        let task_name = args
            .get("task_name")
            .and_then(|value| value.as_str())
            .unwrap_or("");

        let task_id = format!(
            "dev_server_{}_{}",
            chrono::Utc::now().timestamp_millis(),
            uuid::Uuid::new_v4().simple()
        );

        let mut cmd = if cfg!(target_os = "windows") {
            let mut command = tokio::process::Command::new("powershell.exe");
            command.args(["-NoProfile", "-Command", cmd_str]);
            command
        } else {
            let mut command = tokio::process::Command::new("sh");
            command.args(["-c", cmd_str]);
            command
        };

        cmd.current_dir(&ctx.workspace_root)
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::piped());

        let mut spawned = cmd.spawn().map_err(|e| OdonError::Tool {
            tool: self.name().to_string(),
            message: e.to_string(),
        })?;

        let pid = spawned.id().unwrap_or(0);
        let stdout = spawned.stdout.take();
        let stderr = spawned.stderr.take();

        let task = Arc::new(BackgroundTask {
            child: tokio::sync::Mutex::new(Some(spawned)),
            stdout: std::sync::Mutex::new(String::new()),
            stderr: std::sync::Mutex::new(String::new()),
            exit_code: std::sync::atomic::AtomicI32::new(RUNNING_EXIT_CODE),
            pid,
            command: cmd_str.to_string(),
            started_at: chrono::Utc::now().to_rfc3339(),
        });

        if let Some(mut out) = stdout {
            let task_clone = task.clone();
            tokio::spawn(async move {
                let mut buffer = [0u8; 4096];
                loop {
                    match out.read(&mut buffer).await {
                        Ok(0) => break,
                        Ok(n) => {
                            if let Ok(mut guard) = task_clone.stdout.lock() {
                                guard.push_str(&String::from_utf8_lossy(&buffer[..n]));
                                if guard.len() > MAX_BACKGROUND_BUFFER {
                                    let len = guard.len();
                                    guard.replace_range(len - 2000.., "");
                                }
                            }
                        }
                        Err(_) => break,
                    }
                }
            });
        }

        if let Some(mut err) = stderr {
            let task_clone = task.clone();
            tokio::spawn(async move {
                let mut buffer = [0u8; 4096];
                loop {
                    match err.read(&mut buffer).await {
                        Ok(0) => break,
                        Ok(n) => {
                            if let Ok(mut guard) = task_clone.stderr.lock() {
                                guard.push_str(&String::from_utf8_lossy(&buffer[..n]));
                                if guard.len() > MAX_BACKGROUND_BUFFER {
                                    let len = guard.len();
                                    guard.replace_range(len - 2000.., "");
                                }
                            }
                        }
                        Err(_) => break,
                    }
                }
            });
        }

        let mut tasks = background_tasks().lock().await;
        tasks.insert(task_id.clone(), task);

        Ok(json!({
            "task_id": task_id,
            "pid": pid,
            "command": cmd_str,
            "task_name": task_name,
            "status": "running",
            "poll_with": "dev_server_logs"
        }))
    }
}

pub struct DevServerStopTool;

#[async_trait]
impl Tool for DevServerStopTool {
    fn name(&self) -> &str { "dev_server_stop" }
    fn description(&self) -> &str {
        "Stop a running dev server by task id"
    }
    fn parameters_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "task_id": { "type": "string", "description": "Task ID returned by dev_server_start" }
            },
            "required": ["task_id"]
        })
    }
    fn risk_tier(&self) -> RiskTier { RiskTier::Tier2SystemMutating }

    async fn execute(&self, _ctx: &ToolContext, args: Value) -> Result<Value> {
        let task_id = args.get("task_id").and_then(|value| value.as_str()).ok_or_else(|| OdonError::Tool {
            tool: self.name().to_string(),
            message: "Missing 'task_id' argument".to_string(),
        })?;

        let tasks = background_tasks().lock().await;
        let task = tasks.get(task_id).cloned().ok_or_else(|| OdonError::Tool {
            tool: self.name().to_string(),
            message: format!("Task '{task_id}' not found"),
        })?;
        drop(tasks);

        let mut child_guard = task.child.lock().await;
        if let Some(mut child) = child_guard.take() {
            let killed = child.kill().await.is_ok();
            task.exit_code.store(if killed { 0 } else { -1 }, Ordering::SeqCst);
            Ok(json!({ "task_id": task_id, "pid": task.pid, "killed": killed }))
        } else {
            Ok(json!({ "task_id": task_id, "pid": task.pid, "killed": false, "reason": "already exited" }))
        }
    }
}

pub struct DevServerLogsTool;

#[async_trait]
impl Tool for DevServerLogsTool {
    fn name(&self) -> &str { "dev_server_logs" }
    fn description(&self) -> &str {
        "Read the latest stdout/stderr logs from a running dev server task"
    }
    fn parameters_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "task_id": { "type": "string", "description": "Task ID returned by dev_server_start" },
                "tail": { "type": "integer", "description": "Return only the last N characters (default 8000)" },
                "clear": { "type": "boolean", "description": "Clear the returned buffer after reading" }
            },
            "required": ["task_id"]
        })
    }
    fn risk_tier(&self) -> RiskTier { RiskTier::Tier0ReadOnly }

    async fn execute(&self, _ctx: &ToolContext, args: Value) -> Result<Value> {
        let task_id = args.get("task_id").and_then(|value| value.as_str()).ok_or_else(|| OdonError::Tool {
            tool: self.name().to_string(),
            message: "Missing 'task_id' argument".to_string(),
        })?;

        let tasks = background_tasks().lock().await;
        let task = tasks.get(task_id).cloned().ok_or_else(|| OdonError::Tool {
            tool: self.name().to_string(),
            message: format!("Task '{task_id}' not found"),
        })?;
        drop(tasks);

        let stdout = task.stdout.lock().map(|guard| guard.clone()).unwrap_or_default();
        let stderr = task.stderr.lock().map(|guard| guard.clone()).unwrap_or_default();
        let exit_code = task.exit_code.load(Ordering::SeqCst);
        let finished = exit_code != RUNNING_EXIT_CODE;

        let tail = args.get("tail").and_then(|value| value.as_u64()).unwrap_or(8000) as usize;
        let truncate = |text: String| if text.len() > tail { text[text.len() - tail..].to_string() } else { text };

        Ok(json!({
            "task_id": task_id,
            "pid": task.pid,
            "command": task.command,
            "finished": finished,
            "exit_code": if finished { Some(exit_code) } else { None },
            "stdout": truncate(stdout),
            "stderr": truncate(stderr)
        }))
    }
}
