use async_trait::async_trait;
use odon_core::{OdonError, Result};
use odon_security::RiskTier;
use serde_json::{json, Value};
use tokio::process::Command;

use crate::registry::{Tool, ToolContext};

pub struct GitStatusTool;

#[async_trait]
impl Tool for GitStatusTool {
    fn name(&self) -> &str {
        "git_status"
    }
    fn description(&self) -> &str {
        "Get current git working tree status"
    }
    fn parameters_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {}
        })
    }
    fn risk_tier(&self) -> RiskTier {
        RiskTier::Tier0ReadOnly
    }
    async fn execute(&self, ctx: &ToolContext, _args: Value) -> Result<Value> {
        let output = Command::new("git")
            .arg("status")
            .arg("--short")
            .current_dir(&ctx.workspace_root)
            .output()
            .await
            .map_err(|e| OdonError::Tool {
                tool: self.name().to_string(),
                message: format!("Failed to run git status: {}", e),
            })?;

        let text = String::from_utf8_lossy(&output.stdout).to_string();
        Ok(json!({ "status": text }))
    }
}

pub struct GitDiffTool;

#[async_trait]
impl Tool for GitDiffTool {
    fn name(&self) -> &str {
        "git_diff"
    }
    fn description(&self) -> &str {
        "Show working tree git diff"
    }
    fn parameters_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "cached": { "type": "boolean", "description": "Diff staged changes" }
            }
        })
    }
    fn risk_tier(&self) -> RiskTier {
        RiskTier::Tier0ReadOnly
    }
    async fn execute(&self, ctx: &ToolContext, args: Value) -> Result<Value> {
        let cached = args.get("cached").and_then(|c| c.as_bool()).unwrap_or(false);
        let mut cmd = Command::new("git");
        cmd.arg("diff");
        if cached {
            cmd.arg("--cached");
        }
        cmd.current_dir(&ctx.workspace_root);

        let output = cmd.output().await.map_err(|e| OdonError::Tool {
            tool: self.name().to_string(),
            message: format!("Failed to run git diff: {}", e),
        })?;

        let text = String::from_utf8_lossy(&output.stdout).to_string();
        Ok(json!({ "diff": text }))
    }
}

pub struct GitCommitTool;

#[async_trait]
impl Tool for GitCommitTool {
    fn name(&self) -> &str {
        "git_commit"
    }
    fn description(&self) -> &str {
        "Stage files and create a git commit"
    }
    fn parameters_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "message": { "type": "string", "description": "Commit message" },
                "stage_all": { "type": "boolean", "description": "Automatically stage tracked modified files" }
            },
            "required": ["message"]
        })
    }
    fn risk_tier(&self) -> RiskTier {
        RiskTier::Tier1WorkspaceMutating
    }
    async fn execute(&self, ctx: &ToolContext, args: Value) -> Result<Value> {
        let msg = args.get("message").and_then(|m| m.as_str()).ok_or_else(|| {
            OdonError::Tool {
                tool: self.name().to_string(),
                message: "Missing 'message' parameter".to_string(),
            }
        })?;
        let stage_all = args.get("stage_all").and_then(|s| s.as_bool()).unwrap_or(true);

        if stage_all {
            let _ = Command::new("git")
                .arg("add")
                .arg("-A")
                .current_dir(&ctx.workspace_root)
                .output()
                .await;
        }

        let output = Command::new("git")
            .arg("commit")
            .arg("-m")
            .arg(msg)
            .current_dir(&ctx.workspace_root)
            .output()
            .await
            .map_err(|e| OdonError::Tool {
                tool: self.name().to_string(),
                message: format!("Failed to create git commit: {}", e),
            })?;

        let stdout = String::from_utf8_lossy(&output.stdout).to_string();
        let stderr = String::from_utf8_lossy(&output.stderr).to_string();

        Ok(json!({
            "success": output.status.success(),
            "output": if output.status.success() { stdout } else { stderr }
        }))
    }
}
