use async_trait::async_trait;
use odon_core::{OdonError, Result};
use odon_security::RiskTier;
use serde_json::{json, Value};
use std::net::{IpAddr, SocketAddr};
use tracing::info;

use crate::net_guard;
use crate::registry::{Tool, ToolContext};

/// DoH (DNS-over-HTTPS) resolver to bypass ISP blocking/DNS poisoning (e.g. DDG blocking in ID).
pub async fn doh_resolve_host(host: &str) -> Option<IpAddr> {
    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(4))
        .build()
        .ok()?;

    // 1. Try Cloudflare DoH (JSON API)
    let cf_url = format!(
        "https://cloudflare-dns.com/dns-query?name={}&type=A",
        urlencoding::encode(host)
    );
    if let Ok(res) = client
        .get(&cf_url)
        .header("accept", "application/dns-json")
        .send()
        .await
    {
        if res.status().is_success() {
            if let Ok(val) = res.json::<Value>().await {
                if let Some(answers) = val.get("Answer").and_then(|a| a.as_array()) {
                    for ans in answers {
                        if let Some(ip_str) = ans.get("data").and_then(|d| d.as_str()) {
                            if let Ok(ip) = ip_str.parse::<IpAddr>() {
                                return Some(ip);
                            }
                        }
                    }
                }
            }
        }
    }

    // 2. Try Google DoH (JSON API)
    let google_url = format!(
        "https://dns.google/resolve?name={}&type=A",
        urlencoding::encode(host)
    );
    if let Ok(res) = client
        .get(&google_url)
        .header("accept", "application/json")
        .send()
        .await
    {
        if res.status().is_success() {
            if let Ok(val) = res.json::<Value>().await {
                if let Some(answers) = val.get("Answer").and_then(|a| a.as_array()) {
                    for ans in answers {
                        if let Some(ip_str) = ans.get("data").and_then(|d| d.as_str()) {
                            if let Ok(ip) = ip_str.parse::<IpAddr>() {
                                return Some(ip);
                            }
                        }
                    }
                }
            }
        }
    }

    None
}

fn decode_ddg_url(raw: &str) -> String {
    if let Some(idx) = raw.find("uddg=") {
        let after = &raw[idx + 5..];
        let end = after.find('&').unwrap_or(after.len());
        let slice = &after[..end];
        let decoded = url::form_urlencoded::parse(format!("u={}", slice).as_bytes())
            .find(|(k, _)| k == "u")
            .map(|(_, v)| v.into_owned())
            .unwrap_or_else(|| slice.to_string());
        return decoded;
    }
    if raw.starts_with("//") {
        return format!("https:{}", raw);
    }
    raw.to_string()
}

