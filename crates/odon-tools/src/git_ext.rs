use async_trait::async_trait;
use odon_core::{OdonError, Result};
use odon_security::RiskTier;
use reqwest::header::{HeaderMap, HeaderValue, AUTHORIZATION, CONTENT_TYPE, USER_AGENT};
use serde_json::{json, Value};
use std::io::Read;
use std::time::Duration;
use tokio::process::Command;
use tokio::time::timeout;

use crate::registry::{Tool, ToolContext};

const GIT_EXT_TIMEOUT: Duration = Duration::from_secs(60);
const GIT_EXT_OUTPUT_LIMIT: usize = 256 * 1024;

fn clip_output(raw: &[u8]) -> (String, bool) {
    if raw.len() <= GIT_EXT_OUTPUT_LIMIT {
        (String::from_utf8_lossy(raw).to_string(), false)
    } else {
        (
            String::from_utf8_lossy(&raw[..GIT_EXT_OUTPUT_LIMIT]).to_string(),
            true,
        )
    }
}

/// A git ref/revision must not smuggle options or shell-ish characters into argv.
fn valid_git_ref_arg(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 200
        && !value.starts_with('-')
        && !value.contains("..")
        && !value.contains(char::is_whitespace)
        && !value.contains(['\\', '<', '>', '|', '"', '\'', '`', '$', ';', '&'])
}

/// Remote name: simple porcelain-safe identifier.
fn valid_remote_name(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 100
        && !value.starts_with('-')
        && value
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_' || c == '.')
}

/// Reject transport helpers that execute commands (`ext::`, `fd::`) and option
/// injection, while still allowing https/ssh/git/scp-like and local paths.
fn valid_remote_url(value: &str) -> bool {
    if value.is_empty() || value.len() > 2048 || value.starts_with('-') {
        return false;
    }
    if value
        .chars()
        .any(|c| c == '\0' || c.is_control())
    {
        return false;
    }
    let lower = value.to_ascii_lowercase();
    for forbidden in ["ext::", "fd::", "--upload-pack=", "--receive-pack="] {
        if lower.contains(forbidden) {
            return false;
        }
    }
    lower.starts_with("https://")
        || lower.starts_with("http://")
        || lower.starts_with("ssh://")
        || lower.starts_with("git://")
        || lower.starts_with("file://")
        || value.starts_with('/')
        || (value.len() > 2 && value.as_bytes()[1] == b':') // C:\ local path
        || value.contains('@') // scp-like syntax user@host:path
}

async fn run_git(workspace: &str, args: &[&str]) -> Result<(i32, String, String)> {
    if args
        .first()
        .map(|arg| arg.starts_with('-'))
        .unwrap_or(false)
    {
        return Err(OdonError::Tool {
            tool: "git_ext".to_string(),
            message: "git subcommand must not start with '-'".to_string(),
        });
    }
    let mut cmd = Command::new("git");
    cmd.current_dir(workspace);
    cmd.args(args);
    cmd.stdout(std::process::Stdio::piped());
    cmd.stderr(std::process::Stdio::piped());

    let execution = timeout(GIT_EXT_TIMEOUT, cmd.output()).await;
    let output = match execution {
        Ok(output) => output.map_err(|e| OdonError::Tool {
            tool: "git_ext".to_string(),
            message: format!("Failed to run git command: {e}"),
        })?,
        Err(_) => {
            return Err(OdonError::Tool {
                tool: "git_ext".to_string(),
                message: "git command timed out after 60 seconds".to_string(),
            });
        }
    };

    let (stdout, stdout_truncated) = clip_output(&output.stdout);
    let (stderr, stderr_truncated) = clip_output(&output.stderr);
    let code = output
        .status
        .code()
        .unwrap_or(if output.status.success() { 0 } else { 1 });
    let truncated = stdout_truncated || stderr_truncated;
    // Preserve the existing tuple API, but keep bounded output for callers.
    let _ = truncated;
    Ok((code, stdout, stderr))
}

pub struct GitHubApiClient {
    client: reqwest::Client,
    token: Option<String>,
}

impl GitHubApiClient {
    pub fn new() -> Self {
        Self {
            client: reqwest::Client::new(),
            token: std::env::var("GITHUB_TOKEN").ok(),
        }
    }

    fn headers(&self) -> HeaderMap {
        let mut headers = HeaderMap::new();
        headers.insert(
            USER_AGENT,
            HeaderValue::from_static("odon-github-connector/1.0"),
        );
        headers.insert(CONTENT_TYPE, HeaderValue::from_static("application/json"));
        if let Some(t) = &self.token {
            if let Ok(val) = HeaderValue::from_str(&format!("Bearer {}", t)) {
                headers.insert(AUTHORIZATION, val);
            }
        }
        headers
    }

    async fn response_json(tool: &str, response: reqwest::Response) -> Result<Value> {
        let status = response.status();
        let body: Value = response.json().await.unwrap_or_else(|_| json!({}));
        if !status.is_success() {
            return Err(OdonError::Tool {
                tool: tool.to_string(),
                message: format!("GitHub API returned HTTP {status}: {body}"),
            });
        }
        Ok(body)
    }
}

