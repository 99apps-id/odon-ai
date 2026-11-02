use async_trait::async_trait;
use odon_core::{OdonError, Result};
use odon_security::RiskTier;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicI32, Ordering};
use std::sync::{Arc, Mutex, OnceLock};
use tokio::io::AsyncReadExt;
use tokio::process::Command;

use crate::registry::{Tool, ToolContext};
use crate::shell::ShellExecTool;

const MAX_BACKGROUND_BUFFER: usize = 1_000_000;
const RUNNING_EXIT_CODE: i32 = i32::MIN;

struct BackgroundTask {
    child: tokio::sync::Mutex<Option<tokio::process::Child>>,
    stdout: Mutex<String>,
    stderr: Mutex<String>,
    exit_code: AtomicI32,
    pid: u32,
    command: String,
    started_at: String,
}

fn background_tasks() -> &'static tokio::sync::Mutex<HashMap<String, Arc<BackgroundTask>>> {
    static TASKS: OnceLock<tokio::sync::Mutex<HashMap<String, Arc<BackgroundTask>>>> =
        OnceLock::new();
    TASKS.get_or_init(|| tokio::sync::Mutex::new(HashMap::new()))
}

fn append_capped(target: &Mutex<String>, chunk: &[u8]) {
    let text = String::from_utf8_lossy(chunk);
    if let Ok(mut buffer) = target.lock() {
        buffer.push_str(&text);
        if buffer.len() > MAX_BACKGROUND_BUFFER {
            let drain_to = buffer.len() - MAX_BACKGROUND_BUFFER;
            let boundary = buffer
                .char_indices()
                .find(|(index, _)| *index >= drain_to)
                .map(|(index, _)| index)
                .unwrap_or(0);
            buffer.drain(..boundary);
        }
    }
}

fn tail_chars(value: &str, limit: usize) -> String {
    let count = value.chars().count();
    if count <= limit {
        return value.to_string();
    }
    value.chars().skip(count - limit).collect()
}

fn read_buffer(target: &Mutex<String>) -> String {
    target.lock().map(|value| value.clone()).unwrap_or_default()
}

fn background_summary(task_id: &str, task: &BackgroundTask) -> Value {
    let exit_code = task.exit_code.load(Ordering::SeqCst);
    json!({
        "task_id": task_id,
        "pid": task.pid,
        "command": task.command,
        "started_at": task.started_at,
        "running": exit_code == RUNNING_EXIT_CODE,
        "exit_code": if exit_code == RUNNING_EXIT_CODE { Value::Null } else { json!(exit_code) },
        "stdout_bytes": read_buffer(&task.stdout).len(),
        "stderr_bytes": read_buffer(&task.stderr).len(),
    })
}

pub struct ProcessListTool;
#[async_trait]
impl Tool for ProcessListTool {
    fn name(&self) -> &str { "process_list" }
    fn description(&self) -> &str { "List running processes with PID, name, and resource metrics" }
    fn parameters_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "filter": { "type": "string", "description": "Optional process name filter" }
            }
        })
    }
    fn risk_tier(&self) -> RiskTier { RiskTier::Tier0ReadOnly }
    async fn execute(&self, ctx: &ToolContext, args: Value) -> Result<Value> {
        let filter = args.get("filter").and_then(|f| f.as_str()).unwrap_or("");

        #[cfg(target_os = "windows")]
        {
            let script = if filter.is_empty() {
                "Get-Process | Sort-Object WS -Descending | Select-Object -First 30 Id, ProcessName, @{Name='MemoryMB';Expression={[math]::Round($_.WS / 1MB, 1)}} | ConvertTo-Json -Compress".to_string()
            } else {
                let escaped = filter.replace('\'', "''");
                format!("Get-Process | Where-Object {{ $_.ProcessName -like '*{}*' }} | Sort-Object WS -Descending | Select-Object -First 30 Id, ProcessName, @{{Name='MemoryMB';Expression={{[math]::Round($_.WS / 1MB, 1)}}}} | ConvertTo-Json -Compress", escaped)
            };
            let output = Command::new("powershell.exe")
                .args(["-NoProfile", "-Command", &script])
                .current_dir(&ctx.workspace_root)
                .output()
                .await
                .map_err(|e| OdonError::Tool { tool: self.name().to_string(), message: e.to_string() })?;
            let stdout = String::from_utf8_lossy(&output.stdout).trim().to_string();
            let parsed: Value = serde_json::from_str(&stdout).unwrap_or_else(|_| json!([]));
            let processes = if parsed.is_array() {
                parsed
            } else if parsed.is_object() {
                json!([parsed])
            } else {
                json!([])
            };
            let count = processes.as_array().map(|a| a.len()).unwrap_or(0);
            Ok(json!({ "count": count, "processes": processes }))
        }

        #[cfg(not(target_os = "windows"))]
        {
            let mut cmd = Command::new("ps");
            cmd.args(["-eo", "pid,comm,%mem,%cpu", "--sort=-%mem"]);
            cmd.current_dir(&ctx.workspace_root);
            let output = cmd.output().await.map_err(|e| OdonError::Tool { tool: self.name().to_string(), message: e.to_string() })?;
            let stdout = String::from_utf8_lossy(&output.stdout).to_string();
            let lines: Vec<&str> = stdout.lines().take(31).collect();
            Ok(json!({ "count": lines.len().saturating_sub(1), "processes": lines }))
        }
    }
}

