use async_trait::async_trait;
use odon_core::{OdonError, Result};
use odon_security::{ApprovalDecision, RiskTier};
use serde_json::{json, Value};
use std::fs;

use crate::mcp::{load_mcp_servers, save_mcp_servers, McpServerConfig};
use crate::registry::{Tool, ToolContext};

fn odon_home() -> std::path::PathBuf {
    std::env::var_os("ODON_HOME")
        .map(std::path::PathBuf::from)
        .or_else(|| std::env::var_os("USERPROFILE").map(std::path::PathBuf::from))
        .or_else(|| std::env::var_os("HOME").map(std::path::PathBuf::from))
        .unwrap_or_else(|| std::path::PathBuf::from("."))
}

fn daemon_base_url() -> String {
    std::env::var("ODON_DAEMON_URL").unwrap_or_else(|_| "http://127.0.0.1:4096".to_string())
}

// 1. Artifact Operations
pub struct ArtifactDeleteTool;
#[async_trait]
impl Tool for ArtifactDeleteTool {
    fn name(&self) -> &str {
        "artifact_delete"
    }
    fn description(&self) -> &str {
        "Delete an artifact by ID"
    }
    fn parameters_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "id": { "type": "string", "description": "Artifact ID to delete" }
            },
            "required": ["id"]
        })
    }
    fn risk_tier(&self) -> RiskTier {
        RiskTier::Tier1WorkspaceMutating
    }
    async fn execute(&self, ctx: &ToolContext, args: Value) -> Result<Value> {
        let id = args.get("id").and_then(|i| i.as_str()).unwrap_or("");
        if let Some(storage) = &ctx.storage {
            let deleted = storage.delete_artifact(id).unwrap_or(false);
            return Ok(json!({ "id": id, "deleted": deleted }));
        }
        Ok(json!({ "id": id, "deleted": true }))
    }
}

pub struct ArtifactExportTool;
#[async_trait]
impl Tool for ArtifactExportTool {
    fn name(&self) -> &str {
        "artifact_export"
    }
    fn description(&self) -> &str {
        "Export an artifact directly to a workspace file on disk"
    }
    fn parameters_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "artifact_id": { "type": "string" },
                "destination_path": { "type": "string" }
            },
            "required": ["artifact_id", "destination_path"]
        })
    }
    fn risk_tier(&self) -> RiskTier {
        RiskTier::Tier1WorkspaceMutating
    }
    async fn execute(&self, ctx: &ToolContext, args: Value) -> Result<Value> {
        let art_id = args
            .get("artifact_id")
            .and_then(|a| a.as_str())
            .unwrap_or("");
        let dest = args
            .get("destination_path")
            .and_then(|d| d.as_str())
            .unwrap_or("");

        if let Some(storage) = &ctx.storage {
            if let Ok(art) = storage.get_artifact(art_id) {
                let full = crate::workspace_path::resolve_write(&ctx.workspace_root, dest)?;
                if let Some(parent) = full.parent() {
                    let _ = fs::create_dir_all(parent);
                }
                let text_content = match &art.payload {
                    odon_core::ArtifactPayload::Document { markdown, .. } => markdown.clone(),
                    odon_core::ArtifactPayload::Code { files, .. } => files
                        .iter()
                        .map(|f| format!("// {}\n{}\n", f.path, f.content))
                        .collect::<Vec<_>>()
                        .join("\n"),
                    odon_core::ArtifactPayload::Patch { diff, .. } => diff.clone(),
                    _ => art.summary.clone(),
                };
                fs::write(&full, &text_content).map_err(|e| OdonError::Tool {
                    tool: self.name().to_string(),
                    message: e.to_string(),
                })?;
                return Ok(
                    json!({ "artifact_id": art_id, "exported_to": dest, "bytes": text_content.len() }),
                );
            }
        }
        Ok(json!({ "artifact_id": art_id, "exported_to": dest }))
    }
}

// 2. Approval Engine Operations
pub struct ApprovalListTool;
#[async_trait]
impl Tool for ApprovalListTool {
    fn name(&self) -> &str {
        "approval_list"
    }
    fn description(&self) -> &str {
        "List currently pending security approval requests requiring authorization"
    }
    fn parameters_schema(&self) -> Value {
        json!({ "type": "object", "properties": {} })
    }
    fn risk_tier(&self) -> RiskTier {
        RiskTier::Tier0ReadOnly
    }
    async fn execute(&self, ctx: &ToolContext, _args: Value) -> Result<Value> {
        let pending = ctx.approval_engine.list_pending().await;
        Ok(json!({ "total": pending.len(), "requests": pending }))
    }
}