pub fn clean_html_snippet(raw: &str) -> String {
    // Strip non-visible blocks before removing tags; otherwise CSS and JS text
    // leaks into the "readable" page sent to the agent.
    let mut visible = raw.to_string();
    for pattern in [
        r"(?is)<script[^>]*>.*?</script>",
        r"(?is)<style[^>]*>.*?</style>",
        r"(?is)<noscript[^>]*>.*?</noscript>",
        r"(?is)<svg[^>]*>.*?</svg>",
        r"(?is)<!--.*?-->",
    ] {
        if let Ok(re) = regex::Regex::new(pattern) {
            visible = re.replace_all(&visible, " ").to_string();
        }
    }
    let re = regex::Regex::new(r"<[^>]+>").ok();
    let text = if let Some(r) = re {
        r.replace_all(&visible, " ").to_string()
    } else {
        visible
    };
    text.replace("&amp;", "&")
        .replace("&quot;", "\"")
        .replace("&#x27;", "'")
        .replace("&lt;", "<")
        .replace("&gt;", ">")
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
}

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
        let query = args
            .get("query")
            .and_then(|q| q.as_str())
            .ok_or_else(|| OdonError::Tool {
                tool: self.name().to_string(),
                message: "Missing 'query' parameter".to_string(),
            })?;
        let limit = args.get("limit").and_then(|l| l.as_u64()).unwrap_or(5) as usize;

        info!("Executing web search for: {}", query);

        // 1. Attempt DuckDuckGo HTML using DoH resolved IP to bypass ISP blocking
        let ddg_ip = doh_resolve_host("html.duckduckgo.com").await;
        let mut client_builder = reqwest::Client::builder()
            .timeout(std::time::Duration::from_secs(8))
            .user_agent("Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/130.0.0.0 Safari/537.36");

        if let Some(ip) = ddg_ip {
            client_builder =
                client_builder.resolve("html.duckduckgo.com", SocketAddr::new(ip, 443));
        }
        let client = client_builder
            .build()
            .unwrap_or_else(|_| reqwest::Client::new());

        let url = format!(
            "https://html.duckduckgo.com/html/?q={}",
            urlencoding::encode(query)
        );
        let resp = client
            .get(&url)
            .header(
                "Accept",
                "text/html,application/xhtml+xml,application/xml;q=0.9,*/*;q=0.8",
            )
            .header("Accept-Language", "en-US,en;q=0.9,id;q=0.8")
            .send()
            .await;

        let mut results = Vec::new();

        if let Ok(response) = resp {
            if let Ok(html) = response.text().await {
                let snippet_re =
                    regex::Regex::new(r#"<a class="result__snippet[^>]*>([\s\S]*?)</a>"#).ok();
                let link_re = regex::Regex::new(
                    r#"<a class="result__url[^>]*href="([^"]+)"[^>]*>([\s\S]*?)</a>"#,
                )
                .ok();

                if let (Some(s_re), Some(l_re)) = (snippet_re, link_re) {
                    let links: Vec<_> = l_re.captures_iter(&html).take(limit).collect();
                    let snippets: Vec<_> = s_re.captures_iter(&html).take(limit).collect();

                    for (i, l_cap) in links.iter().enumerate() {
                        let raw_link = l_cap.get(1).map(|m| m.as_str().trim()).unwrap_or("");
                        let title = clean_html_snippet(
                            l_cap.get(2).map(|m| m.as_str().trim()).unwrap_or("Result"),
                        );
                        let snippet = snippets
                            .get(i)
                            .and_then(|s| s.get(1))
                            .map(|m| clean_html_snippet(m.as_str().trim()))
                            .unwrap_or_default();
                        let link = decode_ddg_url(raw_link);

                        if !link.is_empty() {
                            results.push(json!({
                                "title": title,
                                "url": link,
                                "snippet": snippet,
                                "source": "duckduckgo"
                            }));
                        }
                    }
                }
            }
        }

        // 2. Fallback to Wikipedia OpenSearch API if DDG HTML was empty or blocked
        if results.is_empty() {
            let wiki_url = format!(
                "https://en.wikipedia.org/w/api.php?action=opensearch&search={}&limit={}&namespace=0&format=json",
                urlencoding::encode(query),
                limit
            );
            if let Ok(wiki_res) = client.get(&wiki_url).send().await {
                if let Ok(val) = wiki_res.json::<Value>().await {
                    if let Some(arr) = val.as_array() {
                        if arr.len() >= 4 {
                            let titles = arr[1].as_array();
                            let snippets = arr[2].as_array();
                            let urls = arr[3].as_array();
                            if let (Some(t_arr), Some(s_arr), Some(u_arr)) =
                                (titles, snippets, urls)
                            {
                                for i in 0..t_arr.len() {
                                    let t = t_arr.get(i).and_then(|x| x.as_str()).unwrap_or("");
                                    let s = s_arr.get(i).and_then(|x| x.as_str()).unwrap_or("");
                                    let u = u_arr.get(i).and_then(|x| x.as_str()).unwrap_or("");
                                    if !u.is_empty() {
                                        results.push(json!({
                                            "title": t,
                                            "url": u,
                                            "snippet": s,
                                            "source": "wikipedia"
                                        }));
                                    }
                                }
                            }
                        }
                    }
                }
            }
        }

        // 3. Fallback to HackerNews Algolia API
        if results.is_empty() {
            let hn_url = format!(
                "https://hn.algolia.com/api/v1/search?query={}&tags=story&hitsPerPage={}",
                urlencoding::encode(query),
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
                            if !title.is_empty() {
                                results.push(json!({
                                    "title": title,
                                    "url": if url.is_empty() { format!("https://news.ycombinator.com/item?id={}", hit.get("objectID").and_then(|o| o.as_str()).unwrap_or("")) } else { url.to_string() },
                                    "snippet": format!("Posted by {} ({} points)", author, points),
                                    "source": "hackernews"
                                }));
                            }
                        }
                    }
                }
            }
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
        "Fetch the plain text content of a web page URL (with automatic DoH resolution fallback)"
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
        let url_str = args
            .get("url")
            .and_then(|u| u.as_str())
            .ok_or_else(|| OdonError::Tool {
                tool: self.name().to_string(),
                message: "Missing 'url' parameter".to_string(),
            })?;

        let validated = net_guard::validate_url(url_str).await?;
        let url_str = validated.as_str();
        let host = validated.host_str().unwrap_or("").to_string();
        let client = reqwest::Client::builder()
            .timeout(std::time::Duration::from_secs(12))
            .redirect(net_guard::redirect_policy(host.clone()))
            .user_agent("Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/130.0.0.0 Safari/537.36")
            .build()
            .unwrap_or_else(|_| reqwest::Client::new());

        let resp_res = client.get(url_str).send().await;

        let resp = match resp_res {
            Ok(r) => Ok(r),
            Err(e) => {
                // If failed due to DNS resolution error or connect error, attempt DoH fallback
                let parsed_url = reqwest::Url::parse(url_str).ok();
                if let Some(u) = parsed_url {
                    if let Some(host) = u.host_str() {
                        let port = u.port().unwrap_or(if u.scheme() == "https" { 443 } else { 80 });
                        if let Some(ip) = doh_resolve_host(host).await {
                            if !net_guard::allow_private_network() && net_guard::is_blocked_ip(ip) {
                                return Err(OdonError::Tool {
                                    tool: self.name().to_string(),
                                    message: format!("DoH resolved {host} to a private address ({ip}); request blocked"),
                                });
                            }
                            if let Ok(doh_client) = reqwest::Client::builder()
                                .timeout(std::time::Duration::from_secs(12))
                                .redirect(net_guard::redirect_policy(host.to_string()))
                                .user_agent("Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/130.0.0.0 Safari/537.36")
                                .resolve(host, SocketAddr::new(ip, port))
                                .build()
                            {
                                doh_client.get(url_str).send().await
                            } else {
                                Err(e)
                            }
                        } else {
                            Err(e)
                        }
                    } else {
                        Err(e)
                    }
                } else {
                    Err(e)
                }
            }
        }.map_err(|e| OdonError::Tool {
            tool: self.name().to_string(),
            message: format!("Failed to fetch URL {}: {}", url_str, e),
        })?;

        let status = resp.status().as_u16();
        let body = resp.text().await.unwrap_or_default();

        // Extract <title> if present
        let title_re = regex::Regex::new(r#"(?i)<title[^>]*>([^<]+)</title>"#).ok();
        let page_title = title_re
            .as_ref()
            .and_then(|r| r.captures(&body))
            .and_then(|c| c.get(1))
            .map(|m| m.as_str().trim().to_string())
            .unwrap_or_default();

        let clean_text = clean_html_snippet(&body);
        let truncated = if clean_text.len() > 10000 {
            format!("{}... [truncated]", &clean_text[..10000])
        } else {
            clean_text
        };

        Ok(json!({
            "url": url_str,
            "status": status,
            "title": page_title,
            "content": truncated
        }))
    }
}

