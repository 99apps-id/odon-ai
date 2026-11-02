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
        let cached = args
            .get("cached")
            .and_then(|c| c.as_bool())
            .unwrap_or(false);
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
        let msg = args
            .get("message")
            .and_then(|m| m.as_str())
            .ok_or_else(|| OdonError::Tool {
                tool: self.name().to_string(),
                message: "Missing 'message' parameter".to_string(),
            })?;
        let stage_all = args
            .get("stage_all")
            .and_then(|s| s.as_bool())
            .unwrap_or(true);

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

pub struct GitLogTool;

#[async_trait]
impl Tool for GitLogTool {
    fn name(&self) -> &str {
        "git_log"
    }
    fn description(&self) -> &str {
        "View recent git commit history"
    }
    fn parameters_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "limit": { "type": "integer", "description": "Number of commits to retrieve (default 10)" }
            }
        })
    }
    fn risk_tier(&self) -> RiskTier {
        RiskTier::Tier0ReadOnly
    }
    async fn execute(&self, ctx: &ToolContext, args: Value) -> Result<Value> {
        let limit = args.get("limit").and_then(|l| l.as_u64()).unwrap_or(10);
        let output = Command::new("git")
            .arg("log")
            .arg(format!("-n{}", limit))
            .arg("--oneline")
            .current_dir(&ctx.workspace_root)
            .output()
            .await
            .map_err(|e| OdonError::Tool {
                tool: self.name().to_string(),
                message: format!("Failed to run git log: {}", e),
            })?;

        let text = String::from_utf8_lossy(&output.stdout).to_string();
        let commits: Vec<String> = text.lines().map(|s| s.to_string()).collect();
        Ok(json!({
            "commits_count": commits.len(),
            "commits": commits
        }))
    }
}

pub struct GitBranchTool;

#[async_trait]
impl Tool for GitBranchTool {
    fn name(&self) -> &str {
        "git_branch"
    }
    fn description(&self) -> &str {
        "List repository branches or create a new branch"
    }
    fn parameters_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "create": { "type": "string", "description": "New branch name to create and checkout" }
            }
        })
    }
    fn risk_tier(&self) -> RiskTier {
        RiskTier::Tier1WorkspaceMutating
    }
    async fn execute(&self, ctx: &ToolContext, args: Value) -> Result<Value> {
        if let Some(new_branch) = args.get("create").and_then(|b| b.as_str()) {
            let output = Command::new("git")
                .arg("checkout")
                .arg("-b")
                .arg(new_branch)
                .current_dir(&ctx.workspace_root)
                .output()
                .await
                .map_err(|e| OdonError::Tool {
                    tool: self.name().to_string(),
                    message: format!("Failed to create git branch {}: {}", new_branch, e),
                })?;

            Ok(json!({
                "action": "create",
                "branch": new_branch,
                "success": output.status.success()
            }))
        } else {
            let output = Command::new("git")
                .arg("branch")
                .arg("-a")
                .current_dir(&ctx.workspace_root)
                .output()
                .await
                .map_err(|e| OdonError::Tool {
                    tool: self.name().to_string(),
                    message: format!("Failed to list git branches: {}", e),
                })?;

            let text = String::from_utf8_lossy(&output.stdout).to_string();
            let branches: Vec<String> = text.lines().map(|s| s.trim().to_string()).collect();
            Ok(json!({
                "action": "list",
                "branches": branches
            }))
        }
    }
}

pub struct GitAddTool;

#[async_trait]
impl Tool for GitAddTool {
    fn name(&self) -> &str {
        "git_add"
    }
    fn description(&self) -> &str {
        "Stage specific paths or all changes with git add"
    }
    fn parameters_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "paths": { "type": "array", "items": { "type": "string" }, "description": "Specific files to stage (defaults to '.')" }
            }
        })
    }
    fn risk_tier(&self) -> RiskTier {
        RiskTier::Tier1WorkspaceMutating
    }
    async fn execute(&self, ctx: &ToolContext, args: Value) -> Result<Value> {
        let mut cmd = Command::new("git");
        cmd.arg("add");
        if let Some(paths) = args.get("paths").and_then(|p| p.as_array()) {
            for p in paths {
                if let Some(s) = p.as_str() {
                    cmd.arg(s);
                }
            }
        } else {
            cmd.arg(".");
        }
        cmd.current_dir(&ctx.workspace_root);

        let output = cmd.output().await.map_err(|e| OdonError::Tool {
            tool: self.name().to_string(),
            message: format!("Failed to run git add: {}", e),
        })?;

        Ok(json!({ "success": output.status.success() }))
    }
}

