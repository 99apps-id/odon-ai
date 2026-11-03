use async_trait::async_trait;
use odon_core::{OdonError, Result};
use odon_security::RiskTier;
use serde_json::{json, Value};
use std::time::Duration;
use tokio::process::Command;
use tokio::time::timeout;

use crate::registry::{Tool, ToolContext};

const GIT_TIMEOUT: Duration = Duration::from_secs(30);
const GIT_OUTPUT_LIMIT: usize = 256 * 1024;

#[derive(Debug)]
pub struct GitOutput {
    pub success: bool,
    pub exit_code: i32,
    pub stdout: String,
    pub stderr: String,
    pub duration_ms: u128,
    pub truncated: bool,
}

impl GitOutput {
    fn into_json(self, extra: Value) -> Value {
        let mut value = extra;
        if let Some(object) = value.as_object_mut() {
            object.insert("success".into(), json!(self.success));
            object.insert("exit_code".into(), json!(self.exit_code));
            object.insert("stdout".into(), json!(self.stdout));
            object.insert("stderr".into(), json!(self.stderr));
            object.insert("duration_ms".into(), json!(self.duration_ms));
            object.insert("truncated".into(), json!(self.truncated));
        }
        value
    }
}

fn clip_output(raw: &[u8]) -> (String, bool) {
    if raw.len() <= GIT_OUTPUT_LIMIT {
        (String::from_utf8_lossy(raw).to_string(), false)
    } else {
        (
            String::from_utf8_lossy(&raw[..GIT_OUTPUT_LIMIT]).to_string(),
            true,
        )
    }
}

fn valid_git_ref(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 200
        && !value.starts_with('-')
        && !value.contains("..")
        && !value.contains(char::is_whitespace)
        && !value.contains(['\\', '<', '>', '|', '"', '\'', '`', '$', ';', '&'])
}