pub struct ProbeUrlTool;

#[async_trait]
impl Tool for ProbeUrlTool {
    fn name(&self) -> &str {
        "probe_url"
    }
    fn description(&self) -> &str {
        "Probe a URL for HTTP status, response headers, and latency"
    }
    fn parameters_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "url": { "type": "string", "description": "URL to probe" }
            },
            "required": ["url"]
        })
    }
    fn risk_tier(&self) -> RiskTier {
        RiskTier::Tier0ReadOnly
    }
    async fn execute(&self, _ctx: &ToolContext, args: Value) -> Result<Value> {
        let url_str = args
            .get("url")
            .and_then(|u| u.as_str())
            .ok_or_else(|| OdonError::Tool {
                tool: self.name().to_string(),
                message: "Missing 'url' parameter".to_string(),
            })?;

        let validated = net_guard::validate_url(url_str).await?;
        let url_str = validated.as_str();
        let client = reqwest::Client::builder()
            .timeout(std::time::Duration::from_secs(6))
            .redirect(net_guard::redirect_policy(
                validated.host_str().unwrap_or("").to_string(),
            ))
            .build()
            .unwrap_or_else(|_| reqwest::Client::new());

        let start = std::time::Instant::now();
        let resp = client
            .head(url_str)
            .send()
            .await
            .map_err(|e| OdonError::Tool {
                tool: self.name().to_string(),
                message: format!("Probe error on {}: {}", url_str, e),
            })?;

        let elapsed = start.elapsed().as_millis();
        let status = resp.status().as_u16();
        let headers: serde_json::Map<String, Value> = resp
            .headers()
            .iter()
            .map(|(k, v)| (k.to_string(), json!(v.to_str().unwrap_or(""))))
            .collect();

        Ok(json!({
            "url": url_str,
            "status": status,
            "latency_ms": elapsed,
            "headers": headers
        }))
    }
}

pub struct HttpGetTool;

#[async_trait]
impl Tool for HttpGetTool {
    fn name(&self) -> &str {
        "http_get"
    }
    fn description(&self) -> &str {
        "Send an HTTP GET request and retrieve raw status and body"
    }
    fn parameters_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "url": { "type": "string", "description": "HTTP URL to fetch" }
            },
            "required": ["url"]
        })
    }
    fn risk_tier(&self) -> RiskTier {
        RiskTier::Tier0ReadOnly
    }
    async fn execute(&self, _ctx: &ToolContext, args: Value) -> Result<Value> {
        WebFetchTool.execute(_ctx, args).await
    }
}