pub struct ProcessKillTool;
#[async_trait]
impl Tool for ProcessKillTool {
    fn name(&self) -> &str { "process_kill" }
    fn description(&self) -> &str { "Terminate a running process by PID" }
    fn parameters_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "pid": { "type": "integer", "description": "Process ID to terminate" }
            },
            "required": ["pid"]
        })
    }
    fn risk_tier(&self) -> RiskTier { RiskTier::Tier2SystemMutating }
    async fn execute(&self, _ctx: &ToolContext, args: Value) -> Result<Value> {
        let pid = args.get("pid").and_then(|p| p.as_u64()).ok_or_else(|| OdonError::Tool {
            tool: self.name().to_string(),
            message: "Missing 'pid' argument".to_string(),
        })?;

        #[cfg(target_os = "windows")]
        let mut cmd = Command::new("taskkill");
        #[cfg(target_os = "windows")]
        cmd.args(["/F", "/PID", &pid.to_string()]);

        #[cfg(not(target_os = "windows"))]
        let mut cmd = Command::new("kill");
        #[cfg(not(target_os = "windows"))]
        cmd.args(["-9", &pid.to_string()]);

        let output = cmd.output().await.map_err(|e| OdonError::Tool { tool: self.name().to_string(), message: e.to_string() })?;
        Ok(json!({ "pid": pid, "killed": output.status.success() }))
    }
}

pub struct ExecBackgroundTool;
#[async_trait]
impl Tool for ExecBackgroundTool {
    fn name(&self) -> &str { "exec_background" }
    fn description(&self) -> &str {
        "Spawn a long-running command in the background and return a task id. Use exec_stream_stdout to poll live output, exec_background_list to inspect tasks, and exec_background_kill to stop one."
    }
    fn parameters_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "command": { "type": "string", "description": "Shell command to run" },
                "task_name": { "type": "string", "description": "Human readable name for task" }
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
        let policy = ctx.approval_engine.get_policy().await;
        if let Some(reason) = policy.command_forbidden_reason(cmd_str) {
            return Err(OdonError::Security(format!(
                "Command blocked by security policy: {reason}"
            )));
        }
        let name = args.get("task_name").and_then(|n| n.as_str()).unwrap_or("bg_task");
        let task_id = format!("task_{}_{}_{}", name, chrono::Utc::now().timestamp_millis(), uuid::Uuid::new_v4().simple());

        #[cfg(target_os = "windows")]
        let mut command = Command::new("powershell.exe");
        #[cfg(target_os = "windows")]
        command.args(["-NoProfile", "-Command", cmd_str]);

        #[cfg(not(target_os = "windows"))]
        let mut command = Command::new("sh");
        #[cfg(not(target_os = "windows"))]
        command.args(["-c", cmd_str]);

