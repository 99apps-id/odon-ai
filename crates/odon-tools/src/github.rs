use async_trait::async_trait;
use odon_core::{OdonError, Result};
use odon_security::RiskTier;
use reqwest::header::{HeaderMap, HeaderValue, AUTHORIZATION, CONTENT_TYPE, USER_AGENT};
use serde_json::{json, Value};

use crate::registry::{Tool, ToolContext};

pub struct GitHubClient {
    client: reqwest::Client,
    token: Option<String>,
}

/// API root for one repository, with the owner/repo percent-encoded into the
/// path. Both come from tool arguments (model- or query-controlled), and a
/// raw `/`, `?`, `#` or `..` would rewrite the request path or smuggle extra
/// query parameters against the user's authenticated token.
pub(crate) fn github_repo_api(owner: &str, repo: &str) -> String {
    format!(
        "https://api.github.com/repos/{}/{}",
        crate::web::urlencoding::encode(owner),
        crate::web::urlencoding::encode(repo)
    )
}

impl GitHubClient {
    pub fn new() -> Self {
        let token = std::env::var("GITHUB_TOKEN").ok();
        Self {
            client: reqwest::Client::new(),
            token,
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
        let text = response.text().await.map_err(|e| OdonError::Tool {
            tool: tool.to_string(),
            message: format!("Failed to read GitHub response: {}", e),
        })?;

        if !status.is_success() {
            let detail = serde_json::from_str::<Value>(&text)
                .ok()
                .and_then(|body| {
                    body.get("message")
                        .and_then(Value::as_str)
                        .map(str::to_owned)
                })
                .unwrap_or_else(|| text.chars().take(300).collect());
            return Err(OdonError::Tool {
                tool: tool.to_string(),
                message: format!("GitHub API returned {}: {}", status, detail),
            });
        }

        serde_json::from_str(&text).map_err(|e| OdonError::Tool {
            tool: tool.to_string(),
            message: format!("Failed to parse GitHub response: {}", e),
        })
    }
}

pub struct GitHubGetRepoTool {
    gh: GitHubClient,
}

impl GitHubGetRepoTool {
    pub fn new() -> Self {
        Self {
            gh: GitHubClient::new(),
        }
    }
}

#[async_trait]
impl Tool for GitHubGetRepoTool {
    fn name(&self) -> &str {
        "github_get_repo"
    }
    fn description(&self) -> &str {
        "Get metadata about a GitHub repository (owner, name, stars, default branch)"
    }
    fn parameters_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "owner": { "type": "string", "description": "GitHub owner / organization" },
                "repo": { "type": "string", "description": "Repository name" }
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

        let url = github_repo_api(owner, repo);
        let resp = self
            .gh
            .client
            .get(&url)
            .headers(self.gh.headers())
            .send()
            .await
            .map_err(|e| OdonError::Tool {
                tool: self.name().to_string(),
                message: format!("GitHub request failed: {}", e),
            })?;

        let body = GitHubClient::response_json(self.name(), resp).await?;

        Ok(json!({
            "full_name": body.get("full_name"),
            "description": body.get("description"),
            "default_branch": body.get("default_branch"),
            "open_issues_count": body.get("open_issues_count"),
            "html_url": body.get("html_url")
        }))
    }
}

pub struct GitHubListIssuesTool {
    gh: GitHubClient,
}

impl GitHubListIssuesTool {
    pub fn new() -> Self {
        Self {
            gh: GitHubClient::new(),
        }
    }
}

#[async_trait]
impl Tool for GitHubListIssuesTool {
    fn name(&self) -> &str {
        "github_list_issues"
    }
    fn description(&self) -> &str {
        "List recent issues or PRs for a GitHub repository"
    }
    fn parameters_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "owner": { "type": "string", "description": "Repository owner" },
                "repo": { "type": "string", "description": "Repository name" },
                "state": { "type": "string", "enum": ["open", "closed", "all"], "default": "open" }
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
        let state = args.get("state").and_then(|s| s.as_str()).unwrap_or("open");

