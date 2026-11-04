use crate::registry::{Tool, ToolContext};
use async_trait::async_trait;
use odon_core::error::{OdonError, Result};
use odon_security::RiskTier;
use reqwest::Client;
use serde_json::{json, Value};
use std::time::Duration;

const DEFAULT_TIMEOUT: Duration = Duration::from_secs(60);
const OUTPUT_LIMIT: usize = 64 * 1024;

fn bounded_body(raw: &[u8]) -> (String, bool) {
    if raw.len() <= OUTPUT_LIMIT {
        (String::from_utf8_lossy(raw).to_string(), false)
    } else {
        (
            String::from_utf8_lossy(&raw[..OUTPUT_LIMIT]).to_string(),
            true,
        )
    }
}

fn http_client() -> Result<Client> {
    Client::builder()
        .user_agent("odon-api-testing/0.1")
        .build()
        .map_err(|e| OdonError::Tool {
            tool: "api_testing".to_string(),
            message: format!("Failed to build HTTP client: {e}"),
        })
}

fn timeout_or_default(args: &Value, default: Duration) -> Duration {
    args.get("timeout_secs")
        .and_then(|v| v.as_u64())
        .map(Duration::from_secs)
        .unwrap_or(default)
}

fn request_timeout(args: &Value) -> Duration {
    timeout_or_default(args, DEFAULT_TIMEOUT)
}

pub struct RestClientTool;

#[async_trait]
impl Tool for RestClientTool {
    fn name(&self) -> &str {
        "rest_client"
    }
    fn description(&self) -> &str {
        "Send a REST API request and return structured HTTP response metadata"
    }
    fn parameters_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "method": {
                    "type": "string",
                    "enum": ["GET", "POST", "PUT", "PATCH", "DELETE", "HEAD", "OPTIONS"]
                },
                "url": {
                    "type": "string",
                    "format": "uri"
                },
                "headers": {
                    "type": "object",
                    "additionalProperties": { "type": "string" }
                },
                "body": {
                    "type": "object",
                    "description": "JSON body for requests"
                },
                "timeout_secs": {
                    "type": "integer",
                    "minimum": 1,
                    "maximum": 900
                }
            },
            "required": ["method", "url"]
        })
    }
    fn risk_tier(&self) -> RiskTier {
        RiskTier::Tier2SystemMutating
    }

    async fn execute(&self, ctx: &ToolContext, args: Value) -> Result<Value> {
        let method =
            args.get("method")
                .and_then(|v| v.as_str())
                .ok_or_else(|| OdonError::Tool {
                    tool: self.name().to_string(),
                    message: "method is required".to_string(),
                })?;
        let url = args
            .get("url")
            .and_then(|v| v.as_str())
            .ok_or_else(|| OdonError::Tool {
                tool: self.name().to_string(),
                message: "url is required".to_string(),
            })?;
        let headers = args.get("headers").and_then(|v| v.as_object()).cloned();
        let body = args.get("body").cloned();
        let time_limit = request_timeout(&args);

        let client = http_client()?;
        let mut request = match method {
            "GET" => client.get(url),
            "POST" => client.post(url),
            "PUT" => client.put(url),
            "PATCH" => client.patch(url),
            "DELETE" => client.delete(url),
            "HEAD" => client.head(url),
            "OPTIONS" => client.request(reqwest::Method::OPTIONS, url),
            other => {
                return Err(OdonError::Tool {
                    tool: self.name().to_string(),
                    message: format!("Unsupported HTTP method: {other}"),
                })
            }
        };

        if let Some(map) = headers {
            for (k, v) in map {
                if let Some(value) = v.as_str() {
                    request = request.header(k.as_str(), value);
                }
            }
        }

        if let Some(body_value) = body {
            request = request.json(&body_value);
        }

        let response = tokio::time::timeout(time_limit, request.send())
            .await
            .map_err(|_| OdonError::Tool {
                tool: self.name().to_string(),
                message: "HTTP request timed out".to_string(),
            })?
            .map_err(|error| OdonError::Tool {
                tool: self.name().to_string(),
                message: error.to_string(),
            })?;

        let status = response.status().as_u16();
        let headers_map = response
            .headers()
            .iter()
            .map(|(k, v)| {
                (
                    k.as_str().to_string(),
                    Value::String(v.to_str().unwrap_or("").to_string()),
                )
            })
            .collect::<serde_json::Map<String, Value>>();
        let bytes = response.bytes().await.map_err(|e| OdonError::Tool {
            tool: self.name().to_string(),
            message: format!("Failed to read response body: {e}"),
        })?;
        let (text, truncated) = bounded_body(&bytes);

        Ok(json!({
            "method": method,
            "url": url,
            "status": status,
            "headers": headers_map,
            "body": text,
            "body_truncated": truncated
        }))
    }
}

pub struct GraphqlClientTool;

