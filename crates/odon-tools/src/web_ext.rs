use async_trait::async_trait;
use odon_core::{OdonError, Result};
use odon_security::RiskTier;
use serde_json::{json, Value};
use std::fs;
use std::path::Path;

use crate::net_guard;
use crate::registry::{Tool, ToolContext};
use crate::web::urlencoding;
use std::path::{Component, PathBuf};

fn safe_workspace_path(root: &str, relative: &str) -> Result<PathBuf> {
    let path = Path::new(relative);
    if path.is_absolute()
        || path.components().any(|component| {
            matches!(
                component,
                Component::ParentDir | Component::RootDir | Component::Prefix(_)
            )
        })
    {
        return Err(OdonError::Tool {
            tool: "filesystem".to_string(),
            message: format!("Path must stay inside the workspace: {relative}"),
        });
    }

    let root = PathBuf::from(root);
    let canonical_root = std::fs::canonicalize(&root).map_err(|error| OdonError::Tool {
        tool: "filesystem".to_string(),
        message: format!("Cannot resolve workspace root {}: {error}", root.display()),
    })?;
    let joined = root.join(path);
    let mut existing_ancestor = joined.as_path();
    while !existing_ancestor.exists() {
        existing_ancestor = existing_ancestor.parent().ok_or_else(|| OdonError::Tool {
            tool: "filesystem".to_string(),
            message: format!("Cannot resolve parent for {relative}"),
        })?;
    }
    let canonical_ancestor =
        std::fs::canonicalize(existing_ancestor).map_err(|error| OdonError::Tool {
            tool: "filesystem".to_string(),
            message: format!("Cannot resolve {}: {error}", existing_ancestor.display()),
        })?;
    if !canonical_ancestor.starts_with(&canonical_root) {
        return Err(OdonError::Tool {
            tool: "filesystem".to_string(),
            message: format!("Path escapes the workspace through a link: {relative}"),
        });
    }
    Ok(joined)
}

#[cfg(test)]
mod path_tests {
    use super::safe_workspace_path;
    use std::fs;
    use std::path::PathBuf;

    fn test_dir(name: &str) -> PathBuf {
        let path = std::env::temp_dir().join(format!(
            "odon-web-ext-{name}-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        fs::create_dir_all(&path).unwrap();
        path
    }

    #[test]
    fn workspace_path_allows_children_and_rejects_parent_traversal() {
        let root = test_dir("inside");
        fs::create_dir_all(root.join("nested")).unwrap();
        assert!(safe_workspace_path(root.to_str().unwrap(), "nested/file.txt").is_ok());
        assert!(safe_workspace_path(root.to_str().unwrap(), "../outside.txt").is_err());
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn workspace_path_rejects_link_to_outside() {
        let root = test_dir("root");
        let outside = test_dir("outside");
        let link = root.join("escape");

        #[cfg(unix)]
        let linked = std::os::unix::fs::symlink(&outside, &link).is_ok();
        #[cfg(windows)]
        let linked = std::os::windows::fs::symlink_dir(&outside, &link).is_ok();

        if linked {
            assert!(safe_workspace_path(root.to_str().unwrap(), "escape/secret.txt").is_err());
            fs::remove_dir_all(&link).ok();
        }
        fs::remove_dir_all(root).ok();
        fs::remove_dir_all(outside).ok();
    }
}

pub struct HttpPutTool;
#[async_trait]
impl Tool for HttpPutTool {
    fn name(&self) -> &str {
        "http_put"
    }
    fn description(&self) -> &str {
        "Send an HTTP PUT request with JSON payload"
    }
    fn parameters_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "url": { "type": "string" },
                "body": { "type": "object" }
            },
            "required": ["url"]
        })
    }
    fn risk_tier(&self) -> RiskTier {
        RiskTier::Tier2SystemMutating
    }
    async fn execute(&self, _ctx: &ToolContext, args: Value) -> Result<Value> {
        let url = args.get("url").and_then(|u| u.as_str()).unwrap_or("");
        let body = args.get("body").cloned().unwrap_or(json!({}));
        let validated = net_guard::validate_url(url).await?;
        let client = reqwest::Client::builder()
            .redirect(net_guard::redirect_policy(
                validated.host_str().unwrap_or("").to_string(),
            ))
            .build()
            .unwrap_or_else(|_| reqwest::Client::new());
        let resp = client
            .put(validated.as_str())
            .json(&body)
            .send()
            .await
            .map_err(|e| OdonError::Tool {
                tool: self.name().to_string(),
                message: e.to_string(),
            })?;
        let status = resp.status().as_u16();
        let text = resp.text().await.unwrap_or_default();
        Ok(json!({ "status_code": status, "response": text }))
    }
}

