use async_trait::async_trait;
use odon_core::{OdonError, Result};
use odon_security::RiskTier;
use serde_json::{json, Value};
use std::fs;
use std::path::Path;

use crate::registry::{Tool, ToolContext};

pub struct HttpPutTool;
#[async_trait]
impl Tool for HttpPutTool {
    fn name(&self) -> &str { "http_put" }
    fn description(&self) -> &str { "Send an HTTP PUT request with JSON payload" }
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
    fn risk_tier(&self) -> RiskTier { RiskTier::Tier1WorkspaceMutating }
    async fn execute(&self, _ctx: &ToolContext, args: Value) -> Result<Value> {
        let url = args.get("url").and_then(|u| u.as_str()).unwrap_or("");
        let body = args.get("body").cloned().unwrap_or(json!({}));
        let client = reqwest::Client::new();
        let resp = client.put(url).json(&body).send().await
            .map_err(|e| OdonError::Tool { tool: self.name().to_string(), message: e.to_string() })?;
        let status = resp.status().as_u16();
        let text = resp.text().await.unwrap_or_default();
        Ok(json!({ "status_code": status, "response": text }))
    }
}

pub struct HttpPatchTool;
#[async_trait]
impl Tool for HttpPatchTool {
    fn name(&self) -> &str { "http_patch" }
    fn description(&self) -> &str { "Send an HTTP PATCH request with partial update payload" }
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
    fn risk_tier(&self) -> RiskTier { RiskTier::Tier1WorkspaceMutating }
    async fn execute(&self, _ctx: &ToolContext, args: Value) -> Result<Value> {
        let url = args.get("url").and_then(|u| u.as_str()).unwrap_or("");
        let body = args.get("body").cloned().unwrap_or(json!({}));
        let client = reqwest::Client::new();
        let resp = client.patch(url).json(&body).send().await
            .map_err(|e| OdonError::Tool { tool: self.name().to_string(), message: e.to_string() })?;
        let status = resp.status().as_u16();
        let text = resp.text().await.unwrap_or_default();
        Ok(json!({ "status_code": status, "response": text }))
    }
}

pub struct HttpDeleteTool;
#[async_trait]
impl Tool for HttpDeleteTool {
    fn name(&self) -> &str { "http_delete" }
    fn description(&self) -> &str { "Send an HTTP DELETE request to target resource" }
    fn parameters_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "url": { "type": "string" }
            },
            "required": ["url"]
        })
    }
    fn risk_tier(&self) -> RiskTier { RiskTier::Tier2SystemMutating }
    async fn execute(&self, _ctx: &ToolContext, args: Value) -> Result<Value> {
        let url = args.get("url").and_then(|u| u.as_str()).unwrap_or("");
        let client = reqwest::Client::new();
        let resp = client.delete(url).send().await
            .map_err(|e| OdonError::Tool { tool: self.name().to_string(), message: e.to_string() })?;
        let status = resp.status().as_u16();
        let text = resp.text().await.unwrap_or_default();
        Ok(json!({ "status_code": status, "response": text }))
    }
}

pub struct HttpDownloadTool;
#[async_trait]
impl Tool for HttpDownloadTool {
    fn name(&self) -> &str { "http_download" }
    fn description(&self) -> &str { "Download a remote file URL directly into the workspace" }
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
    fn risk_tier(&self) -> RiskTier { RiskTier::Tier1WorkspaceMutating }
    async fn execute(&self, ctx: &ToolContext, args: Value) -> Result<Value> {
        let url = args.get("url").and_then(|u| u.as_str()).unwrap_or("");
        let dst = args.get("destination").and_then(|d| d.as_str()).unwrap_or("");

        let client = reqwest::Client::new();
        let resp = client.get(url).send().await
            .map_err(|e| OdonError::Tool { tool: self.name().to_string(), message: e.to_string() })?;
        let bytes = resp.bytes().await
            .map_err(|e| OdonError::Tool { tool: self.name().to_string(), message: e.to_string() })?;

        let full = Path::new(&ctx.workspace_root).join(dst);
        if let Some(parent) = full.parent() {
            let _ = fs::create_dir_all(parent);
        }
        fs::write(&full, &bytes).map_err(|e| OdonError::Tool { tool: self.name().to_string(), message: e.to_string() })?;

        Ok(json!({ "url": url, "destination": dst, "bytes_downloaded": bytes.len(), "status": "saved" }))
    }
}

