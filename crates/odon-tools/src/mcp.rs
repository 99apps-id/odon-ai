use async_trait::async_trait;
use bytes::Bytes;
use futures_util::{Stream, StreamExt};
use odon_core::{OdonError, Result};
use odon_security::RiskTier;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::collections::HashMap;
use std::path::PathBuf;
use std::pin::Pin;
use std::process::Stdio;
use std::sync::Arc;
use std::time::Duration;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::process::{Child, ChildStdin, ChildStdout, Command};
use tokio::sync::{Mutex, RwLock};
use tokio::time::timeout;

use crate::net_guard;
use crate::registry::{Tool, ToolContext};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct McpToolInfo {
    pub name: String,
    pub description: String,
    pub input_schema: Value,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct McpServerConfig {
    pub id: String,
    pub name: String,
    pub transport: String, // "stdio" or "sse"
    #[serde(default)]
    pub command: Option<String>,
    #[serde(default)]
    pub args: Option<Vec<String>>,
    #[serde(default)]
    pub env: Option<HashMap<String, String>>,
    #[serde(default)]
    pub url: Option<String>,
    #[serde(default = "default_true")]
    pub enabled: bool,
    #[serde(default)]
    pub tools: Vec<McpToolInfo>,
}

fn default_true() -> bool {
    true
}

fn get_mcp_config_path() -> PathBuf {
    let base = std::env::var_os("ODON_HOME")
        .map(PathBuf::from)
        .or_else(|| std::env::var_os("USERPROFILE").map(PathBuf::from))
        .or_else(|| std::env::var_os("HOME").map(PathBuf::from))
        .unwrap_or_else(|| PathBuf::from("."));
    base.join(".odon").join("mcp-servers.json")
}

pub fn load_mcp_servers() -> Vec<McpServerConfig> {
    let path = get_mcp_config_path();
    if let Ok(content) = std::fs::read_to_string(&path) {
        if let Ok(servers) = serde_json::from_str::<Vec<McpServerConfig>>(&content) {
            return servers;
        }
    }
    // No seeded servers: an empty config is honest, and the UI has a preset
    // catalog for adding real entries.
    Vec::new()
}

pub fn save_mcp_servers(servers: &[McpServerConfig]) -> Result<()> {
    let path = get_mcp_config_path();
    if let Some(parent) = path.parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    let data = serde_json::to_string_pretty(servers).map_err(|e| {
        odon_core::OdonError::Internal(format!("Failed to serialize MCP servers: {}", e))
    })?;
    std::fs::write(&path, data).map_err(|e| {
        odon_core::OdonError::Internal(format!("Failed to write MCP servers: {}", e))
    })?;
    Ok(())
}

const MCP_REQUEST_TIMEOUT: Duration = Duration::from_secs(30);

struct StdioMcpClient {
    child: Child,
    stdin: ChildStdin,
    stdout: tokio::io::Lines<BufReader<ChildStdout>>,
    next_id: u64,
}

impl StdioMcpClient {
    async fn connect(config: &McpServerConfig) -> Result<Self> {
        if !config.transport.eq_ignore_ascii_case("stdio") {
            return Err(OdonError::Tool {
                tool: "mcp".to_string(),
                message: format!(
                    "MCP transport '{}' is not supported yet; configure server '{}' with stdio",
                    config.transport, config.name
                ),
            });
        }
        let command = config
            .command
            .as_deref()
            .filter(|value| !value.trim().is_empty())
            .ok_or_else(|| OdonError::Tool {
                tool: "mcp".to_string(),
                message: format!("MCP server '{}' has no command", config.name),
            })?;
        let mut process = Command::new(command);
        process
            .args(config.args.as_deref().unwrap_or_default())
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .kill_on_drop(true);
        if let Some(env) = &config.env {
            process.envs(env);
        }
        let mut child = process.spawn().map_err(|error| OdonError::Tool {
            tool: "mcp".to_string(),
            message: format!("Failed to start MCP server '{}': {error}", config.name),
        })?;
        let stdin = child.stdin.take().ok_or_else(|| OdonError::Tool {
            tool: "mcp".to_string(),
            message: "MCP server stdin was unavailable".to_string(),
        })?;
        let stdout = child.stdout.take().ok_or_else(|| OdonError::Tool {
            tool: "mcp".to_string(),
            message: "MCP server stdout was unavailable".to_string(),
        })?;
        let mut client = Self {
            child,
            stdin,
            stdout: BufReader::new(stdout).lines(),
            next_id: 1,
        };
        client.initialize().await?;
        Ok(client)
    }

    async fn initialize(&mut self) -> Result<()> {
        self.request(
            "initialize",
            json!({
                "protocolVersion": "2024-11-05",
                "capabilities": {},
                "clientInfo": { "name": "odon", "version": env!("CARGO_PKG_VERSION") }
            }),
        )
        .await?;
        self.send(json!({
            "jsonrpc": "2.0",
            "method": "notifications/initialized",
            "params": {}
        }))
        .await
    }

    async fn request(&mut self, method: &str, params: Value) -> Result<Value> {
        let id = self.next_id;
        self.next_id += 1;
        self.send(json!({ "jsonrpc": "2.0", "id": id, "method": method, "params": params }))
            .await?;
        timeout(MCP_REQUEST_TIMEOUT, self.read_response(id))
            .await
            .map_err(|_| OdonError::Tool {
                tool: "mcp".to_string(),
                message: format!("MCP request '{method}' timed out after 30 seconds"),
            })?
    }

    async fn send(&mut self, message: Value) -> Result<()> {
        let mut encoded = serde_json::to_vec(&message).map_err(|error| OdonError::Tool {
            tool: "mcp".to_string(),
            message: format!("Failed to encode MCP request: {error}"),
        })?;
        encoded.push(b'\n');
        self.stdin
            .write_all(&encoded)
            .await
            .map_err(|error| OdonError::Tool {
                tool: "mcp".to_string(),
                message: format!("Failed to write MCP request: {error}"),
            })?;
        self.stdin.flush().await.map_err(|error| OdonError::Tool {
            tool: "mcp".to_string(),
            message: format!("Failed to flush MCP request: {error}"),
        })
    }

    async fn read_response(&mut self, id: u64) -> Result<Value> {
        while let Some(line) = self
            .stdout
            .next_line()
            .await
            .map_err(|error| OdonError::Tool {
                tool: "mcp".to_string(),
                message: format!("Failed reading MCP response: {error}"),
            })?
        {
            let Ok(response) = serde_json::from_str::<Value>(&line) else {
                continue;
            };
            if response.get("id").and_then(Value::as_u64) != Some(id) {
                continue;
            }
            if let Some(error) = response.get("error") {
                return Err(OdonError::Tool {
                    tool: "mcp".to_string(),
                    message: format!("MCP server error: {error}"),
                });
            }
            return response
                .get("result")
                .cloned()
                .ok_or_else(|| OdonError::Tool {
                    tool: "mcp".to_string(),
                    message: "MCP response had no result".to_string(),
                });
        }
        Err(OdonError::Tool {
            tool: "mcp".to_string(),
            message: "MCP server closed stdout before responding".to_string(),
        })
    }

    async fn close(mut self) {
        let _ = self.stdin.shutdown().await;
        let _ = self.child.kill().await;
    }
}

type SseByteStream = Pin<Box<dyn Stream<Item = std::result::Result<Bytes, reqwest::Error>> + Send>>;

struct SseMcpClient {
    http: reqwest::Client,
    endpoint: String,
    stream: SseByteStream,
    buffer: String,
    next_id: u64,
}

async fn next_sse_event(
    stream: &mut SseByteStream,
    buffer: &mut String,
) -> Result<Option<(String, String)>> {
    loop {
        if let Some(index) = buffer.find("\n\n") {
            let raw = buffer[..index].to_string();
            buffer.drain(..index + 2);
            let mut event = "message".to_string();
            let mut data: Vec<String> = Vec::new();
            for line in raw.lines() {
                if let Some(value) = line.strip_prefix("event:") {
                    event = value.trim().to_string();
                } else if let Some(value) = line.strip_prefix("data:") {
                    data.push(value.strip_prefix(' ').unwrap_or(value).to_string());
                }
            }
            return Ok(Some((event, data.join("\n"))));
        }
        match stream.next().await {
            Some(Ok(chunk)) => {
                buffer.push_str(&String::from_utf8_lossy(&chunk));
                if buffer.contains('\r') {
                    *buffer = buffer.replace("\r\n", "\n");
                }
            }
            Some(Err(error)) => {
                return Err(OdonError::Tool {
                    tool: "mcp".to_string(),
                    message: format!("MCP SSE stream failed: {error}"),
                });
            }
            None => return Ok(None),
        }
    }
}

impl SseMcpClient {
    async fn connect(config: &McpServerConfig) -> Result<Self> {
        let url = config
            .url
            .as_deref()
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .ok_or_else(|| OdonError::Tool {
                tool: "mcp".to_string(),
                message: format!("MCP SSE server '{}' has no url", config.name),
            })?;
        let base = reqwest::Url::parse(url).map_err(|error| OdonError::Tool {
            tool: "mcp".to_string(),
            message: format!("Invalid MCP SSE URL '{url}': {error}"),
        })?;
        let base = net_guard::validate_url(base.as_str()).await?;
        let http = reqwest::Client::builder()
            .redirect(net_guard::redirect_policy(
                base.host_str().unwrap_or("").to_string(),
            ))
            .build()
            .unwrap_or_else(|_| reqwest::Client::new());
        let response = http
            .get(base.clone())
            .header(reqwest::header::ACCEPT, "text/event-stream")
            .send()
            .await
            .map_err(|error| OdonError::Tool {
                tool: "mcp".to_string(),
                message: format!(
                    "Failed to connect MCP SSE server '{}': {error}",
                    config.name
                ),
            })?;
        if !response.status().is_success() {
            return Err(OdonError::Tool {
                tool: "mcp".to_string(),
                message: format!(
                    "MCP SSE server '{}' returned HTTP {}",
                    config.name,
                    response.status()
                ),
            });
        }
        let mut stream: SseByteStream = Box::pin(response.bytes_stream());
        let mut buffer = String::new();
        let endpoint = timeout(MCP_REQUEST_TIMEOUT, async {
            loop {
                let Some((event, data)) = next_sse_event(&mut stream, &mut buffer).await? else {
                    return Err(OdonError::Tool {
                        tool: "mcp".to_string(),
                        message: "MCP SSE stream closed before sending an endpoint".to_string(),
                    });
                };
                if event == "endpoint" && !data.trim().is_empty() {
                    let resolved = base.join(data.trim()).map_err(|error| OdonError::Tool {
                        tool: "mcp".to_string(),
                        message: format!("Invalid MCP SSE endpoint '{}': {error}", data),
                    })?;
                    return Ok(resolved.to_string());
                }
            }
        })
        .await
        .map_err(|_| OdonError::Tool {
            tool: "mcp".to_string(),
            message: format!(
                "MCP SSE server '{}' did not send an endpoint within 30 seconds",
                config.name
            ),
        })??;

        let mut client = Self {
            http,
            endpoint,
            stream,
            buffer,
            next_id: 1,
        };
        client.initialize().await?;
        Ok(client)
    }

    async fn initialize(&mut self) -> Result<()> {
        self.request(
            "initialize",
            json!({
                "protocolVersion": "2024-11-05",
                "capabilities": {},
                "clientInfo": { "name": "odon", "version": env!("CARGO_PKG_VERSION") }
            }),
        )
        .await?;
        self.notify("notifications/initialized", json!({})).await
    }

    async fn post(&mut self, payload: &Value) -> Result<()> {
        let response = self
            .http
            .post(&self.endpoint)
            .json(payload)
            .send()
            .await
            .map_err(|error| OdonError::Tool {
                tool: "mcp".to_string(),
                message: format!("Failed to POST MCP request: {error}"),
            })?;
        if !response.status().is_success() {
            let status = response.status();
            let body = response.text().await.unwrap_or_default();
            return Err(OdonError::Tool {
                tool: "mcp".to_string(),
                message: format!("MCP endpoint returned HTTP {status}: {body}"),
            });
        }
        Ok(())
    }

    async fn notify(&mut self, method: &str, params: Value) -> Result<()> {
        self.post(&json!({
            "jsonrpc": "2.0",
            "method": method,
            "params": params
        }))
        .await
    }

    async fn request(&mut self, method: &str, params: Value) -> Result<Value> {
        let id = self.next_id;
        self.next_id += 1;
        self.post(&json!({
            "jsonrpc": "2.0",
            "id": id,
            "method": method,
            "params": params
        }))
        .await?;
        timeout(MCP_REQUEST_TIMEOUT, self.read_response(id))
            .await
            .map_err(|_| OdonError::Tool {
                tool: "mcp".to_string(),
                message: format!("MCP SSE request '{method}' timed out after 30 seconds"),
            })?
    }

    async fn read_response(&mut self, id: u64) -> Result<Value> {
        while let Some((event, data)) = next_sse_event(&mut self.stream, &mut self.buffer).await? {
            if event != "message" {
                continue;
            }
            let Ok(response) = serde_json::from_str::<Value>(&data) else {
                continue;
            };
            if response.get("id").and_then(Value::as_u64) != Some(id) {
                continue;
            }
            if let Some(error) = response.get("error") {
                return Err(OdonError::Tool {
                    tool: "mcp".to_string(),
                    message: format!("MCP server error: {error}"),
                });
            }
            return response
                .get("result")
                .cloned()
                .ok_or_else(|| OdonError::Tool {
                    tool: "mcp".to_string(),
                    message: "MCP response had no result".to_string(),
                });
        }
        Err(OdonError::Tool {
            tool: "mcp".to_string(),
            message: "MCP SSE stream closed before responding".to_string(),
        })
    }

    async fn close(self) {}
}

fn find_server(server: &str) -> Result<McpServerConfig> {
    load_mcp_servers()
        .into_iter()
        .find(|item| item.enabled && (item.id == server || item.name.eq_ignore_ascii_case(server)))
        .ok_or_else(|| OdonError::Tool {
            tool: "mcp".to_string(),
            message: format!("Enabled MCP server '{server}' was not found"),
        })
}

enum McpClient {
    Stdio(StdioMcpClient),
    Sse(SseMcpClient),
}

impl McpClient {
    async fn request(&mut self, method: &str, params: Value) -> Result<Value> {
        match self {
            Self::Stdio(client) => client.request(method, params).await,
            Self::Sse(client) => client.request(method, params).await,
        }
    }

    async fn close(self) {
        match self {
            Self::Stdio(client) => client.close().await,
            Self::Sse(client) => client.close().await,
        }
    }
}

type McpClientPool = RwLock<HashMap<String, Arc<Mutex<McpClient>>>>;

fn client_pool() -> &'static McpClientPool {
    static POOL: std::sync::OnceLock<McpClientPool> = std::sync::OnceLock::new();
    POOL.get_or_init(|| RwLock::new(HashMap::new()))
}

