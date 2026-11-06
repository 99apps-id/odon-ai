//! Integration tools: ODON automations, Windows scheduled tasks, public
//! tunnels (cloudflared / ngrok / localtunnel) and HTML parsing with the
//! Python engines (BeautifulSoup, lxml, selectolax, parsel).

use async_trait::async_trait;
use odon_core::{OdonError, Result};
use odon_security::RiskTier;
use serde_json::{json, Value};
use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use tokio::process::Child;

use crate::registry::{Tool, ToolContext};

fn tool_error(tool: &str, message: impl Into<String>) -> OdonError {
    OdonError::Tool {
        tool: tool.to_string(),
        message: message.into(),
    }
}

// ---------------------------------------------------------------------------
// automation_create
// ---------------------------------------------------------------------------

pub struct AutomationCreateTool;

#[async_trait]
impl Tool for AutomationCreateTool {
    fn name(&self) -> &str {
        "automation_create"
    }

    fn description(&self) -> &str {
        "Create a new ODON automation (scheduled agent job) in the daemon. The running daemon picks it up within ~20 seconds."
    }

    fn parameters_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "name": { "type": "string" },
                "description": { "type": "string" },
                "trigger_type": { "type": "string", "enum": ["interval", "cron", "on_commit", "manual"], "description": "Default: manual" },
                "trigger_value": { "type": "string", "description": "e.g. 'Every 2 hours', '0 8 * * *', '15m'" },
                "prompt": { "type": "string", "description": "The instruction or prompt for the agent" },
                "model": { "type": "string" },
                "enabled": { "type": "boolean" },
                "retry_count": { "type": "integer", "minimum": 0, "maximum": 10 },
                "retry_delay_secs": { "type": "integer", "minimum": 0, "maximum": 3600 },
                "depends_on": { "type": "array", "items": { "type": "string" } }
            },
            "required": ["prompt"]
        })
    }

    fn risk_tier(&self) -> RiskTier {
        RiskTier::Tier1WorkspaceMutating
    }

    async fn execute(&self, _ctx: &ToolContext, args: Value) -> Result<Value> {
        let tool = self.name();
        let get = |key: &str| {
            args.get(key)
                .and_then(Value::as_str)
                .unwrap_or("")
                .to_string()
        };
        let prompt = get("prompt");
        if prompt.trim().is_empty() {
            return Err(tool_error(tool, "Missing 'prompt' parameter"));
        }
        let id = format!(
            "auto-{}-{}",
            chrono::Utc::now().timestamp_millis(),
            uuid::Uuid::new_v4().simple()
        );
        let name = {
            let value = get("name");
            if value.is_empty() {
                "Automation".to_string()
            } else {
                value
            }
        };
        let trigger_type = {
            let value = get("trigger_type");
            if value.is_empty() {
                "manual".to_string()
            } else {
                value
            }
        };
        let model = {
            let value = get("model");
            if value.is_empty() {
                "odon-auto".to_string()
            } else {
                value
            }
        };
        let job = json!({
            "id": id,
            "name": name,
            "description": get("description"),
            "trigger_type": trigger_type,
            "trigger_value": get("trigger_value"),
            "prompt": prompt,
            "model": model,
            "enabled": args.get("enabled").and_then(Value::as_bool).unwrap_or(true),
            "retry_count": args.get("retry_count").and_then(Value::as_u64).unwrap_or(0),
            "retry_delay_secs": args.get("retry_delay_secs").and_then(Value::as_u64).unwrap_or(0),
            "depends_on": args.get("depends_on").cloned().unwrap_or(json!([])),
        });

        let (status, result) =
            crate::ops_ext::daemon_json(reqwest::Method::POST, "/api/automations", Some(job), tool)
                .await?;
        if !(200..300).contains(&status) {
            return Err(tool_error(
                tool,
                format!("Could not create automation: HTTP {status}"),
            ));
        }
        Ok(json!({ "id": id, "http_status": status, "result": result }))
    }
}

