pub mod anthropic;
pub mod antigravity;
pub mod codex;
pub mod codex_catalog;
pub mod copilot;
pub mod host_creds;
pub mod multimodal;
pub mod oauth;
pub mod openai;
pub mod simulated;
pub mod stream;
pub mod trait_def;

pub use anthropic::AnthropicProvider;
pub use antigravity::AntigravityProvider;
pub use codex::OpenAiCodexProvider;
pub use oauth::{
    LoginResponse, OAuthAccount, OAuthHandler, OAuthProvider, OAuthService, OAuthToken,
    PendingLogin,
};
pub use openai::OpenAiCompatibleProvider;
pub use simulated::SimulatedProvider;
pub use stream::{StreamConsumer, StreamEvent};
pub use trait_def::{ChatRequest, ModelProvider, ToolDefinition};

/// How many extra attempts a transient provider failure earns.
pub(crate) const TRANSIENT_RETRY_ATTEMPTS: u32 = 2;

/// Statuses worth retrying: the provider or an upstream gateway is cold
/// starting (`5xx`), rate limiting (`429`), or the request timed out (`408`).
/// Every provider shares this rule so a chat turn survives the same hiccup
/// regardless of which provider the operator selected.
pub(crate) fn is_transient_status(status: reqwest::StatusCode) -> bool {
    status.is_server_error() || status.as_u16() == 429 || status.as_u16() == 408
}

/// Linear backoff between transient retries.
pub(crate) fn retry_backoff(attempt: u32) -> std::time::Duration {
    std::time::Duration::from_millis(600 * u64::from(attempt))
}

/// Send a request, retrying transient failures (5xx/429/408 and connect or
/// timeout errors) with linear backoff. The builder closure is called once per
/// attempt so the request body is rebuilt each time.
pub(crate) async fn send_with_retry<F>(
    label: &str,
    mut build: F,
) -> odon_core::Result<reqwest::Response>
where
    F: FnMut() -> reqwest::RequestBuilder,
{
    let mut attempt = 0u32;
    loop {
        match build().send().await {
            Ok(response) => {
                let status = response.status();
                if is_transient_status(status) && attempt < TRANSIENT_RETRY_ATTEMPTS {
                    attempt += 1;
                    tracing::warn!(
                        "{} returned HTTP {} - retry {}/{}",
                        label,
                        status,
                        attempt,
                        TRANSIENT_RETRY_ATTEMPTS
                    );
                    tokio::time::sleep(retry_backoff(attempt)).await;
                    continue;
                }
                return Ok(response);
            }
            Err(e) if attempt < TRANSIENT_RETRY_ATTEMPTS && (e.is_timeout() || e.is_connect()) => {
                attempt += 1;
                tracing::warn!(
                    "{} request failed ({}): retry {}/{}",
                    label,
                    e,
                    attempt,
                    TRANSIENT_RETRY_ATTEMPTS
                );
                tokio::time::sleep(retry_backoff(attempt)).await;
            }
            Err(e) => {
                return Err(odon_core::OdonError::Provider {
                    provider: label.to_string(),
                    message: format!("Could not reach {}: {}", label, e),
                })
            }
        }
    }
}

/// Truncate a string on a UTF-8 char boundary, appending an ellipsis when cut.
/// `String::truncate(n)` panics when `n` lands inside a multi-byte character.
pub(crate) fn truncate_str_boundary(body: &str, max_bytes: usize) -> String {
    if body.len() <= max_bytes {
        return body.to_string();
    }
    let mut end = max_bytes;
    while end > 0 && !body.is_char_boundary(end) {
        end -= 1;
    }
    format!("{}…", &body[..end])
}

#[cfg(test)]
mod tests {
    use super::*;
    use reqwest::StatusCode;

    #[test]
    fn rate_limits_and_server_errors_are_transient() {
        for code in [408u16, 429, 500, 502, 503, 504, 529] {
            let status = StatusCode::from_u16(code).expect("valid status");
            assert!(is_transient_status(status), "HTTP {code} should retry");
        }
    }

    #[test]
    fn success_and_client_errors_are_not_transient() {
        for code in [200u16, 201, 204, 400, 401, 403, 404, 422] {
            let status = StatusCode::from_u16(code).expect("valid status");
            assert!(!is_transient_status(status), "HTTP {code} must not retry");
        }
    }

    #[test]
    fn backoff_grows_with_each_attempt() {
        assert_eq!(retry_backoff(1).as_millis(), 600);
        assert!(retry_backoff(2) > retry_backoff(1));
        assert_eq!(TRANSIENT_RETRY_ATTEMPTS, 2);
    }

    #[test]
    fn truncate_keeps_multibyte_boundaries() {
        let text = "a…b";
        let cut = truncate_str_boundary(text, 2);
        assert!(cut.ends_with('…'));
        assert_eq!(cut, "a…");
    }
}