// 1. git_merge
pub struct GitMergeTool;
#[async_trait]
impl Tool for GitMergeTool {
    fn name(&self) -> &str {
        "git_merge"
    }
    fn description(&self) -> &str {
        "Merge specified branch into current working branch"
    }
    fn parameters_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "branch": { "type": "string", "description": "Branch name to merge" },
                "message": { "type": "string", "description": "Merge commit message" }
            },
            "required": ["branch"]
        })
    }
    fn risk_tier(&self) -> RiskTier {
        RiskTier::Tier1WorkspaceMutating
    }
    async fn execute(&self, ctx: &ToolContext, args: Value) -> Result<Value> {
        let branch = args
            .get("branch")
            .and_then(|b| b.as_str())
            .unwrap_or("main");
        let msg = args.get("message").and_then(|m| m.as_str());

        if !valid_git_ref_arg(branch) {
            return Err(OdonError::Tool {
                tool: self.name().to_string(),
                message: format!("invalid merge branch: {branch:?}"),
            });
        }
        let mut git_args = vec!["merge", branch];
        if let Some(m) = msg {
            git_args.push("-m");
            git_args.push(m);
        }
        let (code, stdout, stderr) = run_git(&ctx.workspace_root, &git_args).await?;
        Ok(json!({ "exit_code": code, "stdout": stdout, "stderr": stderr, "merged": code == 0 }))
    }
}

// 2. git_rebase
pub struct GitRebaseTool;
#[async_trait]
impl Tool for GitRebaseTool {
    fn name(&self) -> &str {
        "git_rebase"
    }
    fn description(&self) -> &str {
        "Rebase current branch onto target branch or upstream"
    }
    fn parameters_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "onto": { "type": "string", "description": "Target branch to rebase onto" },
                "abort": { "type": "boolean", "description": "Abort ongoing rebase", "default": false }
            }
        })
    }
    fn risk_tier(&self) -> RiskTier {
        RiskTier::Tier1WorkspaceMutating
    }
    async fn execute(&self, ctx: &ToolContext, args: Value) -> Result<Value> {
        let abort = args.get("abort").and_then(|a| a.as_bool()).unwrap_or(false);
        let onto = args.get("onto").and_then(|o| o.as_str()).unwrap_or("main");

        let git_args = if abort {
            vec!["rebase", "--abort"]
        } else if !valid_git_ref_arg(onto) {
            return Err(OdonError::Tool {
                tool: self.name().to_string(),
                message: format!("invalid rebase target: {onto:?}"),
            });
        } else {
            vec!["rebase", onto]
        };
        let (code, stdout, stderr) = run_git(&ctx.workspace_root, &git_args).await?;
        Ok(json!({ "exit_code": code, "stdout": stdout, "stderr": stderr, "rebased": code == 0 }))
    }
}

// 3. git_cherry_pick
pub struct GitCherryPickTool;
#[async_trait]
impl Tool for GitCherryPickTool {
    fn name(&self) -> &str {
        "git_cherry_pick"
    }
    fn description(&self) -> &str {
        "Apply changes introduced by specified commit SHA"
    }
    fn parameters_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "commit": { "type": "string", "description": "Commit SHA to cherry pick" }
            },
            "required": ["commit"]
        })
    }
    fn risk_tier(&self) -> RiskTier {
        RiskTier::Tier1WorkspaceMutating
    }
    async fn execute(&self, ctx: &ToolContext, args: Value) -> Result<Value> {
        let commit = args.get("commit").and_then(|c| c.as_str()).unwrap_or("");
        if !valid_git_ref_arg(commit) {
            return Err(OdonError::Tool {
                tool: self.name().to_string(),
                message: format!("invalid commit ref: {commit:?}"),
            });
        }
        let (code, stdout, stderr) = run_git(&ctx.workspace_root, &["cherry-pick", commit]).await?;
        Ok(json!({ "commit": commit, "exit_code": code, "stdout": stdout, "stderr": stderr }))
    }
}

// 4. git_revert
pub struct GitRevertTool;
#[async_trait]
impl Tool for GitRevertTool {
    fn name(&self) -> &str {
        "git_revert"
    }
    fn description(&self) -> &str {
        "Revert existing commit by creating an inverse commit"
    }
    fn parameters_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "commit": { "type": "string", "description": "Commit SHA to revert" }
            },
            "required": ["commit"]
        })
    }
    fn risk_tier(&self) -> RiskTier {
        RiskTier::Tier1WorkspaceMutating
    }
    async fn execute(&self, ctx: &ToolContext, args: Value) -> Result<Value> {
        let commit = args.get("commit").and_then(|c| c.as_str()).unwrap_or("");
        if !valid_git_ref_arg(commit) {
            return Err(OdonError::Tool {
                tool: self.name().to_string(),
                message: format!("invalid commit ref: {commit:?}"),
            });
        }
        let (code, stdout, stderr) =
            run_git(&ctx.workspace_root, &["revert", "--no-edit", commit]).await?;
        Ok(json!({ "commit": commit, "exit_code": code, "stdout": stdout, "stderr": stderr }))
    }
}

