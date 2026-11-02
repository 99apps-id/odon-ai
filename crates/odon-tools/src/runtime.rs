use crate::registry::{Tool, ToolContext};
use async_trait::async_trait;
use odon_core::error::Result;
use odon_security::RiskTier;
use serde_json::{json, Value};
use std::net::TcpStream;

pub struct DevServerStatusTool;

#[async_trait]
impl Tool for DevServerStatusTool {
    fn name(&self) -> &str { "dev_server_status" }
    fn description(&self) -> &str {
        "Check whether candidate dev server ports are open and responding"
    }
    fn parameters_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "host": { "type": "string", "default": "127.0.0.1" },
                "ports": { "type": "array", "items": { "type": "integer", "minimum": 1, "maximum": 65535 } }
            }
        })
    }
    fn risk_tier(&self) -> RiskTier { RiskTier::Tier0ReadOnly }

    async fn execute(&self, _ctx: &ToolContext, args: Value) -> Result<Value> {
        let host = args.get("host").and_then(Value::as_str).unwrap_or("127.0.0.1");
        let ports = args.get("ports").and_then(Value::as_array).map(|items| {
            items.iter().filter_map(|item| item.as_u64()).filter(|port| *port > 0 && *port <= 65535).map(|port| port as u16).collect::<Vec<_>>()
        }).unwrap_or_else(|| vec![3000, 5173, 8080, 8000, 4000, 4200, 5000, 8888, 9000]);

        let mut results = Vec::new();
        for port in ports {
            let open = is_port_open(host, port);
            results.push(json!({ "host": host, "port": port, "open": open }));
        }
        Ok(json!({ "host": host, "results": results }))
    }
}

pub struct BrowserStatusTool;

#[async_trait]
impl Tool for BrowserStatusTool {
    fn name(&self) -> &str { "browser_status" }
    fn description(&self) -> &str {
        "Report available browser automation capabilities on this host"
    }
    fn parameters_schema(&self) -> Value {
        json!({ "type": "object", "properties": {} })
    }
    fn risk_tier(&self) -> RiskTier { RiskTier::Tier0ReadOnly }

    async fn execute(&self, _ctx: &ToolContext, _args: Value) -> Result<Value> {
        let has_chrome = has_executable("chrome") || has_executable("google-chrome") || has_executable("chromium") || has_executable("chromium-browser");
        let has_edge = has_executable("msedge");
        Ok(json!({
            "chrome": has_chrome,
            "edge": has_edge,
            "headless_screenshot": has_chrome || has_edge,
            "persistent_session": false,
            "note": if has_chrome || has_edge { "Headless screenshots are available via browser_screenshot." } else { "No Chromium-class browser detected on PATH." }
        }))
    }
}

fn is_port_open(host: &str, port: u16) -> bool {
    TcpStream::connect_timeout(&std::net::SocketAddr::new(host.parse().unwrap_or(std::net::IpAddr::V4(std::net::Ipv4Addr::LOCALHOST)), port), std::time::Duration::from_millis(200)).is_ok()
}

fn has_executable(name: &str) -> bool {
    std::process::Command::new(name).arg("--version").stdout(std::process::Stdio::null()).stderr(std::process::Stdio::null()).status().map(|s| s.success()).unwrap_or(false)
}