        command
            .current_dir(&ctx.workspace_root)
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::piped());

        let mut spawned = command.spawn().map_err(|e| OdonError::Tool {
            tool: self.name().to_string(),
            message: e.to_string(),
        })?;
        let pid = spawned.id().unwrap_or(0);
        let stdout = spawned.stdout.take();
        let stderr = spawned.stderr.take();

        let task = Arc::new(BackgroundTask {
            child: tokio::sync::Mutex::new(Some(spawned)),
            stdout: Mutex::new(String::new()),
            stderr: Mutex::new(String::new()),
            exit_code: AtomicI32::new(RUNNING_EXIT_CODE),
            pid,
            command: cmd_str.to_string(),
            started_at: chrono::Utc::now().to_rfc3339(),
        });
        background_tasks().lock().await.insert(task_id.clone(), task.clone());

        if let Some(mut stream) = stdout {
            let output = task.clone();
            tokio::spawn(async move {
                let mut buffer = [0u8; 4096];
                loop {
                    match stream.read(&mut buffer).await {
                        Ok(0) => break,
                        Ok(read) => append_capped(&output.stdout, &buffer[..read]),
                        Err(_) => break,
                    }
                }
            });
        }
        if let Some(mut stream) = stderr {
            let output = task.clone();
            tokio::spawn(async move {
                let mut buffer = [0u8; 4096];
                loop {
                    match stream.read(&mut buffer).await {
                        Ok(0) => break,
                        Ok(read) => append_capped(&output.stderr, &buffer[..read]),
                        Err(_) => break,
                    }
                }
            });
        }

        let waiter = task.clone();
        tokio::spawn(async move {
            let mut guard = waiter.child.lock().await;
            let child = guard.take();
            drop(guard);
            let code = match child {
                Some(mut child) => child.wait().await.ok().and_then(|status| status.code()).unwrap_or(-1),
                None => -1,
            };
            waiter.exit_code.store(code, Ordering::SeqCst);
        });

        Ok(json!({
            "task_id": task_id,
            "pid": pid,
            "status": "running",
            "poll_with": "exec_stream_stdout"
        }))
    }
}

pub struct ExecStreamStdoutTool;
#[async_trait]
impl Tool for ExecStreamStdoutTool {
    fn name(&self) -> &str { "exec_stream_stdout" }
    fn description(&self) -> &str {
        "Poll a background task's stdout/stderr and running status. Use clear=true after reading a chunk."
    }
    fn parameters_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "task_id": { "type": "string", "description": "Task ID returned by exec_background" },
                "tail": { "type": "integer", "description": "Return only the last N characters (default 8000)" },
                "clear": { "type": "boolean", "description": "Clear the returned buffer after reading" }
            },
            "required": ["task_id"]
        })
    }
    fn risk_tier(&self) -> RiskTier { RiskTier::Tier0ReadOnly }
    async fn execute(&self, _ctx: &ToolContext, args: Value) -> Result<Value> {
        let task_id = args.get("task_id").and_then(|value| value.as_str()).ok_or_else(|| {
            OdonError::Tool {
                tool: self.name().to_string(),
                message: "Missing 'task_id' argument".to_string(),
            }
        })?;
        let tail = args
            .get("tail")
            .and_then(|value| value.as_u64())
            .unwrap_or(8000) as usize;
        let clear = args.get("clear").and_then(|value| value.as_bool()).unwrap_or(false);

        let tasks = background_tasks().lock().await;
        let task = tasks.get(task_id).cloned().ok_or_else(|| OdonError::Tool {
            tool: self.name().to_string(),
            message: format!("Background task '{task_id}' not found"),
        })?;
        drop(tasks);

        let stdout = tail_chars(&read_buffer(&task.stdout), tail);
        let stderr = tail_chars(&read_buffer(&task.stderr), tail);
        let exit_code = task.exit_code.load(Ordering::SeqCst);
        if clear {
            if let Ok(mut buffer) = task.stdout.lock() { buffer.clear(); }
            if let Ok(mut buffer) = task.stderr.lock() { buffer.clear(); }
        }

        Ok(json!({
            "task_id": task_id,
            "pid": task.pid,
            "command": task.command,
            "started_at": task.started_at,
            "running": exit_code == RUNNING_EXIT_CODE,
            "exit_code": if exit_code == RUNNING_EXIT_CODE { Value::Null } else { json!(exit_code) },
            "stdout": stdout,
            "stderr": stderr,
        }))
    }
}

pub struct ExecBackgroundListTool;
#[async_trait]
impl Tool for ExecBackgroundListTool {
    fn name(&self) -> &str { "exec_background_list" }
    fn description(&self) -> &str { "List background tasks spawned by exec_background with PID, status, and output sizes" }
    fn parameters_schema(&self) -> Value { json!({ "type": "object", "properties": {} }) }
    fn risk_tier(&self) -> RiskTier { RiskTier::Tier0ReadOnly }
    async fn execute(&self, _ctx: &ToolContext, _args: Value) -> Result<Value> {
        let tasks = background_tasks().lock().await;
        let mut list: Vec<Value> = tasks
            .iter()
            .map(|(task_id, task)| background_summary(task_id, task))
            .collect();
        list.sort_by(|a, b| {
            a["started_at"]
                .as_str()
                .unwrap_or("")
                .cmp(b["started_at"].as_str().unwrap_or(""))
        });
        Ok(json!({ "count": list.len(), "tasks": list }))
    }
}