// ---------------------------------------------------------------------------
// Windows scheduled tasks (schtasks)
// ---------------------------------------------------------------------------

fn valid_task_name(name: &str) -> bool {
    !name.is_empty()
        && !name.starts_with('-')
        && !name.contains(['\\', '/', ':', '*', '?', '"', '<', '>', '|'])
        && !name.chars().any(char::is_control)
}

fn schtasks_requirements() -> crate::registry::ToolRequirements {
    crate::registry::ToolRequirements {
        any_binaries: vec!["schtasks".into()],
        ..Default::default()
    }
}

async fn run_schtasks(tool: &str, args: &[&str]) -> Result<(bool, String)> {
    let output = tokio::process::Command::new("schtasks")
        .args(args)
        .kill_on_drop(true)
        .output()
        .await
        .map_err(|error| tool_error(tool, format!("Could not run schtasks: {error}")))?;
    let mut text = String::from_utf8_lossy(&output.stdout).into_owned();
    let err_text = String::from_utf8_lossy(&output.stderr).into_owned();
    if !err_text.trim().is_empty() {
        if !text.is_empty() {
            text.push('\n');
        }
        text.push_str(err_text.trim());
    }
    Ok((output.status.success(), text.trim().to_string()))
}

pub struct SchTasksCreateTool;

#[async_trait]
impl Tool for SchTasksCreateTool {
    fn name(&self) -> &str {
        "schtasks_create"
    }

    fn description(&self) -> &str {
        "Create a Windows scheduled task (schtasks) that runs a command on a schedule"
    }

    fn parameters_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "name": { "type": "string", "description": "Task name" },
                "command": { "type": "string", "description": "Program (and arguments) to run, e.g. 'C:\\tools\\job.exe --flag'" },
                "schedule": { "type": "string", "enum": ["MINUTE", "HOURLY", "DAILY", "WEEKLY", "MONTHLY", "ONCE"], "description": "Default: DAILY" },
                "time": { "type": "string", "description": "Start time as HH:MM(24h), e.g. '08:00' (ONCE/DAILY/WEEKLY/MONTHLY)" },
                "days": { "type": "string", "description": "For WEEKLY/MONTHLY: day list, e.g. 'MON,WED' or '1'" }
            },
            "required": ["name", "command"]
        })
    }

    fn risk_tier(&self) -> RiskTier {
        RiskTier::Tier2SystemMutating
    }

    fn requirements(&self) -> crate::registry::ToolRequirements {
        schtasks_requirements()
    }

    async fn execute(&self, _ctx: &ToolContext, args: Value) -> Result<Value> {
        let tool = self.name();
        let name = args
            .get("name")
            .and_then(Value::as_str)
            .unwrap_or("")
            .trim()
            .to_string();
        let command = args
            .get("command")
            .and_then(Value::as_str)
            .unwrap_or("")
            .trim()
            .to_string();
        if !valid_task_name(&name) {
            return Err(tool_error(tool, format!("Invalid task name: {name:?}")));
        }
        if command.is_empty() {
            return Err(tool_error(tool, "Missing 'command' parameter"));
        }
        let schedule = args
            .get("schedule")
            .and_then(Value::as_str)
            .unwrap_or("DAILY")
            .to_ascii_uppercase();
        if !["MINUTE", "HOURLY", "DAILY", "WEEKLY", "MONTHLY", "ONCE"].contains(&schedule.as_str())
        {
            return Err(tool_error(tool, format!("Invalid schedule: {schedule}")));
        }
        let mut argv: Vec<String> = vec![
            "/Create".into(),
            "/TN".into(),
            name.clone(),
            "/TR".into(),
            command,
            "/SC".into(),
            schedule,
        ];
        if let Some(time) = args
            .get("time")
            .and_then(Value::as_str)
            .filter(|v| !v.trim().is_empty())
        {
            argv.push("/ST".into());
            argv.push(time.trim().to_string());
        }
        if let Some(days) = args
            .get("days")
            .and_then(Value::as_str)
            .filter(|v| !v.trim().is_empty())
        {
            argv.push("/D".into());
            argv.push(days.trim().to_string());
        }
        argv.push("/F".into());
        let refs: Vec<&str> = argv.iter().map(String::as_str).collect();
        let (ok, output) = run_schtasks(tool, &refs).await?;
        Ok(json!({ "task": name, "created": ok, "output": output }))
    }
}