pub struct GitCheckoutTool;

#[async_trait]
impl Tool for GitCheckoutTool {
    fn name(&self) -> &str {
        "git_checkout"
    }
    fn description(&self) -> &str {
        "Switch branches or restore working tree files"
    }
    fn parameters_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "target": { "type": "string", "description": "Branch name or file path to checkout" }
            },
            "required": ["target"]
        })
    }
    fn risk_tier(&self) -> RiskTier {
        RiskTier::Tier1WorkspaceMutating
    }
    async fn execute(&self, ctx: &ToolContext, args: Value) -> Result<Value> {
        let target = args
            .get("target")
            .and_then(|t| t.as_str())
            .unwrap_or("HEAD");
        let output = Command::new("git")
            .arg("checkout")
            .arg(target)
            .current_dir(&ctx.workspace_root)
            .output()
            .await
            .map_err(|e| OdonError::Tool {
                tool: self.name().to_string(),
                message: format!("Failed to run git checkout {}: {}", target, e),
            })?;

        let text = String::from_utf8_lossy(&output.stderr).to_string();
        Ok(json!({ "target": target, "success": output.status.success(), "message": text }))
    }
}

pub struct GitShowTool;

#[async_trait]
impl Tool for GitShowTool {
    fn name(&self) -> &str {
        "git_show"
    }
    fn description(&self) -> &str {
        "Show commit logs, file contents, or diff from a commit object"
    }
    fn parameters_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "object": { "type": "string", "description": "Commit SHA or object ref (default: HEAD)" }
            }
        })
    }
    fn risk_tier(&self) -> RiskTier {
        RiskTier::Tier0ReadOnly
    }
    async fn execute(&self, ctx: &ToolContext, args: Value) -> Result<Value> {
        let object = args
            .get("object")
            .and_then(|o| o.as_str())
            .unwrap_or("HEAD");
        let output = Command::new("git")
            .arg("show")
            .arg("--stat")
            .arg(object)
            .current_dir(&ctx.workspace_root)
            .output()
            .await
            .map_err(|e| OdonError::Tool {
                tool: self.name().to_string(),
                message: format!("Failed to run git show {}: {}", object, e),
            })?;

        let text = String::from_utf8_lossy(&output.stdout).to_string();
        Ok(json!({ "object": object, "output": text }))
    }
}

pub struct GitStashTool;

#[async_trait]
impl Tool for GitStashTool {
    fn name(&self) -> &str {
        "git_stash"
    }
    fn description(&self) -> &str {
        "Stash changes in the working directory"
    }
    fn parameters_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "message": { "type": "string", "description": "Optional message for the stash" }
            }
        })
    }
    fn risk_tier(&self) -> RiskTier {
        RiskTier::Tier1WorkspaceMutating
    }
    async fn execute(&self, ctx: &ToolContext, args: Value) -> Result<Value> {
        let mut cmd = Command::new("git");
        cmd.arg("stash").arg("push");
        if let Some(msg) = args.get("message").and_then(|m| m.as_str()) {
            cmd.arg("-m").arg(msg);
        }
        cmd.current_dir(&ctx.workspace_root);

        let output = cmd.output().await.map_err(|e| OdonError::Tool {
            tool: self.name().to_string(),
            message: format!("Failed to run git stash: {}", e),
        })?;

        let text = String::from_utf8_lossy(&output.stdout).to_string();
        Ok(json!({ "output": text, "success": output.status.success() }))
    }
}

pub struct GitStashPopTool;

#[async_trait]
impl Tool for GitStashPopTool {
    fn name(&self) -> &str {
        "git_stash_pop"
    }
    fn description(&self) -> &str {
        "Apply and drop the latest stashed state"
    }
    fn parameters_schema(&self) -> Value {
        json!({ "type": "object", "properties": {} })
    }
    fn risk_tier(&self) -> RiskTier {
        RiskTier::Tier1WorkspaceMutating
    }
    async fn execute(&self, ctx: &ToolContext, _args: Value) -> Result<Value> {
        let output = Command::new("git")
            .arg("stash")
            .arg("pop")
            .current_dir(&ctx.workspace_root)
            .output()
            .await
            .map_err(|e| OdonError::Tool {
                tool: self.name().to_string(),
                message: format!("Failed to run git stash pop: {}", e),
            })?;

        let text = String::from_utf8_lossy(&output.stdout).to_string();
        Ok(json!({ "output": text, "success": output.status.success() }))
    }
}