fn config_key(config: &McpServerConfig) -> String {
    format!(
        "{}|{}|{}|{}|{}",
        config.id,
        config.transport,
        config.command.as_deref().unwrap_or(""),
        config.args.as_deref().unwrap_or_default().join("\u{1f}"),
        config.url.as_deref().unwrap_or("")
    )
}

async fn reconnect(config: &McpServerConfig) -> Result<Arc<Mutex<McpClient>>> {
    let client = timeout(MCP_REQUEST_TIMEOUT, async {
        match config.transport.to_ascii_lowercase().as_str() {
            "stdio" => StdioMcpClient::connect(config)
                .await
                .map(McpClient::Stdio),
            "sse" | "http+sse" | "http_sse" => SseMcpClient::connect(config)
                .await
                .map(McpClient::Sse),
            other => Err(OdonError::Tool {
                tool: "mcp".to_string(),
                message: format!(
                    "Unsupported MCP transport '{other}' on server '{}'; use stdio or sse",
                    config.name
                ),
            }),
        }
    })
    .await
    .map_err(|_| OdonError::Tool {
        tool: "mcp".to_string(),
        message: format!(
            "MCP server '{}' did not initialize within 30 seconds",
            config.name
        ),
    })??;
    Ok(Arc::new(Mutex::new(client)))
}