pub struct HttpPatchTool;
#[async_trait]
impl Tool for HttpPatchTool {
    fn name(&self) -> &str {
        "http_patch"
    }
    fn description(&self) -> &str {
        "Send an HTTP PATCH request with partial update payload"
    }
    fn parameters_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "url": { "type": "string" },
                "body": { "type": "object" }
            },
            "required": ["url"]
        })
    }
    fn risk_tier(&self) -> RiskTier {
        RiskTier::Tier2SystemMutating
    }
    async fn execute(&self, _ctx: &ToolContext, args: Value) -> Result<Value> {
        let url = args.get("url").and_then(|u| u.as_str()).unwrap_or("");
        let body = args.get("body").cloned().unwrap_or(json!({}));
        let validated = net_guard::validate_url(url).await?;
        let client = reqwest::Client::builder()
            .redirect(net_guard::redirect_policy(
                validated.host_str().unwrap_or("").to_string(),
            ))
            .build()
            .unwrap_or_else(|_| reqwest::Client::new());
        let resp = client
            .patch(validated.as_str())
            .json(&body)
            .send()
            .await
            .map_err(|e| OdonError::Tool {
                tool: self.name().to_string(),
                message: e.to_string(),
            })?;
        let status = resp.status().as_u16();
        let text = resp.text().await.unwrap_or_default();
        Ok(json!({ "status_code": status, "response": text }))
    }
}

pub struct HttpDeleteTool;
#[async_trait]
impl Tool for HttpDeleteTool {
    fn name(&self) -> &str {
        "http_delete"
    }
    fn description(&self) -> &str {
        "Send an HTTP DELETE request to target resource"
    }
    fn parameters_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "url": { "type": "string" }
            },
            "required": ["url"]
        })
    }
    fn risk_tier(&self) -> RiskTier {
        RiskTier::Tier2SystemMutating
    }
    async fn execute(&self, _ctx: &ToolContext, args: Value) -> Result<Value> {
        let url = args.get("url").and_then(|u| u.as_str()).unwrap_or("");
        let validated = net_guard::validate_url(url).await?;
        let client = reqwest::Client::builder()
            .redirect(net_guard::redirect_policy(
                validated.host_str().unwrap_or("").to_string(),
            ))
            .build()
            .unwrap_or_else(|_| reqwest::Client::new());
        let resp = client
            .delete(validated.as_str())
            .send()
            .await
            .map_err(|e| OdonError::Tool {
                tool: self.name().to_string(),
                message: e.to_string(),
            })?;
        let status = resp.status().as_u16();
        let text = resp.text().await.unwrap_or_default();
        Ok(json!({ "status_code": status, "response": text }))
    }
}

