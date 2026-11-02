use async_trait::async_trait;
use odon_core::{OdonError, Result};
use odon_security::{ApprovalDecision, ApprovalEngine, AuditLogger, RiskTier};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::HashMap;
use std::sync::Arc;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolSchema {
    pub name: String,
    pub description: String,
    pub parameters: Value,
    pub risk_tier: u8,
}

#[derive(Clone)]
pub struct ToolContext {
    pub session_id: String,
    pub workspace_root: String,
    pub approval_engine: Arc<ApprovalEngine>,
    pub audit_logger: Arc<AuditLogger>,
    pub storage: Option<Arc<odon_storage::StorageEngine>>,
}

#[async_trait]
pub trait Tool: Send + Sync {
    fn name(&self) -> &str;
    fn description(&self) -> &str;
    fn parameters_schema(&self) -> Value;
    fn risk_tier(&self) -> RiskTier;

    async fn execute(&self, ctx: &ToolContext, args: Value) -> Result<Value>;
}

pub struct ToolRegistry {
    tools: HashMap<String, Box<dyn Tool>>,
}

impl Default for ToolRegistry {
    fn default() -> Self {
        Self::new()
    }
}

impl ToolRegistry {
    pub fn new() -> Self {
        Self {
            tools: HashMap::new(),
        }
    }

    pub fn register(&mut self, tool: Box<dyn Tool>) {
        self.tools.insert(tool.name().to_string(), tool);
    }

    pub fn normalize_name(name: &str) -> String {
        let stripped = name
            .trim()
            .trim_start_matches("defaultapi:")
            .trim_start_matches("default_api:")
            .trim_start_matches("functions.")
            .trim_start_matches("function.")
            .trim_start_matches("tools:")
            .trim_start_matches("tool:")
            .trim();

        let lower = stripped.to_lowercase().replace('-', "_");
        match lower.as_str() {
            "websearch" | "search_web" | "search" | "google_search" | "duckduckgo_search" => "web_search".to_string(),
            "websearch_news" | "search_news" | "news_search" | "news" => "web_search_news".to_string(),
            "websearch_code" | "search_code" | "code_search" => "web_search_code".to_string(),
            "fetch" | "http_get" | "curl" | "browse" => "web_fetch".to_string(),
            "readfile" | "read" | "fs_read" => "read_file".to_string(),
            "writefile" | "write" | "fs_write" => "write_file".to_string(),
            "listdirectory" | "list_dir" | "listdir" | "ls" => "list_directory".to_string(),
            "createdirectory" | "create_dir" | "mkdir" => "create_directory".to_string(),
            "deletefile" | "rm" | "fs_delete" => "delete_file".to_string(),
            "bash" | "shell" | "sh" | "terminal" => "bash_run".to_string(),
            "processlist" | "ps" => "process_list".to_string(),
            "memorysearch" => "memory_search".to_string(),
            _ => lower,
        }
    }

    pub fn get(&self, name: &str) -> Option<&Box<dyn Tool>> {
        if let Some(tool) = self.tools.get(name) {
            return Some(tool);
        }
        let norm = Self::normalize_name(name);
        self.tools.get(&norm)
    }

    pub fn contains(&self, name: &str) -> bool {
        if self.tools.contains_key(name) {
            return true;
        }
        let norm = Self::normalize_name(name);
        self.tools.contains_key(&norm)
    }

    pub fn schemas(&self) -> Vec<ToolSchema> {
        self.tools
            .values()
            .map(|t| ToolSchema {
                name: t.name().to_string(),
                description: t.description().to_string(),
                parameters: t.parameters_schema(),
                risk_tier: t.risk_tier() as u8,
            })
            .collect()
    }

    pub async fn execute_tool(
        &self,
        name: &str,
        ctx: &ToolContext,
        args: Value,
    ) -> Result<Value> {
        let tool = self.get(name).ok_or_else(|| OdonError::Tool {
            tool: name.to_string(),
            message: format!("Tool '{}' not found in registry", name),
        })?;

        let desc = format!("Executing tool: {}", name);
        let start = std::time::Instant::now();

        // Plan mode: only read-only tools may run until the plan is accepted.
        if tool.risk_tier() > RiskTier::Tier0ReadOnly {
            if let Some(storage) = &ctx.storage {
                if crate::harness::is_plan_mode(storage, &ctx.session_id) {
                    return Err(OdonError::Tool {
                        tool: name.to_string(),
                        message: "Plan mode is active: only read-only tools are allowed. Present the plan, then call plan_mode with enabled=false to execute.".to_string(),
                    });
                }
            }
        }

        // Hard command-policy check before approval: catastrophic commands are
        // refused regardless of the execution mode or a stored "always allow".
        for key in ["command", "script"] {
            if let Some(command) = args.get(key).and_then(|value| value.as_str()) {
                let policy = ctx.approval_engine.get_policy().await;
                if let Some(reason) = policy.command_forbidden_reason(command) {
                    let _ = ctx
                        .audit_logger
                        .record(
                            &ctx.session_id,
                            name,
                            tool.risk_tier(),
                            &format!("Blocked by command policy: {reason}"),
                            false,
                            false,
                            0,
                        )
                        .await;
                    return Err(OdonError::Security(format!(
                        "Command blocked by security policy: {reason}"
                    )));
                }
            }
        }

        // Security approval check
        let decision = ctx
            .approval_engine
            .evaluate_and_request(&ctx.session_id, name, &args, &desc)
            .await
            .map_err(|e| OdonError::Security(e))?;

        match decision {
            ApprovalDecision::Approved | ApprovalDecision::ApprovedAlways => {
                let res = tool.execute(ctx, args).await;
                let duration = start.elapsed().as_millis() as u64;
                let is_ok = res.is_ok();

                ctx.audit_logger
                    .record(
                        &ctx.session_id,
                        name,
                        tool.risk_tier(),
                        &desc,
                        true,
                        is_ok,
                        duration,
                    )
                    .await;

                res
            }
            ApprovalDecision::Rejected { reason } => {
                ctx.audit_logger
                    .record(
                        &ctx.session_id,
                        name,
                        tool.risk_tier(),
                        &format!("Rejected: {}", reason),
                        false,
                        false,
                        start.elapsed().as_millis() as u64,
                    )
                    .await;

                Err(OdonError::Security(format!(
                    "Tool '{}' execution rejected: {}",
                    name, reason
                )))
            }
            ApprovalDecision::Expired => {
                ctx.audit_logger
                    .record(
                        &ctx.session_id,
                        name,
                        tool.risk_tier(),
                        "Approval request timed out",
                        false,
                        false,
                        start.elapsed().as_millis() as u64,
                    )
                    .await;

                Err(OdonError::Security(format!(
                    "Tool '{}' approval request timed out",
                    name
                )))
            }
        }
    }
}