/// Reuse initialized MCP connections across calls. A failed request evicts the
/// client so the next call reconnects instead of retrying a dead stream.
async fn call_server(config: &McpServerConfig, method: &str, params: Value) -> Result<Value> {
    let key = config_key(config);
    let client = {
        let existing = client_pool().read().await.get(&key).cloned();
        match existing {
            Some(client) => client,
            None => {
                let candidate = reconnect(config).await?;
                let mut pool = client_pool().write().await;
                pool.entry(key.clone())
                    .or_insert_with(|| candidate)
                    .clone()
            }
        }
    };

    let mut guard = client.lock().await;
    match guard.request(method, params).await {
        Ok(result) => Ok(result),
        Err(error) => {
            let mut pool = client_pool().write().await;
            if pool
                .get(&key)
                .map(|current| Arc::ptr_eq(current, &client))
                .unwrap_or(false)
            {
                pool.remove(&key);
            }
            drop(guard);
            if let Ok(mutex) = Arc::try_unwrap(client) {
                let owned_client = mutex.into_inner();
                owned_client.close().await;
            }
            Err(error)
        }
    }
}

pub struct McpListToolsTool;

#[async_trait]
impl Tool for McpListToolsTool {
    fn name(&self) -> &str {
        "mcp_list_tools"
    }
    fn description(&self) -> &str {
        "List all tools exposed by configured external Model Context Protocol (MCP) servers"
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
    async fn execute(&self, _ctx: &ToolContext, args: Value) -> Result<Value> {
        let server_filter = args.get("server").and_then(Value::as_str);
        let servers: Vec<_> = load_mcp_servers()
            .into_iter()
            .filter(|server| {
                server.enabled
                    && server_filter
                        .map(|filter| {
                            server.id == filter || server.name.eq_ignore_ascii_case(filter)
                        })
                        .unwrap_or(true)
            })
            .collect();
        let mut discovered = Vec::with_capacity(servers.len());
        let mut total_tools = 0usize;
        for server in servers {
            match call_server(&server, "tools/list", json!({})).await {
                Ok(result) => {
                    let tools = result
                        .get("tools")
                        .and_then(Value::as_array)
                        .cloned()
                        .unwrap_or_default();
                    total_tools += tools.len();
                    discovered.push(json!({
                        "id": server.id,
                        "name": server.name,
                        "transport": server.transport,
                        "status": "online",
                        "tools": tools
                    }));
                }
                Err(error) => discovered.push(json!({
                    "id": server.id,
                    "name": server.name,
                    "transport": server.transport,
                    "status": "error",
                    "error": error.to_string(),
                    "tools": []
                })),
            }
        }
        Ok(json!({ "total_tools": total_tools, "servers": discovered }))
    }
}

pub struct McpCallTool;

#[async_trait]
impl Tool for McpCallTool {
    fn name(&self) -> &str {
        "mcp_call"
    }
    fn description(&self) -> &str {
        "Invoke a tool on a connected MCP server"
    }
    fn parameters_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "server": { "type": "string", "description": "MCP server name or id" },
                "tool": { "type": "string", "description": "Tool name on the server" },
                "arguments": { "type": "object", "description": "Arguments payload" }
            },
            "required": ["server", "tool"]
        })
    }
    fn risk_tier(&self) -> RiskTier {
        RiskTier::Tier1WorkspaceMutating
    }
    async fn execute(&self, _ctx: &ToolContext, args: Value) -> Result<Value> {
        let server = args
            .get("server")
            .and_then(|s| s.as_str())
            .unwrap_or("default");
        let tool = args.get("tool").and_then(|t| t.as_str()).unwrap_or("ping");
        let arguments = args.get("arguments").cloned().unwrap_or_else(|| json!({}));
        let config = find_server(server)?;
        call_server(
            &config,
            "tools/call",
            json!({ "name": tool, "arguments": arguments }),
        )
        .await
    }
}