// 5. git_reset_hard
pub struct GitResetHardTool;
#[async_trait]
impl Tool for GitResetHardTool {
    fn name(&self) -> &str {
        "git_reset_hard"
    }
    fn description(&self) -> &str {
        "Hard reset working directory and HEAD to target commit (destructive)"
    }
    fn parameters_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "target": { "type": "string", "description": "Target commit or ref (default: HEAD)", "default": "HEAD" }
            }
        })
    }
    fn risk_tier(&self) -> RiskTier {
        RiskTier::Tier2SystemMutating
    }
    async fn execute(&self, ctx: &ToolContext, args: Value) -> Result<Value> {
        let target = args
            .get("target")
            .and_then(|t| t.as_str())
            .unwrap_or("HEAD");
        if !valid_git_ref_arg(target) {
            return Err(OdonError::Tool {
                tool: self.name().to_string(),
                message: format!("invalid reset target: {target:?}"),
            });
        }
        let (code, stdout, stderr) =
            run_git(&ctx.workspace_root, &["reset", "--hard", target]).await?;
        Ok(json!({ "target": target, "exit_code": code, "stdout": stdout, "stderr": stderr }))
    }
}

// 6. git_clean
pub struct GitCleanTool;
#[async_trait]
impl Tool for GitCleanTool {
    fn name(&self) -> &str {
        "git_clean"
    }
    fn description(&self) -> &str {
        "Remove untracked files from the working tree"
    }
    fn parameters_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "force": { "type": "boolean", "description": "Force removal", "default": true },
                "dry_run": { "type": "boolean", "description": "Only show what would be removed", "default": false }
            }
        })
    }
    fn risk_tier(&self) -> RiskTier {
        RiskTier::Tier2SystemMutating
    }
    async fn execute(&self, ctx: &ToolContext, args: Value) -> Result<Value> {
        let dry = args
            .get("dry_run")
            .and_then(|d| d.as_bool())
            .unwrap_or(false);
        let git_args = if dry {
            vec!["clean", "-fdn"]
        } else {
            vec!["clean", "-fd"]
        };
        let (code, stdout, stderr) = run_git(&ctx.workspace_root, &git_args).await?;
        Ok(json!({ "dry_run": dry, "exit_code": code, "stdout": stdout, "stderr": stderr }))
    }
}

// 7. git_tag
pub struct GitTagTool;
#[async_trait]
impl Tool for GitTagTool {
    fn name(&self) -> &str {
        "git_tag"
    }
    fn description(&self) -> &str {
        "List existing git tags or create a new release tag"
    }
    fn parameters_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "tag": { "type": "string", "description": "Tag name to create (optional)" },
                "message": { "type": "string", "description": "Tag annotation message" }
            }
        })
    }
    fn risk_tier(&self) -> RiskTier {
        RiskTier::Tier1WorkspaceMutating
    }
    async fn execute(&self, ctx: &ToolContext, args: Value) -> Result<Value> {
        let tag = args.get("tag").and_then(|t| t.as_str());
        let msg = args.get("message").and_then(|m| m.as_str());

        if let Some(t) = tag {
            let mut git_args = vec!["tag", "-a", t];
            if let Some(m) = msg {
                git_args.push("-m");
                git_args.push(m);
            } else {
                git_args.push("-m");
                git_args.push(t);
            }
            let (code, stdout, stderr) = run_git(&ctx.workspace_root, &git_args).await?;
            Ok(json!({ "created_tag": t, "exit_code": code, "stdout": stdout, "stderr": stderr }))
        } else {
            let (code, stdout, _) = run_git(&ctx.workspace_root, &["tag", "-l"]).await?;
            let tags: Vec<&str> = stdout.lines().collect();
            Ok(json!({ "exit_code": code, "tags": tags }))
        }
    }
}

// 8. git_remote_add & 9. git_remote_set_url
pub struct GitRemoteAddTool;
#[async_trait]
impl Tool for GitRemoteAddTool {
    fn name(&self) -> &str {
        "git_remote_add"
    }
    fn description(&self) -> &str {
        "Add a new remote repository origin or upstream"
    }
    fn parameters_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "name": { "type": "string", "description": "Remote name (e.g. origin, upstream)" },
                "url": { "type": "string", "description": "Remote repository URL" }
            },
            "required": ["name", "url"]
        })
    }
    fn risk_tier(&self) -> RiskTier {
        RiskTier::Tier1WorkspaceMutating
    }
    async fn execute(&self, ctx: &ToolContext, args: Value) -> Result<Value> {
        let name = args
            .get("name")
            .and_then(|n| n.as_str())
            .unwrap_or("origin");
        let url = args.get("url").and_then(|u| u.as_str()).unwrap_or("");
        let (code, stdout, stderr) =
            run_git(&ctx.workspace_root, &["remote", "add", name, url]).await?;
        Ok(
            json!({ "remote": name, "url": url, "exit_code": code, "stdout": stdout, "stderr": stderr }),
        )
    }
}

pub struct GitRemoteSetUrlTool;
#[async_trait]
impl Tool for GitRemoteSetUrlTool {
    fn name(&self) -> &str {
        "git_remote_set_url"
    }
    fn description(&self) -> &str {
        "Change the URL of an existing git remote"
    }
    fn parameters_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "name": { "type": "string", "description": "Remote name" },
                "url": { "type": "string", "description": "New URL" }
            },
            "required": ["name", "url"]
        })
    }
    fn risk_tier(&self) -> RiskTier {
        RiskTier::Tier1WorkspaceMutating
    }
    async fn execute(&self, ctx: &ToolContext, args: Value) -> Result<Value> {
        let name = args
            .get("name")
            .and_then(|n| n.as_str())
            .unwrap_or("origin");
        let url = args.get("url").and_then(|u| u.as_str()).unwrap_or("");
        let (code, stdout, stderr) =
            run_git(&ctx.workspace_root, &["remote", "set-url", name, url]).await?;
        Ok(
            json!({ "remote": name, "url": url, "exit_code": code, "stdout": stdout, "stderr": stderr }),
        )
    }
}