async fn run_git(ctx: &ToolContext, tool_name: &str, args: &[String]) -> Result<GitOutput> {
    let started = std::time::Instant::now();
    let output = timeout(
        GIT_TIMEOUT,
        Command::new("git")
            .args(args)
            .current_dir(&ctx.workspace_root)
            .output(),
    )
    .await
    .map_err(|_| OdonError::Tool {
        tool: tool_name.to_string(),
        message: "git command timed out after 30 seconds".to_string(),
    })?
    .map_err(|e| OdonError::Tool {
        tool: tool_name.to_string(),
        message: format!("Failed to run git: {e}"),
    })?;
    let (stdout, stdout_truncated) = clip_output(&output.stdout);
    let (stderr, stderr_truncated) = clip_output(&output.stderr);
    Ok(GitOutput {
        success: output.status.success(),
        exit_code: output.status.code().unwrap_or(-1),
        stdout,
        stderr,
        duration_ms: started.elapsed().as_millis(),
        truncated: stdout_truncated || stderr_truncated,
    })
}

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
        let output = run_git(ctx, self.name(), &["status".into(), "--short".into()]).await?;
        let status = if output.success {
            output.stdout.clone()
        } else {
            output.stderr.clone()
        };
        Ok(output.into_json(json!({ "status": status })))
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
        let mut args = vec!["diff".to_string()];
        if cached {
            args.push("--cached".into());
        }
        let output = run_git(ctx, self.name(), &args).await?;
        let diff = if output.success {
            output.stdout.clone()
        } else {
            output.stderr.clone()
        };
        Ok(output.into_json(json!({ "diff": diff })))
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
            let added = run_git(ctx, self.name(), &["add".into(), "-A".into()]).await?;
            if !added.success {
                return Err(OdonError::Tool {
                    tool: self.name().to_string(),
                    message: format!("git add -A failed: {}", added.stderr.trim()),
                });
            }
        }

        let output = run_git(
            ctx,
            self.name(),
            &["commit".into(), "-m".into(), msg.to_string()],
        )
        .await?;
        let text = if output.success {
            output.stdout.clone()
        } else {
            output.stderr.clone()
        };
        Ok(output.into_json(json!({ "output": text })))
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
        let limit = args
            .get("limit")
            .and_then(|l| l.as_u64())
            .unwrap_or(10)
            .clamp(1, 100);
        let output = run_git(
            ctx,
            self.name(),
            &["log".into(), format!("-n{limit}"), "--oneline".into()],
        )
        .await?;
        let text = if output.success {
            output.stdout.clone()
        } else {
            output.stderr.clone()
        };
        let commits: Vec<String> = text.lines().map(|s| s.to_string()).collect();
        Ok(output.into_json(json!({
            "commits_count": commits.len(),
            "commits": commits
        })))
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
            if !valid_git_ref(new_branch) {
                return Err(OdonError::Tool {
                    tool: self.name().to_string(),
                    message: "invalid branch name".to_string(),
                });
            }
            let output = run_git(
                ctx,
                self.name(),
                &["checkout".into(), "-b".into(), new_branch.to_string()],
            )
            .await?;
            let message = if output.success {
                output.stdout.clone()
            } else {
                output.stderr.clone()
            };
            return Ok(output.into_json(json!({
                "action": "create",
                "branch": new_branch,
                "message": message
            })));
        }

        let output = run_git(ctx, self.name(), &["branch".into(), "-a".into()]).await?;
        let text = if output.success {
            output.stdout.clone()
        } else {
            output.stderr.clone()
        };
        let branches: Vec<String> = text.lines().map(|s| s.trim().to_string()).collect();
        Ok(output.into_json(json!({
            "action": "list",
            "branches": branches
        })))
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
        let mut paths = Vec::new();
        if let Some(values) = args.get("paths").and_then(|p| p.as_array()) {
            for value in values {
                let path = value.as_str().unwrap_or("");
                if path.is_empty() || path.starts_with('-') {
                    return Err(OdonError::Tool {
                        tool: self.name().to_string(),
                        message: "git_add paths must be non-empty and must not start with '-'"
                            .to_string(),
                    });
                }
                crate::workspace_path::resolve_existing(&ctx.workspace_root, path)?;
                paths.push(path.to_string());
            }
        }
        let mut git_args = vec!["add".to_string()];
        git_args.extend(paths);
        if git_args.len() == 1 {
            git_args.push(".".into());
        }
        let output = run_git(ctx, self.name(), &git_args).await?;
        Ok(output.into_json(json!({})))
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
        if !valid_git_ref(target) {
            return Err(OdonError::Tool {
                tool: self.name().to_string(),
                message: "invalid checkout target".to_string(),
            });
        }
        let output = run_git(ctx, self.name(), &["checkout".into(), target.into()]).await?;
        let message = if output.success {
            output.stdout.clone()
        } else {
            output.stderr.clone()
        };
        Ok(output.into_json(json!({ "target": target, "message": message })))
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
        if !valid_git_ref(object) {
            return Err(OdonError::Tool {
                tool: self.name().to_string(),
                message: "invalid git object or ref".to_string(),
            });
        }
        let output = run_git(
            ctx,
            self.name(),
            &["show".into(), "--stat".into(), object.into()],
        )
        .await?;
        let text = if output.success {
            output.stdout.clone()
        } else {
            output.stderr.clone()
        };
        Ok(output.into_json(json!({ "object": object, "output": text })))
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
        let mut git_args = vec!["stash".into(), "push".into()];
        if let Some(msg) = args.get("message").and_then(|m| m.as_str()) {
            if msg.len() > 500 {
                return Err(OdonError::Tool {
                    tool: self.name().to_string(),
                    message: "stash message is too long".to_string(),
                });
            }
            git_args.push("-m".into());
            git_args.push(msg.to_string());
        }
        let output = run_git(ctx, self.name(), &git_args).await?;
        let text = if output.success {
            output.stdout.clone()
        } else {
            output.stderr.clone()
        };
        Ok(output.into_json(json!({ "output": text })))
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
