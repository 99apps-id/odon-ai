//! Agent harness tools: task plan, reasoning scratchpad, and plan mode.
//! Behaviour mirrors the Termigo harness (todo_write / todo_read / think / plan_mode).

use async_trait::async_trait;
use odon_core::{OdonError, Result};
use odon_security::RiskTier;
use serde_json::{json, Value};

use crate::registry::{Tool, ToolContext};

const STATUSES: [&str; 3] = ["pending", "in_progress", "completed"];
const MAX_TODOS: usize = 100;

fn tool_err(tool: &str, message: impl Into<String>) -> OdonError {
    OdonError::Tool {
        tool: tool.to_string(),
        message: message.into(),
    }
}

/// Validate and normalise a todo list. Pure so it can be unit tested.
pub(crate) fn normalize_todos(tool: &str, raw: &Value) -> Result<Value> {
    let items = raw
        .as_array()
        .ok_or_else(|| tool_err(tool, "'todos' must be an array"))?;
    if items.len() > MAX_TODOS {
        return Err(tool_err(
            tool,
            format!("at most {MAX_TODOS} todo items are allowed"),
        ));
    }

    let mut out = Vec::with_capacity(items.len());
    let mut in_progress = 0;
    for (idx, item) in items.iter().enumerate() {
        let title = item
            .get("title")
            .and_then(|t| t.as_str())
            .map(str::trim)
            .filter(|t| !t.is_empty())
            .ok_or_else(|| {
                tool_err(tool, format!("todo #{} needs a non-empty 'title'", idx + 1))
            })?;
        let status = item
            .get("status")
            .and_then(|s| s.as_str())
            .unwrap_or("pending");
        if !STATUSES.contains(&status) {
            return Err(tool_err(
                tool,
                format!(
                    "todo #{} has invalid status '{status}' (use pending, in_progress, completed)",
                    idx + 1
                ),
            ));
        }
        if status == "in_progress" {
            in_progress += 1;
        }
        let id = item
            .get("id")
            .and_then(|i| i.as_str())
            .map(str::to_string)
            .unwrap_or_else(|| format!("t{}", idx + 1));
        out.push(json!({ "id": id, "title": title, "status": status }));
    }

    if in_progress > 1 {
        return Err(tool_err(
            tool,
            "exactly one todo may be in_progress at a time",
        ));
    }
    Ok(Value::Array(out))
}

fn summarize(items: &Value) -> Value {
    let list = items.as_array().cloned().unwrap_or_default();
    let count = |s: &str| list.iter().filter(|i| i["status"] == s).count();
    json!({
        "total": list.len(),
        "pending": count("pending"),
        "in_progress": count("in_progress"),
        "completed": count("completed"),
    })
}

pub struct TodoWriteTool;

#[async_trait]
impl Tool for TodoWriteTool {
    fn name(&self) -> &str {
        "todo_write"
    }
    fn description(&self) -> &str {
        "Replace the task plan for this session. Use for any task with three or more steps; keep exactly one item in_progress."
    }
    fn parameters_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "todos": {
                    "type": "array",
                    "description": "The full ordered plan",
                    "items": {
                        "type": "object",
                        "properties": {
                            "id": { "type": "string" },
                            "title": { "type": "string", "description": "Imperative description of the step" },
                            "status": { "type": "string", "enum": STATUSES }
                        },
                        "required": ["title", "status"]
                    }
                }
            },
            "required": ["todos"]
        })
    }
    fn risk_tier(&self) -> RiskTier {
        RiskTier::Tier0ReadOnly
    }
    async fn execute(&self, ctx: &ToolContext, args: Value) -> Result<Value> {
        let raw = args
            .get("todos")
            .ok_or_else(|| tool_err(self.name(), "Missing 'todos' parameter"))?;
        let items = normalize_todos(self.name(), raw)?;
        let storage = ctx
            .storage
            .as_ref()
            .ok_or_else(|| tool_err(self.name(), "Storage engine unavailable in tool context"))?;
        storage
            .set_todos(&ctx.session_id, &items)
            .map_err(|e| tool_err(self.name(), format!("Failed to save plan: {e}")))?;
        Ok(json!({ "todos": items, "summary": summarize(&items) }))
    }
}

