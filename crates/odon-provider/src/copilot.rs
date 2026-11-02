//! Live GitHub Copilot model catalog.
//!
//! The Copilot `/models` endpoint is the source of truth for which models the
//! current account can actually use, their supported endpoints, and their
//! context windows. We filter out models disabled by policy or hidden from the
//! model picker so the UI never offers a model that would return
//! `modelnotsupported`.

use std::sync::OnceLock;
use std::time::{Duration, Instant};

use serde_json::Value;
use tokio::sync::Mutex;

#[derive(Debug, Clone)]
pub struct CopilotModelInfo {
    pub id: String,
    pub name: String,
    pub context: u64,
    pub vendor: String,
    pub endpoints: Vec<String>,
    pub preview: bool,
}

const CACHE_TTL: Duration = Duration::from_secs(300);

type CopilotCacheValue = (Instant, Vec<CopilotModelInfo>);
static CACHE: OnceLock<Mutex<Option<CopilotCacheValue>>> = OnceLock::new();

/// Fetch the model catalog using the host-discovered Copilot session. Returns
/// `None` when no host credentials are available or the API call fails.
pub async fn copilot_models() -> Option<Vec<CopilotModelInfo>> {
    let cache = CACHE.get_or_init(|| Mutex::new(None));
    {
        let guard = cache.lock().await;
        if let Some((fetched_at, models)) = guard.as_ref() {
            if fetched_at.elapsed() < CACHE_TTL {
                return Some(models.clone());
            }
        }
    }

    let token = crate::host_creds::discover_github_copilot().await?;
    let models = fetch_models_with_token(&token.access_token).await?;
    *cache.lock().await = Some((Instant::now(), models.clone()));
    Some(models)
}

/// Fetch the catalog with a specific bearer token (used by the provider, which
/// may hold an OAuth-login token rather than a host-discovered one).
pub async fn fetch_models_with_token(token: &str) -> Option<Vec<CopilotModelInfo>> {
    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(10))
        .build()
        .ok()?;
    let response = client
        .get("https://api.githubcopilot.com/models")
        .header("Authorization", format!("Bearer {token}"))
        .header("Editor-Version", "vscode/1.110.0")
        .header("Editor-Plugin-Version", "copilot-chat/0.38.0")
        .header("User-Agent", "GitHubCopilotChat/0.38.0")
        .header("copilot-integration-id", "vscode-chat")
        .header("x-github-api-version", "2025-04-01")
        .send()
        .await
        .ok()?;
    if !response.status().is_success() {
        return None;
    }
    let value: Value = response.json().await.ok()?;
    Some(parse_models(&value))
}

pub fn parse_models(value: &Value) -> Vec<CopilotModelInfo> {
    let mut models = Vec::new();
    let Some(items) = value.get("data").and_then(|data| data.as_array()) else {
        return models;
    };
    for item in items {
        let kind = item
            .pointer("/capabilities/type")
            .and_then(|value| value.as_str())
            .unwrap_or("");
        if kind != "chat" {
            continue;
        }
        let picker_enabled = item
            .get("model_picker_enabled")
            .and_then(|value| value.as_bool())
            .unwrap_or(false);
        let policy = item
            .pointer("/policy/state")
            .and_then(|value| value.as_str())
            .unwrap_or("");
        if !picker_enabled || policy.eq_ignore_ascii_case("disabled") {
            continue;
        }
        let Some(id) = item.get("id").and_then(|value| value.as_str()) else {
            continue;
        };
        let name = item
            .get("name")
            .and_then(|value| value.as_str())
            .unwrap_or(id)
            .to_string();
        let context = item
            .pointer("/capabilities/limits/max_context_window_tokens")
            .and_then(|value| value.as_u64())
            .unwrap_or(0);
        let vendor = item
            .get("vendor")
            .and_then(|value| value.as_str())
            .unwrap_or("")
            .to_string();
        let endpoints = item
            .get("supported_endpoints")
            .and_then(|value| value.as_array())
            .map(|items| {
                items
                    .iter()
                    .filter_map(|item| item.as_str().map(str::to_string))
                    .collect::<Vec<_>>()
            })
            .unwrap_or_default();
        let preview = item
            .get("preview")
            .and_then(|value| value.as_bool())
            .unwrap_or(false);
        models.push(CopilotModelInfo {
            id: id.to_string(),
            name,
            context,
            vendor,
            endpoints,
            preview,
        });
    }
    models
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn filters_disabled_and_non_chat_models() {
        let payload = json!({
            "data": [
                {
                    "id": "gpt-6-luna",
                    "name": "GPT-6 Luna",
                    "model_picker_enabled": true,
                    "capabilities": { "type": "chat", "limits": { "max_context_window_tokens": 400000 } },
                    "supported_endpoints": ["/responses"],
                    "policy": { "state": "enabled" },
                    "vendor": "OpenAI"
                },
                {
                    "id": "claude-opus-5.5",
                    "name": "Claude Opus 5.5",
                    "model_picker_enabled": true,
                    "capabilities": { "type": "chat", "limits": { "max_context_window_tokens": 328000 } },
                    "supported_endpoints": ["/v1/messages", "/chat/completions"],
                    "policy": { "state": "disabled" },
                    "vendor": "Anthropic"
                },
                {
                    "id": "embedding",
                    "name": "Embedding",
                    "model_picker_enabled": true,
                    "capabilities": { "type": "embeddings" }
                }
            ]
        });
        let models = parse_models(&payload);
        assert_eq!(models.len(), 1);
        assert_eq!(models[0].id, "gpt-6-luna");
        assert_eq!(models[0].context, 400000);
        assert_eq!(models[0].endpoints, vec!["/responses".to_string()]);
    }
}
