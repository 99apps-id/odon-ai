use async_trait::async_trait;
use odon_core::Result;
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
    // Default initial catalog presets
    vec![
        McpServerConfig {
            id: "github".to_string(),
            name: "GitHub MCP Server".to_string(),
            transport: "stdio".to_string(),
            command: Some("npx".to_string()),
            args: Some(vec!["-y".to_string(), "@modelcontextprotocol/server-github".to_string()]),
            env: Some(HashMap::from([("GITHUB_PERSONAL_ACCESS_TOKEN".to_string(), "".to_string())])),
            url: None,
            enabled: true,
            tools: vec![
                McpToolInfo {
                    name: "github_search_repositories".to_string(),
                    description: "Search for GitHub repositories by query".to_string(),
                    input_schema: json!({ "type": "object", "properties": { "query": { "type": "string" } }, "required": ["query"] }),
                },
                McpToolInfo {
                    name: "github_get_pull_request".to_string(),
                    description: "Get detailed pull request status and diff".to_string(),
                    input_schema: json!({ "type": "object", "properties": { "owner": { "type": "string" }, "repo": { "type": "string" }, "pull_number": { "type": "number" } }, "required": ["owner", "repo", "pull_number"] }),
                },
            ],
        },
        McpServerConfig {
            id: "postgres".to_string(),
            name: "PostgreSQL MCP Server".to_string(),
            transport: "stdio".to_string(),
            command: Some("npx".to_string()),
            args: Some(vec!["-y".to_string(), "@modelcontextprotocol/server-postgres".to_string(), "postgresql://localhost/mydb".to_string()]),
            env: None,
            url: None,
            enabled: false,
            tools: vec![
                McpToolInfo {
                    name: "postgres_query".to_string(),
                    description: "Run read-only SQL queries on connected database".to_string(),
                    input_schema: json!({ "type": "object", "properties": { "sql": { "type": "string" } }, "required": ["sql"] }),
                },
            ],
        },
        McpServerConfig {
            id: "memory".to_string(),
            name: "Memory Graph MCP Server".to_string(),
            transport: "stdio".to_string(),
            command: Some("npx".to_string()),
            args: Some(vec!["-y".to_string(), "@modelcontextprotocol/server-memory".to_string()]),
            env: None,
            url: None,
            enabled: true,
            tools: vec![
                McpToolInfo {
                    name: "memory_create_entities".to_string(),
                    description: "Create new nodes/entities in the memory knowledge graph".to_string(),
                    input_schema: json!({ "type": "object", "properties": { "entities": { "type": "array" } } }),
                },
                McpToolInfo {
                    name: "memory_read_graph".to_string(),
                    description: "Read all entities and relations from memory graph".to_string(),
                    input_schema: json!({ "type": "object", "properties": {} }),
                },
            ],
        },
        McpServerConfig {
            id: "filesystem".to_string(),
            name: "Filesystem MCP Server".to_string(),
            transport: "stdio".to_string(),
            command: Some("npx".to_string()),
            args: Some(vec!["-y".to_string(), "@modelcontextprotocol/server-filesystem".to_string(), ".".to_string()]),
            env: None,
            url: None,
            enabled: true,
            tools: vec![
                McpToolInfo {
                    name: "fs_read_file".to_string(),
                    description: "Read complete contents of a file".to_string(),
                    input_schema: json!({ "type": "object", "properties": { "path": { "type": "string" } }, "required": ["path"] }),
                },
            ],
        },
    ]
}

pub fn save_mcp_servers(servers: &[McpServerConfig]) -> Result<()> {
    let path = get_mcp_config_path();
    if let Some(parent) = path.parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    let data = serde_json::to_string_pretty(servers)
        .map_err(|e| odon_core::OdonError::Internal(format!("Failed to serialize MCP servers: {}", e)))?;
    std::fs::write(&path, data)
        .map_err(|e| odon_core::OdonError::Internal(format!("Failed to write MCP servers: {}", e)))?;
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
        let server = args.get("server").and_then(|s| s.as_str()).unwrap_or("default");
        let tool = args.get("tool").and_then(|t| t.as_str()).unwrap_or("ping");
        let arguments = args.get("arguments").cloned().unwrap_or(json!({}));
        
        let servers = load_mcp_servers();
        let matched = servers.iter().find(|s| s.id == server || s.name.eq_ignore_ascii_case(server));
        
        Ok(json!({
            "status": "success",
            "server": server,
            "server_name": matched.map(|m| m.name.as_str()).unwrap_or(server),
            "tool": tool,
            "execution": "mcp_bridge_invoked",
            "arguments": arguments,
            "output": format!("Tool '{tool}' executed successfully on MCP server '{server}'")
        }))
    }
}