// 10. git_blame
pub struct GitBlameTool;
#[async_trait]
impl Tool for GitBlameTool {
    fn name(&self) -> &str {
        "git_blame"
    }
    fn description(&self) -> &str {
        "Show commit and author information for each line of a file"
    }
    fn parameters_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "path": { "type": "string", "description": "File path to blame" },
                "lines": { "type": "string", "description": "Optional line range e.g. '1,50'" }
            },
            "required": ["path"]
        })
    }
    fn risk_tier(&self) -> RiskTier {
        RiskTier::Tier0ReadOnly
    }
    async fn execute(&self, ctx: &ToolContext, args: Value) -> Result<Value> {
        let rel = args.get("path").and_then(|p| p.as_str()).unwrap_or("");
        let lines = args.get("lines").and_then(|l| l.as_str());

        let mut git_args = vec!["blame", "--line-porcelain"];
        if let Some(l) = lines {
            git_args.push("-L");
            git_args.push(l);
        }
        git_args.push(rel);

        let (code, stdout, stderr) = run_git(&ctx.workspace_root, &git_args).await?;
        Ok(json!({ "path": rel, "exit_code": code, "blame": stdout, "stderr": stderr }))
    }
}

// 11. git_log_file
pub struct GitLogFileTool;
#[async_trait]
impl Tool for GitLogFileTool {
    fn name(&self) -> &str {
        "git_log_file"
    }
    fn description(&self) -> &str {
        "View commit history filtered to a specific file"
    }
    fn parameters_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "path": { "type": "string", "description": "Target file path" },
                "limit": { "type": "integer", "description": "Maximum commits", "default": 10 }
            },
            "required": ["path"]
        })
    }
    fn risk_tier(&self) -> RiskTier {
        RiskTier::Tier0ReadOnly
    }
    async fn execute(&self, ctx: &ToolContext, args: Value) -> Result<Value> {
        let rel = args.get("path").and_then(|p| p.as_str()).unwrap_or("");
        let limit = args.get("limit").and_then(|l| l.as_u64()).unwrap_or(10);
        let limit_str = format!("-{}", limit);

        let (code, stdout, _) = run_git(
            &ctx.workspace_root,
            &["log", &limit_str, "--oneline", "--", rel],
        )
        .await?;
        let commits: Vec<&str> = stdout.lines().collect();
        Ok(json!({ "path": rel, "exit_code": code, "commits": commits }))
    }
}

// 12. git_ls_files
pub struct GitLsFilesTool;
#[async_trait]
impl Tool for GitLsFilesTool {
    fn name(&self) -> &str {
        "git_ls_files"
    }
    fn description(&self) -> &str {
        "List all files tracked by the Git repository"
    }
    fn parameters_schema(&self) -> Value {
        json!({ "type": "object", "properties": {} })
    }
    fn risk_tier(&self) -> RiskTier {
        RiskTier::Tier0ReadOnly
    }
    async fn execute(&self, ctx: &ToolContext, _args: Value) -> Result<Value> {
        let (code, stdout, _) = run_git(&ctx.workspace_root, &["ls-files"]).await?;
        let files: Vec<&str> = stdout.lines().collect();
        Ok(json!({ "exit_code": code, "total_tracked": files.len(), "files": files }))
    }
}

// 13. GitHub Advanced Operations
pub struct GitHubCreateReviewTool {
    gh: GitHubApiClient,
}
impl GitHubCreateReviewTool {
    pub fn new() -> Self {
        Self {
            gh: GitHubApiClient::new(),
        }
    }
}

#[async_trait]
impl Tool for GitHubCreateReviewTool {
    fn name(&self) -> &str {
        "github_create_review"
    }
    fn description(&self) -> &str {
        "Submit a formal code review on a GitHub pull request"
    }
    fn parameters_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "owner": { "type": "string" },
                "repo": { "type": "string" },
                "pull_number": { "type": "integer" },
                "event": { "type": "string", "enum": ["APPROVE", "REQUEST_CHANGES", "COMMENT"] },
                "body": { "type": "string" }
            },
            "required": ["owner", "repo", "pull_number", "event", "body"]
        })
    }
    fn risk_tier(&self) -> RiskTier {
        RiskTier::Tier1WorkspaceMutating
    }
    async fn execute(&self, _ctx: &ToolContext, args: Value) -> Result<Value> {
        let owner = args
            .get("owner")
            .and_then(|o| o.as_str())
            .unwrap_or("99apps-id");
        let repo = args
            .get("repo")
            .and_then(|r| r.as_str())
            .unwrap_or("odon-ai");
        let num = args
            .get("pull_number")
            .and_then(|n| n.as_u64())
            .unwrap_or(1);
        let event = args
            .get("event")
            .and_then(|e| e.as_str())
            .unwrap_or("COMMENT");
        let body = args.get("body").and_then(|b| b.as_str()).unwrap_or("");

        let url = format!("https://api.github.com/repos/{owner}/{repo}/pulls/{num}/reviews");
        let resp = self
            .gh
            .client
            .post(&url)
            .headers(self.gh.headers())
            .json(&json!({ "event": event, "body": body }))
            .send()
            .await
            .map_err(|e| OdonError::Tool {
                tool: self.name().to_string(),
                message: e.to_string(),
            })?;

        let data: Value = resp
            .json()
            .await
            .unwrap_or(json!({ "status": "submitted" }));
        Ok(json!({ "pull_number": num, "review": data }))
    }
}