        let url = format!(
            "{}/issues?state={}&per_page=10",
            github_repo_api(owner, repo),
            state
        );
        let resp = self
            .gh
            .client
            .get(&url)
            .headers(self.gh.headers())
            .send()
            .await
            .map_err(|e| OdonError::Tool {
                tool: self.name().to_string(),
                message: format!("GitHub request failed: {}", e),
            })?;

        let body = GitHubClient::response_json(self.name(), resp).await?;

        Ok(json!({ "issues": body }))
    }
}

pub struct GitHubCreateIssueTool {
    gh: GitHubClient,
}

impl GitHubCreateIssueTool {
    pub fn new() -> Self {
        Self {
            gh: GitHubClient::new(),
        }
    }
}

#[async_trait]
impl Tool for GitHubCreateIssueTool {
    fn name(&self) -> &str {
        "github_create_issue"
    }
    fn description(&self) -> &str {
        "Create a new issue on a GitHub repository (requires GITHUB_TOKEN)"
    }
    fn parameters_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "owner": { "type": "string", "description": "Repository owner" },
                "repo": { "type": "string", "description": "Repository name" },
                "title": { "type": "string", "description": "Issue title" },
                "body": { "type": "string", "description": "Issue body content" }
            },
            "required": ["owner", "repo", "title"]
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
        let title = args.get("title").and_then(|t| t.as_str()).unwrap_or("");
        let body_text = args.get("body").and_then(|b| b.as_str()).unwrap_or("");

        let url = format!("{}/issues", github_repo_api(owner, repo));
        let resp = self
            .gh
            .client
            .post(&url)
            .headers(self.gh.headers())
            .json(&json!({ "title": title, "body": body_text }))
            .send()
            .await
            .map_err(|e| OdonError::Tool {
                tool: self.name().to_string(),
                message: format!("GitHub create issue failed: {}", e),
            })?;

        let body = GitHubClient::response_json(self.name(), resp).await?;

        Ok(json!({ "status": "created", "issue": body }))
    }
}

pub struct GitHubListPullRequestsTool {
    gh: GitHubClient,
}

impl GitHubListPullRequestsTool {
    pub fn new() -> Self {
        Self {
            gh: GitHubClient::new(),
        }
    }
}

#[async_trait]
impl Tool for GitHubListPullRequestsTool {
    fn name(&self) -> &str {
        "github_list_prs"
    }
    fn description(&self) -> &str {
        "List pull requests for a GitHub repository"
    }
    fn parameters_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "owner": { "type": "string", "description": "Repository owner" },
                "repo": { "type": "string", "description": "Repository name" }
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

        let url = format!("{}/pulls?per_page=10", github_repo_api(owner, repo));
        let resp = self
            .gh
            .client
            .get(&url)
            .headers(self.gh.headers())
            .send()
            .await
            .map_err(|e| OdonError::Tool {
                tool: self.name().to_string(),
                message: format!("GitHub request failed: {}", e),
            })?;

        let body = GitHubClient::response_json(self.name(), resp).await?;

        Ok(json!({ "pull_requests": body }))
    }
}
// These GitHub connectors are stateless: `new()` is the only constructor and
// takes no arguments, so `Default` is a plain delegation.
impl Default for GitHubClient {
    fn default() -> Self {
        Self::new()
    }
}

impl Default for GitHubGetRepoTool {
    fn default() -> Self {
        Self::new()
    }
}

impl Default for GitHubListIssuesTool {
    fn default() -> Self {
        Self::new()
    }
}

impl Default for GitHubCreateIssueTool {
    fn default() -> Self {
        Self::new()
    }
}

impl Default for GitHubListPullRequestsTool {
    fn default() -> Self {
        Self::new()
    }
}