pub struct GitStashListTool;

#[async_trait]
impl Tool for GitStashListTool {
    fn name(&self) -> &str {
        "git_stash_list"
    }
    fn description(&self) -> &str {
        "List all stashes currently saved in the repository"
    }
    fn parameters_schema(&self) -> Value {
        json!({ "type": "object", "properties": {} })
    }
    fn risk_tier(&self) -> RiskTier {
        RiskTier::Tier0ReadOnly
    }
    async fn execute(&self, ctx: &ToolContext, _args: Value) -> Result<Value> {
        let output = Command::new("git")
            .arg("stash")
            .arg("list")
            .current_dir(&ctx.workspace_root)
            .output()
            .await
            .map_err(|e| OdonError::Tool {
                tool: self.name().to_string(),
                message: format!("Failed to run git stash list: {}", e),
            })?;

        let text = String::from_utf8_lossy(&output.stdout).to_string();
        let stashes: Vec<String> = text.lines().map(|s| s.trim().to_string()).collect();
        Ok(json!({ "stashes": stashes }))
    }
}

pub struct GitPullTool;

#[async_trait]
impl Tool for GitPullTool {
    fn name(&self) -> &str {
        "git_pull"
    }
    fn description(&self) -> &str {
        "Pull changes from remote repository"
    }
    fn parameters_schema(&self) -> Value {
        json!({ "type": "object", "properties": {} })
    }
    fn risk_tier(&self) -> RiskTier {
        RiskTier::Tier2SystemMutating
    }
    async fn execute(&self, ctx: &ToolContext, _args: Value) -> Result<Value> {
        let output = Command::new("git")
            .arg("pull")
            .current_dir(&ctx.workspace_root)
            .output()
            .await
            .map_err(|e| OdonError::Tool {
                tool: self.name().to_string(),
                message: format!("Failed to run git pull: {}", e),
            })?;

        let text = String::from_utf8_lossy(&output.stdout).to_string();
        Ok(json!({ "output": text, "success": output.status.success() }))
    }
}

pub struct GitPushTool;

#[async_trait]
impl Tool for GitPushTool {
    fn name(&self) -> &str {
        "git_push"
    }
    fn description(&self) -> &str {
        "Push committed changes to remote repository"
    }
    fn parameters_schema(&self) -> Value {
        json!({ "type": "object", "properties": {} })
    }
    fn risk_tier(&self) -> RiskTier {
        RiskTier::Tier2SystemMutating
    }
    async fn execute(&self, ctx: &ToolContext, _args: Value) -> Result<Value> {
        let output = Command::new("git")
            .arg("push")
            .current_dir(&ctx.workspace_root)
            .output()
            .await
            .map_err(|e| OdonError::Tool {
                tool: self.name().to_string(),
                message: format!("Failed to run git push: {}", e),
            })?;

        let text = String::from_utf8_lossy(&output.stdout).to_string();
        Ok(json!({ "output": text, "success": output.status.success() }))
    }
}

pub struct GitResetTool;

#[async_trait]
impl Tool for GitResetTool {
    fn name(&self) -> &str {
        "git_reset"
    }
    fn description(&self) -> &str {
        "Reset current HEAD to the specified state"
    }
    fn parameters_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "commit": { "type": "string", "description": "Target commit (default: HEAD)" }
            }
        })
    }
    fn risk_tier(&self) -> RiskTier {
        RiskTier::Tier2SystemMutating
    }
    async fn execute(&self, ctx: &ToolContext, args: Value) -> Result<Value> {
        let commit = args
            .get("commit")
            .and_then(|c| c.as_str())
            .unwrap_or("HEAD");
        let output = Command::new("git")
            .arg("reset")
            .arg(commit)
            .current_dir(&ctx.workspace_root)
            .output()
            .await
            .map_err(|e| OdonError::Tool {
                tool: self.name().to_string(),
                message: format!("Failed to run git reset: {}", e),
            })?;

        let text = String::from_utf8_lossy(&output.stdout).to_string();
        Ok(json!({ "output": text, "success": output.status.success() }))
    }
}