pub struct McpCallStreamTool;

#[async_trait]
impl Tool for McpCallStreamTool {
    fn name(&self) -> &str {
        "mcp_call_stream"
    }
    fn description(&self) -> &str {
        "Invoke an MCP tool with a progress-capable request (streaming is not implemented yet)"
    }
    fn parameters_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "server": { "type": "string", "description": "MCP server id" },
                "tool": { "type": "string", "description": "Tool name on the server" },
                "arguments": { "type": "object", "description": "Arguments payload" }
            },
            "required": ["server", "tool"]
        })
    }
    fn risk_tier(&self) -> RiskTier {
        RiskTier::Tier1WorkspaceMutating
    }
    fn availability(&self) -> crate::registry::ToolAvailability {
        crate::registry::ToolAvailability::Unavailable {
            reason: "MCP progress streaming is not implemented yet; use mcp_call".to_string(),
        }
    }
    async fn execute(&self, _ctx: &ToolContext, args: Value) -> Result<Value> {
        let server = args
            .get("server")
            .and_then(|s| s.as_str())
            .unwrap_or("default");
        let tool = args.get("tool").and_then(|t| t.as_str()).unwrap_or("ping");
        let arguments = args.get("arguments").cloned().unwrap_or_else(|| json!({}));
        let config = find_server(server)?;
        let result = call_server(
            &config,
            "tools/call",
            json!({ "name": tool, "arguments": arguments }),
        )
        .await?;
        Ok(json!({ "streamed": false, "result": result }))
    }
}