pub struct HttpDownloadTool;
#[async_trait]
impl Tool for HttpDownloadTool {
    fn name(&self) -> &str {
        "http_download"
    }
    fn description(&self) -> &str {
        "Download a remote file URL directly into the workspace"
    }
    fn parameters_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "url": { "type": "string", "description": "Remote file URL" },
                "destination": { "type": "string", "description": "Local workspace relative file path" }
            },
            "required": ["url", "destination"]
        })
    }
    fn risk_tier(&self) -> RiskTier {
        RiskTier::Tier1WorkspaceMutating
    }
    async fn execute(&self, ctx: &ToolContext, args: Value) -> Result<Value> {
        let url = args.get("url").and_then(|u| u.as_str()).unwrap_or("");
        let dst = args
            .get("destination")
            .and_then(|d| d.as_str())
            .unwrap_or("");
        let validated = net_guard::validate_url(url).await?;
        let full = safe_workspace_path(&ctx.workspace_root, dst)?;

        let client = reqwest::Client::builder()
            .redirect(net_guard::redirect_policy(
                validated.host_str().unwrap_or("").to_string(),
            ))
            .build()
            .unwrap_or_else(|_| reqwest::Client::new());
        let resp = client
            .get(validated.as_str())
            .send()
            .await
            .map_err(|e| OdonError::Tool {
                tool: self.name().to_string(),
                message: e.to_string(),
            })?;
        let bytes = resp.bytes().await.map_err(|e| OdonError::Tool {
            tool: self.name().to_string(),
            message: e.to_string(),
        })?;

        if let Some(parent) = full.parent() {
            let _ = fs::create_dir_all(parent);
        }
        fs::write(&full, &bytes).map_err(|e| OdonError::Tool {
            tool: self.name().to_string(),
            message: e.to_string(),
        })?;

        Ok(
            json!({ "url": url, "destination": dst, "bytes_downloaded": bytes.len(), "status": "saved" }),
        )
    }
}

pub struct HttpUploadTool;
#[async_trait]
impl Tool for HttpUploadTool {
    fn name(&self) -> &str {
        "http_upload"
    }
    fn description(&self) -> &str {
        "Upload a local workspace file to a remote HTTP endpoint"
    }
    fn parameters_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "file_path": { "type": "string" },
                "url": { "type": "string" }
            },
            "required": ["file_path", "url"]
        })
    }
    fn risk_tier(&self) -> RiskTier {
        RiskTier::Tier1WorkspaceMutating
    }
    async fn execute(&self, ctx: &ToolContext, args: Value) -> Result<Value> {
        let rel = args.get("file_path").and_then(|f| f.as_str()).unwrap_or("");
        let url = args.get("url").and_then(|u| u.as_str()).unwrap_or("");
        let full = safe_workspace_path(&ctx.workspace_root, rel)?;
        let bytes = fs::read(&full).map_err(|e| OdonError::Tool {
            tool: self.name().to_string(),
            message: e.to_string(),
        })?;
        let validated = net_guard::validate_url(url).await?;

        let client = reqwest::Client::builder()
            .redirect(net_guard::redirect_policy(
                validated.host_str().unwrap_or("").to_string(),
            ))
            .build()
            .unwrap_or_else(|_| reqwest::Client::new());
        let resp = client
            .post(validated.as_str())
            .body(bytes)
            .send()
            .await
            .map_err(|e| OdonError::Tool {
                tool: self.name().to_string(),
                message: e.to_string(),
            })?;

        Ok(json!({ "file_path": rel, "url": url, "status_code": resp.status().as_u16() }))
    }
}

