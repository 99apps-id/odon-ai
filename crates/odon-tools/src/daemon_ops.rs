use async_trait::async_trait;
use odon_core::{OdonError, Result};
use odon_security::RiskTier;
use serde_json::{json, Value};

use crate::ops_ext::daemon_json;
use crate::registry::{Tool, ToolContext};

async fn get(path: &str, tool_name: &str) -> Result<(u16, Value)> {
    daemon_json(reqwest::Method::GET, path, None, tool_name).await
}

pub struct DaemonStatusTool;
#[async_trait]
impl Tool for DaemonStatusTool {
    fn name(&self) -> &str {
        "daemon_status"
    }
    fn description(&self) -> &str {
        "Inspect the ODON daemon runtime, workspace, relay, and automation status"
    }
    fn parameters_schema(&self) -> Value {
        json!({ "type": "object", "properties": {} })
    }
    fn risk_tier(&self) -> RiskTier {
        RiskTier::Tier0ReadOnly
    }
    async fn execute(&self, _ctx: &ToolContext, _args: Value) -> Result<Value> {
        let (status, value) = get("/api/status", self.name()).await?;
        if !(200..300).contains(&status) {
            return Err(OdonError::Tool {
                tool: self.name().to_string(),
                message: format!("Daemon returned HTTP {status}"),
            });
        }
        Ok(value)
    }
}

pub struct DaemonMetricsTool;
#[async_trait]
impl Tool for DaemonMetricsTool {
    fn name(&self) -> &str {
        "daemon_metrics"
    }
    fn description(&self) -> &str {
        "Inspect runtime performance and service metrics from the ODON daemon"
    }
    fn parameters_schema(&self) -> Value {
        json!({ "type": "object", "properties": {} })
    }
    fn risk_tier(&self) -> RiskTier {
        RiskTier::Tier0ReadOnly
    }
    async fn execute(&self, _ctx: &ToolContext, _args: Value) -> Result<Value> {
        let (status, value) = get("/api/metrics", self.name()).await?;
        if !(200..300).contains(&status) {
            return Err(OdonError::Tool {
                tool: self.name().to_string(),
                message: format!("Daemon returned HTTP {status}"),
            });
        }
        Ok(value)
    }
}

pub struct ToolDiagnosticsTool;
#[async_trait]
impl Tool for ToolDiagnosticsTool {
    fn name(&self) -> &str {
        "tool_diagnostics"
    }
    fn description(&self) -> &str {
        "Inspect tool availability, readiness, visibility, and missing requirements"
    }
    fn parameters_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "tool": { "type": "string", "description": "Optional exact tool name filter" },
                "only_issues": { "type": "boolean", "description": "Return only tools with issues" }
            }
        })
    }
    fn risk_tier(&self) -> RiskTier {
        RiskTier::Tier0ReadOnly
    }
    async fn execute(&self, _ctx: &ToolContext, args: Value) -> Result<Value> {
        let filter = args.get("tool").and_then(Value::as_str);
        let only_issues = args
            .get("only_issues")
            .and_then(Value::as_bool)
            .unwrap_or(false);
        let mut diagnostics = crate::build_full_tool_registry().diagnostics();
        diagnostics.retain(|item| {
            filter.map_or(true, |name| item.name == name) && (!only_issues || !item.ready)
        });
        Ok(json!({
            "count": diagnostics.len(),
            "diagnostics": diagnostics
        }))
    }
}

pub struct DoctorTool;
#[async_trait]
impl Tool for DoctorTool {
    fn name(&self) -> &str {
        "doctor"
    }
    fn description(&self) -> &str {
        "Run a health check over daemon connectivity, storage, providers, and tool readiness"
    }
    fn parameters_schema(&self) -> Value {
        json!({ "type": "object", "properties": {} })
    }
    fn risk_tier(&self) -> RiskTier {
        RiskTier::Tier0ReadOnly
    }
    async fn execute(&self, ctx: &ToolContext, _args: Value) -> Result<Value> {
        let (daemon_status, daemon) = get("/api/status", self.name()).await.map_or(
            (503, json!({ "error": "daemon unreachable" })),
            |(status, value)| (status, value),
        );
        let diagnostics = crate::build_full_tool_registry().diagnostics();
        let ready = diagnostics.iter().filter(|item| item.ready).count();
        let unavailable: Vec<&str> = diagnostics
            .iter()
            .filter(|item| !item.ready)
            .map(|item| item.name.as_str())
            .collect();
        let provider_count = ctx
            .storage
            .as_ref()
            .and_then(|storage| storage.list_provider_configs().ok())
            .map(|providers| providers.len())
            .unwrap_or(0);
        Ok(json!({
            "healthy": (200..300).contains(&daemon_status) && ctx.storage.is_some(),
            "daemon": { "reachable": (200..300).contains(&daemon_status), "http_status": daemon_status, "status": daemon },
            "storage": { "available": ctx.storage.is_some() },
            "providers": { "configured": provider_count },
            "tools": {
                "total": diagnostics.len(),
                "ready": ready,
                "unavailable": unavailable
            },
            "session": ctx.session_id
        }))
    }
}