pub struct ApprovalDecideTool;
#[async_trait]
impl Tool for ApprovalDecideTool {
    fn name(&self) -> &str {
        "approval_decide"
    }
    fn description(&self) -> &str {
        "Submit a decision (approve_once, approve_always, deny) for an approval request"
    }
    fn parameters_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "id": { "type": "string", "description": "Approval request ID" },
                "decision": { "type": "string", "enum": ["approved", "approve_once", "approved_always", "rejected", "deny"] },
                "reason": { "type": "string", "description": "Optional rejection reason" }
            },
            "required": ["id", "decision"]
        })
    }
    fn risk_tier(&self) -> RiskTier {
        RiskTier::Tier1WorkspaceMutating
    }
    async fn execute(&self, ctx: &ToolContext, args: Value) -> Result<Value> {
        let id = args.get("id").and_then(|i| i.as_str()).unwrap_or("");
        let dec_str = args
            .get("decision")
            .and_then(|d| d.as_str())
            .unwrap_or("approved");
        let dec = match dec_str.to_ascii_lowercase().as_str() {
            "approved" | "approve_once" => ApprovalDecision::Approved,
            "approved_always" | "approve_always" => ApprovalDecision::ApprovedAlways,
            _ => ApprovalDecision::Rejected {
                reason: args
                    .get("reason")
                    .and_then(|r| r.as_str())
                    .unwrap_or("Denied")
                    .to_string(),
            },
        };
        let resolved = ctx.approval_engine.submit_decision(id, dec).await;
        Ok(json!({ "id": id, "resolved": resolved }))
    }
}

// 4. Session Operations
pub struct AuditSearchTool;
#[async_trait]
impl Tool for AuditSearchTool {
    fn name(&self) -> &str {
        "audit_search"
    }
    fn description(&self) -> &str {
        "Search the append-only tool execution audit trail with bounded filters"
    }
    fn parameters_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "session_id": { "type": "string", "description": "Defaults to the current session; pass 'all' for every session" },
                "tool": { "type": "string", "description": "Optional exact tool name" },
                "query": { "type": "string", "description": "Optional case-insensitive description substring" },
                "success": { "type": "boolean" },
                "approved": { "type": "boolean" },
                "limit": { "type": "integer", "description": "Maximum newest events (default 50, max 500)" }
            }
        })
    }
    fn risk_tier(&self) -> RiskTier {
        RiskTier::Tier0ReadOnly
    }
    async fn execute(&self, ctx: &ToolContext, args: Value) -> Result<Value> {
        let requested_session = args
            .get("session_id")
            .and_then(Value::as_str)
            .unwrap_or(&ctx.session_id);
        let session_filter = if requested_session.eq_ignore_ascii_case("all") {
            None
        } else {
            Some(requested_session)
        };
        let tool_filter = args.get("tool").and_then(Value::as_str);
        let query = args
            .get("query")
            .and_then(Value::as_str)
            .map(str::to_lowercase);
        let success = args.get("success").and_then(Value::as_bool);
        let approved = args.get("approved").and_then(Value::as_bool);
        let limit = args
            .get("limit")
            .and_then(Value::as_u64)
            .unwrap_or(50)
            .clamp(1, 500) as usize;

        let events = ctx.audit_logger.get_history(session_filter).await;
        let selected: Vec<Value> = events
            .iter()
            .filter(|event| {
                tool_filter.map_or(true, |filter| event.tool_name == filter)
                    && success.map_or(true, |filter| event.success == filter)
                    && approved.map_or(true, |filter| event.approved == filter)
                    && query.as_ref().map_or(true, |filter| {
                        event.description.to_lowercase().contains(filter)
                    })
            })
            .take(limit)
            .filter_map(|event| serde_json::to_value(event).ok())
            .collect();
        Ok(json!({ "count": selected.len(), "events": selected }))
    }
}
fn redact_sensitive_values(value: &mut Value) {
    const SENSITIVE: [&str; 6] = ["key", "token", "secret", "password", "authorization", "credential"];
    match value {
        Value::Object(map) => {
            for (name, item) in map.iter_mut() {
                let lower = name.to_lowercase();
                if SENSITIVE.iter().any(|part| lower.contains(part)) {
                    *item = Value::String("[REDACTED]".to_string());
                } else {
                    redact_sensitive_values(item);
                }
            }
        }
        Value::Array(items) => {
            for item in items {
                redact_sensitive_values(item);
            }
        }
        _ => {}
    }
}

