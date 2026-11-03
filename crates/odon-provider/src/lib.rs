pub mod anthropic;
pub mod antigravity;
pub mod codex;
pub mod codex_catalog;
pub mod copilot;
pub mod host_creds;
pub mod multimodal;
pub mod muse;
pub mod oauth;
pub mod openai;
pub mod simulated;
pub mod stream;
pub mod trait_def;

pub use anthropic::AnthropicProvider;
pub use antigravity::AntigravityProvider;
pub use codex::OpenAiCodexProvider;
pub use muse::MuseProvider;
pub use oauth::{
    LoginResponse, OAuthAccount, OAuthHandler, OAuthProvider, OAuthService, OAuthToken,
    PendingLogin,
};
pub use openai::OpenAiCompatibleProvider;
pub use simulated::SimulatedProvider;
pub use stream::{StreamConsumer, StreamEvent};
pub use trait_def::{ChatRequest, ModelProvider, ToolDefinition};

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
