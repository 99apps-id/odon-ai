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
        headers.insert(USER_AGENT, HeaderValue::from_static("odon-github-connector/1.0"));
        headers.insert(CONTENT_TYPE, HeaderValue::from_static("application/json"));
        if let Some(t) = &self.token {
            if let Ok(val) = HeaderValue::from_str(&format!("Bearer {}", t)) {
                headers.insert(AUTHORIZATION, val);
            }
        }
        headers
    }
}

pub struct GitHubGetRepoTool {
    gh: GitHubClient,
}

impl GitHubGetRepoTool {
    pub fn new() -> Self {
        Self { gh: GitHubClient::new() }
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
        let owner = args.get("owner").and_then(|o| o.as_str()).unwrap_or("99apps-id");
        let repo = args.get("repo").and_then(|r| r.as_str()).unwrap_or("odon-ai");

        let url = format!("https://api.github.com/repos/{}/{}", owner, repo);
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

        let body = resp.json::<Value>().await.map_err(|e| OdonError::Tool {
            tool: self.name().to_string(),
            message: format!("Failed to parse GitHub response: {}", e),
        })?;

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
        Self { gh: GitHubClient::new() }
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
        let owner = args.get("owner").and_then(|o| o.as_str()).unwrap_or("99apps-id");
        let repo = args.get("repo").and_then(|r| r.as_str()).unwrap_or("odon-ai");
        let state = args.get("state").and_then(|s| s.as_str()).unwrap_or("open");

        let url = format!("https://api.github.com/repos/{}/{}/issues?state={}&per_page=10", owner, repo, state);
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

        let body = resp.json::<Value>().await.map_err(|e| OdonError::Tool {
            tool: self.name().to_string(),
            message: format!("Failed to parse GitHub response: {}", e),
        })?;

        Ok(json!({ "issues": body }))
    }
}