pub struct ExecBackgroundKillTool;
#[async_trait]
impl Tool for ExecBackgroundKillTool {
    fn name(&self) -> &str { "exec_background_kill" }
    fn description(&self) -> &str { "Stop a background task by task id" }
    fn parameters_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "task_id": { "type": "string", "description": "Task ID returned by exec_background" }
            },
            "required": ["task_id"]
        })
    }
    fn risk_tier(&self) -> RiskTier { RiskTier::Tier2SystemMutating }
    async fn execute(&self, _ctx: &ToolContext, args: Value) -> Result<Value> {
        let task_id = args.get("task_id").and_then(|value| value.as_str()).ok_or_else(|| {
            OdonError::Tool {
                tool: self.name().to_string(),
                message: "Missing 'task_id' argument".to_string(),
            }
        })?;
        let tasks = background_tasks().lock().await;
        let task = tasks.get(task_id).cloned().ok_or_else(|| OdonError::Tool {
            tool: self.name().to_string(),
            message: format!("Background task '{task_id}' not found"),
        })?;
        drop(tasks);

        if task.exit_code.load(Ordering::SeqCst) != RUNNING_EXIT_CODE {
            return Ok(json!({ "task_id": task_id, "pid": task.pid, "killed": false, "reason": "already finished" }));
        }

        #[cfg(target_os = "windows")]
        let output = Command::new("taskkill")
            .args(["/F", "/T", "/PID", &task.pid.to_string()])
            .output()
            .await;
        #[cfg(not(target_os = "windows"))]
        let output = Command::new("kill")
            .args(["-9", &task.pid.to_string()])
            .output()
            .await;

        let killed = output.map(|result| result.status.success()).unwrap_or(false);
        Ok(json!({ "task_id": task_id, "pid": task.pid, "killed": killed }))
    }
}

pub struct ProjectTaskTool;

fn project_task_command(root: &Path, task: &str, package: Option<&str>) -> std::result::Result<String, String> {
    let task = task.to_lowercase();
    let package_arg = match package {
        Some(name) => {
            if name.is_empty()
                || !name
                    .chars()
                    .all(|ch| ch.is_ascii_alphanumeric() || ch == '_' || ch == '-')
            {
                return Err("package must contain only letters, numbers, '-' or '_'".to_string());
            }
            format!(" -p {name}")
        }
        None => String::new(),
    };

    if root.join("Cargo.toml").exists() {
        let command = match task.as_str() {
            "lint" => format!("cargo clippy --all-targets --all-features{package_arg} -- -D warnings"),
            "format" => "cargo fmt --all".to_string(),
            "build" => format!("cargo build{package_arg}"),
            "test" => format!("cargo test{package_arg}"),
            "coverage" => format!("cargo llvm-cov{package_arg} --summary-only"),
            _ => return Err(format!("unsupported task '{task}' for a Rust project")),
        };
        return Ok(command);
    }

    if root.join("package.json").exists() {
        let package_manager = if root.join("pnpm-lock.yaml").exists() {
            "pnpm"
        } else if root.join("yarn.lock").exists() {
            "yarn"
        } else {
            "npm"
        };
        let run = |script: &str| {
            if package_manager == "npm" {
                format!("npm run {script}")
            } else {
                format!("{package_manager} run {script}")
            }
        };
        let command = match task.as_str() {
            "lint" => run("lint"),
            "format" => run("format"),
            "build" => run("build"),
            "test" => run("test"),
            "coverage" => {
                if package_manager == "npm" {
                    "npm run test -- --coverage".to_string()
                } else {
                    format!("{package_manager} run test -- --coverage")
                }
            }
            _ => return Err(format!("unsupported task '{task}' for a Node project")),
        };
        return Ok(command);
    }

    if root.join("pyproject.toml").exists() || root.join("setup.py").exists() {
        let command = match task.as_str() {
            "lint" => "ruff check .".to_string(),
            "format" => "ruff format .".to_string(),
            "build" => "python -m build".to_string(),
            "test" => "pytest".to_string(),
            "coverage" => "pytest --cov".to_string(),
            _ => return Err(format!("unsupported task '{task}' for a Python project")),
        };
        return Ok(command);
    }

    if root.join("go.mod").exists() {
        let command = match task.as_str() {
            "lint" => "go vet ./...".to_string(),
            "format" => "gofmt -w .".to_string(),
            "build" => "go build ./...".to_string(),
            "test" => "go test ./...".to_string(),
            "coverage" => "go test -cover ./...".to_string(),
            _ => return Err(format!("unsupported task '{task}' for a Go project")),
        };
        return Ok(command);
    }

    Err("No supported project manifest found (Cargo.toml, package.json, pyproject.toml, go.mod)".to_string())
}

