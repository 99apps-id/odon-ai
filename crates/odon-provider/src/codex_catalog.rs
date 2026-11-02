//! Live ChatGPT/Codex model catalog.
//!
//! The Codex backend publishes its account-specific catalogue at
//! `/backend-api/codex/models?client_version=...`. The list changes often and
//! old slugs start returning 400/404, so the UI should read it instead of a
//! hard-coded table.

use std::sync::OnceLock;
use std::time::{Duration, Instant};

use serde_json::Value;
use tokio::sync::Mutex;

const CODEX_BASE: &str = "https://chatgpt.com/backend-api/codex";
const CODEX_CLI_VERSION: &str = "0.159.0";
const CACHE_TTL: Duration = Duration::from_secs(5 * 60);

#[derive(Debug, Clone)]
pub struct CodexModelInfo {
    pub id: String,
    pub name: String,
    pub context: u64,
    pub max_context: u64,
    pub use_responses_lite: bool,
}

type CodexCacheValue = (Instant, Vec<CodexModelInfo>);
static CACHE: OnceLock<Mutex<Option<CodexCacheValue>>> = OnceLock::new();

/// Fetch the catalogue for the ChatGPT login stored by the Codex CLI.
pub async fn codex_models() -> Option<Vec<CodexModelInfo>> {
    let cache = CACHE.get_or_init(|| Mutex::new(None));
    {
        let guard = cache.lock().await;
        if let Some((fetched_at, models)) = guard.as_ref() {
            if fetched_at.elapsed() < CACHE_TTL {
                return Some(models.clone());
            }
        }
    }

    let auth = crate::host_creds::discover_codex()?;
    let models = fetch_models_with_token(&auth.access_token, auth.account_id.as_deref()).await?;
    *cache.lock().await = Some((Instant::now(), models.clone()));
    Some(models)
}

pub async fn fetch_models_with_token(
    token: &str,
    account_id: Option<&str>,
) -> Option<Vec<CodexModelInfo>> {
    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(15))
        .build()
        .ok()?;
    let mut request = client
        .get(format!(
            "{CODEX_BASE}/models?client_version={CODEX_CLI_VERSION}"
        ))
        .header("Authorization", format!("Bearer {token}"))
        .header("originator", "codex_cli_rs")
        .header("version", CODEX_CLI_VERSION)
        .header("User-Agent", format!("codex_cli_rs/{CODEX_CLI_VERSION}"))
        .header("Accept", "application/json");
    if let Some(account_id) = account_id {
        request = request.header("ChatGPT-Account-ID", account_id);
    }
    let response = request.send().await.ok()?;
    if !response.status().is_success() {
        return None;
    }
    let value: Value = response.json().await.ok()?;
    let models = parse_models(&value);
    if models.is_empty() {
        None
    } else {
        Some(models)
    }
}

/// Keep only chat models exposed in the picker. `visibility=hide` entries are
/// internal (reserve capacity, auto-review) and not meant for general chat.
pub fn parse_models(value: &Value) -> Vec<CodexModelInfo> {
    let Some(items) = value.get("models").and_then(|models| models.as_array()) else {
        return Vec::new();
    };
    items
        .iter()
        .filter_map(|item| {
            let id = item.get("slug").and_then(|value| value.as_str())?;
            let supported = item
                .get("supported_in_api")
                .and_then(|value| value.as_bool())
                .unwrap_or(true);
            let visibility = item
                .get("visibility")
                .and_then(|value| value.as_str())
                .unwrap_or("list");
            if !supported || visibility != "list" {
                return None;
            }
            let raw_name = item
                .get("display_name")
                .and_then(|value| value.as_str())
                .unwrap_or(id);
            let name = display_name(raw_name);
            let context = item
                .get("context_window")
                .and_then(|value| value.as_u64())
                .unwrap_or(0);
            let max_context = item
                .get("max_context_window")
                .and_then(|value| value.as_u64())
                .unwrap_or(context);
            let use_responses_lite = item
                .get("use_responses_lite")
                .and_then(|value| value.as_bool())
                .unwrap_or(false);
            Some(CodexModelInfo {
                id: id.to_string(),
                name,
                context,
                max_context,
                use_responses_lite,
            })
        })
        .collect()
}

/// The backend's display names look like `GPT-6.1-Sol`; the picker reads
/// better with spaces in the tier (`GPT-6.1 Sol`).
fn display_name(raw: &str) -> String {
    match raw.strip_prefix("GPT-") {
        Some(rest) => format!("GPT-{}", rest.replace('-', " ")),
        None => raw.to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn keeps_only_visible_api_models() {
        let payload = json!({
            "models": [
                {
                    "slug": "gpt-6.1-sol",
                    "display_name": "GPT-6.1-Sol",
                    "context_window": 500000,
                    "max_context_window": 872000,
                    "visibility": "list",
                    "supported_in_api": true,
                    "use_responses_lite": true
                },
                {
                    "slug": "gpt-reserve",
                    "display_name": "GPT-Reserve",
                    "context_window": 272000,
                    "visibility": "hide",
                    "supported_in_api": true
                },
                {
                    "slug": "not-in-api",
                    "display_name": "Not In API",
                    "visibility": "list",
                    "supported_in_api": false
                }
            ]
        });
        let models = parse_models(&payload);
        assert_eq!(models.len(), 1);
        assert_eq!(models[0].id, "gpt-6.1-sol");
        assert_eq!(models[0].name, "GPT-6.1 Sol");
        assert_eq!(models[0].context, 500000);
        assert_eq!(models[0].max_context, 872000);
        assert!(models[0].use_responses_lite);
    }
}
