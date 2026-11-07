use async_trait::async_trait;
use odon_core::{OdonError, Result};
use odon_security::RiskTier;
use serde_json::{json, Value};
use std::time::Duration;

use crate::ops_ext::daemon_json;
use crate::registry::{Tool, ToolContext};

fn storage(ctx: &ToolContext) -> Result<&odon_storage::StorageEngine> {
    ctx.storage.as_deref().ok_or_else(|| OdonError::Tool {
        tool: "provider_ops".to_string(),
        message: "Storage engine unavailable in tool context".to_string(),
    })
}

pub struct ProviderListTool;
#[async_trait]
impl Tool for ProviderListTool {
    fn name(&self) -> &str {
        "provider_list"
    }
    fn description(&self) -> &str {
        "List configured model providers with redacted credentials"
    }
    fn parameters_schema(&self) -> Value {
        json!({ "type": "object", "properties": {} })
    }
    fn risk_tier(&self) -> RiskTier {
        RiskTier::Tier0ReadOnly
    }
    async fn execute(&self, ctx: &ToolContext, _args: Value) -> Result<Value> {
        let configs = storage(ctx)?
            .list_provider_configs()
            .map_err(|e| OdonError::Tool {
                tool: self.name().to_string(),
                message: e.to_string(),
            })?;
        let providers: Vec<Value> = configs
            .iter()
            .map(|config| {
                json!({
                    "provider_id": config.provider_id,
                    "name": config.name,
                    "base_url": config.base_url,
                    "api_key_configured": !config.api_key.trim().is_empty(),
                    "is_default": config.is_default,
                    "models": serde_json::from_str::<Value>(&config.models_json).unwrap_or_else(|_| json!([])),
                    "updated_at": config.updated_at,
                })
            })
            .collect();
        Ok(json!({ "providers": providers, "count": providers.len() }))
    }
}

pub struct ProviderTestTool;
#[async_trait]
impl Tool for ProviderTestTool {
    fn name(&self) -> &str {
        "provider_test"
    }
    fn description(&self) -> &str {
        "Test a configured provider endpoint by fetching models or invoking a model with a tiny prompt"
    }
    fn parameters_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "provider_id": { "type": "string" },
                "model": { "type": "string", "description": "Optional model ID; when supplied, sends a one-token OpenAI-compatible chat request" }
            },
            "required": ["provider_id"]
        })
    }
    fn risk_tier(&self) -> RiskTier {
        RiskTier::Tier1WorkspaceMutating
    }
    async fn execute(&self, ctx: &ToolContext, args: Value) -> Result<Value> {
        let provider_id = args
            .get("provider_id")
            .and_then(Value::as_str)
            .ok_or_else(|| OdonError::Tool {
                tool: self.name().to_string(),
                message: "Missing 'provider_id' parameter".to_string(),
            })?;
        let config = storage(ctx)?
            .get_provider_config(provider_id)
            .map_err(|e| OdonError::Tool {
                tool: self.name().to_string(),
                message: e.to_string(),
            })?
            .ok_or_else(|| OdonError::Tool {
                tool: self.name().to_string(),
                message: format!("Provider '{provider_id}' was not found"),
            })?;

        let base_url = config
            .base_url
            .as_deref()
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .ok_or_else(|| OdonError::Tool {
                tool: self.name().to_string(),
                message: format!("Provider '{provider_id}' has no base URL"),
            })?
            .trim_end_matches('/')
            .to_string();
        let model = args
            .get("model")
            .and_then(Value::as_str)
            .map(str::trim)
            .filter(|value| !value.is_empty());
        let endpoint = if model.is_some() {
            format!("{base_url}/chat/completions")
        } else {
            format!("{base_url}/models")
        };
        let validated = crate::net_guard::validate_url(&endpoint).await?;
        let started = std::time::Instant::now();
        let client =
            validated.client(reqwest::Client::builder().timeout(Duration::from_secs(30)))?;
        let mut request = if model.is_some() {
            client.post(validated.as_str()).json(&json!({
                "model": model,
                "messages": [{ "role": "user", "content": "ping" }],
                "max_tokens": 1,
                "stream": false
            }))
        } else {
            client.get(validated.as_str())
        };
        if !config.api_key.trim().is_empty() {
            request = request.bearer_auth(config.api_key.trim());
        }
        let response = request.send().await.map_err(|e| OdonError::Tool {
            tool: self.name().to_string(),
            message: format!("Provider request failed: {e}"),
        })?;
        let status = response.status().as_u16();
        let body = response.text().await.unwrap_or_default();
        let parsed: Value = serde_json::from_str(&body).unwrap_or(Value::Null);
        let models: Vec<String> = parsed
            .pointer("/data")
            .and_then(Value::as_array)
            .map(|items| {
                items
                    .iter()
                    .filter_map(|item| item.get("id").and_then(Value::as_str))
                    .map(String::from)
                    .collect()
            })
            .unwrap_or_default();
        let completion = if model.is_some() {
            parsed
                .pointer("/choices/0/message/content")
                .and_then(Value::as_str)
                .map(String::from)
        } else {
            None
        };
        Ok(json!({
            "provider_id": provider_id,
            "endpoint": validated.as_str(),
            "model": model,
            "status_code": status,
            "success": (200..300).contains(&status),
            "model_count": models.len(),
            "models": models,
            "completion": completion,
            "duration_ms": started.elapsed().as_millis()
        }))
    }
}

