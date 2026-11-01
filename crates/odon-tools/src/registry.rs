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

    pub fn get(&self, name: &str) -> Option<&Box<dyn Tool>> {
        self.tools.get(name)
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
        let tool = self.tools.get(name).ok_or_else(|| OdonError::Tool {
            tool: name.to_string(),
            message: format!("Tool '{}' not found in registry", name),
        })?;

        let desc = format!("Executing tool: {}", name);
        let start = std::time::Instant::now();

        // Security approval check
        let decision = ctx
            .approval_engine
            .evaluate_and_request(&ctx.session_id, name, &args, &desc)
            .await
            .map_err(|e| OdonError::Security(e))?;

        match decision {
            ApprovalDecision::Approved => {
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