pub struct SchTasksQueryTool;

#[async_trait]
impl Tool for SchTasksQueryTool {
    fn name(&self) -> &str {
        "schtasks_query"
    }

    fn description(&self) -> &str {
        "List Windows scheduled tasks, or query one task in detail"
    }

    fn parameters_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "name": { "type": "string", "description": "Task name (omit to list all)" }
            }
        })
    }

    fn risk_tier(&self) -> RiskTier {
        RiskTier::Tier0ReadOnly
    }

    fn requirements(&self) -> crate::registry::ToolRequirements {
        schtasks_requirements()
    }

    async fn execute(&self, _ctx: &ToolContext, args: Value) -> Result<Value> {
        let tool = self.name();
        let name = args
            .get("name")
            .and_then(Value::as_str)
            .map(str::trim)
            .unwrap_or("");
        let (ok, output) = if name.is_empty() {
            run_schtasks(tool, &["/Query", "/FO", "LIST"]).await?
        } else {
            if !valid_task_name(name) {
                return Err(tool_error(tool, format!("Invalid task name: {name:?}")));
            }
            run_schtasks(tool, &["/Query", "/TN", name, "/V", "/FO", "LIST"]).await?
        };
        Ok(json!({ "ok": ok, "output": output }))
    }
}

pub struct SchTasksDeleteTool;

#[async_trait]
impl Tool for SchTasksDeleteTool {
    fn name(&self) -> &str {
        "schtasks_delete"
    }

    fn description(&self) -> &str {
        "Delete a Windows scheduled task"
    }

    fn parameters_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "name": { "type": "string" },
                "confirm": { "type": "boolean", "description": "Must be true" }
            },
            "required": ["name", "confirm"]
        })
    }

    fn risk_tier(&self) -> RiskTier {
        RiskTier::Tier2SystemMutating
    }

    fn requirements(&self) -> crate::registry::ToolRequirements {
        schtasks_requirements()
    }

    async fn execute(&self, _ctx: &ToolContext, args: Value) -> Result<Value> {
        let tool = self.name();
        let name = args
            .get("name")
            .and_then(Value::as_str)
            .unwrap_or("")
            .trim()
            .to_string();
        if !valid_task_name(&name) {
            return Err(tool_error(tool, format!("Invalid task name: {name:?}")));
        }
        if !args
            .get("confirm")
            .and_then(Value::as_bool)
            .unwrap_or(false)
        {
            return Err(tool_error(
                tool,
                "Pass confirm=true to delete a scheduled task",
            ));
        }
        let (ok, output) = run_schtasks(tool, &["/Delete", "/TN", &name, "/F"]).await?;
        Ok(json!({ "task": name, "deleted": ok, "output": output }))
    }
}

// ---------------------------------------------------------------------------
// Public tunnels (cloudflared / ngrok / localtunnel)
// ---------------------------------------------------------------------------

pub(crate) struct TunnelInfo {
    pub id: String,
    pub provider: String,
    pub local_port: u16,
    pub url: Option<String>,
    pub pid: Option<u32>,
}

static TUNNELS: std::sync::OnceLock<Arc<Mutex<HashMap<String, TunnelInfo>>>> =
    std::sync::OnceLock::new();
static TUNNEL_CHILDREN: std::sync::OnceLock<Arc<Mutex<HashMap<String, Child>>>> =
    std::sync::OnceLock::new();

