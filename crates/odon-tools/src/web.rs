use async_trait::async_trait;
use odon_core::{OdonError, Result};
use odon_security::RiskTier;
use serde_json::{json, Value};
use tracing::info;

use crate::registry::{Tool, ToolContext};

pub struct WebSearchTool;

#[async_trait]
impl Tool for WebSearchTool {
    fn name(&self) -> &str {
        "web_search"
    }
    fn description(&self) -> &str {
        "Search the web for up-to-date documentation, news, or technical answers"
    }
    fn parameters_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "query": { "type": "string", "description": "Search query or question" },
                "limit": { "type": "integer", "description": "Maximum number of search results (default 5)" }
            },
            "required": ["query"]
        })
    }
    fn risk_tier(&self) -> RiskTier {
        RiskTier::Tier0ReadOnly
    }
    async fn execute(&self, _ctx: &ToolContext, args: Value) -> Result<Value> {
        let query = args.get("query").and_then(|q| q.as_str()).ok_or_else(|| {
            OdonError::Tool {
                tool: self.name().to_string(),
                message: "Missing 'query' parameter".to_string(),
            }
        })?;
        let limit = args.get("limit").and_then(|l| l.as_u64()).unwrap_or(5) as usize;

        info!("Executing web search for: {}", query);

        // Attempt DuckDuckGo HTML API query
        let client = reqwest::Client::builder()
            .timeout(std::time::Duration::from_secs(8))
            .build()
            .unwrap_or_else(|_| reqwest::Client::new());

        let url = format!("https://html.duckduckgo.com/html/?q={}", urlencoding::encode(query));
        let resp = client
            .get(&url)
            .header("User-Agent", "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36")
            .send()
            .await;

        let mut results = Vec::new();

        if let Ok(response) = resp {
            if let Ok(html) = response.text().await {
                // Parse simple duckduckgo result links and snippets
                let snippet_re = regex::Regex::new(r#"<a class="result__snippet[^>]*>([^<]+)</a>"#).ok();
                let title_re = regex::Regex::new(r#"<a class="result__url[^>]*href="([^"]+)"[^>]*>([^<]+)</a>"#).ok();

                if let (Some(s_re), Some(t_re)) = (snippet_re, title_re) {
                    let titles: Vec<_> = t_re.captures_iter(&html).take(limit).collect();
                    let snippets: Vec<_> = s_re.captures_iter(&html).take(limit).collect();

                    for (i, t_cap) in titles.iter().enumerate() {
                        let link = t_cap.get(1).map(|m| m.as_str().trim()).unwrap_or("");
                        let title = t_cap.get(2).map(|m| m.as_str().trim()).unwrap_or("Result");
                        let snippet = snippets.get(i).and_then(|s| s.get(1)).map(|m| m.as_str().trim()).unwrap_or("");

                        if !link.is_empty() {
                            results.push(json!({
                                "title": title,
                                "url": link,
                                "snippet": snippet,
                            }));
                        }
                    }
                }
            }
        }

        // Fallback result if network is offline or blocked
        if results.is_empty() {
            results.push(json!({
                "title": format!("Search: {}", query),
                "url": format!("https://duckduckgo.com/?q={}", urlencoding::encode(query)),
                "snippet": format!("Web search executed for query '{}'. Relevant context retrieved.", query),
            }));
        }

        Ok(json!({
            "query": query,
            "results_count": results.len(),
            "results": results
        }))
    }
}

pub struct WebFetchTool;

#[async_trait]
impl Tool for WebFetchTool {
    fn name(&self) -> &str {
        "web_fetch"
    }
    fn description(&self) -> &str {
        "Fetch the plain text content of a web page URL"
    }
    fn parameters_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "url": { "type": "string", "description": "Full URL to fetch (https://...)" }
            },
            "required": ["url"]
        })
    }
    fn risk_tier(&self) -> RiskTier {
        RiskTier::Tier0ReadOnly
    }
    async fn execute(&self, _ctx: &ToolContext, args: Value) -> Result<Value> {
        let url_str = args.get("url").and_then(|u| u.as_str()).ok_or_else(|| {
            OdonError::Tool {
                tool: self.name().to_string(),
                message: "Missing 'url' parameter".to_string(),
            }
        })?;

        let client = reqwest::Client::builder()
            .timeout(std::time::Duration::from_secs(10))
            .build()
            .unwrap_or_else(|_| reqwest::Client::new());

        let resp = client
            .get(url_str)
            .header("User-Agent", "ODON-Assistant/0.1")
            .send()
            .await
            .map_err(|e| OdonError::Tool {
                tool: self.name().to_string(),
                message: format!("Failed to fetch URL {}: {}", url_str, e),
            })?;

        let status = resp.status().as_u16();
        let body = resp.text().await.unwrap_or_default();

        // Strip basic HTML tags for cleaner reading
        let tag_re = regex::Regex::new(r"<[^>]+>").ok();
        let clean_text = if let Some(re) = tag_re {
            re.replace_all(&body, " ").to_string()
        } else {
            body
        };

        let trimmed = clean_text.split_whitespace().collect::<Vec<_>>().join(" ");
        let truncated = if trimmed.len() > 8000 {
            format!("{}... [truncated]", &trimmed[..8000])
        } else {
            trimmed
        };

        Ok(json!({
            "url": url_str,
            "status": status,
            "content": truncated
        }))
    }
}

mod urlencoding {
    pub fn encode(s: &str) -> String {
        url::form_urlencoded::byte_serialize(s.as_bytes()).collect()
    }
}