pub struct WebSearchNewsTool;
#[async_trait]
impl Tool for WebSearchNewsTool {
    fn name(&self) -> &str {
        "web_search_news"
    }
    fn description(&self) -> &str {
        "Search recent tech, software, and world news articles via Google News and HackerNews"
    }
    fn parameters_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "query": { "type": "string", "description": "Topic or news event to search" },
                "limit": { "type": "integer", "description": "Max articles to return", "default": 5 }
            },
            "required": ["query"]
        })
    }
    fn risk_tier(&self) -> RiskTier {
        RiskTier::Tier0ReadOnly
    }
    async fn execute(&self, _ctx: &ToolContext, args: Value) -> Result<Value> {
        let q = args.get("query").and_then(|q| q.as_str()).unwrap_or("");
        let limit = args.get("limit").and_then(|l| l.as_u64()).unwrap_or(5) as usize;

        let client = reqwest::Client::builder()
            .timeout(std::time::Duration::from_secs(10))
            .user_agent("Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36")
            .build()
            .unwrap_or_else(|_| reqwest::Client::new());

        let mut articles = Vec::new();

        // 1. Google News RSS
        let gnews_url = format!(
            "https://news.google.com/rss/search?q={}&hl=id&gl=ID&ceid=ID:id",
            urlencoding::encode(q)
        );
        if let Ok(res) = client.get(&gnews_url).send().await {
            if let Ok(xml) = res.text().await {
                let item_re = regex::Regex::new(r#"(?s)<item>(.*?)</item>"#).ok();
                let title_re = regex::Regex::new(r#"<title>(.*?)</title>"#).ok();
                let link_re = regex::Regex::new(r#"<link>(.*?)</link>"#).ok();
                let pubdate_re = regex::Regex::new(r#"<pubDate>(.*?)</pubDate>"#).ok();
                let source_re = regex::Regex::new(r#"<source[^>]*>(.*?)</source>"#).ok();

                if let (Some(it_re), Some(t_re), Some(l_re)) = (item_re, title_re, link_re) {
                    for cap in it_re.captures_iter(&xml).take(limit) {
                        let item_xml = cap.get(1).map(|m| m.as_str()).unwrap_or("");
                        let title = t_re
                            .captures(item_xml)
                            .and_then(|c| c.get(1))
                            .map(|m| m.as_str().trim())
                            .unwrap_or("");
                        let link = l_re
                            .captures(item_xml)
                            .and_then(|c| c.get(1))
                            .map(|m| m.as_str().trim())
                            .unwrap_or("");
                        let pubdate = pubdate_re
                            .as_ref()
                            .and_then(|r| r.captures(item_xml))
                            .and_then(|c| c.get(1))
                            .map(|m| m.as_str().trim())
                            .unwrap_or("");
                        let source = source_re
                            .as_ref()
                            .and_then(|r| r.captures(item_xml))
                            .and_then(|c| c.get(1))
                            .map(|m| m.as_str().trim())
                            .unwrap_or("Google News");

                        if !title.is_empty() && !link.is_empty() {
                            articles.push(json!({
                                "title": title,
                                "url": link,
                                "source": source,
                                "published_at": pubdate,
                                "snippet": format!("News via {source}: {title}")
                            }));
                        }
                    }
                }
            }
        }

        // 2. HackerNews News search fallback
        if articles.is_empty() {
            let hn_url = format!(
                "https://hn.algolia.com/api/v1/search?query={}&tags=story&hitsPerPage={}",
                urlencoding::encode(q),
                limit
            );
            if let Ok(hn_res) = client.get(&hn_url).send().await {
                if let Ok(val) = hn_res.json::<Value>().await {
                    if let Some(hits) = val.get("hits").and_then(|h| h.as_array()) {
                        for hit in hits {
                            let title = hit.get("title").and_then(|t| t.as_str()).unwrap_or("");
                            let url = hit.get("url").and_then(|u| u.as_str()).unwrap_or("");
                            let author = hit.get("author").and_then(|a| a.as_str()).unwrap_or("");
                            let points = hit.get("points").and_then(|p| p.as_i64()).unwrap_or(0);
                            let date = hit.get("created_at").and_then(|c| c.as_str()).unwrap_or("");

                            if !title.is_empty() {
                                articles.push(json!({
                                    "title": title,
                                    "url": if url.is_empty() { format!("https://news.ycombinator.com/item?id={}", hit.get("objectID").and_then(|o| o.as_str()).unwrap_or("")) } else { url.to_string() },
                                    "source": "HackerNews",
                                    "published_at": date,
                                    "snippet": format!("Posted by {author} ({points} points)")
                                }));
                            }
                        }
                    }
                }
            }
        }

        Ok(json!({
            "query": q,
            "articles_count": articles.len(),
            "articles": articles
        }))
    }
}

pub struct WebSearchCodeTool;
#[async_trait]
impl Tool for WebSearchCodeTool {
    fn name(&self) -> &str {
        "web_search_code"
    }
    fn description(&self) -> &str {
        "Search public code repositories and packages on GitHub and crates.io"
    }
    fn parameters_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "query": { "type": "string", "description": "Search keyword or library name" },
                "language": { "type": "string", "description": "Programming language (e.g. rust, typescript, python)" }
            },
            "required": ["query"]
        })
    }
    fn risk_tier(&self) -> RiskTier {
        RiskTier::Tier0ReadOnly
    }
    async fn execute(&self, _ctx: &ToolContext, args: Value) -> Result<Value> {
        let q = args.get("query").and_then(|q| q.as_str()).unwrap_or("");
        let lang = args.get("language").and_then(|l| l.as_str()).unwrap_or("");

        let client = reqwest::Client::builder()
            .timeout(std::time::Duration::from_secs(10))
            .user_agent("odon-ai-desktop/1.0")
            .build()
            .unwrap_or_else(|_| reqwest::Client::new());

        let mut results = Vec::new();

        // 1. GitHub Repository search
        let query_str = if lang.is_empty() {
            q.to_string()
        } else {
            format!("{}+language:{}", q, lang)
        };
        let gh_url = format!(
            "https://api.github.com/search/repositories?q={}&sort=stars&order=desc&per_page=5",
            urlencoding::encode(&query_str)
        );

        if let Ok(res) = client
            .get(&gh_url)
            .header("Accept", "application/vnd.github.v3+json")
            .send()
            .await
        {
            if let Ok(val) = res.json::<Value>().await {
                if let Some(items) = val.get("items").and_then(|i| i.as_array()) {
                    for item in items {
                        let full_name =
                            item.get("full_name").and_then(|n| n.as_str()).unwrap_or("");
                        let html_url = item.get("html_url").and_then(|u| u.as_str()).unwrap_or("");
                        let desc = item
                            .get("description")
                            .and_then(|d| d.as_str())
                            .unwrap_or("");
                        let stars = item
                            .get("stargazers_count")
                            .and_then(|s| s.as_i64())
                            .unwrap_or(0);
                        let language = item.get("language").and_then(|l| l.as_str()).unwrap_or("");

                        results.push(json!({
                            "repo": full_name,
                            "url": html_url,
                            "description": desc,
                            "stars": stars,
                            "language": language
                        }));
                    }
                }
            }
        }

        Ok(json!({
            "query": q,
            "language": lang,
            "results_count": results.len(),
            "results": results
        }))
    }
}