pub struct TodoReadTool;

#[async_trait]
impl Tool for TodoReadTool {
    fn name(&self) -> &str {
        "todo_read"
    }
    fn description(&self) -> &str {
        "Read the current task plan for this session."
    }
    fn parameters_schema(&self) -> Value {
        json!({ "type": "object", "properties": {} })
    }
    fn risk_tier(&self) -> RiskTier {
        RiskTier::Tier0ReadOnly
    }
    async fn execute(&self, ctx: &ToolContext, _args: Value) -> Result<Value> {
        let storage = ctx
            .storage
            .as_ref()
            .ok_or_else(|| tool_err(self.name(), "Storage engine unavailable in tool context"))?;
        let items = storage
            .get_todos(&ctx.session_id)
            .map_err(|e| tool_err(self.name(), format!("Failed to read plan: {e}")))?;
        Ok(json!({ "todos": items, "summary": summarize(&items) }))
    }
}

pub struct ThinkTool;

#[async_trait]
impl Tool for ThinkTool {
    fn name(&self) -> &str {
        "think"
    }
    fn description(&self) -> &str {
        "Record a short line of reasoning before acting. Does not change anything."
    }
    fn parameters_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": { "thoughts": { "type": "string", "description": "The reasoning to record" } },
            "required": ["thoughts"]
        })
    }
    fn risk_tier(&self) -> RiskTier {
        RiskTier::Tier0ReadOnly
    }
    async fn execute(&self, _ctx: &ToolContext, args: Value) -> Result<Value> {
        let thoughts = args
            .get("thoughts")
            .and_then(|t| t.as_str())
            .ok_or_else(|| tool_err(self.name(), "Missing 'thoughts' parameter"))?;
        Ok(json!({ "recorded": true, "chars": thoughts.chars().count() }))
    }
}

/// Plan mode is persisted as a session-scoped memory flag so the runtime (and UI)
/// can read it: when on, the agent must only read and plan, never mutate.
pub struct PlanModeTool;

fn plan_mode_key(session_id: &str) -> String {
    format!("plan_mode:{session_id}")
}

#[async_trait]
impl Tool for PlanModeTool {
    fn name(&self) -> &str {
        "plan_mode"
    }
    fn description(&self) -> &str {
        "Enter or leave plan mode. In plan mode only read-only tools may be used; present a plan, then leave plan mode to execute."
    }
    fn parameters_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": { "enabled": { "type": "boolean", "description": "true to enter plan mode, false to leave it" } },
            "required": ["enabled"]
        })
    }
    fn risk_tier(&self) -> RiskTier {
        RiskTier::Tier0ReadOnly
    }
    async fn execute(&self, ctx: &ToolContext, args: Value) -> Result<Value> {
        let enabled = args
            .get("enabled")
            .and_then(|e| e.as_bool())
            .ok_or_else(|| tool_err(self.name(), "Missing boolean 'enabled' parameter"))?;
        let storage = ctx
            .storage
            .as_ref()
            .ok_or_else(|| tool_err(self.name(), "Storage engine unavailable in tool context"))?;
        storage
            .set_memory(
                &plan_mode_key(&ctx.session_id),
                if enabled { "on" } else { "off" },
                "plan_mode",
            )
            .map_err(|e| tool_err(self.name(), format!("Failed to save plan mode: {e}")))?;
        Ok(json!({ "plan_mode": enabled }))
    }
}

/// Persist the session-scoped plan-mode flag without going through the tool loop.
pub fn set_plan_mode(
    storage: &odon_storage::StorageEngine,
    session_id: &str,
    enabled: bool,
) -> odon_core::Result<()> {
    storage
        .set_memory(
            &plan_mode_key(session_id),
            if enabled { "on" } else { "off" },
            "plan_mode",
        )
        .map_err(|e| odon_core::OdonError::Storage(e.to_string()))
}