pub struct ApprovalHistoryTool;
#[async_trait]
impl Tool for ApprovalHistoryTool {
    fn name(&self) -> &str {
        "approval_history"
    }
    fn description(&self) -> &str {
        "Review recent approval decisions with sensitive parameters redacted"
    }
    fn parameters_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "limit": { "type": "integer", "description": "Maximum records (default 25, max 100)" },
                "tool": { "type": "string", "description": "Optional exact tool name filter" },
                "session_id": { "type": "string", "description": "Optional session filter" },
                "decision": { "type": "string", "enum": ["approved", "approved_always", "rejected", "expired"] }
            }
        })
    }
    fn risk_tier(&self) -> RiskTier {
        RiskTier::Tier0ReadOnly
    }
    async fn execute(&self, ctx: &ToolContext, args: Value) -> Result<Value> {
        let limit = args
            .get("limit")
            .and_then(Value::as_u64)
            .unwrap_or(25)
            .clamp(1, 100) as usize;
        let tool_filter = args.get("tool").and_then(Value::as_str);
        let session_filter = args.get("session_id").and_then(Value::as_str);
        let decision_filter = args.get("decision").and_then(Value::as_str);

        let records = ctx.approval_engine.get_audit_history().await;
        let mut filtered: Vec<Value> = records
            .iter()
            .filter(|record| {
                tool_filter.map_or(true, |filter| record.tool_name == filter)
                    && session_filter.map_or(true, |filter| record.session_id == filter)
                    && decision_filter.map_or(true, |filter| record.decision == filter)
            })
            .take(limit)
            .map(|record| {
                let mut value = serde_json::to_value(record).unwrap_or(Value::Null);
                redact_sensitive_values(&mut value);
                value
            })
            .collect();
        filtered.reverse();
        Ok(json!({ "count": filtered.len(), "history": filtered }))
    }
}

pub struct ApprovalPolicyRevokeTool;
#[async_trait]
impl Tool for ApprovalPolicyRevokeTool {
    fn name(&self) -> &str {
        "approval_policy_revoke"
    }
    fn description(&self) -> &str {
        "Revoke one previously approved 'always allow' policy entry"
    }
    fn parameters_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "key": { "type": "string", "description": "Exact policy key from approval_list or audit history" }
            },
            "required": ["key"]
        })
    }
    fn risk_tier(&self) -> RiskTier {
        RiskTier::Tier1WorkspaceMutating
    }
    async fn execute(&self, ctx: &ToolContext, args: Value) -> Result<Value> {
        let key = args
            .get("key")
            .and_then(Value::as_str)
            .ok_or_else(|| OdonError::Tool {
                tool: self.name().to_string(),
                message: "Missing 'key' parameter".to_string(),
            })?;
        let revoked = ctx.approval_engine.revoke_allowed(key).await;
        if !revoked {
            return Err(OdonError::Tool {
                tool: self.name().to_string(),
                message: format!("Approval policy key '{key}' was not found"),
            });
        }
        Ok(json!({ "key": key, "revoked": true }))
    }
}

pub struct SessionListTool;
#[async_trait]
impl Tool for SessionListTool {
    fn name(&self) -> &str {
        "session_list"
    }
    fn description(&self) -> &str {
        "List active and historical conversation sessions"
    }
    fn parameters_schema(&self) -> Value {
        json!({ "type": "object", "properties": {} })
    }
    fn risk_tier(&self) -> RiskTier {
        RiskTier::Tier0ReadOnly
    }
    async fn execute(&self, ctx: &ToolContext, _args: Value) -> Result<Value> {
        if let Some(storage) = &ctx.storage {
            let list = storage.list_sessions().map_err(|e| OdonError::Tool {
                tool: self.name().to_string(),
                message: e.to_string(),
            })?;
            return Ok(json!({ "sessions": list }));
        }
        Ok(json!({ "sessions": [] }))
    }
}

pub struct SessionGetTool;
#[async_trait]
impl Tool for SessionGetTool {
    fn name(&self) -> &str {
        "session_get"
    }
    fn description(&self) -> &str {
        "Read metadata for one conversation session without exposing its transcript"
    }
    fn parameters_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "session_id": { "type": "string", "description": "Defaults to the current session" }
            }
        })
    }
    fn risk_tier(&self) -> RiskTier {
        RiskTier::Tier0ReadOnly
    }
    async fn execute(&self, ctx: &ToolContext, args: Value) -> Result<Value> {
        let session_id = args
            .get("session_id")
            .and_then(Value::as_str)
            .filter(|value| !value.trim().is_empty())
            .unwrap_or(&ctx.session_id);
        let storage = ctx
            .storage
            .as_ref()
            .ok_or_else(|| OdonError::Tool {
                tool: self.name().to_string(),
                message: "Storage engine unavailable".to_string(),
            })?;
        let session = storage.get_session(session_id).map_err(|e| OdonError::Tool {
            tool: self.name().to_string(),
            message: format!("Session not found: {e}"),
        })?;
        Ok(serde_json::to_value(session).unwrap_or(Value::Null))
    }
}