pub struct McpRemoveServerTool;

#[async_trait]
impl Tool for McpRemoveServerTool {
    fn name(&self) -> &str {
        "mcp_remove_server"
    }
    fn description(&self) -> &str {
        "Remove an MCP server from local configuration by server ID"
    }
    fn parameters_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "server_id": { "type": "string", "description": "ID of server to remove" }
            },
            "required": ["server_id"]
        })
    }
    fn risk_tier(&self) -> RiskTier {
        RiskTier::Tier1WorkspaceMutating
    }
    async fn execute(&self, _ctx: &ToolContext, args: Value) -> Result<Value> {
        let sid = args.get("server_id").and_then(|s| s.as_str()).unwrap_or("");
        if sid.is_empty() {
            return Err(OdonError::Tool {
                tool: self.name().to_string(),
                message: "server_id is required".into(),
            });
        }
        let mut servers = load_mcp_servers();
        let before = servers.len();
        if servers.iter().all(|server| server.id != sid && !server.name.eq_ignore_ascii_case(sid)) {
            return Err(OdonError::Tool {
                tool: self.name().to_string(),
                message: format!("No MCP server matched '{sid}'"),
            });
        }
        servers.retain(|server| server.id != sid && !server.name.eq_ignore_ascii_case(sid));
        save_mcp_servers(&servers)?;
        let removed = servers.len() != before;
        Ok(json!({
            "server_id": sid,
            "removed": removed,
            "message": if removed {
                format!("Server '{sid}' was removed from the MCP configuration")
            } else {
                format!("No MCP server matched '{sid}'")
            }
        }))
    }
}