pub struct SecurityAuditHeadersTool;
#[async_trait]
impl Tool for SecurityAuditHeadersTool {
    fn name(&self) -> &str {
        "security_audit_headers"
    }
    fn description(&self) -> &str {
        "Audit HTTP security headers (CSP, HSTS, X-Frame-Options, CORS, Permissions-Policy)"
    }
    fn parameters_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "url": { "type": "string", "description": "Website URL to scan" }
            },
            "required": ["url"]
        })
    }
    fn risk_tier(&self) -> RiskTier {
        RiskTier::Tier0ReadOnly
    }
    async fn execute(&self, _ctx: &ToolContext, args: Value) -> Result<Value> {
        let url = args.get("url").and_then(|u| u.as_str()).unwrap_or("");
        let validated = net_guard::validate_url(url).await?;
        let client = reqwest::Client::builder()
            .timeout(std::time::Duration::from_secs(10))
            .redirect(net_guard::redirect_policy(
                validated.host_str().unwrap_or("").to_string(),
            ))
            .build()
            .map_err(|e| OdonError::Tool {
                tool: self.name().to_string(),
                message: e.to_string(),
            })?;

        let resp = client
            .get(validated.as_str())
            .send()
            .await
            .map_err(|e| OdonError::Tool {
                tool: self.name().to_string(),
                message: e.to_string(),
            })?;
        let hdrs = resp.headers();

        let csp = hdrs
            .get("content-security-policy")
            .and_then(|v| v.to_str().ok());
        let hsts = hdrs
            .get("strict-transport-security")
            .and_then(|v| v.to_str().ok());
        let x_frame = hdrs.get("x-frame-options").and_then(|v| v.to_str().ok());
        let x_content = hdrs
            .get("x-content-type-options")
            .and_then(|v| v.to_str().ok());
        let cors = hdrs
            .get("access-control-allow-origin")
            .and_then(|v| v.to_str().ok());

        let mut score = 100;
        let mut missing = Vec::new();
        if csp.is_none() {
            score -= 25;
            missing.push("Content-Security-Policy");
        }
        if hsts.is_none() {
            score -= 20;
            missing.push("Strict-Transport-Security");
        }
        if x_frame.is_none() {
            score -= 15;
            missing.push("X-Frame-Options");
        }
        if x_content.is_none() {
            score -= 10;
            missing.push("X-Content-Type-Options");
        }

        Ok(json!({
            "url": url,
            "security_score": score.max(0),
            "missing_headers": missing,
            "headers": {
                "csp": csp,
                "hsts": hsts,
                "x_frame_options": x_frame,
                "x_content_type_options": x_content,
                "cors": cors
            }
        }))
    }
}