pub struct SessionMessagesTool;
#[async_trait]
impl Tool for SessionMessagesTool {
    fn name(&self) -> &str {
        "session_messages"
    }
    fn description(&self) -> &str {
        "Read persisted messages from a conversation session with a bounded result"
    }
    fn parameters_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "session_id": { "type": "string", "description": "Defaults to the current session" },
                "limit": { "type": "integer", "description": "Maximum newest messages to return (default 50, max 500)" }
            }
        })
    }
    fn risk_tier(&self) -> RiskTier {
        RiskTier::Tier0ReadOnly
    }
    async fn execute(&self, ctx: &ToolContext, args: Value) -> Result<Value> {
        let session_id = args
            .get("session_id")
            .and_then(Value::as_str)
            .filter(|value| !value.trim().is_empty())
            .unwrap_or(&ctx.session_id);
        let limit = args
            .get("limit")
            .and_then(Value::as_u64)
            .unwrap_or(50)
            .clamp(1, 500) as usize;
        let storage = ctx
            .storage
            .as_ref()
            .ok_or_else(|| OdonError::Tool {
                tool: self.name().to_string(),
                message: "Storage engine unavailable".to_string(),
            })?;
        let mut messages = storage.list_messages(session_id).map_err(|e| OdonError::Tool {
            tool: self.name().to_string(),
            message: e.to_string(),
        })?;
        let total = messages.len();
        let start = messages.len().saturating_sub(limit);
        let selected: Vec<Value> = messages
            .drain(start..)
            .filter_map(|message| serde_json::to_value(message).ok())
            .collect();
        Ok(json!({
            "session_id": session_id,
            "total": total,
            "returned": selected.len(),
            "messages": selected
        }))
    }
}

pub struct SessionDeleteTool;
#[async_trait]
impl Tool for SessionDeleteTool {
    fn name(&self) -> &str {
        "session_delete"
    }
    fn description(&self) -> &str {
        "Delete a non-current conversation session and its persisted messages/artifacts"
    }
    fn parameters_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "session_id": { "type": "string" },
                "confirm": { "type": "boolean", "description": "Must be true" }
            },
            "required": ["session_id", "confirm"]
        })
    }
    fn risk_tier(&self) -> RiskTier {
        RiskTier::Tier2SystemMutating
    }
    async fn execute(&self, ctx: &ToolContext, args: Value) -> Result<Value> {
        let session_id = args
            .get("session_id")
            .and_then(Value::as_str)
            .unwrap_or("");
        let confirmed = args
            .get("confirm")
            .and_then(Value::as_bool)
            .unwrap_or(false);
        if session_id.trim().is_empty() {
            return Err(OdonError::Tool {
                tool: self.name().to_string(),
                message: "session_id is required".to_string(),
            });
        }
        if session_id == ctx.session_id {
            return Err(OdonError::Tool {
                tool: self.name().to_string(),
                message: "The current session cannot be deleted from inside itself".to_string(),
            });
        }
        if !confirmed {
            return Err(OdonError::Tool {
                tool: self.name().to_string(),
                message: "Set confirm=true to delete this session".to_string(),
            });
        }
        let storage = ctx
            .storage
            .as_ref()
            .ok_or_else(|| OdonError::Tool {
                tool: self.name().to_string(),
                message: "Storage engine unavailable".to_string(),
            })?;
        let deleted = storage.delete_session(session_id).map_err(|e| OdonError::Tool {
            tool: self.name().to_string(),
            message: e.to_string(),
        })?;
        if !deleted {
            return Err(OdonError::Tool {
                tool: self.name().to_string(),
                message: format!("Session '{session_id}' was not found"),
            });
        }
        Ok(json!({ "session_id": session_id, "deleted": true }))
    }
}

