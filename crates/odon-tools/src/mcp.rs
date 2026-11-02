use async_trait::async_trait;
use odon_core::{OdonError, Result};
use odon_security::RiskTier;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::collections::HashMap;
use std::path::PathBuf;

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
    async fn execute(&self, _ctx: &ToolContext, _args: Value) -> Result<Value> {
        let servers = load_mcp_servers();
        let enabled_servers: Vec<_> = servers.into_iter().filter(|s| s.enabled).collect();
        let total_tools: usize = enabled_servers.iter().map(|s| s.tools.len()).sum();
        Ok(json!({
            "total_tools": total_tools,
            "servers": enabled_servers
        }))
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
        Err(OdonError::Tool {
            tool: self.name().to_string(),
            message: format!(
                "MCP execution is not implemented in this build. Server '{server}' and tool '{tool}' are configured, but no stdio/SSE MCP bridge is running."
            ),
        })
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
        Err(OdonError::Tool {
            tool: self.name().to_string(),
            message: format!(
                "Streaming MCP execution is not implemented in this build. Server '{server}' and tool '{tool}' are configured, but no stdio/SSE MCP bridge is running."
            ),
        })
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