pub struct HttpPostTool;

#[async_trait]
impl Tool for HttpPostTool {
    fn name(&self) -> &str {
        "http_post"
    }
    fn description(&self) -> &str {
        "Send an HTTP POST request with a JSON body"
    }
    fn parameters_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "url": { "type": "string", "description": "Target HTTP URL" },
                "body": { "type": "object", "description": "JSON payload" }
            },
            "required": ["url"]
        })
    }
    fn risk_tier(&self) -> RiskTier {
        RiskTier::Tier1WorkspaceMutating
    }
    async fn execute(&self, _ctx: &ToolContext, args: Value) -> Result<Value> {
        let url_str = args
            .get("url")
            .and_then(|u| u.as_str())
            .ok_or_else(|| OdonError::Tool {
                tool: self.name().to_string(),
                message: "Missing 'url' parameter".to_string(),
            })?;
        let body = args.get("body").unwrap_or(&Value::Null);
        let validated = net_guard::validate_url(url_str).await?;
        let url_str = validated.as_str();

        let client = reqwest::Client::builder()
            .timeout(std::time::Duration::from_secs(10))
            .redirect(net_guard::redirect_policy(
                validated.host_str().unwrap_or("").to_string(),
            ))
            .build()
            .unwrap_or_else(|_| reqwest::Client::new());

        let resp = client
            .post(url_str)
            .json(body)
            .send()
            .await
            .map_err(|e| OdonError::Tool {
                tool: self.name().to_string(),
                message: format!("POST request failed to {}: {}", url_str, e),
            })?;

        let status = resp.status().as_u16();
        let text = resp.text().await.unwrap_or_default();

        Ok(json!({
            "url": url_str,
            "status": status,
            "response": text
        }))
    }
}

pub struct DnsLookupTool;

#[async_trait]
impl Tool for DnsLookupTool {
    fn name(&self) -> &str {
        "dns_lookup"
    }
    fn description(&self) -> &str {
        "Resolve hostnames to IP addresses for security and network analysis"
    }
    fn parameters_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "host": { "type": "string", "description": "Domain or hostname to resolve (e.g. 'example.com')" }
            },
            "required": ["host"]
        })
    }
    fn risk_tier(&self) -> RiskTier {
        RiskTier::Tier0ReadOnly
    }
    async fn execute(&self, _ctx: &ToolContext, args: Value) -> Result<Value> {
        let host = args
            .get("host")
            .and_then(|h| h.as_str())
            .ok_or_else(|| OdonError::Tool {
                tool: self.name().to_string(),
                message: "Missing 'host' parameter".to_string(),
            })?;

        let addrs = tokio::net::lookup_host(format!("{}:80", host))
            .await
            .map_err(|e| OdonError::Tool {
                tool: self.name().to_string(),
                message: format!("DNS resolution error for {}: {}", host, e),
            })?;

        let ips: Vec<String> = addrs.map(|a| a.ip().to_string()).collect();
        Ok(json!({ "host": host, "resolved_ips": ips }))
    }
}

pub struct PortScanTool;

#[async_trait]
impl Tool for PortScanTool {
    fn name(&self) -> &str {
        "port_scan"
    }
    fn description(&self) -> &str {
        "Perform a TCP connect port check on authorized targets"
    }
    fn parameters_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "host": { "type": "string", "description": "Target IP or hostname" },
                "ports": { "type": "array", "items": { "type": "integer" }, "description": "Ports to probe (e.g. [80, 443, 22, 8080])" }
            },
            "required": ["host"]
        })
    }
    fn risk_tier(&self) -> RiskTier {
        RiskTier::Tier1WorkspaceMutating
    }
    async fn execute(&self, _ctx: &ToolContext, args: Value) -> Result<Value> {
        let host = args
            .get("host")
            .and_then(|h| h.as_str())
            .ok_or_else(|| OdonError::Tool {
                tool: self.name().to_string(),
                message: "Missing 'host' parameter".to_string(),
            })?;

        let default_ports = vec![json!(22), json!(80), json!(443), json!(3000), json!(8080)];
        let ports_arr = args
            .get("ports")
            .and_then(|p| p.as_array())
            .unwrap_or(&default_ports);

        let mut results = Vec::new();
        for p in ports_arr {
            let port = p.as_u64().unwrap_or(80) as u16;
            let target = format!("{}:{}", host, port);
            let is_open = tokio::time::timeout(
                std::time::Duration::from_millis(1500),
                tokio::net::TcpStream::connect(&target),
            )
            .await
            .map(|r| r.is_ok())
            .unwrap_or(false);

            results.push(json!({
                "port": port,
                "open": is_open
            }));
        }

        Ok(json!({ "host": host, "results": results }))
    }
}

pub struct BrowserOpenTool;