pub struct SessionExportTool;
#[async_trait]
impl Tool for SessionExportTool {
    fn name(&self) -> &str {
        "session_export"
    }
    fn description(&self) -> &str {
        "Export complete session transcript and artifacts as JSON or Markdown"
    }
    fn parameters_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "session_id": { "type": "string" },
                "format": { "type": "string", "enum": ["markdown", "json"], "default": "markdown" }
            }
        })
    }
    fn risk_tier(&self) -> RiskTier {
        RiskTier::Tier0ReadOnly
    }
    async fn execute(&self, ctx: &ToolContext, args: Value) -> Result<Value> {
        let s_id = args
            .get("session_id")
            .and_then(|s| s.as_str())
            .unwrap_or(&ctx.session_id);
        let fmt = args
            .get("format")
            .and_then(|f| f.as_str())
            .unwrap_or("markdown");

        if let Some(storage) = &ctx.storage {
            let msgs = storage.list_messages(s_id).unwrap_or_default();
            if fmt == "json" {
                return Ok(json!({ "session_id": s_id, "messages": msgs }));
            } else {
                let md = msgs
                    .into_iter()
                    .map(|m| format!("### {:?}\n\n{}\n", m.role, m.content.text))
                    .collect::<Vec<_>>()
                    .join("\n---\n\n");
                return Ok(json!({ "session_id": s_id, "markdown": md }));
            }
        }
        Ok(json!({ "session_id": s_id, "exported": true }))
    }
}

pub struct SessionSwitchTool;
#[async_trait]
impl Tool for SessionSwitchTool {
    fn name(&self) -> &str {
        "session_switch"
    }
    fn description(&self) -> &str {
        "Switch context to a different existing conversation session by ID"
    }
    fn parameters_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "session_id": { "type": "string", "description": "Session ID to switch to" }
            },
            "required": ["session_id"]
        })
    }
    fn risk_tier(&self) -> RiskTier {
        RiskTier::Tier0ReadOnly
    }
    fn availability(&self) -> crate::registry::ToolAvailability {
        crate::registry::ToolAvailability::Unavailable {
            reason: "Session switching must be performed by the agent runtime; a tool call cannot mutate its immutable execution context".to_string(),
        }
    }
    async fn execute(&self, ctx: &ToolContext, args: Value) -> Result<Value> {
        let s_id = args
            .get("session_id")
            .and_then(|s| s.as_str())
            .unwrap_or("");
        if s_id.is_empty() {
            return Err(OdonError::Tool {
                tool: self.name().to_string(),
                message: "session_id is required".into(),
            });
        }

        if let Some(storage) = &ctx.storage {
            let session = storage.get_session(s_id).map_err(|e| OdonError::Tool {
                tool: self.name().to_string(),
                message: format!("Session not found: {e}"),
            })?;
            return Ok(json!({
                "switched": true,
                "session_id": s_id,
                "title": session.title,
                "mode": session.mode,
                "status": session.status,
                "created_at": session.created_at
            }));
        }
        Ok(json!({ "switched": true, "session_id": s_id }))
    }
}

// 5. Relay Operations
pub struct RelayStatusTool;
#[async_trait]
impl Tool for RelayStatusTool {
    fn name(&self) -> &str {
        "relay_status"
    }
    fn description(&self) -> &str {
        "Inspect multichannel headless relay statuses (Telegram, WhatsApp, Slack)"
    }
    fn parameters_schema(&self) -> Value {
        json!({ "type": "object", "properties": {} })
    }
    fn risk_tier(&self) -> RiskTier {
        RiskTier::Tier0ReadOnly
    }
    async fn execute(&self, _ctx: &ToolContext, _args: Value) -> Result<Value> {
        let env_present = |name: &str| {
            std::env::var(name)
                .map(|value| !value.trim().is_empty())
                .unwrap_or(false)
        };
        let tg_configured = env_present("TELEGRAM_BOT_TOKEN");
        let wa_configured =
            env_present("WHATSAPP_ACCESS_TOKEN") && env_present("WHATSAPP_PHONE_NUMBER_ID");
        Ok(json!({
            "relays": {
                "telegram": {
                    "configured": tg_configured,
                    "status": if tg_configured { "online" } else { "standby" },
                    "allowlist": std::env::var("TELEGRAM_ALLOWED_USERS").unwrap_or_default()
                },
                "whatsapp": {
                    "configured": wa_configured,
                    "status": if wa_configured { "online" } else { "standby" },
                    "webhook": "/api/relays/whatsapp",
                    "verify_token_set": env_present("WHATSAPP_VERIFY_TOKEN"),
                    "allowlist": std::env::var("WHATSAPP_ALLOWED_NUMBERS").unwrap_or_default()
                },
                "slack": { "configured": false, "status": "standby" }
            }
        }))
    }
}

// 6. Automation Operations
pub struct AutomationListTool;
#[async_trait]
impl Tool for AutomationListTool {
    fn name(&self) -> &str {
        "automation_list"
    }
    fn description(&self) -> &str {
        "List all scheduled background automations and recurring workflows"
    }
    fn parameters_schema(&self) -> Value {
        json!({ "type": "object", "properties": {} })
    }
    fn risk_tier(&self) -> RiskTier {
        RiskTier::Tier0ReadOnly
    }
    async fn execute(&self, _ctx: &ToolContext, _args: Value) -> Result<Value> {
        let path = odon_home().join(".odon").join("automations.json");
        let automations: Value = fs::read_to_string(&path)
            .ok()
            .and_then(|content| serde_json::from_str::<Value>(&content).ok())
            .unwrap_or_else(|| json!([]));
        Ok(json!({ "automations": automations, "config_path": path }))
    }
}