/// Whether plan mode is currently on for a session.
pub fn is_plan_mode(storage: &odon_storage::StorageEngine, session_id: &str) -> bool {
    matches!(
        storage.get_memory(&plan_mode_key(session_id)),
        Ok(Some(item)) if item.value == "on"
    )
}

pub struct TodoUpdateTool;

#[async_trait]
impl Tool for TodoUpdateTool {
    fn name(&self) -> &str {
        "todo_update"
    }
    fn description(&self) -> &str {
        "Incrementally update status or title of an existing item in the todo list"
    }
    fn parameters_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "id": { "type": "string", "description": "ID or title of item to update" },
                "title": { "type": "string", "description": "New title if updating" },
                "status": { "type": "string", "enum": ["pending", "in_progress", "completed"] }
            },
            "required": ["id", "status"]
        })
    }
    fn risk_tier(&self) -> RiskTier {
        RiskTier::Tier1WorkspaceMutating
    }
    async fn execute(&self, ctx: &ToolContext, args: Value) -> Result<Value> {
        let storage = ctx
            .storage
            .as_ref()
            .ok_or_else(|| tool_err(self.name(), "Storage engine unavailable"))?;
        let id = args.get("id").and_then(|i| i.as_str()).unwrap_or("");
        let new_status = args
            .get("status")
            .and_then(|s| s.as_str())
            .unwrap_or("completed");

        let key = format!("todos:{}", ctx.session_id);
        let existing = storage.get_memory(&key).ok().flatten();
        let mut list: Vec<Value> = if let Some(mem) = existing {
            serde_json::from_str(&mem.value).unwrap_or_default()
        } else {
            Vec::new()
        };

        let mut updated: Vec<Value> = Vec::new();
        for item in list.iter_mut() {
            let item_id = item.get("id").and_then(|i| i.as_str()).unwrap_or("");
            let item_title = item.get("title").and_then(|t| t.as_str()).unwrap_or("");
            if item_id == id || item_title == id {
                item["status"] = json!(new_status);
                if let Some(t) = args.get("title").and_then(|t| t.as_str()) {
                    item["title"] = json!(t);
                }
                updated.push(item.clone());
                break;
            }
        }

        if updated.is_empty() {
            return Err(tool_err(
                self.name(),
                format!("todo item '{id}' was not found; use todo_read first"),
            ));
        }

        // Reuse the same validation/invariants as todo_write: bounded list,
        // valid status, and exactly one in_progress item.
        let validated = normalize_todos(self.name(), &json!(list))?;
        let serialized = serde_json::to_string(&validated).unwrap_or_default();
        let _ = storage.set_memory(&key, &serialized, "todos");

        Ok(json!({
            "updated_id": updated[0]["id"],
            "status": updated[0]["status"],
            "count": validated.as_array().map(|items| items.len()).unwrap_or(0),
            "todos": validated
        }))
    }
}

pub struct RememberTool;