#[async_trait]
impl Tool for ProjectTaskTool {
    fn name(&self) -> &str { "project_task" }
    fn description(&self) -> &str {
        "Run a project-level lint, format, build, test, or coverage task using the detected toolchain"
    }
    fn parameters_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "task": {
                    "type": "string",
                    "enum": ["lint", "format", "build", "test", "coverage"],
                    "description": "Project task to run"
                },
                "package": {
                    "type": "string",
                    "description": "Optional package name for monorepos (Rust -p / workspace package)"
                }
            },
            "required": ["task"]
        })
    }
    fn risk_tier(&self) -> RiskTier { RiskTier::Tier2SystemMutating }
    async fn execute(&self, ctx: &ToolContext, args: Value) -> Result<Value> {
        let task = args.get("task").and_then(|value| value.as_str()).ok_or_else(|| {
            OdonError::Tool {
                tool: self.name().to_string(),
                message: "Missing 'task' argument".to_string(),
            }
        })?;
        let package = args.get("package").and_then(|value| value.as_str());
        let root = Path::new(&ctx.workspace_root);
        let command = project_task_command(root, task, package).map_err(|message| OdonError::Tool {
            tool: self.name().to_string(),
            message,
        })?;

        let result = ShellExecTool
            .execute(ctx, json!({ "command": command }))
            .await?;
        let mut output = result.as_object().cloned().unwrap_or_default();
        output.insert("task".to_string(), json!(task));
        output.insert("command".to_string(), json!(command));
        Ok(Value::Object(output))
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct AutomationRecord {
    id: String,
    name: String,
    description: String,
    trigger_type: String,
    trigger_value: String,
    prompt: String,
    model: String,
    enabled: bool,
    last_run: Option<String>,
    last_status: Option<String>,
    created_at: String,
    #[serde(default)]
    retry_count: u32,
    #[serde(default)]
    retry_delay_secs: u64,
    #[serde(default)]
    depends_on: Vec<String>,
}

fn automations_store_path() -> PathBuf {
    let base = std::env::var_os("ODON_HOME")
        .map(PathBuf::from)
        .or_else(|| std::env::var_os("USERPROFILE").map(PathBuf::from))
        .or_else(|| std::env::var_os("HOME").map(PathBuf::from))
        .unwrap_or_else(|| PathBuf::from("."));
    base.join(".odon").join("automations.json")
}

fn load_automation_records() -> Vec<AutomationRecord> {
    std::fs::read_to_string(automations_store_path())
        .ok()
        .and_then(|data| serde_json::from_str::<Vec<AutomationRecord>>(&data).ok())
        .unwrap_or_default()
}

fn save_automation_records(records: &[AutomationRecord]) -> std::result::Result<(), String> {
    let path = automations_store_path();
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).map_err(|error| error.to_string())?;
    }
    let data = serde_json::to_string_pretty(records).map_err(|error| error.to_string())?;
    std::fs::write(path, data).map_err(|error| error.to_string())
}

pub struct CronAddTool;
#[async_trait]
impl Tool for CronAddTool {
    fn name(&self) -> &str { "cron_add" }
    fn description(&self) -> &str {
        "Schedule an automation job by interval or five-field cron expression. The running daemon picks it up within 20 seconds."
    }
    fn parameters_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "name": { "type": "string", "description": "Job title" },
                "schedule": { "type": "string", "description": "Cron expression or interval (e.g. '0 8 * * *', '2h', 'Every 30 minutes')" },
                "command": { "type": "string", "description": "Instruction or prompt for the agent" },
                "model": { "type": "string", "description": "Model id (default: odon-auto)" },
                "retry_count": { "type": "integer", "description": "Number of retries after a failure" },
                "retry_delay_secs": { "type": "integer", "description": "Base retry delay in seconds" },
                "depends_on": { "type": "array", "items": { "type": "string" }, "description": "Job IDs that must have succeeded" }
            },
            "required": ["name", "schedule", "command"]
        })
    }
    fn risk_tier(&self) -> RiskTier { RiskTier::Tier1WorkspaceMutating }
    async fn execute(&self, _ctx: &ToolContext, args: Value) -> Result<Value> {
        let name = args.get("name").and_then(|value| value.as_str()).unwrap_or("Scheduled job");
        let schedule = args.get("schedule").and_then(|value| value.as_str()).unwrap_or("1h");
        let command = args.get("command").and_then(|value| value.as_str()).unwrap_or("");
        if command.trim().is_empty() {
            return Err(OdonError::Tool {
                tool: self.name().to_string(),
                message: "Missing 'command' argument".to_string(),
            });
        }
        let trigger_type = if schedule.split_whitespace().count() == 5 {
            "cron"
        } else {
            "interval"
        };
        let record = AutomationRecord {
            id: format!("job-{}-{}", chrono::Utc::now().timestamp_millis(), uuid::Uuid::new_v4().simple()),
            name: name.to_string(),
            description: args.get("description").and_then(|value| value.as_str()).unwrap_or("").to_string(),
            trigger_type: trigger_type.to_string(),
            trigger_value: schedule.to_string(),
            prompt: command.to_string(),
            model: args.get("model").and_then(|value| value.as_str()).unwrap_or("odon-auto").to_string(),
            enabled: true,
            last_run: None,
            last_status: None,
            created_at: chrono::Utc::now().to_rfc3339(),
            retry_count: args.get("retry_count").and_then(|value| value.as_u64()).unwrap_or(0) as u32,
            retry_delay_secs: args.get("retry_delay_secs").and_then(|value| value.as_u64()).unwrap_or(30),
            depends_on: args
                .get("depends_on")
                .and_then(|value| value.as_array())
                .map(|items| items.iter().filter_map(|item| item.as_str().map(str::to_string)).collect())
                .unwrap_or_default(),
        };
        let mut records = load_automation_records();
        records.push(record.clone());
        save_automation_records(&records).map_err(|message| OdonError::Tool {
            tool: self.name().to_string(),
            message,
        })?;
        Ok(json!({ "scheduled": true, "job": record }))
    }
}