pub struct ProviderDeleteTool;
#[async_trait]
impl Tool for ProviderDeleteTool {
    fn name(&self) -> &str {
        "provider_delete"
    }
    fn description(&self) -> &str {
        "Remove a configured OpenAI-compatible provider and its stored credential"
    }
    fn parameters_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "provider_id": { "type": "string" },
                "confirm": { "type": "boolean", "description": "Must be true" }
            },
            "required": ["provider_id", "confirm"]
        })
    }
    fn risk_tier(&self) -> RiskTier {
        RiskTier::Tier2SystemMutating
    }
    async fn execute(&self, ctx: &ToolContext, args: Value) -> Result<Value> {
        let provider_id = args
            .get("provider_id")
            .and_then(Value::as_str)
            .ok_or_else(|| OdonError::Tool {
                tool: self.name().to_string(),
                message: "Missing 'provider_id' parameter".to_string(),
            })?;
        if !args
            .get("confirm")
            .and_then(Value::as_bool)
            .unwrap_or(false)
        {
            return Err(OdonError::Tool {
                tool: self.name().to_string(),
                message: "Set confirm=true to remove this provider credential".to_string(),
            });
        }
        let engine = storage(ctx)?;
        let existing = engine
            .get_provider_config(provider_id)
            .map_err(|e| OdonError::Tool {
                tool: self.name().to_string(),
                message: e.to_string(),
            })?
            .ok_or_else(|| OdonError::Tool {
                tool: self.name().to_string(),
                message: format!("Provider '{provider_id}' was not found"),
            })?;
        // Ask the daemon first so the in-memory provider is unregistered too;
        // fall back to direct storage for CLI-only execution contexts.
        let daemon_deleted = daemon_json(
            reqwest::Method::DELETE,
            &format!("/api/providers/config/{}", provider_id),
            None,
            self.name(),
        )
        .await
        .map(|(status, _)| (200..300).contains(&status))
        .unwrap_or(false);
        let deleted = daemon_deleted
            || engine
                .delete_provider_config(provider_id)
                .map_err(|e| OdonError::Tool {
                    tool: self.name().to_string(),
                    message: e.to_string(),
                })?;
        Ok(json!({
            "provider_id": provider_id,
            "was_default": existing.is_default,
            "deleted": deleted,
            "runtime_unregistered": daemon_deleted,
            "status": "removed"
        }))
    }
}

pub struct ProviderSetDefaultTool;
#[async_trait]
impl Tool for ProviderSetDefaultTool {
    fn name(&self) -> &str {
        "provider_set_default"
    }
    fn description(&self) -> &str {
        "Mark a configured model provider as the default"
    }
    fn parameters_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "provider_id": { "type": "string" }
            },
            "required": ["provider_id"]
        })
    }
    fn risk_tier(&self) -> RiskTier {
        RiskTier::Tier1WorkspaceMutating
    }
    async fn execute(&self, ctx: &ToolContext, args: Value) -> Result<Value> {
        let provider_id = args
            .get("provider_id")
            .and_then(Value::as_str)
            .ok_or_else(|| OdonError::Tool {
                tool: self.name().to_string(),
                message: "Missing 'provider_id' parameter".to_string(),
            })?;
        let engine = storage(ctx)?;
        let config = engine
            .get_provider_config(provider_id)
            .map_err(|e| OdonError::Tool {
                tool: self.name().to_string(),
                message: e.to_string(),
            })?
            .ok_or_else(|| OdonError::Tool {
                tool: self.name().to_string(),
                message: format!("Provider '{provider_id}' was not found"),
            })?;
        let models = serde_json::from_str::<Vec<String>>(&config.models_json).unwrap_or_default();
        // Ask the daemon first so the in-memory runtime default is updated too;
        // fall back to direct storage for CLI-only execution contexts.
        let daemon_updated = daemon_json(
            reqwest::Method::POST,
            "/api/providers/config",
            Some(json!({
                "provider_id": config.provider_id,
                "api_key": config.api_key,
                "base_url": config.base_url,
                "is_default": true,
                "name": config.name,
                "models": models
            })),
            self.name(),
        )
        .await
        .map(|(status, _)| (200..300).contains(&status))
        .unwrap_or(false);
        if !daemon_updated {
            engine
                .save_provider_config(
                    &config.provider_id,
                    &config.api_key,
                    config.base_url.as_deref(),
                    true,
                    config.name.as_deref(),
                    &config.models_json,
                    None,
                )
                .map_err(|e| OdonError::Tool {
                    tool: self.name().to_string(),
                    message: e.to_string(),
                })?;
        }
        Ok(json!({
            "provider_id": provider_id,
            "is_default": true,
            "runtime_updated": daemon_updated,
            "status": "updated"
        }))
    }
}