pub struct GitHubListCommentsTool {
    gh: GitHubApiClient,
}
impl GitHubListCommentsTool {
    pub fn new() -> Self {
        Self {
            gh: GitHubApiClient::new(),
        }
    }
}

#[async_trait]
impl Tool for GitHubListCommentsTool {
    fn name(&self) -> &str {
        "github_list_comments"
    }
    fn description(&self) -> &str {
        "List review and issue comments on a pull request or issue"
    }
    fn parameters_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "owner": { "type": "string" },
                "repo": { "type": "string" },
                "issue_number": { "type": "integer" }
            },
            "required": ["owner", "repo", "issue_number"]
        })
    }
    fn risk_tier(&self) -> RiskTier {
        RiskTier::Tier0ReadOnly
    }
    async fn execute(&self, _ctx: &ToolContext, args: Value) -> Result<Value> {
        let owner = args
            .get("owner")
            .and_then(|o| o.as_str())
            .unwrap_or("99apps-id");
        let repo = args
            .get("repo")
            .and_then(|r| r.as_str())
            .unwrap_or("odon-ai");
        let num = args
            .get("issue_number")
            .and_then(|n| n.as_u64())
            .unwrap_or(1);

        let url = format!("https://api.github.com/repos/{owner}/{repo}/issues/{num}/comments");
        let resp = self
            .gh
            .client
            .get(&url)
            .headers(self.gh.headers())
            .send()
            .await
            .map_err(|e| OdonError::Tool {
                tool: self.name().to_string(),
                message: e.to_string(),
            })?;
        let comments: Value = resp.json().await.unwrap_or(json!([]));
        Ok(json!({ "comments": comments }))
    }
}

pub struct GitHubMergePrTool {
    gh: GitHubApiClient,
}
impl GitHubMergePrTool {
    pub fn new() -> Self {
        Self {
            gh: GitHubApiClient::new(),
        }
    }
}

#[async_trait]
impl Tool for GitHubMergePrTool {
    fn name(&self) -> &str {
        "github_merge_pr"
    }
    fn description(&self) -> &str {
        "Merge an approved pull request on GitHub"
    }
    fn parameters_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "owner": { "type": "string" },
                "repo": { "type": "string" },
                "pull_number": { "type": "integer" },
                "merge_method": { "type": "string", "enum": ["merge", "squash", "rebase"], "default": "squash" }
            },
            "required": ["owner", "repo", "pull_number"]
        })
    }
    fn risk_tier(&self) -> RiskTier {
        RiskTier::Tier2SystemMutating
    }
    async fn execute(&self, _ctx: &ToolContext, args: Value) -> Result<Value> {
        let owner = args
            .get("owner")
            .and_then(|o| o.as_str())
            .unwrap_or("99apps-id");
        let repo = args
            .get("repo")
            .and_then(|r| r.as_str())
            .unwrap_or("odon-ai");
        let num = args
            .get("pull_number")
            .and_then(|n| n.as_u64())
            .unwrap_or(1);
        let method = args
            .get("merge_method")
            .and_then(|m| m.as_str())
            .unwrap_or("squash");

        let url = format!("https://api.github.com/repos/{owner}/{repo}/pulls/{num}/merge");
        let resp = self
            .gh
            .client
            .put(&url)
            .headers(self.gh.headers())
            .json(&json!({ "merge_method": method }))
            .send()
            .await
            .map_err(|e| OdonError::Tool {
                tool: self.name().to_string(),
                message: e.to_string(),
            })?;
        let data: Value = resp.json().await.unwrap_or(json!({ "merged": true }));
        Ok(json!({ "pull_number": num, "result": data }))
    }
}

pub struct GitHubWorkflowRunsTool {
    gh: GitHubApiClient,
}
impl GitHubWorkflowRunsTool {
    pub fn new() -> Self {
        Self {
            gh: GitHubApiClient::new(),
        }
    }
}