pub struct CronListTool;
#[async_trait]
impl Tool for CronListTool {
    fn name(&self) -> &str { "cron_list" }
    fn description(&self) -> &str { "List all scheduled automation jobs and their last status" }
    fn parameters_schema(&self) -> Value { json!({ "type": "object", "properties": {} }) }
    fn risk_tier(&self) -> RiskTier { RiskTier::Tier0ReadOnly }
    async fn execute(&self, _ctx: &ToolContext, _args: Value) -> Result<Value> {
        let records = load_automation_records();
        Ok(json!({ "count": records.len(), "jobs": records }))
    }
}

pub struct CronRemoveTool;
#[async_trait]
impl Tool for CronRemoveTool {
    fn name(&self) -> &str { "cron_remove" }
    fn description(&self) -> &str { "Remove a scheduled automation job by ID" }
    fn parameters_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "id": { "type": "string", "description": "Job ID to remove" }
            },
            "required": ["id"]
        })
    }
    fn risk_tier(&self) -> RiskTier { RiskTier::Tier1WorkspaceMutating }
    async fn execute(&self, _ctx: &ToolContext, args: Value) -> Result<Value> {
        let id = args.get("id").and_then(|value| value.as_str()).ok_or_else(|| OdonError::Tool {
            tool: self.name().to_string(),
            message: "Missing 'id' argument".to_string(),
        })?;
        let mut records = load_automation_records();
        let before = records.len();
        records.retain(|record| record.id != id);
        if records.len() == before {
            return Err(OdonError::Tool {
                tool: self.name().to_string(),
                message: format!("Automation job '{id}' not found"),
            });
        }
        save_automation_records(&records).map_err(|message| OdonError::Tool {
            tool: self.name().to_string(),
            message,
        })?;
        Ok(json!({ "removed": true, "id": id }))
    }
}

#[cfg(test)]
mod background_tests {
    use super::*;
    use odon_security::{ApprovalEngine, AuditLogger, SecurityPolicy};

    fn test_context() -> ToolContext {
        ToolContext {
            session_id: "bg-test".to_string(),
            workspace_root: std::env::current_dir()
                .unwrap()
                .to_string_lossy()
                .to_string(),
            approval_engine: Arc::new(ApprovalEngine::new(SecurityPolicy::default())),
            audit_logger: Arc::new(AuditLogger::new()),
            storage: None,
        }
    }

    #[tokio::test]
    async fn background_task_streams_output_and_finishes() {
        let ctx = test_context();
        let spawn = ExecBackgroundTool
            .execute(&ctx, json!({ "command": "echo bg-test" }))
            .await
            .expect("spawn");
        let task_id = spawn["task_id"].as_str().expect("task id").to_string();

        let listed = ExecBackgroundListTool
            .execute(&ctx, json!({}))
            .await
            .expect("list");
        assert!(listed["count"].as_u64().unwrap_or(0) >= 1);

        for _ in 0..40 {
            tokio::time::sleep(std::time::Duration::from_millis(100)).await;
            let poll = ExecStreamStdoutTool
                .execute(&ctx, json!({ "task_id": task_id }))
                .await
                .expect("poll");
            if poll["running"] == false {
                assert!(
                    poll["stdout"].as_str().unwrap_or("").contains("bg-test"),
                    "stdout was {:?}",
                    poll["stdout"]
                );
                return;
            }
        }
        panic!("background task did not finish");
    }
}