#[async_trait]
impl Tool for BrowserOpenTool {
    fn name(&self) -> &str {
        "browser_open"
    }
    fn description(&self) -> &str {
        "Open a web page or URL in the system browser or embedded viewer"
    }
    fn parameters_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "url": { "type": "string", "description": "The URL to open in browser" }
            },
            "required": ["url"]
        })
    }
    fn risk_tier(&self) -> RiskTier {
        RiskTier::Tier1WorkspaceMutating
    }
    async fn execute(&self, _ctx: &ToolContext, args: Value) -> Result<Value> {
        let url_str = args
            .get("url")
            .and_then(|u| u.as_str())
            .ok_or_else(|| OdonError::Tool {
                tool: self.name().to_string(),
                message: "Missing 'url' parameter".to_string(),
            })?;

        let validated = net_guard::validate_url(url_str).await?;
        let url_str = validated.as_str();

        #[cfg(target_os = "windows")]
        let res = tokio::process::Command::new("explorer.exe")
            .arg(url_str)
            .spawn();

        #[cfg(target_os = "macos")]
        let res = tokio::process::Command::new("open").arg(url_str).spawn();

        #[cfg(target_os = "linux")]
        let res = tokio::process::Command::new("xdg-open")
            .arg(url_str)
            .spawn();

        match res {
            Ok(_) => Ok(json!({
                "status": "opened",
                "url": url_str
            })),
            Err(e) => Err(OdonError::Tool {
                tool: self.name().to_string(),
                message: format!("Failed to open browser: {}", e),
            }),
        }
    }
}

pub struct BrowserNavigateTool;

#[async_trait]
impl Tool for BrowserNavigateTool {
    fn name(&self) -> &str {
        "browser_navigate"
    }
    fn description(&self) -> &str {
        "Navigate browser to URL, inspect HTTP status, headers, and extract title and rendered page content"
    }
    fn parameters_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "url": { "type": "string", "description": "Target webpage URL to inspect" }
            },
            "required": ["url"]
        })
    }
    fn risk_tier(&self) -> RiskTier {
        RiskTier::Tier0ReadOnly
    }
    async fn execute(&self, ctx: &ToolContext, args: Value) -> Result<Value> {
        WebFetchTool.execute(ctx, args).await
    }
}

pub struct BrowserScreenshotTool;

#[async_trait]
impl Tool for BrowserScreenshotTool {
    fn name(&self) -> &str {
        "browser_screenshot"
    }
    fn description(&self) -> &str {
        "Capture a full-page or viewport screenshot of a target URL and save image to workspace"
    }
    fn parameters_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "url": { "type": "string", "description": "Webpage URL to capture" },
                "output_path": { "type": "string", "description": "Relative file path to save PNG (e.g. 'screenshot.png')" }
            },
            "required": ["url"]
        })
    }
    fn risk_tier(&self) -> RiskTier {
        RiskTier::Tier1WorkspaceMutating
    }
    async fn execute(&self, ctx: &ToolContext, args: Value) -> Result<Value> {
        let url_str = args
            .get("url")
            .and_then(|u| u.as_str())
            .ok_or_else(|| OdonError::Tool {
                tool: self.name().to_string(),
                message: "Missing 'url' parameter".to_string(),
            })?;
        let validated = net_guard::validate_url(url_str).await?;
        let url_str = validated.as_str();

        let out_path = args
            .get("output_path")
            .and_then(|p| p.as_str())
            .unwrap_or("screenshot.png");
        let rel = std::path::Path::new(out_path);
        if rel.is_absolute()
            || rel.components().any(|component| {
                matches!(
                    component,
                    std::path::Component::ParentDir
                        | std::path::Component::RootDir
                        | std::path::Component::Prefix(_)
                )
            })
        {
            return Err(OdonError::Tool {
                tool: self.name().to_string(),
                message: "output_path must stay inside the workspace".to_string(),
            });
        }
        let full_out = std::path::Path::new(&ctx.workspace_root).join(rel);
        if let Some(parent) = full_out.parent() {
            let _ = std::fs::create_dir_all(parent);
        }
        let out_path = full_out.to_string_lossy().to_string();

        // Attempt screenshot via headless chrome / edge on Windows if available
        #[cfg(target_os = "windows")]
        {
            let edge_path = r"C:\Program Files (x86)\Microsoft\Edge\Application\msedge.exe";
            let chrome_path = r"C:\Program Files\Google\Chrome\Application\chrome.exe";
            let bin = if std::path::Path::new(edge_path).exists() {
                Some(edge_path)
            } else if std::path::Path::new(chrome_path).exists() {
                Some(chrome_path)
            } else {
                None
            };

            if let Some(b) = bin {
                let status = tokio::process::Command::new(b)
                    .args([
                        "--headless",
                        "--disable-gpu",
                        &format!("--screenshot={}", out_path),
                        "--window-size=1280,800",
                        url_str,
                    ])
                    .status()
                    .await;
                if let Ok(st) = status {
                    if st.success() {
                        return Ok(json!({
                            "captured": true,
                            "url": url_str,
                            "path": out_path
                        }));
                    }
                }
            }
        }

        Err(OdonError::Tool {
            tool: self.name().to_string(),
            message: "No headless Chrome/Edge executable was found, so the screenshot could not be captured. Install a Chromium browser or use browser_navigate for page content.".to_string(),
        })
    }
}

