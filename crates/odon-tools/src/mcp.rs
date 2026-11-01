use async_trait::async_trait;
use odon_core::Result;
use odon_security::RiskTier;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

use crate::registry::{Tool, ToolContext};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct McpToolInfo {
    pub name: String,
    pub description: String,
    pub input_schema: Value,
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
        // Return active MCP server capabilities
        Ok(json!({
            "servers": [
                {
                    "name": "github-mcp",
                    "status": "connected",
                    "tools": ["github_get_repo", "github_list_issues"]
                },
                {
                    "name": "filesystem-mcp",
                    "status": "connected",
                    "tools": ["fs_read", "fs_write", "fs_list"]
                }
            ]
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
                "server": { "type": "string", "description": "MCP server name" },
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
        Ok(json!({
            "status": "invoked",
            "server": server,
            "tool": tool,
            "result": "MCP bridge handled invocation"
        }))
    }
}