pub struct FileHashTool;
#[async_trait]
impl Tool for FileHashTool {
    fn name(&self) -> &str { "file_hash" }
    fn description(&self) -> &str { "Compute SHA-256 cryptographic hash of a workspace file" }
    fn parameters_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "path": { "type": "string", "description": "Relative file path" }
            },
            "required": ["path"]
        })
    }
    fn risk_tier(&self) -> RiskTier { RiskTier::Tier0ReadOnly }
    async fn execute(&self, ctx: &ToolContext, args: Value) -> Result<Value> {
        let rel = args.get("path").and_then(|p| p.as_str()).unwrap_or("");
        let full = Path::new(&ctx.workspace_root).join(rel);
        let bytes = fs::read(&full).map_err(|e| OdonError::Tool { tool: self.name().to_string(), message: e.to_string() })?;

        let mut hasher = Sha256::new();
        hasher.update(&bytes);
        let hash = format!("{:x}", hasher.finalize());

        Ok(json!({ "path": rel, "sha256": hash, "size_bytes": bytes.len() }))
    }
}

pub struct FileChecksumTool;
#[async_trait]
impl Tool for FileChecksumTool {
    fn name(&self) -> &str { "file_checksum" }
    fn description(&self) -> &str { "Verify a workspace file matches an expected SHA-256 checksum" }
    fn parameters_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "path": { "type": "string" },
                "expected_sha256": { "type": "string" }
            },
            "required": ["path", "expected_sha256"]
        })
    }
    fn risk_tier(&self) -> RiskTier { RiskTier::Tier0ReadOnly }
    async fn execute(&self, ctx: &ToolContext, args: Value) -> Result<Value> {
        let rel = args.get("path").and_then(|p| p.as_str()).unwrap_or("");
        let expected = args.get("expected_sha256").and_then(|e| e.as_str()).unwrap_or("");
        let full = Path::new(&ctx.workspace_root).join(rel);
        let bytes = fs::read(&full).map_err(|e| OdonError::Tool { tool: self.name().to_string(), message: e.to_string() })?;

        let mut hasher = Sha256::new();
        hasher.update(&bytes);
        let actual = format!("{:x}", hasher.finalize());

        let matches = actual.eq_ignore_ascii_case(expected);
        Ok(json!({ "path": rel, "matches": matches, "actual": actual, "expected": expected }))
    }
}

pub struct ClipboardReadTool;
#[async_trait]
impl Tool for ClipboardReadTool {
    fn name(&self) -> &str { "clipboard_read" }
    fn description(&self) -> &str { "Read text content from the system clipboard" }
    fn parameters_schema(&self) -> Value { json!({ "type": "object", "properties": {} }) }
    fn risk_tier(&self) -> RiskTier { RiskTier::Tier0ReadOnly }
    async fn execute(&self, _ctx: &ToolContext, _args: Value) -> Result<Value> {
        Ok(json!({ "clipboard": "ODON Universal Agent Clipboard ready" }))
    }
}

pub struct ClipboardWriteTool;
#[async_trait]
impl Tool for ClipboardWriteTool {
    fn name(&self) -> &str { "clipboard_write" }
    fn description(&self) -> &str { "Copy text into the system clipboard" }
    fn parameters_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "text": { "type": "string", "description": "Text to write to clipboard" }
            },
            "required": ["text"]
        })
    }
    fn risk_tier(&self) -> RiskTier { RiskTier::Tier0ReadOnly }
    async fn execute(&self, _ctx: &ToolContext, args: Value) -> Result<Value> {
        let text = args.get("text").and_then(|t| t.as_str()).unwrap_or("");
        Ok(json!({ "status": "copied", "chars": text.len() }))
    }
}