pub struct SecurityAuditCertTool;
#[async_trait]
impl Tool for SecurityAuditCertTool {
    fn name(&self) -> &str {
        "security_audit_cert"
    }
    fn description(&self) -> &str {
        "Inspect SSL/TLS certificate validity, issuer, and expiration date for HTTPS host"
    }
    fn parameters_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "host": { "type": "string", "description": "Hostname e.g. 'github.com'" }
            },
            "required": ["host"]
        })
    }
    fn risk_tier(&self) -> RiskTier {
        RiskTier::Tier0ReadOnly
    }
    fn availability(&self) -> crate::registry::ToolAvailability {
        crate::registry::ToolAvailability::Unavailable {
            reason: "TLS certificate inspection is not implemented yet".to_string(),
        }
    }
    async fn execute(&self, _ctx: &ToolContext, args: Value) -> Result<Value> {
        let host = args
            .get("host")
            .and_then(|h| h.as_str())
            .unwrap_or("github.com");
        Err(OdonError::Tool {
            tool: self.name().to_string(),
            message: format!("TLS certificate inspection is not implemented yet ({host})"),
        })
    }
}

pub struct NetworkTracerouteTool;
#[async_trait]
impl Tool for NetworkTracerouteTool {
    fn name(&self) -> &str {
        "network_traceroute"
    }
    fn description(&self) -> &str {
        "Trace network latency and hop route to target destination"
    }
    fn parameters_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "host": { "type": "string" }
            },
            "required": ["host"]
        })
    }
    fn risk_tier(&self) -> RiskTier {
        RiskTier::Tier1WorkspaceMutating
    }
    fn availability(&self) -> crate::registry::ToolAvailability {
        crate::registry::ToolAvailability::Experimental
    }
    async fn execute(&self, _ctx: &ToolContext, args: Value) -> Result<Value> {
        let host = args
            .get("host")
            .and_then(|h| h.as_str())
            .ok_or_else(|| OdonError::Tool {
                tool: self.name().to_string(),
                message: "Missing 'host' parameter".to_string(),
            })?;
        let host = host.trim().trim_end_matches('/');
        if host.is_empty() || host.starts_with('-') || host.len() > 253 {
            return Err(OdonError::Tool {
                tool: self.name().to_string(),
                message: "Invalid traceroute target".to_string(),
            });
        }
        // Reuse the SSRF guard by validating the target as an HTTPS URL.
        let validated = crate::net_guard::validate_url(&format!("https://{host}")).await?;
        let target_host = validated
            .host_str()
            .ok_or_else(|| OdonError::Tool {
                tool: self.name().to_string(),
                message: "Traceroute target has no host".to_string(),
            })?
            .to_string();

        #[cfg(target_os = "windows")]
        let mut command = {
            let mut cmd = tokio::process::Command::new("tracert");
            cmd.args(["-d", "-h", "15", "-w", "1000"]).arg(&target_host);
            cmd
        };
        #[cfg(not(target_os = "windows"))]
        let mut command = {
            let mut cmd = tokio::process::Command::new("traceroute");
            cmd.args(["-m", "15", "-w", "1"]).arg(&target_host);
            cmd
        };

        let started = std::time::Instant::now();
        let output = command
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::piped())
            .output()
            .await
            .map_err(|e| OdonError::Tool {
                tool: self.name().to_string(),
                message: format!(
                    "Could not run traceroute; ensure the system traceroute utility is installed: {e}"
                ),
            })?;
        let stdout = String::from_utf8_lossy(&output.stdout);
        let stderr = String::from_utf8_lossy(&output.stderr);
        let lines: Vec<&str> = stdout.lines().take(32).collect();
        if !output.status.success() {
            return Err(OdonError::Tool {
                tool: self.name().to_string(),
                message: format!(
                    "Traceroute failed with exit code {}: {}",
                    output.status.code().unwrap_or(-1),
                    stderr.trim()
                ),
            });
        }
        Ok(json!({
            "target": target_host,
            "success": output.status.success(),
            "hops": lines,
            "duration_ms": started.elapsed().as_millis(),
            "truncated": stdout.lines().count() > lines.len()
        }))
    }
}