fn tunnels() -> &'static Arc<Mutex<HashMap<String, TunnelInfo>>> {
    TUNNELS.get_or_init(|| Arc::new(Mutex::new(HashMap::new())))
}

fn tunnel_children() -> &'static Arc<Mutex<HashMap<String, Child>>> {
    TUNNEL_CHILDREN.get_or_init(|| Arc::new(Mutex::new(HashMap::new())))
}

fn tunnel_error(tool: &str, message: impl Into<String>) -> OdonError {
    tool_error(tool, message)
}

fn first_https_url(text: &str) -> Option<String> {
    for token in text.split_whitespace() {
        let token = token.trim_matches(|ch| {
            matches!(
                ch,
                '"' | '\'' | '(' | ')' | '[' | ']' | '{' | '}' | '<' | '>'
            )
        });
        if let Some(rest) = token.strip_prefix("https://") {
            let host = rest.split('/').next().unwrap_or("");
            if host.contains('.') && !host.contains(['>', '"']) {
                return Some(token.trim_end_matches('.').to_string());
            }
        }
    }
    None
}

pub struct TunnelStartTool;

#[async_trait]
impl Tool for TunnelStartTool {
    fn name(&self) -> &str {
        "tunnel_start"
    }

    fn description(&self) -> &str {
        "Expose a local port (default the ODON daemon on 4096) over the internet with cloudflared, ngrok, or localtunnel, and return the public URL"
    }

    fn parameters_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "provider": { "type": "string", "enum": ["cloudflared", "ngrok", "localtunnel"], "description": "Default: cloudflared" },
                "local_port": { "type": "integer", "description": "Local port to expose (default 4096)" },
                "name": { "type": "string", "description": "Optional friendly name" }
            }
        })
    }

    fn risk_tier(&self) -> RiskTier {
        RiskTier::Tier2SystemMutating
    }

    async fn execute(&self, _ctx: &ToolContext, args: Value) -> Result<Value> {
        let tool = self.name();
        let provider = args
            .get("provider")
            .and_then(Value::as_str)
            .unwrap_or("cloudflared")
            .to_ascii_lowercase();
        if !["cloudflared", "ngrok", "localtunnel"].contains(&provider.as_str()) {
            return Err(tunnel_error(tool, format!("Unknown provider: {provider}")));
        }
        let local_port = args
            .get("local_port")
            .and_then(Value::as_u64)
            .unwrap_or(4096)
            .clamp(1, 65535) as u16;

        let id = format!(
            "tun-{}-{}",
            chrono::Utc::now().timestamp_millis(),
            uuid::Uuid::new_v4().simple()
        );

        let (program, mut argv): (&str, Vec<String>) = match provider.as_str() {
            "cloudflared" => (
                "cloudflared",
                vec![
                    "tunnel".into(),
                    "--url".into(),
                    format!("http://127.0.0.1:{local_port}"),
                ],
            ),
            "ngrok" => ("ngrok", vec!["http".into(), local_port.to_string()]),
            _ => (
                "npx",
                vec![
                    "localtunnel".into(),
                    "--port".into(),
                    local_port.to_string(),
                ],
            ),
        };
        if let Some(name) = args
            .get("name")
            .and_then(Value::as_str)
            .filter(|v| !v.trim().is_empty())
        {
            if provider == "ngrok" {
                argv.push("--name".into());
                argv.push(name.trim().to_string());
            }
        }

        let mut child = tokio::process::Command::new(program)
            .args(&argv)
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::piped())
            .kill_on_drop(true)
            .spawn()
            .map_err(|error| {
                let hint = match error.kind() {
                    std::io::ErrorKind::NotFound => format!(
                        "{program} is not installed or not on PATH. Install it first, then retry."
                    ),
                    _ => format!("Could not start {program}: {error}"),
                };
                tunnel_error(tool, hint)
            })?;
        let pid = child.id();
        // Capture the child's output in the background: cloudflared and
        // localtunnel print the public URL to stdout/stderr, and an unread
        // pipe would block the child so the URL would never be detected.
        if let Some(stdout) = child.stdout.take() {
            spawn_url_watcher(id.clone(), stdout);
        }
        if let Some(stderr) = child.stderr.take() {
            spawn_url_watcher(id.clone(), stderr);
        }
        tunnel_children().lock().unwrap().insert(id.clone(), child);
        let info = TunnelInfo {
            id: id.clone(),
            provider: provider.clone(),
            local_port,
            url: None,
            pid,
        };
        tunnels().lock().unwrap().insert(id.clone(), info);

        // Wait briefly for the public URL to appear.
        let url = wait_for_tunnel_url(&id, &provider, 25).await;
        if let Some(found) = &url {
            if let Some(info) = tunnels().lock().unwrap().get_mut(&id) {
                info.url = Some(found.clone());
            }
        }

        Ok(json!({
            "id": id,
            "provider": provider,
            "local_port": local_port,
            "url": url,
            "status": if url.is_some() { "ready" } else { "pending" },
            "pid": pid,
        }))
    }
}

