use crate::registry::{Tool, ToolContext};
use async_trait::async_trait;
use odon_core::error::{OdonError, Result};
use odon_security::RiskTier;
use serde_json::{json, Value};
use std::time::Duration;

const HTTP_TIMEOUT: Duration = Duration::from_secs(60);
const OUTPUT_LIMIT: usize = 64 * 1024;

fn bounded_output(raw: &[u8]) -> (String, bool) {
    if raw.len() <= OUTPUT_LIMIT {
        (String::from_utf8_lossy(raw).to_string(), false)
    } else {
        (
            String::from_utf8_lossy(&raw[..OUTPUT_LIMIT]).to_string(),
            true,
        )
    }
}

pub struct MetricDashboardTool;

#[async_trait]
impl Tool for MetricDashboardTool {
    fn name(&self) -> &str {
        "metric_dashboard"
    }
    fn description(&self) -> &str {
        "Collect metric dashboard data from a configured endpoint"
    }
    fn parameters_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "url": {
                    "type": "string",
                    "format": "uri",
                    "description": "Metrics endpoint URL"
                },
                "timeout_secs": {
                    "type": "integer",
                    "minimum": 1,
                    "maximum": 900
                }
            },
            "required": ["url"]
        })
    }
    fn risk_tier(&self) -> RiskTier {
        RiskTier::Tier2SystemMutating
    }

    async fn execute(&self, _ctx: &ToolContext, args: Value) -> Result<Value> {
        let url = args
            .get("url")
            .and_then(|v| v.as_str())
            .ok_or_else(|| OdonError::Tool {
                tool: self.name().to_string(),
                message: "url is required".to_string(),
            })?;
        let time_limit = args
            .get("timeout_secs")
            .and_then(|v| v.as_u64())
            .map(Duration::from_secs)
            .unwrap_or(HTTP_TIMEOUT);
        let (client, url) = crate::net_guard::ValidatedTarget::pinned_client_for(
            url,
            reqwest::Client::builder().user_agent("odon-monitoring/0.1"),
        )
        .await?;
        let url = url.as_str();
        let response = tokio::time::timeout(time_limit, client.get(url).send())
            .await
            .map_err(|_| OdonError::Tool {
                tool: self.name().to_string(),
                message: "Request timed out".to_string(),
            })?
            .map_err(|error| OdonError::Tool {
                tool: self.name().to_string(),
                message: error.to_string(),
            })?;
        let status = response.status().as_u16();
        let bytes = response.bytes().await.map_err(|e| OdonError::Tool {
            tool: self.name().to_string(),
            message: format!("Failed to read body: {e}"),
        })?;
        let (text, truncated) = bounded_output(&bytes);
        Ok(json!({
            "url": url,
            "status": status,
            "body": text,
            "body_truncated": truncated
        }))
    }
}

pub struct UptimeMonitorTool;

#[async_trait]
impl Tool for UptimeMonitorTool {
    fn name(&self) -> &str {
        "uptime_monitor"
    }
    fn description(&self) -> &str {
        "Check uptime status for a target endpoint"
    }
    fn parameters_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "url": {
                    "type": "string",
                    "format": "uri"
                },
                "timeout_secs": {
                    "type": "integer",
                    "minimum": 1,
                    "maximum": 900
                }
            },
            "required": ["url"]
        })
    }
    fn risk_tier(&self) -> RiskTier {
        RiskTier::Tier2SystemMutating
    }

    async fn execute(&self, _ctx: &ToolContext, args: Value) -> Result<Value> {
        let url = args
            .get("url")
            .and_then(|v| v.as_str())
            .ok_or_else(|| OdonError::Tool {
                tool: self.name().to_string(),
                message: "url is required".to_string(),
            })?;
        let time_limit = args
            .get("timeout_secs")
            .and_then(|v| v.as_u64())
            .map(Duration::from_secs)
            .unwrap_or(HTTP_TIMEOUT);
        let (client, url) = crate::net_guard::ValidatedTarget::pinned_client_for(
            url,
            reqwest::Client::builder().user_agent("odon-monitoring/0.1"),
        )
        .await?;
        let url = url.as_str();
        let response = tokio::time::timeout(time_limit, client.head(url).send())
            .await
            .map_err(|_| OdonError::Tool {
                tool: self.name().to_string(),
                message: "Request timed out".to_string(),
            })?
            .map_err(|error| OdonError::Tool {
                tool: self.name().to_string(),
                message: error.to_string(),
            })?;
        let status = response.status().as_u16();
        Ok(json!({
            "url": url,
            "status": status,
            "up": status < 400
        }))
    }
}