pub struct WebScrapeTool;
#[async_trait]
impl Tool for WebScrapeTool {
    fn name(&self) -> &str {
        "web_scrape"
    }
    fn description(&self) -> &str {
        "Scrape web page and extract structured text, links, and markdown content"
    }
    fn parameters_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "url": { "type": "string", "description": "Target web page URL" }
            },
            "required": ["url"]
        })
    }
    fn risk_tier(&self) -> RiskTier {
        RiskTier::Tier0ReadOnly
    }
    async fn execute(&self, _ctx: &ToolContext, args: Value) -> Result<Value> {
        let url = args.get("url").and_then(|u| u.as_str()).unwrap_or("");
        let validated = net_guard::validate_url(url).await?;
        let client = reqwest::Client::builder()
            .timeout(std::time::Duration::from_secs(10))
            .redirect(net_guard::redirect_policy(
                validated.host_str().unwrap_or("").to_string(),
            ))
            .build()
            .map_err(|e| OdonError::Tool {
                tool: self.name().to_string(),
                message: e.to_string(),
            })?;
        let resp = client
            .get(validated.as_str())
            .send()
            .await
            .map_err(|e| OdonError::Tool {
                tool: self.name().to_string(),
                message: e.to_string(),
            })?;
        let html = resp.text().await.unwrap_or_default();

        let re_tag = regex::Regex::new(r"<[^>]*>").unwrap();
        let cleaned = re_tag.replace_all(&html, " ").to_string();
        let words: Vec<&str> = cleaned.split_whitespace().collect();
        let preview = words.into_iter().take(500).collect::<Vec<&str>>().join(" ");

        Ok(json!({ "url": url, "text_preview": preview, "total_chars": html.len() }))
    }
}

pub struct UrlPreviewTool;
#[async_trait]
impl Tool for UrlPreviewTool {
    fn name(&self) -> &str {
        "url_preview"
    }
    fn description(&self) -> &str {
        "Fetch metadata, title, favicon, and OpenGraph preview for a URL"
    }
    fn parameters_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "url": { "type": "string" }
            },
            "required": ["url"]
        })
    }
    fn risk_tier(&self) -> RiskTier {
        RiskTier::Tier0ReadOnly
    }
    async fn execute(&self, ctx: &ToolContext, args: Value) -> Result<Value> {
        let url = args.get("url").and_then(|u| u.as_str()).unwrap_or("");
        let validated = net_guard::validate_url(url).await?;
        let fetched = crate::web::WebFetchTool
            .execute(ctx, json!({ "url": validated.as_str() }))
            .await?;
        let content = fetched
            .get("content")
            .and_then(|c| c.as_str())
            .unwrap_or("")
            .chars()
            .take(600)
            .collect::<String>();
        Ok(json!({
            "url": validated.as_str(),
            "title": fetched.get("title").cloned().unwrap_or(json!("")),
            "description": content,
            "status": fetched.get("status").cloned().unwrap_or(json!(null)),
            "content": fetched.get("content").cloned().unwrap_or(json!(""))
        }))
    }
}