#[async_trait]
impl Tool for GraphqlClientTool {
    fn name(&self) -> &str {
        "graphql_client"
    }
    fn description(&self) -> &str {
        "Send a GraphQL request and return structured response metadata"
    }
    fn parameters_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "url": {
                    "type": "string",
                    "format": "uri"
                },
                "query": {
                    "type": "string",
                    "description": "GraphQL query/mutation string"
                },
                "variables": {
                    "type": "object",
                    "description": "GraphQL variables"
                },
                "headers": {
                    "type": "object",
                    "additionalProperties": { "type": "string" }
                },
                "timeout_secs": {
                    "type": "integer",
                    "minimum": 1,
                    "maximum": 900
                }
            },
            "required": ["url", "query"]
        })
    }
    fn risk_tier(&self) -> RiskTier {
        RiskTier::Tier2SystemMutating
    }

    async fn execute(&self, ctx: &ToolContext, args: Value) -> Result<Value> {
        let url = args
            .get("url")
            .and_then(|v| v.as_str())
            .ok_or_else(|| OdonError::Tool {
                tool: self.name().to_string(),
                message: "url is required".to_string(),
            })?;
        let query = args
            .get("query")
            .and_then(|v| v.as_str())
            .ok_or_else(|| OdonError::Tool {
                tool: self.name().to_string(),
                message: "query is required".to_string(),
            })?;
        let variables = args.get("variables").cloned();
        let headers = args.get("headers").and_then(|v| v.as_object()).cloned();
        let time_limit = request_timeout(&args);

        let client = http_client()?;
        let mut request = client
            .post(url)
            .json(&json!({ "query": query, "variables": variables }));

        if let Some(map) = headers {
            for (k, v) in map {
                if let Some(value) = v.as_str() {
                    request = request.header(k.as_str(), value);
                }
            }
        }

        let response = tokio::time::timeout(time_limit, request.send())
            .await
            .map_err(|_| OdonError::Tool {
                tool: self.name().to_string(),
                message: "GraphQL request timed out".to_string(),
            })?
            .map_err(|error| OdonError::Tool {
                tool: self.name().to_string(),
                message: error.to_string(),
            })?;

        let status = response.status().as_u16();
        let bytes = response.bytes().await.map_err(|e| OdonError::Tool {
            tool: self.name().to_string(),
            message: format!("Failed to read response body: {e}"),
        })?;
        let (text, truncated) = bounded_body(&bytes);

        Ok(json!({
            "url": url,
            "status": status,
            "body": text,
            "body_truncated": truncated
        }))
    }
}

pub struct WebhookTestTool;

#[async_trait]
impl Tool for WebhookTestTool {
    fn name(&self) -> &str {
        "webhook_test"
    }
    fn description(&self) -> &str {
        "Send a webhook payload to a target URL and return response details"
    }
    fn parameters_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "url": {
                    "type": "string",
                    "format": "uri"
                },
                "method": {
                    "type": "string",
                    "enum": ["POST", "PUT"],
                    "default": "POST"
                },
                "headers": {
                    "type": "object",
                    "additionalProperties": { "type": "string" }
                },
                "payload": {
                    "type": "object",
                    "description": "Webhook payload body"
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

    async fn execute(&self, ctx: &ToolContext, args: Value) -> Result<Value> {
        let url = args
            .get("url")
            .and_then(|v| v.as_str())
            .ok_or_else(|| OdonError::Tool {
                tool: self.name().to_string(),
                message: "url is required".to_string(),
            })?;
        let method = args
            .get("method")
            .and_then(|v| v.as_str())
            .unwrap_or("POST");
        let headers = args.get("headers").and_then(|v| v.as_object()).cloned();
        let payload = args.get("payload").cloned().unwrap_or(Value::Null);
        let time_limit = request_timeout(&args);

        let client = http_client()?;
        let mut request = match method {
            "POST" => client.post(url),
            "PUT" => client.put(url),
            other => {
                return Err(OdonError::Tool {
                    tool: self.name().to_string(),
                    message: format!("Unsupported webhook method: {other}"),
                })
            }
        };

        if let Some(map) = headers {
            for (k, v) in map {
                if let Some(value) = v.as_str() {
                    request = request.header(k.as_str(), value);
                }
            }
        }

        let request = request.json(&payload);
        let response = tokio::time::timeout(time_limit, request.send())
            .await
            .map_err(|_| OdonError::Tool {
                tool: self.name().to_string(),
                message: "Webhook request timed out".to_string(),
            })?
            .map_err(|error| OdonError::Tool {
                tool: self.name().to_string(),
                message: error.to_string(),
            })?;

        let status = response.status().as_u16();
        let bytes = response.bytes().await.map_err(|e| OdonError::Tool {
            tool: self.name().to_string(),
            message: format!("Failed to read response body: {e}"),
        })?;
        let (text, truncated) = bounded_body(&bytes);

        Ok(json!({
            "url": url,
            "method": method,
            "status": status,
            "body": text,
            "body_truncated": truncated
        }))
    }
}