pub struct BrowserClickTool;

#[async_trait]
impl Tool for BrowserClickTool {
    fn name(&self) -> &str {
        "browser_click"
    }
    fn description(&self) -> &str {
        "Click on a web element, button, or link by selector or target href"
    }
    fn parameters_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "url": { "type": "string", "description": "Page URL to open before clicking" },
                "selector": { "type": "string", "description": "CSS selector, button text, or link href to click" }
            },
            "required": ["url", "selector"]
        })
    }
    fn risk_tier(&self) -> RiskTier {
        RiskTier::Tier1WorkspaceMutating
    }
    async fn execute(&self, _ctx: &ToolContext, args: Value) -> Result<Value> {
        let url_str = args
            .get("url")
            .and_then(|u| u.as_str())
            .ok_or_else(|| OdonError::Tool {
                tool: self.name().to_string(),
                message: "Missing 'url' parameter".to_string(),
            })?;
        let validated = net_guard::validate_url(url_str).await?;
        let url_str = validated.as_str();

        let selector = args.get("selector").and_then(|s| s.as_str()).unwrap_or("");
        if selector.is_empty() {
            return Err(OdonError::Tool {
                tool: self.name().to_string(),
                message: "Missing 'selector' parameter".to_string(),
            });
        }

        let chrome = find_chrome()?;
        let port = find_free_port().await?;

        let mut cmd = tokio::process::Command::new(chrome);
        cmd.args([
            "--headless",
            "--disable-gpu",
            "--no-first-run",
            &format!("--remote-debugging-port={}", port),
            "--window-size=1280,800",
            url_str,
        ]);
        let mut child = cmd.spawn().map_err(|e| OdonError::Tool {
            tool: self.name().to_string(),
            message: format!("Failed to launch browser: {e}"),
        })?;

        let result = tokio::time::timeout(std::time::Duration::from_secs(45), async move {
            let cdp_ws = wait_for_cdp_websocket(port)
                .await
                .map_err(|e| OdonError::Tool {
                    tool: "browser_click".to_string(),
                    message: format!("CDP not ready: {e}"),
                })?;

            let result = click_via_cdp(cdp_ws, selector.to_string()).await;

            let _ = child.kill().await;
            result
        })
        .await
        .map_err(|_| OdonError::Tool {
            tool: self.name().to_string(),
            message: "Browser click timed out".to_string(),
        })??;

        Ok(result)
    }
}

fn find_chrome() -> Result<&'static str> {
    let edge_path = r"C:\Program Files (x86)\Microsoft\Edge\Application\msedge.exe";
    let chrome_path = r"C:\Program Files\Google\Chrome\Application\chrome.exe";

    if std::path::Path::new(edge_path).exists() {
        Ok(edge_path)
    } else if std::path::Path::new(chrome_path).exists() {
        Ok(chrome_path)
    } else {
        Err(OdonError::Tool {
            tool: "browser_click".to_string(),
            message: "No headless Chrome/Edge executable was found.".to_string(),
        })
    }
}

async fn find_free_port() -> Result<u16> {
    let listener = std::net::TcpListener::bind(("127.0.0.1", 0)).map_err(|e| OdonError::Tool {
        tool: "browser_click".to_string(),
        message: format!("Failed to find free port: {e}"),
    })?;
    let port = listener.local_addr().map_err(|e| OdonError::Tool {
        tool: "browser_click".to_string(),
        message: format!("Failed to read port: {e}"),
    })?;
    drop(listener);
    Ok(port.port())
}