#[async_trait]
impl Tool for GitHubWorkflowRunsTool {
    fn name(&self) -> &str {
        "github_workflow_runs"
    }
    fn description(&self) -> &str {
        "List recent GitHub Actions runs with status, conclusion, branch, and URL"
    }
    fn parameters_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "owner": { "type": "string" },
                "repo": { "type": "string" },
                "workflow_id": { "type": "string", "description": "Optional workflow ID or filename" },
                "limit": { "type": "integer", "description": "Number of runs (default 10)" }
            },
            "required": ["owner", "repo"]
        })
    }
    fn risk_tier(&self) -> RiskTier {
        RiskTier::Tier0ReadOnly
    }
    async fn execute(&self, _ctx: &ToolContext, args: Value) -> Result<Value> {
        let owner = args.get("owner").and_then(|v| v.as_str()).unwrap_or("");
        let repo = args.get("repo").and_then(|v| v.as_str()).unwrap_or("");
        let limit = args
            .get("limit")
            .and_then(|v| v.as_u64())
            .unwrap_or(10)
            .min(50);
        if owner.is_empty() || repo.is_empty() {
            return Err(OdonError::Tool {
                tool: self.name().to_string(),
                message: "owner and repo are required".to_string(),
            });
        }
        let url = match args.get("workflow_id").and_then(|v| v.as_str()) {
            Some(workflow) if !workflow.is_empty() => format!(
                "https://api.github.com/repos/{owner}/{repo}/actions/workflows/{workflow}/runs?per_page={limit}"
            ),
            _ => format!("https://api.github.com/repos/{owner}/{repo}/actions/runs?per_page={limit}"),
        };
        let response = self
            .gh
            .client
            .get(&url)
            .headers(self.gh.headers())
            .send()
            .await
            .map_err(|e| OdonError::Tool {
                tool: self.name().to_string(),
                message: e.to_string(),
            })?;
        let data: Value = GitHubApiClient::response_json("github_workflow_runs", response).await?;
        Ok(data)
    }
}

pub struct GitHubWorkflowJobsTool {
    gh: GitHubApiClient,
}
impl GitHubWorkflowJobsTool {
    pub fn new() -> Self {
        Self {
            gh: GitHubApiClient::new(),
        }
    }
}

#[async_trait]
impl Tool for GitHubWorkflowJobsTool {
    fn name(&self) -> &str {
        "github_workflow_jobs"
    }
    fn description(&self) -> &str {
        "List jobs and step results for a GitHub Actions run"
    }
    fn parameters_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "owner": { "type": "string" },
                "repo": { "type": "string" },
                "run_id": { "type": "string", "description": "GitHub Actions run ID" }
            },
            "required": ["owner", "repo", "run_id"]
        })
    }
    fn risk_tier(&self) -> RiskTier {
        RiskTier::Tier0ReadOnly
    }
    async fn execute(&self, _ctx: &ToolContext, args: Value) -> Result<Value> {
        let owner = args.get("owner").and_then(|v| v.as_str()).unwrap_or("");
        let repo = args.get("repo").and_then(|v| v.as_str()).unwrap_or("");
        let run_id = args.get("run_id").and_then(|v| v.as_str()).unwrap_or("");
        if owner.is_empty() || repo.is_empty() || run_id.is_empty() {
            return Err(OdonError::Tool {
                tool: self.name().to_string(),
                message: "owner, repo, and run_id are required".to_string(),
            });
        }
        let url = format!("https://api.github.com/repos/{owner}/{repo}/actions/runs/{run_id}/jobs");
        let response = self
            .gh
            .client
            .get(&url)
            .headers(self.gh.headers())
            .send()
            .await
            .map_err(|e| OdonError::Tool {
                tool: self.name().to_string(),
                message: e.to_string(),
            })?;
        let data: Value = GitHubApiClient::response_json("github_workflow_jobs", response).await?;
        Ok(data)
    }
}

pub struct GitHubWorkflowLogsTool {
    gh: GitHubApiClient,
}
impl GitHubWorkflowLogsTool {
    pub fn new() -> Self {
        Self {
            gh: GitHubApiClient::new(),
        }
    }
}