pub struct WorkspaceAddTool;
#[async_trait]
impl Tool for WorkspaceAddTool {
    fn name(&self) -> &str {
        "workspace_add"
    }
    fn description(&self) -> &str {
        "Register an existing local directory as an allowed ODON workspace"
    }
    fn parameters_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "path": { "type": "string", "description": "Absolute directory path" }
            },
            "required": ["path"]
        })
    }
    fn risk_tier(&self) -> RiskTier {
        RiskTier::Tier1WorkspaceMutating
    }
    async fn execute(&self, _ctx: &ToolContext, args: Value) -> Result<Value> {
        let path = args
            .get("path")
            .and_then(Value::as_str)
            .ok_or_else(|| OdonError::Tool {
                tool: self.name().to_string(),
                message: "Missing 'path' parameter".to_string(),
            })?;
        if !std::path::Path::new(path).is_dir() {
            return Err(OdonError::Tool {
                tool: self.name().to_string(),
                message: format!("Workspace path does not exist or is not a directory: {path}"),
            });
        }
        let (status, value) = daemon_json(
            reqwest::Method::POST,
            "/api/workspaces",
            Some(json!({ "path": path })),
            self.name(),
        )
        .await?;
        if !(200..300).contains(&status) {
            return Err(OdonError::Tool {
                tool: self.name().to_string(),
                message: format!("Could not add workspace: HTTP {status}"),
            });
        }
        Ok(value)
    }
}

pub struct WorkspaceRemoveTool;
#[async_trait]
impl Tool for WorkspaceRemoveTool {
    fn name(&self) -> &str {
        "workspace_remove"
    }
    fn description(&self) -> &str {
        "Remove a non-default workspace from the daemon allowlist"
    }
    fn parameters_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "path": { "type": "string" },
                "confirm": { "type": "boolean", "description": "Must be true" }
            },
            "required": ["path", "confirm"]
        })
    }
    fn risk_tier(&self) -> RiskTier {
        RiskTier::Tier1WorkspaceMutating
    }
    async fn execute(&self, _ctx: &ToolContext, args: Value) -> Result<Value> {
        let path = args
            .get("path")
            .and_then(Value::as_str)
            .ok_or_else(|| OdonError::Tool {
                tool: self.name().to_string(),
                message: "Missing 'path' parameter".to_string(),
            })?;
        if !args
            .get("confirm")
            .and_then(Value::as_bool)
            .unwrap_or(false)
        {
            return Err(OdonError::Tool {
                tool: self.name().to_string(),
                message: "Set confirm=true to remove this workspace".to_string(),
            });
        }
        let url = format!(
            "{}{}",
            crate::ops_ext::daemon_base_url().trim_end_matches('/'),
            "/api/workspaces"
        );
        let response = reqwest::Client::new()
            .delete(&url)
            .query(&[("path", path)])
            .timeout(std::time::Duration::from_secs(10))
            .send()
            .await
            .map_err(|e| OdonError::Tool {
                tool: self.name().to_string(),
                message: format!("Could not reach the ODON daemon at {url}: {e}"),
            })?;
        let status = response.status().as_u16();
        let value: Value = response.json().await.unwrap_or_else(|_| json!({}));
        if !(200..300).contains(&status) {
            return Err(OdonError::Tool {
                tool: self.name().to_string(),
                message: format!("Could not remove workspace: HTTP {status}"),
            });
        }
        Ok(value)
    }
}

pub struct WorkspaceListTool;
#[async_trait]
impl Tool for WorkspaceListTool {
    fn name(&self) -> &str {
        "workspace_list"
    }
    fn description(&self) -> &str {
        "List registered ODON workspaces and the active default workspace"
    }
    fn parameters_schema(&self) -> Value {
        json!({ "type": "object", "properties": {} })
    }
    fn risk_tier(&self) -> RiskTier {
        RiskTier::Tier0ReadOnly
    }
    async fn execute(&self, _ctx: &ToolContext, _args: Value) -> Result<Value> {
        let (status, value) = get("/api/workspaces", self.name()).await?;
        if !(200..300).contains(&status) {
            return Err(OdonError::Tool {
                tool: self.name().to_string(),
                message: format!("Daemon returned HTTP {status}"),
            });
        }
        Ok(value)
    }
}