async fn wait_for_cdp_websocket(port: u16) -> Result<url::Url> {
    let base = format!("http://127.0.0.1:{}", port);
    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_millis(500))
        .build()
        .map_err(|e| OdonError::Tool {
            tool: "browser_click".to_string(),
            message: format!("Failed to build CDP client: {e}"),
        })?;

    for _ in 0..40 {
        if client.get(&base).send().await.map(|r| r.status().is_success()).unwrap_or(false) {
            break;
        }
        tokio::time::sleep(std::time::Duration::from_millis(125)).await;
    }

    let targets_url = format!("{}/json/new?{}", base, urlencoding::encode("about:blank"));
    let body = client.get(&targets_url).send().await.map_err(|e| OdonError::Tool {
        tool: "browser_click".to_string(),
        message: format!("Failed to create CDP target: {e}"),
    })?;
    let target: serde_json::Value = body.json().await.map_err(|e| OdonError::Tool {
        tool: "browser_click".to_string(),
        message: format!("Invalid CDP target response: {e}"),
    })?;
    let ws_url = target
        .get("webSocketDebuggerUrl")
        .and_then(|v| v.as_str())
        .ok_or_else(|| OdonError::Tool {
            tool: "browser_click".to_string(),
            message: "CDP target did not expose a WebSocket URL".to_string(),
        })?;
    url::Url::parse(ws_url).map_err(|e| OdonError::Tool {
        tool: "browser_click".to_string(),
        message: format!("Invalid CDP WebSocket URL: {e}"),
    })
}

async fn click_via_cdp(ws_url: url::Url, selector: String) -> Result<Value> {
    use futures_util::{SinkExt, StreamExt};
    use tokio_tungstenite::tungstenite::protocol::Message;

    let (mut ws, _) = tokio_tungstenite::connect_async(ws_url.as_str())
        .await
        .map_err(|e| OdonError::Tool {
            tool: "browser_click".to_string(),
            message: format!("Failed to connect to CDP: {e}"),
        })?;

    let js = format!(
        "(() => {{ \
            const el = document.querySelector({:?}); \
            if (!el) return 'not_found'; \
            el.click(); \
            return 'clicked'; \
        }})()",
        selector
    );

    let payload = serde_json::json!({
        "id": 1,
        "method": "Runtime.evaluate",
        "params": { "expression": js, "returnByValue": true }
    });

    ws.send(Message::Text(payload.to_string().into()))
        .await
        .map_err(|e| OdonError::Tool {
            tool: "browser_click".to_string(),
            message: format!("Failed to send CDP command: {e}"),
        })?;

    let msg = tokio::time::timeout(std::time::Duration::from_secs(20), ws.next())
        .await
        .map_err(|_| OdonError::Tool {
            tool: "browser_click".to_string(),
            message: "CDP response timed out".to_string(),
        })?
        .ok_or_else(|| OdonError::Tool {
            tool: "browser_click".to_string(),
            message: "CDP connection closed without response".to_string(),
        })?
        .map_err(|e| OdonError::Tool {
            tool: "browser_click".to_string(),
            message: format!("CDP message error: {e}"),
        })?;

    let text = msg.to_text().map_err(|e| OdonError::Tool {
        tool: "browser_click".to_string(),
        message: format!("Invalid CDP message: {e}"),
    })?;
    let response: serde_json::Value = serde_json::from_str(text).map_err(|e| OdonError::Tool {
        tool: "browser_click".to_string(),
        message: format!("Invalid CDP response: {e}"),
    })?;

    Ok(json!({
        "selector": selector,
        "result": response.get("result").and_then(|r| r.get("result")).cloned().unwrap_or(serde_json::Value::Null),
        "cdp_response": response
    }))
}

pub struct BrowserDownloadTool;