#[async_trait]
impl Tool for RememberTool {
    fn name(&self) -> &str {
        "remember"
    }
    fn description(&self) -> &str {
        "Record one durable fact, convention, or lesson about this project for future runs"
    }
    fn parameters_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "fact": { "type": "string", "description": "Single standalone lesson or avoided mistake" }
            },
            "required": ["fact"]
        })
    }
    fn risk_tier(&self) -> RiskTier {
        RiskTier::Tier1WorkspaceMutating
    }
    async fn execute(&self, ctx: &ToolContext, args: Value) -> Result<Value> {
        let fact = args
            .get("fact")
            .and_then(|f| f.as_str())
            .ok_or_else(|| tool_err(self.name(), "Missing 'fact' parameter"))?;

        if let Some(storage) = &ctx.storage {
            let key = format!("memory:facts:{}", ctx.session_id);
            let _ = storage.set_memory(&key, fact, "facts");
        }

        Ok(json!({ "saved": true, "fact": fact }))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn plan_mode_blocks_mutating_tools_until_disabled() {
        use odon_security::{ApprovalEngine, AuditLogger, SecurityPolicy};
        use std::sync::Arc;

        let storage = Arc::new(odon_storage::StorageEngine::new_in_memory().unwrap());
        let dir = std::env::temp_dir().join(format!("odon-plan-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let mut policy = SecurityPolicy::default();
        policy.mode = odon_security::ExecutionMode::Autonomous;
        let ctx = crate::registry::ToolContext {
            session_id: "s1".to_string(),
            workspace_root: dir.to_string_lossy().to_string(),
            approval_engine: Arc::new(ApprovalEngine::new(policy)),
            audit_logger: Arc::new(AuditLogger::new()),
            storage: Some(storage),
        };
        let reg = crate::build_full_tool_registry();

        reg.execute_tool("plan_mode", &ctx, json!({ "enabled": true }))
            .await
            .unwrap();
        let blocked = reg
            .execute_tool("fs_write", &ctx, json!({ "path": "a.txt", "content": "x" }))
            .await;
        assert!(
            blocked.is_err(),
            "mutating tool must be blocked in plan mode"
        );
        assert!(!dir.join("a.txt").exists());
        // Read-only tools still work while planning.
        assert!(reg.execute_tool("fs_list", &ctx, json!({})).await.is_ok());

        reg.execute_tool("plan_mode", &ctx, json!({ "enabled": false }))
            .await
            .unwrap();
        reg.execute_tool("fs_write", &ctx, json!({ "path": "a.txt", "content": "x" }))
            .await
            .unwrap();
        assert!(dir.join("a.txt").exists());
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn accepts_a_valid_plan_and_assigns_ids() {
        let raw = json!([
            { "title": "Map the repo", "status": "completed" },
            { "title": "Fix bug", "status": "in_progress" },
            { "title": "Run tests" }
        ]);
        let out = normalize_todos("todo_write", &raw).unwrap();
        assert_eq!(out[0]["id"], "t1");
        assert_eq!(out[2]["status"], "pending");
        assert_eq!(summarize(&out)["completed"], 1);
    }

    #[test]
    fn rejects_two_in_progress_items() {
        let raw = json!([
            { "title": "a", "status": "in_progress" },
            { "title": "b", "status": "in_progress" }
        ]);
        assert!(normalize_todos("todo_write", &raw).is_err());
    }

    #[test]
    fn rejects_bad_status_and_empty_title() {
        assert!(
            normalize_todos("todo_write", &json!([{ "title": "a", "status": "done" }])).is_err()
        );
        assert!(normalize_todos(
            "todo_write",
            &json!([{ "title": "  ", "status": "pending" }])
        )
        .is_err());
        assert!(normalize_todos("todo_write", &json!("nope")).is_err());
    }

    #[test]
    fn todos_round_trip_through_storage() {
        let storage = odon_storage::StorageEngine::new_in_memory().unwrap();
        // Todos carry a foreign key to their session, and the runtime
        // always creates the session before a turn can write a plan, so
        // the round trip starts from a stored session too.
        let mut session = odon_core::Session::new("t", odon_core::SessionMode::General, "m");
        session.id = "s1".to_string();
        storage.save_session(&session).unwrap();
        let items = normalize_todos(
            "todo_write",
            &json!([{ "title": "x", "status": "pending" }]),
        )
        .unwrap();
        storage.set_todos("s1", &items).unwrap();
        assert_eq!(storage.get_todos("s1").unwrap(), items);
        assert_eq!(storage.get_todos("other").unwrap(), json!([]));
    }

    #[test]
    fn plan_mode_flag_is_session_scoped() {
        let storage = odon_storage::StorageEngine::new_in_memory().unwrap();
        assert!(!is_plan_mode(&storage, "s1"));
        storage
            .set_memory(&plan_mode_key("s1"), "on", "plan_mode")
            .unwrap();
        assert!(is_plan_mode(&storage, "s1"));
        assert!(!is_plan_mode(&storage, "s2"));
    }

    #[test]
    fn set_plan_mode_helper_toggles_flag() {
        let storage = odon_storage::StorageEngine::new_in_memory().unwrap();
        set_plan_mode(&storage, "s1", true).unwrap();
        assert!(is_plan_mode(&storage, "s1"));
        set_plan_mode(&storage, "s1", false).unwrap();
        assert!(!is_plan_mode(&storage, "s1"));
    }
}