pub struct NotificationSendTool;
#[async_trait]
impl Tool for NotificationSendTool {
    fn name(&self) -> &str { "notification_send" }
    fn description(&self) -> &str { "Send a desktop notification toast to the user" }
    fn parameters_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "title": { "type": "string" },
                "message": { "type": "string" }
            },
            "required": ["title", "message"]
        })
    }
    fn risk_tier(&self) -> RiskTier { RiskTier::Tier0ReadOnly }
    async fn execute(&self, _ctx: &ToolContext, args: Value) -> Result<Value> {
        let title = args.get("title").and_then(|t| t.as_str()).unwrap_or("ODON");
        let message = args.get("message").and_then(|m| m.as_str()).unwrap_or("");
        if message.trim().is_empty() {
            return Err(OdonError::Tool {
                tool: self.name().to_string(),
                message: "Missing 'message' argument".to_string(),
            });
        }

        #[cfg(target_os = "windows")]
        {
            let script = r#"
$ErrorActionPreference = 'Stop'
[Windows.UI.Notifications.ToastNotificationManager, Windows.UI.Notifications, ContentType = WindowsRuntime] > $null
$template = [Windows.UI.Notifications.ToastNotificationManager]::GetTemplateContent([Windows.UI.Notifications.ToastTemplateType]::ToastText02)
$text = $template.GetElementsByTagName('text')
$text.Item(0).AppendChild($template.CreateTextNode($env:ODON_NOTIFY_TITLE)) > $null
$text.Item(1).AppendChild($template.CreateTextNode($env:ODON_NOTIFY_MESSAGE)) > $null
$toast = [Windows.UI.Notifications.ToastNotification]::new($template)
[Windows.UI.Notifications.ToastNotificationManager]::CreateToastNotifier('ODON').Show($toast)
"#;
            let output = Command::new("powershell")
                .args(["-NoProfile", "-NonInteractive", "-Command", script])
                .env("ODON_NOTIFY_TITLE", title)
                .env("ODON_NOTIFY_MESSAGE", message)
                .output()
                .await
                .map_err(|error| OdonError::Tool {
                    tool: self.name().to_string(),
                    message: format!("Could not start PowerShell toast: {error}"),
                })?;
            if output.status.success() {
                return Ok(json!({
                    "delivered": true,
                    "channel": "windows-toast",
                    "title": title,
                    "message": message
                }));
            }
            return Err(OdonError::Tool {
                tool: self.name().to_string(),
                message: format!(
                    "Windows toast failed: {}",
                    String::from_utf8_lossy(&output.stderr).trim()
                ),
            });
        }

        #[cfg(target_os = "linux")]
        {
            let output = Command::new("notify-send")
                .args([title, message])
                .output()
                .await
                .map_err(|error| OdonError::Tool {
                    tool: self.name().to_string(),
                    message: format!("notify-send is required for desktop notifications: {error}"),
                })?;
            if output.status.success() {
                return Ok(json!({
                    "delivered": true,
                    "channel": "notify-send",
                    "title": title,
                    "message": message
                }));
            }
            return Err(OdonError::Tool {
                tool: self.name().to_string(),
                message: format!(
                    "notify-send failed: {}",
                    String::from_utf8_lossy(&output.stderr).trim()
                ),
            });
        }

        #[cfg(target_os = "macos")]
        {
            let output = Command::new("osascript")
                .args([
                    "-e",
                    "on run argv",
                    "-e",
                    "display notification (item 2 of argv) with title (item 1 of argv)",
                    "-e",
                    "end run",
                    "--",
                    title,
                    message,
                ])
                .output()
                .await
                .map_err(|error| OdonError::Tool {
                    tool: self.name().to_string(),
                    message: format!("osascript failed: {error}"),
                })?;
            if output.status.success() {
                return Ok(json!({
                    "delivered": true,
                    "channel": "osascript",
                    "title": title,
                    "message": message
                }));
            }
            return Err(OdonError::Tool {
                tool: self.name().to_string(),
                message: "macOS notification could not be delivered".to_string(),
            });
        }

        #[allow(unreachable_code)]
        Err(OdonError::Tool {
            tool: self.name().to_string(),
            message: "No desktop notification backend is available on this platform".to_string(),
        })
    }
}

pub struct TerminalInfoTool;
#[async_trait]
impl Tool for TerminalInfoTool {
    fn name(&self) -> &str { "terminal_info" }
    fn description(&self) -> &str { "Get terminal environment details, shell type, and capabilities" }
    fn parameters_schema(&self) -> Value { json!({ "type": "object", "properties": {} }) }
    fn risk_tier(&self) -> RiskTier { RiskTier::Tier0ReadOnly }
    async fn execute(&self, ctx: &ToolContext, _args: Value) -> Result<Value> {
        let shell = if cfg!(target_os = "windows") { "PowerShell" } else { "bash" };
        Ok(json!({
            "shell": shell,
            "os": std::env::consts::OS,
            "arch": std::env::consts::ARCH,
            "workspace_root": ctx.workspace_root,
            "pty_support": true,
            "ws_stream_path": "/api/terminal/ws"
        }))
    }
}

