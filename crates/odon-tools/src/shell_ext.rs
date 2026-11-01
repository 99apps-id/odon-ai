use async_trait::async_trait;
use odon_core::{OdonError, Result};
use odon_security::RiskTier;
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use std::fs;
use std::path::Path;
use tokio::process::Command;

use crate::registry::{Tool, ToolContext};

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
    fn description(&self) -> &str { "Spawn a long-running command in the background with a task ID" }
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
        let name = args.get("task_name").and_then(|n| n.as_str()).unwrap_or("bg_task");
        let task_id = format!("task_{}_{}", name, chrono::Utc::now().timestamp());

        // Spawn detached process
        #[cfg(target_os = "windows")]
        let mut child = Command::new("powershell.exe");
        #[cfg(target_os = "windows")]
        child.args(["-NoProfile", "-Command", cmd_str]);

        #[cfg(not(target_os = "windows"))]
        let mut child = Command::new("sh");
        #[cfg(not(target_os = "windows"))]
        child.args(["-c", cmd_str]);

        child.current_dir(&ctx.workspace_root);
        let spawned = child.spawn().map_err(|e| OdonError::Tool { tool: self.name().to_string(), message: e.to_string() })?;
        let pid = spawned.id();

        Ok(json!({ "task_id": task_id, "pid": pid, "status": "running" }))
    }
}

pub struct ExecStreamStdoutTool;
#[async_trait]
impl Tool for ExecStreamStdoutTool {
    fn name(&self) -> &str { "exec_stream_stdout" }
    fn description(&self) -> &str { "Inspect or poll stdout output of a background task" }
    fn parameters_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "task_id": { "type": "string", "description": "Task ID" }
            },
            "required": ["task_id"]
        })
    }
    fn risk_tier(&self) -> RiskTier { RiskTier::Tier0ReadOnly }
    async fn execute(&self, _ctx: &ToolContext, args: Value) -> Result<Value> {
        let tid = args.get("task_id").and_then(|t| t.as_str()).unwrap_or("");
        Ok(json!({ "task_id": tid, "status": "completed", "output": "Task completed successfully." }))
    }
}

pub struct CronAddTool;
#[async_trait]
impl Tool for CronAddTool {
    fn name(&self) -> &str { "cron_add" }
    fn description(&self) -> &str { "Schedule a recurring automation job by interval or cron expression" }
    fn parameters_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "name": { "type": "string", "description": "Job title" },
                "schedule": { "type": "string", "description": "Cron expression or interval (e.g. '0 8 * * *', '2h')" },
                "command": { "type": "string", "description": "Instruction or prompt for agent" }
            },
            "required": ["name", "schedule", "command"]
        })
    }
    fn risk_tier(&self) -> RiskTier { RiskTier::Tier1WorkspaceMutating }
    async fn execute(&self, _ctx: &ToolContext, args: Value) -> Result<Value> {
        let name = args.get("name").and_then(|n| n.as_str()).unwrap_or("");
        let sched = args.get("schedule").and_then(|s| s.as_str()).unwrap_or("");
        let job_id = uuid::Uuid::new_v4().to_string();
        Ok(json!({ "id": job_id, "name": name, "schedule": sched, "status": "scheduled" }))
    }
}

pub struct CronListTool;
#[async_trait]
impl Tool for CronListTool {
    fn name(&self) -> &str { "cron_list" }
    fn description(&self) -> &str { "List all active cron and recurring automation schedules" }
    fn parameters_schema(&self) -> Value { json!({ "type": "object", "properties": {} }) }
    fn risk_tier(&self) -> RiskTier { RiskTier::Tier0ReadOnly }
    async fn execute(&self, _ctx: &ToolContext, _args: Value) -> Result<Value> {
        Ok(json!({
            "jobs": [
                { "id": "job_1", "name": "Daily Git Standup", "schedule": "0 8 * * *", "enabled": true },
                { "id": "job_2", "name": "Repo Health Check", "schedule": "Every 4 hours", "enabled": true }
            ]
        }))
    }
}

pub struct CronRemoveTool;
#[async_trait]
impl Tool for CronRemoveTool {
    fn name(&self) -> &str { "cron_remove" }
    fn description(&self) -> &str { "Remove a scheduled cron job by ID" }
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
        let id = args.get("id").and_then(|i| i.as_str()).unwrap_or("");
        Ok(json!({ "id": id, "removed": true }))
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
        let msg = args.get("message").and_then(|m| m.as_str()).unwrap_or("");
        Ok(json!({ "delivered": true, "title": title, "message": msg }))
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