pub struct McpToggleServerTool;
#[async_trait]
impl Tool for McpToggleServerTool {
    fn name(&self) -> &str {
        "mcp_toggle_server"
    }
    fn description(&self) -> &str {
        "Enable or disable a configured MCP server without deleting it"
    }
    fn parameters_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "server_id": { "type": "string", "description": "MCP server ID or name" },
                "enabled": { "type": "boolean", "description": "Omit to toggle the current state" }
            },
            "required": ["server_id"]
        })
    }
    fn risk_tier(&self) -> RiskTier {
        RiskTier::Tier1WorkspaceMutating
    }
    async fn execute(&self, _ctx: &ToolContext, args: Value) -> Result<Value> {
        let sid = args
            .get("server_id")
            .and_then(Value::as_str)
            .ok_or_else(|| OdonError::Tool {
                tool: self.name().to_string(),
                message: "Missing 'server_id' parameter".to_string(),
            })?;
        let mut servers = load_mcp_servers();
        let server = servers
            .iter_mut()
            .find(|server| server.id == sid || server.name.eq_ignore_ascii_case(sid))
            .ok_or_else(|| OdonError::Tool {
                tool: self.name().to_string(),
                message: format!("No MCP server matched '{sid}'"),
            })?;
        let previous = server.enabled;
        server.enabled = args
            .get("enabled")
            .and_then(Value::as_bool)
            .unwrap_or(!previous);
        let enabled = server.enabled;
        let id = server.id.clone();
        let name = server.name.clone();
        save_mcp_servers(&servers)?;
        Ok(json!({
            "server_id": id,
            "name": name,
            "previous_enabled": previous,
            "enabled": enabled,
            "status": "updated"
        }))
    }
}
