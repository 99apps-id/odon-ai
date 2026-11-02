use async_trait::async_trait;
use odon_core::{OdonError, Result};
use odon_security::RiskTier;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::collections::HashMap;
use std::path::PathBuf;
use std::process::Stdio;
use std::time::Duration;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::process::{Child, ChildStdin, ChildStdout, Command};
use tokio::time::timeout;

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
        let command = config.command.as_deref().filter(|value| !value.trim().is_empty()).ok_or_else(|| {
            OdonError::Tool {
                tool: "mcp".to_string(),
                message: format!("MCP server '{}' has no command", config.name),
            }
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
        self.stdin.write_all(&encoded).await.map_err(|error| OdonError::Tool {
            tool: "mcp".to_string(),
            message: format!("Failed to write MCP request: {error}"),
        })?;
        self.stdin.flush().await.map_err(|error| OdonError::Tool {
            tool: "mcp".to_string(),
            message: format!("Failed to flush MCP request: {error}"),
        })
    }

    async fn read_response(&mut self, id: u64) -> Result<Value> {
        while let Some(line) = self.stdout.next_line().await.map_err(|error| OdonError::Tool {
            tool: "mcp".to_string(),
            message: format!("Failed reading MCP response: {error}"),
        })? {
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
            return response.get("result").cloned().ok_or_else(|| OdonError::Tool {
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

fn find_server(server: &str) -> Result<McpServerConfig> {
    load_mcp_servers()
        .into_iter()
        .find(|item| item.enabled && (item.id == server || item.name.eq_ignore_ascii_case(server)))
        .ok_or_else(|| OdonError::Tool {
            tool: "mcp".to_string(),
            message: format!("Enabled MCP server '{server}' was not found"),
        })
}

async fn call_server(config: &McpServerConfig, method: &str, params: Value) -> Result<Value> {
    let mut client = timeout(MCP_REQUEST_TIMEOUT, StdioMcpClient::connect(config))
        .await
        .map_err(|_| OdonError::Tool {
            tool: "mcp".to_string(),
            message: format!("MCP server '{}' did not initialize within 30 seconds", config.name),
        })??;
    let result = client.request(method, params).await;
    client.close().await;
    result
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
        "Invoke an MCP tool and stream intermediate progress chunks"
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
    async fn execute(&self, _ctx: &ToolContext, args: Value) -> Result<Value> {
        let server = args.get("server").and_then(|s| s.as_str()).unwrap_or("default");
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
            return Err(OdonError::Tool { tool: self.name().to_string(), message: "server_id is required".into() });
        }
        let mut servers = load_mcp_servers();
        let before = servers.len();
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