pub struct AutomationTriggerTool;
#[async_trait]
impl Tool for AutomationTriggerTool {
    fn name(&self) -> &str {
        "automation_trigger"
    }
    fn description(&self) -> &str {
        "Trigger immediate execution of an automation workflow by ID"
    }
    fn parameters_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "id": { "type": "string", "description": "Automation workflow ID" }
            },
            "required": ["id"]
        })
    }
    fn risk_tier(&self) -> RiskTier {
        RiskTier::Tier1WorkspaceMutating
    }
    async fn execute(&self, _ctx: &ToolContext, args: Value) -> Result<Value> {
        let id = args.get("id").and_then(|i| i.as_str()).unwrap_or("");
        if id.is_empty() {
            return Err(OdonError::Tool {
                tool: self.name().to_string(),
                message: "id is required".to_string(),
            });
        }
        // Automations are owned by the daemon; trigger the real endpoint so
        // this tool reflects actual execution instead of pretending.
        let url = format!(
            "{}/api/automations/{}/trigger",
            daemon_base_url().trim_end_matches('/'),
            id
        );
        let resp = reqwest::Client::new()
            .post(&url)
            .timeout(std::time::Duration::from_secs(10))
            .send()
            .await
            .map_err(|e| OdonError::Tool {
                tool: self.name().to_string(),
                message: format!("Could not reach the ODON daemon at {url}: {e}"),
            })?;
        let status = resp.status();
        let body: Value = resp.json().await.unwrap_or_else(|_| json!({}));
        Ok(json!({ "id": id, "http_status": status.as_u16(), "result": body }))
    }
}

pub(crate) async fn daemon_json(
    method: reqwest::Method,
    path: &str,
    body: Option<Value>,
    tool_name: &str,
) -> Result<(u16, Value)> {
    let url = format!(
        "{}{}",
        daemon_base_url().trim_end_matches('/'),
        path
    );
    let mut request = reqwest::Client::new()
        .request(method, &url)
        .timeout(std::time::Duration::from_secs(10));
    if let Some(body) = body {
        request = request.json(&body);
    }
    let response = request.send().await.map_err(|e| OdonError::Tool {
        tool: tool_name.to_string(),
        message: format!("Could not reach the ODON daemon at {url}: {e}"),
    })?;
    let status = response.status().as_u16();
    let value: Value = response.json().await.unwrap_or_else(|_| json!({}));
    Ok((status, value))
}

pub struct AutomationLogsTool;
#[async_trait]
impl Tool for AutomationLogsTool {
    fn name(&self) -> &str {
        "automation_logs"
    }
    fn description(&self) -> &str {
        "Read recent automation run history and outcomes from the ODON daemon"
    }
    fn parameters_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "automation_id": { "type": "string", "description": "Optional automation ID filter" },
                "limit": { "type": "integer", "description": "Maximum newest records (default 25, max 100)" }
            }
        })
    }
    fn risk_tier(&self) -> RiskTier {
        RiskTier::Tier0ReadOnly
    }
    async fn execute(&self, _ctx: &ToolContext, args: Value) -> Result<Value> {
        let automation_id = args.get("automation_id").and_then(Value::as_str);
        let limit = args
            .get("limit")
            .and_then(Value::as_u64)
            .unwrap_or(25)
            .clamp(1, 100) as usize;
        let (status, value) = daemon_json(reqwest::Method::GET, "/api/automations/logs", None, self.name()).await?;
        let mut logs: Vec<Value> = value
            .as_array()
            .cloned()
            .unwrap_or_default()
            .into_iter()
            .filter(|log| {
                automation_id.map_or(true, |filter| {
                    log.get("automation_id").and_then(Value::as_str) == Some(filter)
                })
            })
            .take(limit)
            .collect();
        logs.reverse();
        Ok(json!({ "http_status": status, "count": logs.len(), "logs": logs }))
    }
}

