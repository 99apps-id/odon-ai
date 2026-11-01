use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ProviderKind {
    OpenAiCompatible,
    OpenAiCodex,
    Antigravity,
    Muse,
    Anthropic,
    OpenRouter,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum AuthKind {
    ApiKey(String),
    OAuth {
        access_token: String,
        refresh_token: Option<String>,
        expires_at: Option<i64>,
        account_id: Option<String>,
    },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ModelInfo {
    pub id: String,
    pub name: String,
    pub provider: ProviderKind,
    pub description: String,
    pub supports_vision: bool,
    pub supports_tools: bool,
    pub supports_artifacts: bool,
    pub context_length: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProviderConfig {
    pub kind: ProviderKind,
    pub base_url: String,
    pub auth: AuthKind,
    pub default_model: String,
    pub custom_headers: std::collections::HashMap<String, String>,
}

impl ProviderConfig {
    pub fn new_openai_compatible(base_url: impl Into<String>, api_key: impl Into<String>, default_model: impl Into<String>) -> Self {
        Self {
            kind: ProviderKind::OpenAiCompatible,
            base_url: base_url.into(),
            auth: AuthKind::ApiKey(api_key.into()),
            default_model: default_model.into(),
            custom_headers: std::collections::HashMap::new(),
        }
    }

    pub fn new_antigravity(access_token: impl Into<String>, refresh_token: Option<String>) -> Self {
        Self {
            kind: ProviderKind::Antigravity,
            base_url: "https://cloudcode-pa.googleapis.com".to_string(),
            auth: AuthKind::OAuth {
                access_token: access_token.into(),
                refresh_token,
                expires_at: None,
                account_id: None,
            },
            default_model: "antigravity-gemini".to_string(),
            custom_headers: std::collections::HashMap::new(),
        }
    }

    pub fn new_openai_codex(access_token: impl Into<String>, account_id: Option<String>) -> Self {
        Self {
            kind: ProviderKind::OpenAiCodex,
            base_url: "https://chatgpt.com/backend-api".to_string(),
            auth: AuthKind::OAuth {
                access_token: access_token.into(),
                refresh_token: None,
                expires_at: None,
                account_id,
            },
            default_model: "gpt-4o".to_string(),
            custom_headers: std::collections::HashMap::new(),
        }
    }

    pub fn new_muse(access_token: impl Into<String>) -> Self {
        Self {
            kind: ProviderKind::Muse,
            base_url: "https://api.muse.ai/v1".to_string(),
            auth: AuthKind::OAuth {
                access_token: access_token.into(),
                refresh_token: None,
                expires_at: None,
                account_id: None,
            },
            default_model: "muse-code".to_string(),
            custom_headers: std::collections::HashMap::new(),
        }
    }
}