#[async_trait]
impl Tool for BrowserDownloadTool {
    fn name(&self) -> &str {
        "browser_download"
    }
    fn description(&self) -> &str {
        "Trigger a browser download via selector, wait for the file, and save it into the workspace"
    }
    fn parameters_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "url": { "type": "string", "description": "Page URL to open before triggering the download" },
                "selector": { "type": "string", "description": "CSS selector or link text that triggers the download" },
                "filename": { "type": "string", "description": "Optional expected filename to wait for" },
                "timeout_seconds": { "type": "integer", "minimum": 1, "maximum": 600, "default": 120 }
            },
            "required": ["url", "selector"]
        })
    }
    fn risk_tier(&self) -> RiskTier {
        RiskTier::Tier1WorkspaceMutating
    }
    async fn execute(&self, ctx: &ToolContext, args: Value) -> Result<Value> {
        let url_str = args
            .get("url")
            .and_then(|u| u.as_str())
            .ok_or_else(|| OdonError::Tool {
                tool: self.name().to_string(),
                message: "Missing 'url' parameter".to_string(),
            })?;
        let validated = net_guard::validate_url(url_str).await?;
        let url_str = validated.as_str();

        use futures_util::{SinkExt, StreamExt};
        use tokio_tungstenite::tungstenite::protocol::Message;

        let selector = args
            .get("selector")
            .and_then(|s| s.as_str())
            .ok_or_else(|| OdonError::Tool {
                tool: self.name().to_string(),
                message: "Missing 'selector' parameter".to_string(),
            })?;

        let expected_filename = args
            .get("filename")
            .and_then(|s| s.as_str())
            .filter(|s| !s.trim().is_empty())
            .map(|s| s.to_string());

        let timeout = args
            .get("timeout_seconds")
            .and_then(Value::as_u64)
            .unwrap_or(120);

        let download_dir = std::path::Path::new(&ctx.workspace_root).join(".odon").join("downloads");
        std::fs::create_dir_all(&download_dir).map_err(|e| OdonError::Tool {
            tool: self.name().to_string(),
            message: format!("Failed to create download directory: {e}"),
        })?;

        let chrome = find_chrome()?;
        let port = find_free_port().await?;

        let mut cmd = tokio::process::Command::new(chrome);
        cmd.args([
            "--headless",
            "--disable-gpu",
            "--no-first-run",
            &format!("--remote-debugging-port={}", port),
            "--window-size=1280,800",
            url_str,
        ]);
        let mut child = cmd.spawn().map_err(|e| OdonError::Tool {
            tool: self.name().to_string(),
            message: format!("Failed to launch browser: {e}"),
        })?;

        let download_dir_for_browser = download_dir.clone();
        let _ = tokio::time::timeout(std::time::Duration::from_secs(45), async move {
            let cdp_ws = wait_for_cdp_websocket(port)
                .await
                .map_err(|e| OdonError::Tool {
                    tool: "browser_download".to_string(),
                    message: format!("CDP not ready: {e}"),
                })?;

            let set_behavior = serde_json::json!({
                "id": 1,
                "method": "Page.setDownloadBehavior",
                "params": {
                    "behavior": "allow",
                    "downloadPath": download_dir_for_browser.to_string_lossy()
                }
            });

            let (mut ws, _) = tokio_tungstenite::connect_async(cdp_ws.as_str())
                .await
                .map_err(|e| OdonError::Tool {
                    tool: "browser_download".to_string(),
                    message: format!("Failed to connect to CDP: {e}"),
                })?;

            ws.send(Message::Text(set_behavior.to_string().into()))
                .await
                .map_err(|e| OdonError::Tool {
                    tool: "browser_download".to_string(),
                    message: format!("Failed to set download behavior: {e}"),
                })?;

            let _ = ws.next().await;

            let js = format!(
                "(() => {{ \
                    const el = document.querySelector({:?}); \
                    if (!el) return 'not_found'; \
                    el.click(); \
                    return 'clicked'; \
                }})()",
                selector
            );

            let payload = serde_json::json!({
                "id": 2,
                "method": "Runtime.evaluate",
                "params": { "expression": js, "returnByValue": true }
            });

            ws.send(Message::Text(payload.to_string().into()))
                .await
                .map_err(|e| OdonError::Tool {
                    tool: "browser_download".to_string(),
                    message: format!("Failed to send CDP command: {e}"),
                })?;

            let _ = ws.next().await;

            drop(ws);
            let _ = child.kill().await;
            Ok(())
        })
        .await
        .map_err(|_| OdonError::Tool {
            tool: self.name().to_string(),
            message: "Browser download timed out".to_string(),
        })??;

        let start = std::time::Instant::now();
        let mut downloaded_path = None;
        while start.elapsed().as_secs() < timeout {
            let entries = std::fs::read_dir(&download_dir).map_err(|e| OdonError::Tool {
                tool: self.name().to_string(),
                message: format!("Failed to read download directory: {e}"),
            })?;

            for entry in entries {
                let entry = entry.map_err(|e| OdonError::Tool {
                    tool: self.name().to_string(),
                    message: format!("Failed to read download entry: {e}"),
                })?;
                let path = entry.path();
                if path.extension().map(|e| e == "crdownload").unwrap_or(false) {
                    continue;
                }
                if let Some(ref expected) = expected_filename {
                    if path.file_name().and_then(|n| n.to_str()) != Some(expected.as_str()) {
                        continue;
                    }
                }
                downloaded_path = Some(path);
                break;
            }

            if downloaded_path.is_some() {
                break;
            }
            tokio::time::sleep(std::time::Duration::from_millis(200)).await;
        }

        match downloaded_path {
            Some(path) => {
                let relative = path.strip_prefix(&ctx.workspace_root).unwrap_or(&path);
                Ok(json!({
                    "download_dir": download_dir.to_string_lossy(),
                    "path": relative.to_string_lossy(),
                    "selector": selector,
                    "status": "downloaded"
                }))
            }
            None => Err(OdonError::Tool {
                tool: self.name().to_string(),
                message: format!(
                    "Download did not complete within {} seconds. Check the selector or page behavior.",
                    timeout
                ),
            }),
        }
    }
}

pub(crate) mod urlencoding {
    pub fn encode(s: &str) -> String {
        url::form_urlencoded::byte_serialize(s.as_bytes()).collect()
    }
}