#[async_trait]
impl Tool for GitHubWorkflowLogsTool {
    fn name(&self) -> &str {
        "github_workflow_logs"
    }
    fn description(&self) -> &str {
        "Download and extract GitHub Actions run logs as text, with size limits"
    }
    fn parameters_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "owner": { "type": "string" },
                "repo": { "type": "string" },
                "run_id": { "type": "string", "description": "GitHub Actions run ID" },
                "max_bytes": { "type": "integer", "description": "Maximum extracted text bytes (default 120000)" }
            },
            "required": ["owner", "repo", "run_id"]
        })
    }
    fn risk_tier(&self) -> RiskTier {
        RiskTier::Tier0ReadOnly
    }
    async fn execute(&self, _ctx: &ToolContext, args: Value) -> Result<Value> {
        let owner = args.get("owner").and_then(|v| v.as_str()).unwrap_or("");
        let repo = args.get("repo").and_then(|v| v.as_str()).unwrap_or("");
        let run_id = args.get("run_id").and_then(|v| v.as_str()).unwrap_or("");
        let valid_slug = |value: &str| {
            !value.is_empty()
                && value.len() <= 100
                && value
                    .chars()
                    .all(|c| c.is_ascii_alphanumeric() || matches!(c, '-' | '_' | '.'))
        };
        if !valid_slug(owner)
            || !valid_slug(repo)
            || run_id.is_empty()
            || !run_id.chars().all(|c| c.is_ascii_digit())
            || run_id.len() > 20
        {
            return Err(OdonError::Tool {
                tool: self.name().to_string(),
                message: "owner, repo, and run_id are required and must be valid identifiers"
                    .to_string(),
            });
        }
        let max_bytes = args
            .get("max_bytes")
            .and_then(|v| v.as_u64())
            .unwrap_or(120_000)
            .clamp(1_000, 1_000_000) as usize;
        const MAX_ARCHIVE_BYTES: usize = 64 * 1024 * 1024;
        const MAX_ENTRY_BYTES: usize = 8 * 1024 * 1024;

        let url = format!("https://api.github.com/repos/{owner}/{repo}/actions/runs/{run_id}/logs");
        let response = self
            .gh
            .client
            .get(&url)
            .headers(self.gh.headers())
            .send()
            .await
            .map_err(|e| OdonError::Tool {
                tool: self.name().to_string(),
                message: e.to_string(),
            })?;
        let status = response.status();
        if !status.is_success() {
            let body = response.text().await.unwrap_or_default();
            return Err(OdonError::Tool {
                tool: self.name().to_string(),
                message: format!("GitHub logs download returned HTTP {status}: {body}"),
            });
        }
        if let Some(length) = response.content_length() {
            if length as usize > MAX_ARCHIVE_BYTES {
                return Err(OdonError::Tool {
                    tool: self.name().to_string(),
                    message: format!(
                        "GitHub logs archive is too large: {} bytes (limit {} bytes)",
                        length, MAX_ARCHIVE_BYTES
                    ),
                });
            }
        }
        let bytes = response.bytes().await.map_err(|e| OdonError::Tool {
            tool: self.name().to_string(),
            message: format!("Could not read logs archive: {e}"),
        })?;
        if bytes.len() > MAX_ARCHIVE_BYTES {
            return Err(OdonError::Tool {
                tool: self.name().to_string(),
                message: format!(
                    "GitHub logs archive is too large: {} bytes (limit {} bytes)",
                    bytes.len(),
                    MAX_ARCHIVE_BYTES
                ),
            });
        }
        let cursor = std::io::Cursor::new(bytes);
        let mut archive = zip::ZipArchive::new(cursor).map_err(|e| OdonError::Tool {
            tool: self.name().to_string(),
            message: format!("GitHub logs were not a valid zip archive: {e}"),
        })?;
        if archive.len() > 500 {
            return Err(OdonError::Tool {
                tool: self.name().to_string(),
                message: format!(
                    "GitHub logs archive has too many entries: {}",
                    archive.len()
                ),
            });
        }

        let mut files = Vec::new();
        let mut remaining = max_bytes;
        let mut truncated = false;
        for index in 0..archive.len() {
            if remaining == 0 {
                truncated = true;
                break;
            }
            let entry = match archive.by_index(index) {
                Ok(entry) => entry,
                Err(_) => continue,
            };
            if entry.is_dir() {
                continue;
            }
            let name = entry.name().to_string();
            let mut limited = entry.take(MAX_ENTRY_BYTES as u64);
            let mut buffer = Vec::new();
            let _ = limited.read_to_end(&mut buffer);
            if buffer.len() >= MAX_ENTRY_BYTES {
                truncated = true;
            }
            let text = String::from_utf8_lossy(&buffer);
            if text.trim().is_empty() {
                continue;
            }
            let clipped: String = text.chars().take(remaining).collect();
            let used = clipped.len();
            if used < text.len() {
                truncated = true;
            }
            remaining = remaining.saturating_sub(used);
            files.push(json!({ "file": name, "content": clipped }));
        }

        Ok(json!({
            "run_id": run_id,
            "files": files,
            "extracted_bytes": max_bytes.saturating_sub(remaining),
            "truncated": truncated
        }))
    }
}

pub struct GitHubPrFilesTool {
    gh: GitHubApiClient,
}
impl GitHubPrFilesTool {
    pub fn new() -> Self {
        Self {
            gh: GitHubApiClient::new(),
        }
    }
}

#[async_trait]
impl Tool for GitHubPrFilesTool {
    fn name(&self) -> &str {
        "github_pr_files"
    }
    fn description(&self) -> &str {
        "List changed files and diff stats for a pull request"
    }
    fn parameters_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "owner": { "type": "string" },
                "repo": { "type": "string" },
                "pull_number": { "type": "integer", "description": "Pull request number" }
            },
            "required": ["owner", "repo", "pull_number"]
        })
    }
    fn risk_tier(&self) -> RiskTier {
        RiskTier::Tier0ReadOnly
    }
    async fn execute(&self, _ctx: &ToolContext, args: Value) -> Result<Value> {
        let owner = args.get("owner").and_then(|v| v.as_str()).unwrap_or("");
        let repo = args.get("repo").and_then(|v| v.as_str()).unwrap_or("");
        let number = args
            .get("pull_number")
            .and_then(|v| v.as_u64())
            .unwrap_or(0);
        if owner.is_empty() || repo.is_empty() || number == 0 {
            return Err(OdonError::Tool {
                tool: self.name().to_string(),
                message: "owner, repo, and pull_number are required".to_string(),
            });
        }
        let url = format!(
            "https://api.github.com/repos/{owner}/{repo}/pulls/{number}/files?per_page=100"
        );
        let response = self
            .gh
            .client
            .get(&url)
            .headers(self.gh.headers())
            .send()
            .await
            .map_err(|e| OdonError::Tool {
                tool: self.name().to_string(),
                message: e.to_string(),
            })?;
        let files: Value = GitHubApiClient::response_json("github_pr_files", response).await?;
        let additions: u64 = files
            .as_array()
            .map(|items| {
                items
                    .iter()
                    .filter_map(|f| f.get("additions").and_then(|v| v.as_u64()))
                    .sum()
            })
            .unwrap_or(0);
        let deletions: u64 = files
            .as_array()
            .map(|items| {
                items
                    .iter()
                    .filter_map(|f| f.get("deletions").and_then(|v| v.as_u64()))
                    .sum()
            })
            .unwrap_or(0);
        Ok(
            json!({ "pull_number": number, "files": files, "additions": additions, "deletions": deletions }),
        )
    }
}