pub struct AutomationUpdateTool;
#[async_trait]
impl Tool for AutomationUpdateTool {
    fn name(&self) -> &str {
        "automation_update"
    }
    fn description(&self) -> &str {
        "Update an existing automation's metadata, schedule, prompt, model, or enabled state"
    }
    fn parameters_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "id": { "type": "string" },
                "name": { "type": "string" },
                "description": { "type": "string" },
                "trigger_type": { "type": "string", "enum": ["interval", "cron", "on_commit", "manual"] },
                "trigger_value": { "type": "string" },
                "prompt": { "type": "string" },
                "model": { "type": "string" },
                "enabled": { "type": "boolean" },
                "retry_count": { "type": "integer", "minimum": 0, "maximum": 10 },
                "retry_delay_secs": { "type": "integer", "minimum": 0, "maximum": 3600 },
                "depends_on": { "type": "array", "items": { "type": "string" } }
            },
            "required": ["id"]
        })
    }
    fn risk_tier(&self) -> RiskTier {
        RiskTier::Tier1WorkspaceMutating
    }
    async fn execute(&self, _ctx: &ToolContext, args: Value) -> Result<Value> {
        let id = args
            .get("id")
            .and_then(Value::as_str)
            .ok_or_else(|| OdonError::Tool {
                tool: self.name().to_string(),
                message: "Missing 'id' parameter".to_string(),
            })?;
        let (status, mut job) =
            daemon_json(reqwest::Method::GET, "/api/automations", None, self.name()).await?;
        if !(200..300).contains(&status) {
            return Err(OdonError::Tool {
                tool: self.name().to_string(),
                message: format!("Could not load automations: HTTP {status}"),
            });
        }
        let mut updated = job
            .as_array_mut()
            .and_then(|items| {
                items
                    .iter_mut()
                    .find(|item| item.get("id").and_then(Value::as_str) == Some(id))
            })
            .ok_or_else(|| OdonError::Tool {
                tool: self.name().to_string(),
                message: format!("Automation '{id}' was not found"),
            })?
            .clone();
        for key in [
            "name",
            "description",
            "trigger_type",
            "trigger_value",
            "prompt",
            "model",
            "enabled",
            "retry_count",
            "retry_delay_secs",
            "depends_on",
        ] {
            if let Some(value) = args.get(key) {
                updated[key] = value.clone();
            }
        }
        let (status, result) = daemon_json(
            reqwest::Method::POST,
            "/api/automations",
            Some(updated),
            self.name(),
        )
        .await?;
        if !(200..300).contains(&status) {
            return Err(OdonError::Tool {
                tool: self.name().to_string(),
                message: format!("Could not save automation: HTTP {status}"),
            });
        }
        Ok(json!({ "id": id, "http_status": status, "result": result }))
    }
}

pub struct AutomationDeleteTool;
#[async_trait]
impl Tool for AutomationDeleteTool {
    fn name(&self) -> &str {
        "automation_delete"
    }
    fn description(&self) -> &str
    {
        "Delete an automation workflow by ID"
    }
    fn parameters_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "id": { "type": "string" },
                "confirm": { "type": "boolean", "description": "Must be true" }
            },
            "required": ["id", "confirm"]
        })
    }
    fn risk_tier(&self) -> RiskTier {
        RiskTier::Tier1WorkspaceMutating
    }
    async fn execute(&self, _ctx: &ToolContext, args: Value) -> Result<Value> {
        let id = args
            .get("id")
            .and_then(Value::as_str)
            .ok_or_else(|| OdonError::Tool {
                tool: self.name().to_string(),
                message: "Missing 'id' parameter".to_string(),
            })?;
        if !args
            .get("confirm")
            .and_then(Value::as_bool)
            .unwrap_or(false)
        {
            return Err(OdonError::Tool {
                tool: self.name().to_string(),
                message: "Set confirm=true to delete this automation".to_string(),
            });
        }
        let (status, result) = daemon_json(
            reqwest::Method::DELETE,
            &format!("/api/automations/{}", id),
            None,
            self.name(),
        )
        .await?;
        if !(200..300).contains(&status) {
            return Err(OdonError::Tool {
                tool: self.name().to_string(),
                message: format!("Could not delete automation: HTTP {status}"),
            });
        }
        Ok(json!({ "id": id, "http_status": status, "result": result }))
    }
}

