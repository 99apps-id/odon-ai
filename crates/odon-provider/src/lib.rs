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