/// Read a tunnel child's output stream and store the first public URL found.
fn spawn_url_watcher(id: String, stream: impl tokio::io::AsyncRead + Unpin + Send + 'static) {
    tokio::spawn(async move {
        use tokio::io::AsyncBufReadExt;
        let mut reader = tokio::io::BufReader::new(stream);
        let mut line = String::new();
        loop {
            line.clear();
            match reader.read_line(&mut line).await {
                Ok(0) => break,
                Ok(_) => {
                    if let Some(url) = first_https_url(&line) {
                        if let Some(info) = tunnels().lock().unwrap().get_mut(&id) {
                            if info.url.is_none() {
                                info.url = Some(url);
                            }
                        }
                        break;
                    }
                }
                Err(_) => break,
            }
        }
    });
}

async fn wait_for_tunnel_url(id: &str, provider: &str, seconds: u64) -> Option<String> {
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(seconds);
    let client = if provider == "ngrok" {
        Some(reqwest::Client::new())
    } else {
        None
    };
    while std::time::Instant::now() < deadline {
        if let Some(url) = tunnels()
            .lock()
            .unwrap()
            .get(id)
            .and_then(|info| info.url.clone())
        {
            return Some(url);
        }
        // ngrok publishes its URLs through a local API.
        if let Some(client) = &client {
            if let Ok(response) = client.get("http://127.0.0.1:4040/api/tunnels").send().await {
                if let Ok(body) = response.json::<Value>().await {
                    if let Some(url) = body
                        .get("tunnels")
                        .and_then(Value::as_array)
                        .and_then(|tunnels| tunnels.first())
                        .and_then(|tunnel| tunnel.get("public_url"))
                        .and_then(Value::as_str)
                    {
                        let url = url.to_string();
                        if let Some(info) = tunnels().lock().unwrap().get_mut(id) {
                            info.url = Some(url.clone());
                        }
                        return Some(url);
                    }
                }
            }
        }
        tokio::time::sleep(std::time::Duration::from_millis(500)).await;
    }
    tunnels()
        .lock()
        .unwrap()
        .get(id)
        .and_then(|info| info.url.clone())
}

pub struct TunnelListTool;

#[async_trait]
impl Tool for TunnelListTool {
    fn name(&self) -> &str {
        "tunnel_list"
    }

    fn description(&self) -> &str {
        "List active public tunnels started with tunnel_start"
    }

    fn parameters_schema(&self) -> Value {
        json!({ "type": "object", "properties": {} })
    }

    fn risk_tier(&self) -> RiskTier {
        RiskTier::Tier0ReadOnly
    }