pub struct GitHubWorkflowListTool {
    gh: GitHubApiClient,
}
impl GitHubWorkflowListTool {
    pub fn new() -> Self {
        Self {
            gh: GitHubApiClient::new(),
        }
    }
}

#[async_trait]
impl Tool for GitHubWorkflowListTool {
    fn name(&self) -> &str {
        "github_workflow_list"
    }
    fn description(&self) -> &str {
        "List GitHub Actions CI/CD workflows and recent runs"
    }
    fn parameters_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "owner": { "type": "string" },
                "repo": { "type": "string" }
            },
            "required": ["owner", "repo"]
        })
    }
    fn risk_tier(&self) -> RiskTier {
        RiskTier::Tier0ReadOnly
    }
    async fn execute(&self, _ctx: &ToolContext, args: Value) -> Result<Value> {
        let owner = args
            .get("owner")
            .and_then(|o| o.as_str())
            .unwrap_or("99apps-id");
        let repo = args
            .get("repo")
            .and_then(|r| r.as_str())
            .unwrap_or("odon-ai");

        let url = format!("https://api.github.com/repos/{owner}/{repo}/actions/workflows");
        let resp = self
            .gh
            .client
            .get(&url)
            .headers(self.gh.headers())
            .send()
            .await
            .map_err(|e| OdonError::Tool {
                tool: self.name().to_string(),
                message: e.to_string(),
            })?;
        let data: Value = resp.json().await.unwrap_or(json!({ "workflows": [] }));
        Ok(data)
    }
}

pub struct GitHubWorkflowRunTool {
    gh: GitHubApiClient,
}
impl GitHubWorkflowRunTool {
    pub fn new() -> Self {
        Self {
            gh: GitHubApiClient::new(),
        }
    }
}

#[async_trait]
impl Tool for GitHubWorkflowRunTool {
    fn name(&self) -> &str {
        "github_workflow_run"
    }
    fn description(&self) -> &str {
        "Trigger a GitHub Actions workflow dispatch run"
    }
    fn parameters_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "owner": { "type": "string" },
                "repo": { "type": "string" },
                "workflow_id": { "type": "string", "description": "Workflow ID or filename" },
                "ref": { "type": "string", "default": "main" }
            },
            "required": ["owner", "repo", "workflow_id"]
        })
    }
    fn risk_tier(&self) -> RiskTier {
        RiskTier::Tier2SystemMutating
    }
    async fn execute(&self, _ctx: &ToolContext, args: Value) -> Result<Value> {
        let owner = args
            .get("owner")
            .and_then(|o| o.as_str())
            .unwrap_or("99apps-id");
        let repo = args
            .get("repo")
            .and_then(|r| r.as_str())
            .unwrap_or("odon-ai");
        let w_id = args
            .get("workflow_id")
            .and_then(|w| w.as_str())
            .unwrap_or("");
        let r_ref = args.get("ref").and_then(|r| r.as_str()).unwrap_or("main");

        let url = format!(
            "https://api.github.com/repos/{owner}/{repo}/actions/workflows/{w_id}/dispatches"
        );
        let resp = self
            .gh
            .client
            .post(&url)
            .headers(self.gh.headers())
            .json(&json!({ "ref": r_ref }))
            .send()
            .await
            .map_err(|e| OdonError::Tool {
                tool: self.name().to_string(),
                message: e.to_string(),
            })?;
        Ok(
            json!({ "status": "dispatched", "workflow": w_id, "ref": r_ref, "code": resp.status().as_u16() }),
        )
    }
}
// These GitHub connectors are stateless: `new()` is the only constructor and
// takes no arguments, so `Default` is a plain delegation.
impl Default for GitHubApiClient {
    fn default() -> Self {
        Self::new()
    }
}

impl Default for GitHubCreateReviewTool {
    fn default() -> Self {
        Self::new()
    }
}

impl Default for GitHubListCommentsTool {
    fn default() -> Self {
        Self::new()
    }
}

impl Default for GitHubMergePrTool {
    fn default() -> Self {
        Self::new()
    }
}

impl Default for GitHubWorkflowRunsTool {
    fn default() -> Self {
        Self::new()
    }
}

impl Default for GitHubWorkflowJobsTool {
    fn default() -> Self {
        Self::new()
    }
}

impl Default for GitHubWorkflowLogsTool {
    fn default() -> Self {
        Self::new()
    }
}

impl Default for GitHubPrFilesTool {
    fn default() -> Self {
        Self::new()
    }
}

impl Default for GitHubWorkflowListTool {
    fn default() -> Self {
        Self::new()
    }
}

impl Default for GitHubWorkflowRunTool {
    fn default() -> Self {
        Self::new()
    }
}