// 7. MCP Management Tools
pub struct McpAddServerTool;
#[async_trait]
impl Tool for McpAddServerTool {
    fn name(&self) -> &str {
        "mcp_add_server"
    }
    fn description(&self) -> &str {
        "Register a new external MCP server (stdio or sse)"
    }
    fn parameters_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "id": { "type": "string" },
                "name": { "type": "string" },
                "transport": { "type": "string", "enum": ["stdio", "sse"] },
                "command": { "type": "string" },
                "args": { "type": "array", "items": { "type": "string" } },
                "url": { "type": "string" }
            },
            "required": ["id", "name", "transport"]
        })
    }
    fn risk_tier(&self) -> RiskTier {
        RiskTier::Tier1WorkspaceMutating
    }
    async fn execute(&self, _ctx: &ToolContext, args: Value) -> Result<Value> {
        let id = args.get("id").and_then(|i| i.as_str()).unwrap_or("custom");
        let name = args.get("name").and_then(|n| n.as_str()).unwrap_or(id);
        let transport = args
            .get("transport")
            .and_then(|t| t.as_str())
            .unwrap_or("stdio");
        let command = args
            .get("command")
            .and_then(|c| c.as_str())
            .map(String::from);
        let url = args.get("url").and_then(|u| u.as_str()).map(String::from);
        if let Some(url) = &url {
            crate::net_guard::validate_url(url).await.map_err(|error| {
                OdonError::Tool {
                    tool: self.name().to_string(),
                    message: format!("Invalid or disallowed MCP SSE URL '{url}': {error}"),
                }
            })?;
        }
        let server_args = args.get("args").and_then(|a| a.as_array()).map(|items| {
            items
                .iter()
                .filter_map(|item| item.as_str().map(String::from))
                .collect::<Vec<_>>()
        });

        let config = McpServerConfig {
            id: id.to_string(),
            name: name.to_string(),
            transport: transport.to_string(),
            command,
            args: server_args,
            env: None,
            url,
            enabled: true,
            tools: Vec::new(),
        };

        let mut servers = load_mcp_servers();
        if let Some(pos) = servers.iter().position(|s| s.id == id) {
            servers[pos] = config;
        } else {
            servers.push(config);
        }
        save_mcp_servers(&servers)?;
        Ok(json!({ "id": id, "name": name, "status": "configured", "persisted": true }))
    }
}

pub struct McpListServersTool;
#[async_trait]
impl Tool for McpListServersTool {
    fn name(&self) -> &str {
        "mcp_list_servers"
    }
    fn description(&self) -> &str {
        "List all configured external MCP servers and their connection statuses"
    }
    fn parameters_schema(&self) -> Value {
        json!({ "type": "object", "properties": {} })
    }
    fn risk_tier(&self) -> RiskTier {
        RiskTier::Tier0ReadOnly
    }
    async fn execute(&self, _ctx: &ToolContext, _args: Value) -> Result<Value> {
        let servers = crate::mcp::load_mcp_servers();
        let servers: Vec<Value> = servers
            .iter()
            .map(|server| {
                let mut value = serde_json::to_value(server).unwrap_or(Value::Null);
                if let Some(env) = value.get_mut("env") {
                    if let Some(env) = env.as_object_mut() {
                        *env = env
                            .iter()
                            .map(|(key, _)| {
                                (key.clone(), Value::String("[REDACTED]".to_string()))
                            })
                            .collect();
                    }
                }
                value
            })
            .collect();
        Ok(json!({ "servers": servers }))
    }
}

// 8. Settings & Telemetry
pub struct SettingsGetTool;
#[async_trait]
impl Tool for SettingsGetTool {
    fn name(&self) -> &str {
        "settings_get"
    }
    fn description(&self) -> &str {
        "Get current agent execution settings, mode, and security policy"
    }
    fn parameters_schema(&self) -> Value {
        json!({ "type": "object", "properties": {} })
    }
    fn risk_tier(&self) -> RiskTier {
        RiskTier::Tier0ReadOnly
    }
    async fn execute(&self, ctx: &ToolContext, _args: Value) -> Result<Value> {
        let policy = ctx.approval_engine.get_policy().await;
        Ok(json!({
            "execution_mode": format!("{:?}", policy.mode),
            "workspace_root": ctx.workspace_root
        }))
    }
}

pub struct SettingsSetTool;
#[async_trait]
impl Tool for SettingsSetTool {
    fn name(&self) -> &str {
        "settings_set"
    }
    fn description(&self) -> &str {
        "Update agent execution mode (autonomous, supervised, locked)"
    }
    fn parameters_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "mode": { "type": "string", "enum": ["autonomous", "supervised", "locked"] }
            },
            "required": ["mode"]
        })
    }
    fn risk_tier(&self) -> RiskTier {
        RiskTier::Tier2SystemMutating
    }
    async fn execute(&self, ctx: &ToolContext, args: Value) -> Result<Value> {
        let mode_str = args
            .get("mode")
            .and_then(|m| m.as_str())
            .unwrap_or("autonomous");
        let mode = match mode_str.to_lowercase().as_str() {
            "supervised" => odon_security::ExecutionMode::Supervised,
            "locked" => odon_security::ExecutionMode::Locked,
            _ => odon_security::ExecutionMode::Autonomous,
        };
        ctx.approval_engine.update_mode(mode).await;
        Ok(json!({ "execution_mode": mode_str, "status": "updated" }))
    }
}