    async fn execute(&self, _ctx: &ToolContext, _args: Value) -> Result<Value> {
        let guard = tunnels().lock().unwrap();
        let items: Vec<Value> = guard
            .values()
            .map(|info| {
                json!({
                    "id": info.id,
                    "provider": info.provider,
                    "local_port": info.local_port,
                    "url": info.url,
                    "pid": info.pid,
                })
            })
            .collect();
        Ok(json!({ "tunnels": items }))
    }
}

pub struct TunnelStopTool;

#[async_trait]
impl Tool for TunnelStopTool {
    fn name(&self) -> &str {
        "tunnel_stop"
    }

    fn description(&self) -> &str {
        "Stop a tunnel started with tunnel_start"
    }

    fn parameters_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "id": { "type": "string", "description": "Tunnel id from tunnel_start" }
            },
            "required": ["id"]
        })
    }

    fn risk_tier(&self) -> RiskTier {
        RiskTier::Tier2SystemMutating
    }

    async fn execute(&self, _ctx: &ToolContext, args: Value) -> Result<Value> {
        let tool = self.name();
        let id = args
            .get("id")
            .and_then(Value::as_str)
            .ok_or_else(|| tunnel_error(tool, "Missing 'id' parameter"))?;
        tunnels().lock().unwrap().remove(id);
        let child = tunnel_children().lock().unwrap().remove(id);
        if let Some(mut child) = child {
            let _ = child.kill().await;
            let _ = child.wait().await;
            return Ok(json!({ "id": id, "stopped": true }));
        }
        Err(tunnel_error(
            tool,
            format!("No active tunnel with id '{id}'"),
        ))
    }
}

// ---------------------------------------------------------------------------
// HTML parsing (bs4 / lxml / selectolax / parsel via Python)
// ---------------------------------------------------------------------------

const HTML_PARSE_SCRIPT: &str = r#"
import sys, json

engine = sys.argv[1] if len(sys.argv) > 1 else "bs4"
strategy = sys.argv[2] if len(sys.argv) > 2 else "css"
selector = sys.argv[3] if len(sys.argv) > 3 else ""
try:
    limit = int(sys.argv[4]) if len(sys.argv) > 4 else 100
except ValueError:
    limit = 100
html = sys.stdin.read()

def item(text, html_str, attrs):
    text = (text or "").strip()
    if len(text) > 2000:
        text = text[:2000]
    html_str = (html_str or "")[:4000]
    clean = {}
    if attrs:
        for k, v in attrs.items():
            if isinstance(v, list):
                v = " ".join(str(x) for x in v)
            clean[str(k)] = str(v)[:500]
    return {"text": text, "html": html_str, "attrs": clean}

def emit(items):
    print(json.dumps(items[:limit]))

try:
    if engine == "bs4":
        from bs4 import BeautifulSoup
        soup = BeautifulSoup(html, "lxml")
        if strategy == "text":
            nodes = [soup]
        elif strategy == "links":
            nodes = soup.find_all("a", href=True)
        else:
            nodes = soup.select(selector)
        emit([item(n.get_text(), str(n)[:4000], dict(n.attrs) if hasattr(n, "attrs") else {}) for n in nodes])
    elif engine == "lxml":
        from lxml import html as lhtml
        root = lhtml.fromstring(html)
        if strategy == "text":
            nodes = [root]
        elif strategy == "links":
            nodes = root.xpath("//a[@href]")
        else:
            nodes = root.xpath(selector)
        out = []
        for n in nodes:
            if isinstance(n, str):
                out.append(item(n, "", {}))
            else:
                attrs = dict(n.attrib) if hasattr(n, "attrib") else {}
                out.append(item(n.text_content() if hasattr(n, "text_content") else str(n), lhtml.tostring(n, encoding="unicode")[:4000], attrs))
        emit(out)
    elif engine == "selectolax":
        from selectolax.parser import HTMLParser
        tree = HTMLParser(html)
        if strategy == "text":
            nodes = [tree.body] if tree.body is not None else [tree.root]
        elif strategy == "links":
            nodes = tree.css("a[href]")
        else:
            nodes = tree.css(selector)
        out = []
        for n in nodes:
            attrs = dict(n.attrs) if hasattr(n, "attrs") else {}
            out.append(item(n.text(), n.html, attrs))
        emit(out)
    elif engine == "parsel":
        from parsel import Selector
        sel = Selector(text=html)
        if strategy == "text":
            emit([item(sel.css("body::text").get(), html[:4000], {})])
        elif strategy == "links":
            emit([item(a, "", {"href": a}) for a in sel.css("a::attr(href)").getall()])
        else:
            raw = sel.css(selector).getall() if strategy == "css" else sel.xpath(selector).getall()
            emit([item(value, str(value)[:4000], {}) if isinstance(value, str) else item(str(value), str(value)[:4000], {}) for value in raw])
    else:
        print(json.dumps({"error": "unknown engine: " + engine}))