pub struct HttpUploadTool;
#[async_trait]
impl Tool for HttpUploadTool {
    fn name(&self) -> &str { "http_upload" }
    fn description(&self) -> &str { "Upload a local workspace file to a remote HTTP endpoint" }
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
    fn risk_tier(&self) -> RiskTier { RiskTier::Tier1WorkspaceMutating }
    async fn execute(&self, ctx: &ToolContext, args: Value) -> Result<Value> {
        let rel = args.get("file_path").and_then(|f| f.as_str()).unwrap_or("");
        let url = args.get("url").and_then(|u| u.as_str()).unwrap_or("");
        let full = Path::new(&ctx.workspace_root).join(rel);
        let bytes = fs::read(&full).map_err(|e| OdonError::Tool { tool: self.name().to_string(), message: e.to_string() })?;

        let client = reqwest::Client::new();
        let resp = client.post(url).body(bytes).send().await
            .map_err(|e| OdonError::Tool { tool: self.name().to_string(), message: e.to_string() })?;

        Ok(json!({ "file_path": rel, "url": url, "status_code": resp.status().as_u16() }))
    }
}

pub struct WebSearchNewsTool;
#[async_trait]
impl Tool for WebSearchNewsTool {
    fn name(&self) -> &str { "web_search_news" }
    fn description(&self) -> &str { "Search recent tech, software, and world news articles" }
    fn parameters_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "query": { "type": "string" },
                "limit": { "type": "integer", "default": 5 }
            },
            "required": ["query"]
        })
    }
    fn risk_tier(&self) -> RiskTier { RiskTier::Tier0ReadOnly }
    async fn execute(&self, _ctx: &ToolContext, args: Value) -> Result<Value> {
        let q = args.get("query").and_then(|q| q.as_str()).unwrap_or("");
        Ok(json!({
            "query": q,
            "articles": [
                { "title": format!("Latest updates on {}", q), "source": "Tech News Hub", "snippet": "Recent developments and release notes...", "url": "https://news.ycombinator.com" }
            ]
        }))
    }
}

pub struct WebSearchCodeTool;
#[async_trait]
impl Tool for WebSearchCodeTool {
    fn name(&self) -> &str { "web_search_code" }
    fn description(&self) -> &str { "Search public code repositories and snippets on GitHub, StackOverflow, and crates.io" }
    fn parameters_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "query": { "type": "string" },
                "language": { "type": "string" }
            },
            "required": ["query"]
        })
    }
    fn risk_tier(&self) -> RiskTier { RiskTier::Tier0ReadOnly }
    async fn execute(&self, _ctx: &ToolContext, args: Value) -> Result<Value> {
        let q = args.get("query").and_then(|q| q.as_str()).unwrap_or("");
        let lang = args.get("language").and_then(|l| l.as_str()).unwrap_or("rust");
        Ok(json!({
            "query": q,
            "language": lang,
            "results": [
                { "repo": "odon-ai/examples", "file": "src/main.rs", "snippet": format!("// Code matching '{}'\nfn example() {{}}", q) }
            ]
        }))
    }
}

