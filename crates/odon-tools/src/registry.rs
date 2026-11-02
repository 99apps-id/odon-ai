use async_trait::async_trait;
use odon_core::{OdonError, Result};
use odon_security::{ApprovalDecision, ApprovalEngine, AuditLogger, RiskTier};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::HashMap;
use std::path::Path;
use std::sync::Arc;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "status", rename_all = "snake_case")]
pub enum ToolAvailability {
    Stable,
    Experimental,
    Unavailable { reason: String },
}

impl ToolAvailability {
    pub fn label(&self) -> &'static str {
        match self {
            Self::Stable => "stable",
            Self::Experimental => "experimental",
            Self::Unavailable { .. } => "unavailable",
        }
    }
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct ToolRequirements {
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub binaries: Vec<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub environment: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolSchema {
    pub name: String,
    pub description: String,
    pub parameters: Value,
    pub risk_tier: u8,
    pub availability: ToolAvailability,
    pub requirements: ToolRequirements,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolDiagnostic {
    pub name: String,
    pub availability: ToolAvailability,
    pub ready: bool,
    pub issues: Vec<String>,
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
    fn availability(&self) -> ToolAvailability {
        ToolAvailability::Stable
    }
    fn requirements(&self) -> ToolRequirements {
        ToolRequirements::default()
    }

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

fn binary_on_path(binary: &str) -> bool {
    let candidate = Path::new(binary);
    if candidate.components().count() > 1 {
        return candidate.is_file();
    }

    let Some(path) = std::env::var_os("PATH") else {
        return false;
    };
    let extensions = if cfg!(windows) {
        std::env::var_os("PATHEXT")
            .map(|value| {
                value
                    .to_string_lossy()
                    .split(';')
                    .filter(|item| !item.is_empty())
                    .map(str::to_string)
                    .collect::<Vec<_>>()
            })
            .unwrap_or_else(|| vec![".COM".into(), ".EXE".into(), ".BAT".into(), ".CMD".into()])
    } else {
        vec![String::new()]
    };

    std::env::split_paths(&path).any(|directory| {
        extensions
            .iter()
            .any(|extension| directory.join(format!("{binary}{extension}")).is_file())
    })
}

impl ToolRegistry {
    pub fn new() -> Self {
        Self {
            tools: HashMap::new(),
        }
    }

    pub fn register(&mut self, tool: Box<dyn Tool>) {
        let name = tool.name().to_string();
        assert!(
            !self.tools.contains_key(&name),
            "duplicate tool registration: {name}"
        );
        self.tools.insert(name, tool);
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
                availability: t.availability(),
                requirements: t.requirements(),
            })
            .collect()
    }

    pub fn diagnostics(&self) -> Vec<ToolDiagnostic> {
        let mut diagnostics = self
            .tools
            .values()
            .map(|tool| {
                let availability = tool.availability();
                let requirements = tool.requirements();
                let mut issues = Vec::new();

                if let ToolAvailability::Unavailable { reason } = &availability {
                    issues.push(reason.clone());
                }
                for binary in &requirements.binaries {
                    if !binary_on_path(binary) {
                        issues.push(format!("required binary '{binary}' was not found on PATH"));
                    }
                }
                for variable in &requirements.environment {
                    if std::env::var(variable)
                        .map(|value| value.trim().is_empty())
                        .unwrap_or(true)
                    {
                        issues.push(format!(
                            "required environment variable '{variable}' is not set"
                        ));
                    }
                }

                if tool.parameters_schema().get("type").and_then(Value::as_str) != Some("object") {
                    issues.push("parameter schema root must have type 'object'".to_string());
                }

                ToolDiagnostic {
                    name: tool.name().to_string(),
                    availability,
                    ready: issues.is_empty(),
                    issues,
                }
            })
            .collect::<Vec<_>>();
        diagnostics.sort_by(|a, b| a.name.cmp(&b.name));
        diagnostics
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

        if let ToolAvailability::Unavailable { reason } = tool.availability() {
            return Err(OdonError::Tool {
                tool: name.to_string(),
                message: format!("Tool is unavailable in this build: {reason}"),
            });
        }

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