except ImportError as e:
    print(json.dumps({"error": "missing Python library for engine '" + engine + "': " + str(e) + ". Install it, e.g. pip install beautifulsoup4 lxml selectolax parsel"}))
except Exception as e:
    print(json.dumps({"error": str(e)[:500]}))
"#;

pub struct HtmlParseTool;

#[async_trait]
impl Tool for HtmlParseTool {
    fn name(&self) -> &str {
        "html_parse"
    }

    fn description(&self) -> &str {
        "Parse HTML with a Python engine (bs4, lxml, selectolax, parsel): extract text, links, or elements by a CSS/XPath selector. The chosen Python library must be installed."
    }

    fn parameters_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "html": { "type": "string", "description": "HTML to parse (preferred for small pages)" },
                "path": { "type": "string", "description": "Workspace-relative HTML file to read instead of `html`" },
                "engine": { "type": "string", "enum": ["bs4", "lxml", "selectolax", "parsel"], "description": "Default: bs4" },
                "strategy": { "type": "string", "enum": ["css", "xpath", "text", "links"], "description": "Default: css" },
                "selector": { "type": "string", "description": "CSS (or XPath for lxml/parsel) selector" },
                "limit": { "type": "integer", "minimum": 1, "maximum": 500 }
            }
        })
    }

    fn risk_tier(&self) -> RiskTier {
        RiskTier::Tier0ReadOnly
    }

    fn requirements(&self) -> crate::registry::ToolRequirements {
        crate::registry::ToolRequirements {
            any_binaries: vec!["python".into(), "python3".into()],
            ..Default::default()
        }
    }

    async fn execute(&self, ctx: &ToolContext, args: Value) -> Result<Value> {
        let tool = self.name();
        let html = if let Some(html) = args.get("html").and_then(Value::as_str) {
            html.to_string()
        } else if let Some(path) = args.get("path").and_then(Value::as_str) {
            let full = crate::workspace_path::resolve_existing(&ctx.workspace_root, path)?;
            const MAX_BYTES: u64 = 8 * 1024 * 1024;
            let meta = tokio::fs::metadata(&full)
                .await
                .map_err(|error| tool_error(tool, format!("Cannot stat {path}: {error}")))?;
            if meta.len() > MAX_BYTES {
                return Err(tool_error(
                    tool,
                    format!("{path} is too large to parse inline"),
                ));
            }
            tokio::fs::read_to_string(&full)
                .await
                .map_err(|error| tool_error(tool, format!("Cannot read {path}: {error}")))?
        } else {
            return Err(tool_error(tool, "Provide 'html' or 'path'"));
        };

        let engine = args
            .get("engine")
            .and_then(Value::as_str)
            .unwrap_or("bs4")
            .to_ascii_lowercase();
        if !["bs4", "lxml", "selectolax", "parsel"].contains(&engine.as_str()) {
            return Err(tool_error(tool, format!("Unknown engine: {engine}")));
        }
        let strategy = args
            .get("strategy")
            .and_then(Value::as_str)
            .unwrap_or("css")
            .to_ascii_lowercase();
        if !["css", "xpath", "text", "links"].contains(&strategy.as_str()) {
            return Err(tool_error(tool, format!("Unknown strategy: {strategy}")));
        }
        let selector = args
            .get("selector")
            .and_then(Value::as_str)
            .unwrap_or("")
            .to_string();
        if (strategy == "css" || strategy == "xpath") && selector.trim().is_empty() {
            return Err(tool_error(
                tool,
                "Provide 'selector' for css/xpath strategies",
            ));
        }
        let limit = args
            .get("limit")
            .and_then(Value::as_u64)
            .unwrap_or(100)
            .clamp(1, 500);

        let script_path = std::env::temp_dir().join(format!(
            "odon-html-parse-{}.py",
            uuid::Uuid::new_v4().simple()
        ));
        std::fs::write(&script_path, HTML_PARSE_SCRIPT)
            .map_err(|error| tool_error(tool, format!("Cannot write parser script: {error}")))?;

        let attempt = async {
            let mut last_error = String::new();
            for program in ["python", "python3"] {
                let mut cmd = tokio::process::Command::new(program);
                cmd.arg(&script_path)
                    .arg(&engine)
                    .arg(&strategy)
                    .arg(&selector)
                    .arg(limit.to_string());
                cmd.stdin(std::process::Stdio::piped())
                    .stdout(std::process::Stdio::piped())
                    .stderr(std::process::Stdio::piped())
                    .kill_on_drop(true);
                let output = match cmd.spawn() {
                    Ok(mut child) => {
                        use tokio::io::AsyncWriteExt;
                        if let Some(mut stdin) = child.stdin.take() {
                            let _ = stdin.write_all(html.as_bytes()).await;
                        }
                        child.wait_with_output().await
                    }
                    Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                        last_error = format!("{program} not found");
                        continue;
                    }
                    Err(error) => {
                        return Err(tool_error(
                            tool,
                            format!("Could not run {program}: {error}"),
                        ));
                    }
                };
                let output =
                    output.map_err(|error| tool_error(tool, format!("Parser failed: {error}")))?;
                if output.status.success() {
                    let stdout = String::from_utf8_lossy(&output.stdout).into_owned();
                    return Ok::<String, OdonError>(stdout.trim().to_string());
                }
                last_error = String::from_utf8_lossy(&output.stderr).into_owned();
                if last_error.trim().is_empty() {
                    last_error = String::from_utf8_lossy(&output.stdout).into_owned();
                }
                if last_error.contains("No module named") || last_error.contains("can't open") {
                    continue;
                }
                break;
            }
            Err::<String, OdonError>(tool_error(
                tool,
                format!(
                    "Python parser failed: {}",
                    last_error.trim().chars().take(400).collect::<String>()
                ),
            ))
        }
        .await;

        let _ = std::fs::remove_file(&script_path);
        let stdout = attempt?;
        let parsed: Value = serde_json::from_str(&stdout)
            .map_err(|error| tool_error(tool, format!("Parser returned invalid JSON: {error}")))?;
        if let Some(error) = parsed.get("error").and_then(Value::as_str) {
            return Err(tool_error(tool, error.to_string()));
        }
        Ok(json!({ "engine": engine, "strategy": strategy, "items": parsed }))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn task_names_reject_option_injection() {
        assert!(valid_task_name("My Task"));
        assert!(!valid_task_name("-evil"));
        assert!(!valid_task_name("a/b"));
        assert!(!valid_task_name(""));
        assert!(!valid_task_name("bad\nname"));
    }

    #[test]
    fn urls_are_extracted_from_tunnel_output() {
        assert_eq!(
            first_https_url("Your quick Tunnel has been created! Visit it at https://happy-cat-123.trycloudflare.com"),
            Some("https://happy-cat-123.trycloudflare.com".to_string())
        );
        assert_eq!(first_https_url("no url here"), None);
    }
}