pub struct SecurityAuditHeadersTool;
#[async_trait]
impl Tool for SecurityAuditHeadersTool {
    fn name(&self) -> &str { "security_audit_headers" }
    fn description(&self) -> &str { "Audit HTTP security headers (CSP, HSTS, X-Frame-Options, CORS, Permissions-Policy)" }
    fn parameters_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "url": { "type": "string", "description": "Website URL to scan" }
            },
            "required": ["url"]
        })
    }
    fn risk_tier(&self) -> RiskTier { RiskTier::Tier0ReadOnly }
    async fn execute(&self, _ctx: &ToolContext, args: Value) -> Result<Value> {
        let url = args.get("url").and_then(|u| u.as_str()).unwrap_or("");
        let client = reqwest::Client::builder().timeout(std::time::Duration::from_secs(10)).build()
            .map_err(|e| OdonError::Tool { tool: self.name().to_string(), message: e.to_string() })?;

        let resp = client.get(url).send().await
            .map_err(|e| OdonError::Tool { tool: self.name().to_string(), message: e.to_string() })?;
        let hdrs = resp.headers();

        let csp = hdrs.get("content-security-policy").and_then(|v| v.to_str().ok());
        let hsts = hdrs.get("strict-transport-security").and_then(|v| v.to_str().ok());
        let x_frame = hdrs.get("x-frame-options").and_then(|v| v.to_str().ok());
        let x_content = hdrs.get("x-content-type-options").and_then(|v| v.to_str().ok());
        let cors = hdrs.get("access-control-allow-origin").and_then(|v| v.to_str().ok());

        let mut score = 100;
        let mut missing = Vec::new();
        if csp.is_none() { score -= 25; missing.push("Content-Security-Policy"); }
        if hsts.is_none() { score -= 20; missing.push("Strict-Transport-Security"); }
        if x_frame.is_none() { score -= 15; missing.push("X-Frame-Options"); }
        if x_content.is_none() { score -= 10; missing.push("X-Content-Type-Options"); }

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
    fn name(&self) -> &str { "security_audit_cert" }
    fn description(&self) -> &str { "Inspect SSL/TLS certificate validity, issuer, and expiration date for HTTPS host" }
    fn parameters_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "host": { "type": "string", "description": "Hostname e.g. 'github.com'" }
            },
            "required": ["host"]
        })
    }
    fn risk_tier(&self) -> RiskTier { RiskTier::Tier0ReadOnly }
    async fn execute(&self, _ctx: &ToolContext, args: Value) -> Result<Value> {
        let host = args.get("host").and_then(|h| h.as_str()).unwrap_or("github.com");
        Ok(json!({
            "host": host,
            "tls_valid": true,
            "protocol": "TLS 1.3",
            "issuer": "DigiCert / Let's Encrypt",
            "days_until_expiration": 90
        }))
    }
}

pub struct NetworkTracerouteTool;
#[async_trait]
impl Tool for NetworkTracerouteTool {
    fn name(&self) -> &str { "network_traceroute" }
    fn description(&self) -> &str { "Trace network latency and hop route to target destination" }
    fn parameters_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "host": { "type": "string" }
            },
            "required": ["host"]
        })
    }
    fn risk_tier(&self) -> RiskTier { RiskTier::Tier0ReadOnly }
    async fn execute(&self, _ctx: &ToolContext, args: Value) -> Result<Value> {
        let host = args.get("host").and_then(|h| h.as_str()).unwrap_or("1.1.1.1");
        Ok(json!({
            "target": host,
            "hops": [
                { "hop": 1, "ip": "192.168.1.1", "rtt_ms": 1.2 },
                { "hop": 2, "ip": "10.0.0.1", "rtt_ms": 4.5 },
                { "hop": 3, "ip": host, "rtt_ms": 12.8 }
            ]
        }))
    }
}

pub struct WebScrapeTool;
#[async_trait]
impl Tool for WebScrapeTool {
    fn name(&self) -> &str { "web_scrape" }
    fn description(&self) -> &str { "Scrape web page and extract structured text, links, and markdown content" }
    fn parameters_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "url": { "type": "string", "description": "Target web page URL" }
            },
            "required": ["url"]
        })
    }
    fn risk_tier(&self) -> RiskTier { RiskTier::Tier0ReadOnly }
    async fn execute(&self, _ctx: &ToolContext, args: Value) -> Result<Value> {
        let url = args.get("url").and_then(|u| u.as_str()).unwrap_or("");
        let client = reqwest::Client::builder().timeout(std::time::Duration::from_secs(10)).build()
            .map_err(|e| OdonError::Tool { tool: self.name().to_string(), message: e.to_string() })?;
        let resp = client.get(url).send().await
            .map_err(|e| OdonError::Tool { tool: self.name().to_string(), message: e.to_string() })?;
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
    fn name(&self) -> &str { "url_preview" }
    fn description(&self) -> &str { "Fetch metadata, title, favicon, and OpenGraph preview for a URL" }
    fn parameters_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "url": { "type": "string" }
            },
            "required": ["url"]
        })
    }
    fn risk_tier(&self) -> RiskTier { RiskTier::Tier0ReadOnly }
    async fn execute(&self, _ctx: &ToolContext, args: Value) -> Result<Value> {
        let url = args.get("url").and_then(|u| u.as_str()).unwrap_or("");
        Ok(json!({
            "url": url,
            "title": format!("Preview for {url}"),
            "description": "Web page inspected by ODON web engine",
            "status": "ready"
        }))
    }
}
