//! OAuth 2.0 browser PKCE and RFC 8628 device flows for model providers.
//!
//! This module is UI-agnostic. Call [`OAuthService::begin_login`], send the
//! returned serializable [`LoginResponse`] to the caller, then await
//! [`PendingLogin::complete`] in a task. Secrets used while a login is pending
//! are deliberately kept out of the response types.

use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use base64::Engine;
use chrono::{DateTime, Duration as ChronoDuration, Utc};
use odon_core::{OdonError, Result};
use ring::rand::{SecureRandom, SystemRandom};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::fmt;
use std::net::SocketAddr;
use std::str::FromStr;
use std::time::Duration;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpListener;
use url::Url;

const DEFAULT_LOGIN_TIMEOUT: Duration = Duration::from_secs(300);
const HTTP_TIMEOUT: Duration = Duration::from_secs(30);
const MAX_CALLBACK_BYTES: usize = 16 * 1024;
const MUSE_USER_AGENT: &str = "muse-code/1.0.2";

const OPENAI_CLIENT_ID: &str = "app_EMoamEEZ73f0CkXaXp7hrann";
const OPENAI_AUTHORIZE_URL: &str = "https://auth.openai.com/oauth/authorize";
const OPENAI_TOKEN_URL: &str = "https://auth.openai.com/oauth/token";
const OPENAI_SCOPE: &str = "openid profile email offline_access";

const CLAUDE_CLIENT_ID: &str = "9d1c250a-e61b-44d9-88ed-5944d1962f5e";
const CLAUDE_AUTHORIZE_URL: &str = "https://claude.ai/oauth/authorize";
const CLAUDE_TOKEN_URL: &str = "https://api.anthropic.com/v1/oauth/token";
const CLAUDE_SCOPE: &str = "org:create_api_key user:profile user:inference";

const XAI_CLIENT_ID: &str = "b1a00492-073a-47ea-816f-4c329264a828";

const ANTIGRAVITY_AUTHORIZE_URL: &str = "https://accounts.google.com/o/oauth2/v2/auth";
const ANTIGRAVITY_TOKEN_URL: &str = "https://oauth2.googleapis.com/token";
const ANTIGRAVITY_SCOPE: &str = "https://www.googleapis.com/auth/cloud-platform https://www.googleapis.com/auth/userinfo.email https://www.googleapis.com/auth/userinfo.profile https://www.googleapis.com/auth/cclog https://www.googleapis.com/auth/experimentsandconfigs";

fn env_value(name: &str) -> String {
    std::env::var(name)
        .map(|v| v.trim().to_string())
        .unwrap_or_default()
}

// Desktop bundles cannot read the developer machine's ignored `.env.local`.
// The release build may therefore stamp the registered public-client
// credentials into the odond sidecar through its build environment. Runtime
// environment variables deliberately win so operators can rotate them without
// rebuilding. Like every installed OAuth public client, these values are
// extractable from the binary and must not be treated as server-side secrets.
const STAMPED_ANTIGRAVITY_CLIENT_ID: &str =
    match option_env!("TERMIGO_ANTIGRAVITY_CLIENT_ID") {
        Some(value) => value,
        None => "",
    };
const STAMPED_ANTIGRAVITY_CLIENT_SECRET: &str =
    match option_env!("TERMIGO_ANTIGRAVITY_CLIENT_SECRET") {
        Some(value) => value,
        None => "",
    };

/// Antigravity's registered client (same one Termigo uses). Read from the
/// environment / .env.local at runtime, never compiled into the repository.
fn antigravity_credentials() -> (String, String) {
    let pick = |primary: &str, legacy: &str, stamped: &str| {
        let v = env_value(primary);
        if v.is_empty() {
            let legacy_value = env_value(legacy);
            if legacy_value.is_empty() {
                stamped.trim().to_string()
            } else {
                legacy_value
            }
        } else {
            v
        }
    };
    (
        pick(
            "TERMIGO_ANTIGRAVITY_CLIENT_ID",
            "TERMIXGO_ANTIGRAVITY_CLIENT_ID",
            STAMPED_ANTIGRAVITY_CLIENT_ID,
        ),
        pick(
            "TERMIGO_ANTIGRAVITY_CLIENT_SECRET",
            "TERMIXGO_ANTIGRAVITY_CLIENT_SECRET",
            STAMPED_ANTIGRAVITY_CLIENT_SECRET,
        ),
    )
}

fn antigravity_user_agent() -> String {
    let platform = match std::env::consts::OS {
        "macos" => "darwin",
        "windows" => "windows",
        _ => "linux",
    };
    let arch = match std::env::consts::ARCH {
        "x86_64" => "x64",
        "aarch64" => "arm64",
        other => other,
    };
    format!("antigravity/ide/2.11.0 {platform}/{arch}")
}
const XAI_DEVICE_URL: &str = "https://auth.x.ai/oauth2/device/code";
const XAI_TOKEN_URL: &str = "https://auth.x.ai/oauth2/token";
const XAI_SCOPE: &str = "openid profile email offline_access grok-cli:access api:access conversations:read conversations:write";

const GITHUB_CLIENT_ID: &str = "Iv1.b507a08c87ecfe98";
const GITHUB_DEVICE_URL: &str = "https://github.com/login/device/code";
const GITHUB_TOKEN_URL: &str = "https://github.com/login/oauth/access_token";
const COPILOT_TOKEN_URL: &str = "https://api.github.com/copilot_internal/v2/token";

const MUSE_CLIENT_ID: &str = "1031625952748946";
const MUSE_DEVICE_URL: &str = "https://auth.meta.com/oidc/device/authorization/";
const MUSE_TOKEN_URL: &str = "https://auth.meta.com/oidc/device/token/";
const MUSE_MINT_URL: &str = "https://api.meta.ai/muse-code/key";
const MUSE_DEFAULT_VERIFY_URL: &str = "https://auth.meta.com/oauth/device/";

fn auth_error(message: impl Into<String>) -> OdonError {
    OdonError::Auth(message.into())
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum OAuthProvider {
    OpenAiCodex,
    ClaudeOauth,
    Antigravity,
    XaiOauth,
    GithubCopilot,
    Muse,
}

impl OAuthProvider {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::OpenAiCodex => "openai-codex",
            Self::ClaudeOauth => "claude-oauth",
            Self::Antigravity => "antigravity",
            Self::XaiOauth => "xai-oauth",
            Self::GithubCopilot => "github-copilot",
            Self::Muse => "muse",
        }
    }
}

impl fmt::Display for OAuthProvider {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

impl FromStr for OAuthProvider {
    type Err = OdonError;

    fn from_str(value: &str) -> Result<Self> {
        match value {
            "openai-codex" | "chatgpt" => Ok(Self::OpenAiCodex),
            "claude-oauth" | "claude" => Ok(Self::ClaudeOauth),
            "antigravity" => Ok(Self::Antigravity),
            "xai-oauth" | "xai" => Ok(Self::XaiOauth),
            "github-copilot" | "copilot" => Ok(Self::GithubCopilot),
            "muse" => Ok(Self::Muse),
            _ => Err(auth_error(format!("unsupported OAuth provider: {value}"))),
        }
    }
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct OAuthAccount {
    pub account_id: Option<String>,
    pub email: Option<String>,
    pub plan: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OAuthToken {
    pub provider: String,
    pub access_token: String,
    pub refresh_token: Option<String>,
    pub expires_at: Option<DateTime<Utc>>,
    pub account_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub id_token: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub email: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub plan: Option<String>,
}

impl OAuthToken {
    pub fn account(&self) -> OAuthAccount {
        OAuthAccount {
            account_id: self.account_id.clone(),
            email: self.email.clone(),
            plan: self.plan.clone(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "flow", rename_all = "snake_case")]
pub enum LoginResponse {
    Browser {
        provider: String,
        authorization_url: String,
        redirect_uri: String,
    },
    Device {
        provider: String,
        user_code: String,
        verification_uri: String,
        expires_in: u64,
        interval: u64,
    },
}

impl LoginResponse {
    pub fn provider(&self) -> &str {
        match self {
            Self::Browser { provider, .. } | Self::Device { provider, .. } => provider,
        }
    }
}

#[derive(Clone)]
pub struct OAuthService {
    client: reqwest::Client,
    login_timeout: Duration,
}

impl OAuthService {
    pub fn new() -> Result<Self> {
        let client = reqwest::Client::builder()
            .timeout(HTTP_TIMEOUT)
            .build()
            .map_err(|e| auth_error(format!("could not build OAuth HTTP client: {e}")))?;
        Ok(Self {
            client,
            login_timeout: DEFAULT_LOGIN_TIMEOUT,
        })
    }

    pub fn with_client(client: reqwest::Client) -> Self {
        Self {
            client,
            login_timeout: DEFAULT_LOGIN_TIMEOUT,
        }
    }

    pub fn with_login_timeout(mut self, timeout: Duration) -> Self {
        self.login_timeout = timeout;
        self
    }

    pub async fn begin_login(&self, provider: OAuthProvider) -> Result<PendingLogin> {
        match provider {
            OAuthProvider::OpenAiCodex
            | OAuthProvider::ClaudeOauth
            | OAuthProvider::Antigravity => self
                .begin_browser_login(provider)
                .await
                .map(PendingLogin::Browser),
            OAuthProvider::XaiOauth | OAuthProvider::GithubCopilot | OAuthProvider::Muse => self
                .begin_device_login(provider)
                .await
                .map(PendingLogin::Device),
        }
    }

    pub async fn begin_login_for(&self, provider: &str) -> Result<PendingLogin> {
        self.begin_login(provider.parse()?).await
    }

    async fn begin_browser_login(&self, provider: OAuthProvider) -> Result<PendingBrowserLogin> {
        let (verifier, challenge) = OAuthHandler::generate_pkce()?;
        let state = secure_random_b64(32)?;
        let config = browser_config(provider)?;
        let (listener, port) = bind_loopback_port(config.port).await?;
        let redirect_uri = format!("http://localhost:{}{}", port, config.callback_path);
        let (client_id, client_secret) = if provider == OAuthProvider::Antigravity {
            let (id, secret) = antigravity_credentials();
            if id.is_empty() {
                return Err(auth_error(
                    "Antigravity needs its client id: set TERMIGO_ANTIGRAVITY_CLIENT_ID (and TERMIGO_ANTIGRAVITY_CLIENT_SECRET) in the environment or .env.local",
                ));
            }
            (id, secret)
        } else {
            (config.client_id.to_string(), String::new())
        };

        let mut url = Url::parse(config.authorize_url)
            .map_err(|e| auth_error(format!("invalid authorization endpoint: {e}")))?;
        {
            let mut query = url.query_pairs_mut();
            query
                .append_pair("response_type", "code")
                .append_pair("client_id", &client_id)
                .append_pair("redirect_uri", &redirect_uri)
                .append_pair("scope", config.scope)
                .append_pair("code_challenge", &challenge)
                .append_pair("code_challenge_method", "S256")
                .append_pair("state", &state);
            match provider {
                OAuthProvider::OpenAiCodex => {
                    query
                        .append_pair("id_token_add_organizations", "true")
                        .append_pair("codex_cli_simplified_flow", "true")
                        .append_pair("originator", "codex_cli_rs");
                }
                OAuthProvider::ClaudeOauth => {
                    query.append_pair("code", "true");
                }
                OAuthProvider::Antigravity => {
                    query
                        .append_pair("access_type", "offline")
                        .append_pair("prompt", "consent");
                }
                _ => unreachable!("device providers do not use browser PKCE"),
            }
        }

        let response = LoginResponse::Browser {
            provider: provider.to_string(),
            authorization_url: url.into(),
            redirect_uri: redirect_uri.clone(),
        };
        Ok(PendingBrowserLogin {
            client: self.client.clone(),
            config,
            listener,
            verifier,
            state,
            redirect_uri,
            client_id,
            client_secret,
            timeout: self.login_timeout,
            response,
        })
    }

    async fn begin_device_login(&self, provider: OAuthProvider) -> Result<PendingDeviceLogin> {
        let config = device_config(provider)?;
        let mut form = vec![("client_id", config.client_id)];
        if let Some(scope) = config.scope {
            form.push(("scope", scope));
        }
        let mut request = self
            .client
            .post(config.device_url)
            .header("accept", "application/json")
            .form(&form);
        if provider == OAuthProvider::Muse {
            request = request.header("user-agent", MUSE_USER_AGENT);
        }
        let response = request
            .send()
            .await
            .map_err(|e| auth_error(format!("device code request failed: {e}")))?;
        let status = response.status();
        let body = response.text().await.unwrap_or_default();
        if !status.is_success() {
            return Err(auth_error(format!(
                "device code endpoint returned {status}: {body}"
            )));
        }
        let device: DeviceCodeResponse = serde_json::from_str(&body)
            .map_err(|e| auth_error(format!("could not parse device code response: {e}")))?;
        if device.device_code.is_empty() || device.user_code.is_empty() {
            return Err(auth_error(
                "device endpoint returned an incomplete response",
            ));
        }

        let verification_uri = device
            .verification_uri_complete
            .or(device.verification_uri)
            .unwrap_or_else(|| MUSE_DEFAULT_VERIFY_URL.to_string());
        validate_device_verification_uri(provider, &verification_uri)?;
        let interval = device.interval.unwrap_or(5).max(3);
        let expires_in = device
            .expires_in
            .unwrap_or(self.login_timeout.as_secs())
            .min(self.login_timeout.as_secs());
        let response = LoginResponse::Device {
            provider: provider.to_string(),
            user_code: device.user_code,
            verification_uri,
            expires_in,
            interval,
        };
        Ok(PendingDeviceLogin {
            client: self.client.clone(),
            config,
            device_code: device.device_code,
            deadline: tokio::time::Instant::now() + Duration::from_secs(expires_in),
            interval: Duration::from_secs(interval),
            response,
        })
    }

    pub async fn refresh(
        &self,
        provider: OAuthProvider,
        refresh_token: &str,
    ) -> Result<OAuthToken> {
        if refresh_token.trim().is_empty() {
            return Err(auth_error("refresh token must not be empty"));
        }
        match provider {
            OAuthProvider::OpenAiCodex => {
                let response = self
                    .client
                    .post(OPENAI_TOKEN_URL)
                    .header("user-agent", "codex_cli_rs/0.159.0")
                    .header("originator", "codex_cli_rs")
                    .form(&[
                        ("grant_type", "refresh_token"),
                        ("refresh_token", refresh_token),
                        ("client_id", OPENAI_CLIENT_ID),
                        ("scope", OPENAI_SCOPE),
                    ])
                    .send()
                    .await
                    .map_err(|e| auth_error(format!("OpenAI refresh failed: {e}")))?;
                parse_oauth_token_response(
                    response,
                    provider,
                    Some(refresh_token),
                    Duration::from_secs(3600),
                )
                .await
            }
            OAuthProvider::ClaudeOauth => {
                let response = self
                    .client
                    .post(CLAUDE_TOKEN_URL)
                    .header("user-agent", "claude-cli/2.1.280 (external, sdk-cli)")
                    .json(&serde_json::json!({
                        "grant_type": "refresh_token",
                        "refresh_token": refresh_token,
                        "client_id": CLAUDE_CLIENT_ID,
                    }))
                    .send()
                    .await
                    .map_err(|e| auth_error(format!("Claude refresh failed: {e}")))?;
                let mut token = parse_oauth_token_response(
                    response,
                    provider,
                    Some(refresh_token),
                    Duration::from_secs(14_400),
                )
                .await?;
                token.plan = Some("Claude OAuth".to_string());
                Ok(token)
            }
            OAuthProvider::XaiOauth => {
                let response = self
                    .client
                    .post(XAI_TOKEN_URL)
                    .form(&[
                        ("grant_type", "refresh_token"),
                        ("refresh_token", refresh_token),
                        ("client_id", XAI_CLIENT_ID),
                    ])
                    .send()
                    .await
                    .map_err(|e| auth_error(format!("xAI refresh failed: {e}")))?;
                parse_oauth_token_response(
                    response,
                    provider,
                    Some(refresh_token),
                    Duration::from_secs(3600),
                )
                .await
            }
            OAuthProvider::GithubCopilot => mint_copilot_token(&self.client, refresh_token).await,
            OAuthProvider::Antigravity => {
                let (client_id, client_secret) = antigravity_credentials();
                if client_id.is_empty() {
                    return Err(auth_error("Antigravity client id is not configured"));
                }
                let response = self
                    .client
                    .post(ANTIGRAVITY_TOKEN_URL)
                    .header("user-agent", antigravity_user_agent())
                    .form(&[
                        ("grant_type", "refresh_token"),
                        ("refresh_token", refresh_token),
                        ("client_id", client_id.as_str()),
                        ("client_secret", client_secret.as_str()),
                    ])
                    .send()
                    .await
                    .map_err(|e| auth_error(format!("Google refresh failed: {e}")))?;
                let mut token = parse_oauth_token_response(
                    response,
                    provider,
                    Some(refresh_token),
                    Duration::from_secs(3600),
                )
                .await?;
                token.plan = Some("Antigravity".to_string());
                Ok(token)
            }
            OAuthProvider::Muse => mint_muse_key(&self.client, refresh_token).await,
        }
    }

    pub async fn refresh_for(&self, provider: &str, refresh_token: &str) -> Result<OAuthToken> {
        self.refresh(provider.parse()?, refresh_token).await
    }
}

pub enum PendingLogin {
    Browser(PendingBrowserLogin),
    Device(PendingDeviceLogin),
}

impl PendingLogin {
    pub fn response(&self) -> &LoginResponse {
        match self {
            Self::Browser(login) => login.response(),
            Self::Device(login) => login.response(),
        }
    }

    pub async fn complete(self) -> Result<OAuthToken> {
        match self {
            Self::Browser(login) => login.complete().await,
            Self::Device(login) => login.complete().await,
        }
    }
}

pub struct PendingBrowserLogin {
    client: reqwest::Client,
    config: BrowserConfig,
    listener: TcpListener,
    verifier: String,
    state: String,
    redirect_uri: String,
    client_id: String,
    client_secret: String,
    timeout: Duration,
    response: LoginResponse,
}

impl PendingBrowserLogin {
    pub fn response(&self) -> &LoginResponse {
        &self.response
    }

    pub async fn complete(self) -> Result<OAuthToken> {
        let target = tokio::time::timeout(
            self.timeout,
            accept_callback(&self.listener, self.config.callback_path),
        )
        .await
        .map_err(|_| auth_error("timed out waiting for browser sign-in"))??;
        let code = parse_callback_target(&target, self.config.callback_path, Some(&self.state))?;

        let response = match self.config.provider {
            OAuthProvider::OpenAiCodex => self
                .client
                .post(self.config.token_url)
                .header("user-agent", "codex_cli_rs/0.159.0")
                .header("originator", "codex_cli_rs")
                .form(&[
                    ("grant_type", "authorization_code"),
                    ("code", code.as_str()),
                    ("redirect_uri", self.redirect_uri.as_str()),
                    ("client_id", self.config.client_id),
                    ("code_verifier", self.verifier.as_str()),
                ])
                .send()
                .await
                .map_err(|e| auth_error(format!("OpenAI token exchange failed: {e}")))?,
            OAuthProvider::ClaudeOauth => self
                .client
                .post(self.config.token_url)
                .header("user-agent", "claude-cli/2.1.280 (external, sdk-cli)")
                .json(&serde_json::json!({
                    "grant_type": "authorization_code",
                    "code": code,
                    "redirect_uri": self.redirect_uri,
                    "client_id": self.config.client_id,
                    "code_verifier": self.verifier,
                    "state": self.state,
                }))
                .send()
                .await
                .map_err(|e| auth_error(format!("Claude token exchange failed: {e}")))?,
            OAuthProvider::Antigravity => self
                .client
                .post(self.config.token_url)
                .header("user-agent", antigravity_user_agent())
                .form(&[
                    ("grant_type", "authorization_code"),
                    ("code", code.as_str()),
                    ("redirect_uri", self.redirect_uri.as_str()),
                    ("client_id", self.client_id.as_str()),
                    ("client_secret", self.client_secret.as_str()),
                    ("code_verifier", self.verifier.as_str()),
                ])
                .send()
                .await
                .map_err(|e| auth_error(format!("Google token exchange failed: {e}")))?,
            _ => unreachable!("only PKCE providers create browser logins"),
        };

        let default_lifetime = if self.config.provider == OAuthProvider::ClaudeOauth {
            Duration::from_secs(14_400)
        } else {
            Duration::from_secs(3600)
        };
        let mut token =
            parse_oauth_token_response(response, self.config.provider, None, default_lifetime)
                .await?;
        if self.config.provider == OAuthProvider::ClaudeOauth {
            if token.refresh_token.is_none() {
                return Err(auth_error("Claude OAuth did not return a refresh token"));
            }
            token.plan = Some("Claude OAuth".to_string());
        }
        if self.config.provider == OAuthProvider::Antigravity {
            token.plan = Some("Antigravity".to_string());
        }
        Ok(token)
    }
}

pub struct PendingDeviceLogin {
    client: reqwest::Client,
    config: DeviceConfig,
    device_code: String,
    deadline: tokio::time::Instant,
    interval: Duration,
    response: LoginResponse,
}

impl PendingDeviceLogin {
    pub fn response(&self) -> &LoginResponse {
        &self.response
    }

    pub async fn complete(mut self) -> Result<OAuthToken> {
        loop {
            if tokio::time::Instant::now() >= self.deadline {
                return Err(auth_error("device authorization timed out"));
            }
            tokio::time::sleep(self.interval).await;
            let mut request = self.client.post(self.config.token_url).form(&[
                ("client_id", self.config.client_id),
                ("device_code", self.device_code.as_str()),
                ("grant_type", "urn:ietf:params:oauth:grant-type:device_code"),
            ]);
            if self.config.provider == OAuthProvider::Muse {
                request = request
                    .header("accept", "application/json")
                    .header("user-agent", MUSE_USER_AGENT);
            } else if self.config.provider == OAuthProvider::GithubCopilot {
                request = request.header("accept", "application/json");
            }

            let response = match request.send().await {
                Ok(response) => response,
                Err(_) if tokio::time::Instant::now() < self.deadline => continue,
                Err(error) => {
                    return Err(auth_error(format!("device token request failed: {error}")))
                }
            };
            let status = response.status();
            let body = response.text().await.unwrap_or_default();
            let parsed: DeviceTokenResponse = serde_json::from_str(&body).map_err(|e| {
                auth_error(format!(
                    "could not parse device token response ({status}): {e}"
                ))
            })?;

            if let Some(access_token) = parsed.access_token {
                return match self.config.provider {
                    OAuthProvider::GithubCopilot => {
                        mint_copilot_token(&self.client, &access_token).await
                    }
                    OAuthProvider::Muse => mint_muse_key(&self.client, &access_token).await,
                    OAuthProvider::XaiOauth => Ok(token_from_raw(
                        self.config.provider,
                        RawTokenResponse {
                            access_token,
                            refresh_token: parsed.refresh_token,
                            id_token: parsed.id_token,
                            expires_in: parsed.expires_in,
                        },
                        None,
                        Duration::from_secs(3600),
                    )),
                    _ => unreachable!("browser providers do not create device logins"),
                };
            }

            match parsed.error.as_deref() {
                Some("authorization_pending") => continue,
                Some("slow_down") => {
                    self.interval += Duration::from_secs(5);
                    continue;
                }
                Some(error) => {
                    let detail = parsed.error_description.as_deref().unwrap_or(error);
                    return Err(auth_error(format!("device authorization failed: {detail}")));
                }
                None if !status.is_success() => {
                    return Err(auth_error(format!(
                        "device token endpoint returned {status}: {body}"
                    )));
                }
                None => return Err(auth_error("device token response had no access token")),
            }
        }
    }
}

pub struct OAuthHandler;

impl OAuthHandler {
    /// Generates an RFC 7636 verifier and its S256 challenge.
    pub fn generate_pkce() -> Result<(String, String)> {
        let verifier = secure_random_b64(64)?;
        let challenge = URL_SAFE_NO_PAD.encode(Sha256::digest(verifier.as_bytes()));
        Ok((verifier, challenge))
    }

    /// Compatibility helper that captures one code on a loopback port.
    /// New integrations should use [`OAuthService::begin_login`] so state and
    /// callback path validation cannot be omitted.
    pub async fn wait_for_redirect_code(port: u16) -> Result<String> {
        let addr: SocketAddr = format!("127.0.0.1:{port}")
            .parse()
            .map_err(|e| auth_error(format!("invalid callback address: {e}")))?;
        let listener = TcpListener::bind(addr)
            .await
            .map_err(|e| auth_error(format!("failed to bind loopback OAuth port: {e}")))?;
        let target = accept_callback(&listener, "/").await?;
        parse_callback_target(&target, "/", None)
    }
}

#[derive(Clone, Copy)]
struct BrowserConfig {
    provider: OAuthProvider,
    client_id: &'static str,
    authorize_url: &'static str,
    token_url: &'static str,
    scope: &'static str,
    port: u16,
    callback_path: &'static str,
}

fn browser_config(provider: OAuthProvider) -> Result<BrowserConfig> {
    match provider {
        OAuthProvider::OpenAiCodex => Ok(BrowserConfig {
            provider,
            client_id: OPENAI_CLIENT_ID,
            authorize_url: OPENAI_AUTHORIZE_URL,
            token_url: OPENAI_TOKEN_URL,
            scope: OPENAI_SCOPE,
            port: 1455,
            callback_path: "/auth/callback",
        }),
        OAuthProvider::ClaudeOauth => Ok(BrowserConfig {
            provider,
            client_id: CLAUDE_CLIENT_ID,
            authorize_url: CLAUDE_AUTHORIZE_URL,
            token_url: CLAUDE_TOKEN_URL,
            scope: CLAUDE_SCOPE,
            port: 54545,
            callback_path: "/callback",
        }),
        OAuthProvider::Antigravity => Ok(BrowserConfig {
            provider,
            client_id: "",
            authorize_url: ANTIGRAVITY_AUTHORIZE_URL,
            token_url: ANTIGRAVITY_TOKEN_URL,
            scope: ANTIGRAVITY_SCOPE,
            port: 0,
            callback_path: "/auth/callback",
        }),
        _ => Err(auth_error(format!("{provider} does not use browser PKCE"))),
    }
}

#[derive(Clone, Copy)]
struct DeviceConfig {
    provider: OAuthProvider,
    client_id: &'static str,
    device_url: &'static str,
    token_url: &'static str,
    scope: Option<&'static str>,
}

fn device_config(provider: OAuthProvider) -> Result<DeviceConfig> {
    match provider {
        OAuthProvider::XaiOauth => Ok(DeviceConfig {
            provider,
            client_id: XAI_CLIENT_ID,
            device_url: XAI_DEVICE_URL,
            token_url: XAI_TOKEN_URL,
            scope: Some(XAI_SCOPE),
        }),
        OAuthProvider::GithubCopilot => Ok(DeviceConfig {
            provider,
            client_id: GITHUB_CLIENT_ID,
            device_url: GITHUB_DEVICE_URL,
            token_url: GITHUB_TOKEN_URL,
            scope: Some("read:user"),
        }),
        OAuthProvider::Muse => Ok(DeviceConfig {
            provider,
            client_id: MUSE_CLIENT_ID,
            device_url: MUSE_DEVICE_URL,
            token_url: MUSE_TOKEN_URL,
            scope: None,
        }),
        _ => Err(auth_error(format!("{provider} does not use device login"))),
    }
}

#[derive(Deserialize)]
struct DeviceCodeResponse {
    device_code: String,
    user_code: String,
    verification_uri: Option<String>,
    verification_uri_complete: Option<String>,
    interval: Option<u64>,
    expires_in: Option<u64>,
}

#[derive(Deserialize)]
struct DeviceTokenResponse {
    access_token: Option<String>,
    refresh_token: Option<String>,
    id_token: Option<String>,
    expires_in: Option<i64>,
    error: Option<String>,
    error_description: Option<String>,
}

#[derive(Deserialize)]
struct RawTokenResponse {
    access_token: String,
    refresh_token: Option<String>,
    id_token: Option<String>,
    expires_in: Option<i64>,
}

/// Binds the loopback listener and returns the port actually bound. Port 0 asks
/// the OS for an ephemeral port (Google's installed-app flow ignores the port).
async fn bind_loopback_port(port: u16) -> Result<(TcpListener, u16)> {
    let listener = TcpListener::bind(("127.0.0.1", port)).await.map_err(|e| {
        auth_error(format!(
            "could not listen on 127.0.0.1:{port} ({e}); close the process using that port and retry"
        ))
    })?;
    let bound = listener
        .local_addr()
        .map_err(|e| auth_error(format!("could not read the bound port: {e}")))?
        .port();
    Ok((listener, bound))
}

async fn accept_callback(listener: &TcpListener, expected_path: &str) -> Result<String> {
    loop {
        let (mut stream, peer) = listener
            .accept()
            .await
            .map_err(|e| auth_error(format!("callback accept failed: {e}")))?;
        if !peer.ip().is_loopback() {
            continue;
        }

        let target = match read_request_target(&mut stream).await {
            Ok(target) => target,
            Err(_) => {
                write_callback_response(&mut stream, 400, "Invalid OAuth callback").await;
                continue;
            }
        };
        let path_matches = Url::parse(&format!("http://localhost{target}"))
            .map(|url| url.path() == expected_path || expected_path == "/")
            .unwrap_or(false);
        if !path_matches {
            write_callback_response(&mut stream, 404, "OAuth callback not found").await;
            continue;
        }

        write_callback_response(
            &mut stream,
            200,
            "Sign-in received. You can close this tab and return to ODON.",
        )
        .await;
        return Ok(target);
    }
}

async fn read_request_target(stream: &mut tokio::net::TcpStream) -> Result<String> {
    let mut request = Vec::with_capacity(2048);
    let mut chunk = [0u8; 1024];
    loop {
        let read = tokio::time::timeout(Duration::from_secs(5), stream.read(&mut chunk))
            .await
            .map_err(|_| auth_error("callback request timed out"))?
            .map_err(|e| auth_error(format!("callback read failed: {e}")))?;
        if read == 0 {
            return Err(auth_error(
                "callback connection closed before request completed",
            ));
        }
        request.extend_from_slice(&chunk[..read]);
        if request.len() > MAX_CALLBACK_BYTES {
            return Err(auth_error("callback request exceeded size limit"));
        }
        if request.windows(4).any(|window| window == b"\r\n\r\n") {
            break;
        }
    }
    let text = std::str::from_utf8(&request)
        .map_err(|_| auth_error("callback request was not valid UTF-8"))?;
    let mut parts = text
        .lines()
        .next()
        .ok_or_else(|| auth_error("callback request line was missing"))?
        .split_whitespace();
    if parts.next() != Some("GET") {
        return Err(auth_error("callback request must use GET"));
    }
    let target = parts
        .next()
        .ok_or_else(|| auth_error("callback request target was missing"))?;
    if !target.starts_with('/') || parts.next() != Some("HTTP/1.1") {
        return Err(auth_error("callback request line was invalid"));
    }
    Ok(target.to_string())
}

async fn write_callback_response(stream: &mut tokio::net::TcpStream, status: u16, message: &str) {
    let reason = match status {
        200 => "OK",
        400 => "Bad Request",
        404 => "Not Found",
        _ => "Error",
    };
    let body =
        format!("<!doctype html><meta charset=utf-8><title>ODON OAuth</title><p>{message}</p>");
    let response = format!(
        "HTTP/1.1 {status} {reason}\r\ncontent-type: text/html; charset=utf-8\r\ncontent-length: {}\r\nconnection: close\r\n\r\n{body}",
        body.len()
    );
    let _ = stream.write_all(response.as_bytes()).await;
    let _ = stream.shutdown().await;
}

fn parse_callback_target(
    target: &str,
    expected_path: &str,
    expected_state: Option<&str>,
) -> Result<String> {
    let parsed = Url::parse(&format!("http://localhost{target}"))
        .map_err(|e| auth_error(format!("could not parse OAuth callback: {e}")))?;
    if expected_path != "/" && parsed.path() != expected_path {
        return Err(auth_error("OAuth callback path did not match"));
    }

    let mut code = None;
    let mut state = None;
    let mut error = None;
    for (key, value) in parsed.query_pairs() {
        match key.as_ref() {
            "code" if code.is_none() => code = Some(value.into_owned()),
            "state" if state.is_none() => state = Some(value.into_owned()),
            "error_description" => error = Some(value.into_owned()),
            "error" if error.is_none() => error = Some(value.into_owned()),
            _ => {}
        }
    }
    if let Some(error) = error {
        return Err(auth_error(format!("sign-in was refused: {error}")));
    }
    if let Some(expected) = expected_state {
        let actual = state.ok_or_else(|| auth_error("OAuth callback state was missing"))?;
        if !constant_time_eq(actual.as_bytes(), expected.as_bytes()) {
            return Err(auth_error("OAuth callback state did not match"));
        }
    }
    let code = code.ok_or_else(|| auth_error("OAuth callback had no authorization code"))?;
    if code.is_empty() {
        return Err(auth_error("OAuth callback authorization code was empty"));
    }
    Ok(code)
}

fn constant_time_eq(a: &[u8], b: &[u8]) -> bool {
    if a.len() != b.len() {
        return false;
    }
    let mut diff = 0u8;
    for (x, y) in a.iter().zip(b.iter()) {
        diff |= x ^ y;
    }
    diff == 0
}

fn validate_device_verification_uri(provider: OAuthProvider, uri: &str) -> Result<()> {
    let parsed = Url::parse(uri).map_err(|_| auth_error("invalid device verification URL"))?;
    let host = parsed.host_str().unwrap_or_default();
    let allowed = match provider {
        OAuthProvider::XaiOauth => matches!(
            host,
            "accounts.x.ai" | "auth.x.ai" | "x.ai" | "console.x.ai"
        ),
        OAuthProvider::GithubCopilot => matches!(host, "github.com" | "copilot.github.com"),
        OAuthProvider::Muse => matches!(host, "auth.meta.com" | "meta.com"),
        _ => false,
    };
    if parsed.scheme() != "https"
        || !allowed
        || parsed.username() != ""
        || parsed.password().is_some()
    {
        return Err(auth_error(
            "device verification URL is not an allowed HTTPS endpoint",
        ));
    }
    Ok(())
}

async fn parse_oauth_token_response(
    response: reqwest::Response,
    provider: OAuthProvider,
    fallback_refresh_token: Option<&str>,
    default_lifetime: Duration,
) -> Result<OAuthToken> {
    let status = response.status();
    let body = response.text().await.unwrap_or_default();
    if !status.is_success() {
        return Err(auth_error(format!(
            "{} token endpoint returned {status}: {body}",
            provider
        )));
    }
    let raw: RawTokenResponse = serde_json::from_str(&body)
        .map_err(|e| auth_error(format!("could not parse token response: {e}")))?;
    Ok(token_from_raw(
        provider,
        raw,
        fallback_refresh_token,
        default_lifetime,
    ))
}

fn token_from_raw(
    provider: OAuthProvider,
    raw: RawTokenResponse,
    fallback_refresh_token: Option<&str>,
    default_lifetime: Duration,
) -> OAuthToken {
    let claims = raw.id_token.as_deref().and_then(jwt_claims);
    let account = claims.as_ref().map(account_from_claims).unwrap_or_default();
    let lifetime = raw
        .expires_in
        .filter(|seconds| *seconds > 0)
        .map(|seconds| Duration::from_secs(seconds as u64))
        .unwrap_or(default_lifetime);
    OAuthToken {
        provider: provider.to_string(),
        access_token: raw.access_token,
        refresh_token: raw
            .refresh_token
            .or_else(|| fallback_refresh_token.map(str::to_string)),
        expires_at: ChronoDuration::from_std(lifetime)
            .ok()
            .map(|duration| Utc::now() + duration),
        account_id: account.account_id,
        id_token: raw.id_token,
        email: account.email,
        plan: account.plan,
    }
}

async fn mint_copilot_token(client: &reqwest::Client, github_token: &str) -> Result<OAuthToken> {
    let response = client
        .get(COPILOT_TOKEN_URL)
        .header("authorization", format!("token {github_token}"))
        .header("user-agent", "GitHubCopilotChat/0.38.0")
        .header("accept", "application/json")
        .send()
        .await
        .map_err(|e| auth_error(format!("Copilot token request failed: {e}")))?;
    let status = response.status();
    let body = response.text().await.unwrap_or_default();
    if !status.is_success() {
        return Err(auth_error(format!(
            "Copilot token endpoint returned {status}: {body}"
        )));
    }
    #[derive(Deserialize)]
    struct CopilotResponse {
        token: String,
        expires_at: Option<i64>,
    }
    let minted: CopilotResponse = serde_json::from_str(&body)
        .map_err(|e| auth_error(format!("could not parse Copilot token response: {e}")))?;
    let expires_at = minted
        .expires_at
        .and_then(|timestamp| DateTime::from_timestamp(timestamp, 0))
        .or_else(|| Some(Utc::now() + ChronoDuration::minutes(30)));
    Ok(OAuthToken {
        provider: OAuthProvider::GithubCopilot.to_string(),
        access_token: minted.token,
        refresh_token: Some(github_token.to_string()),
        expires_at,
        account_id: None,
        id_token: None,
        email: None,
        plan: Some("GitHub Copilot".to_string()),
    })
}

async fn mint_muse_key(client: &reqwest::Client, dca_token: &str) -> Result<OAuthToken> {
    let response = client
        .post(MUSE_MINT_URL)
        .header("authorization", format!("Bearer {dca_token}"))
        .header("user-agent", MUSE_USER_AGENT)
        .header("accept", "application/json")
        .header("x-api-version", "1.0.0")
        .json(&serde_json::json!({ "onboard": true }))
        .send()
        .await
        .map_err(|e| auth_error(format!("Muse key mint failed: {e}")))?;
    let status = response.status();
    let body = response.text().await.unwrap_or_default();
    if !status.is_success() {
        return Err(auth_error(format!(
            "Muse key endpoint returned {status}: {body}"
        )));
    }
    #[derive(Deserialize)]
    struct MuseResponse {
        api_key: Option<String>,
        key: Option<String>,
        user_email: Option<String>,
    }
    let minted: MuseResponse = serde_json::from_str(&body)
        .map_err(|e| auth_error(format!("could not parse Muse key response: {e}")))?;
    let key = minted
        .api_key
        .or(minted.key)
        .ok_or_else(|| auth_error("Muse key response had no API key"))?;
    Ok(OAuthToken {
        provider: OAuthProvider::Muse.to_string(),
        access_token: key,
        refresh_token: Some(dca_token.to_string()),
        expires_at: None,
        account_id: minted.user_email.clone(),
        id_token: None,
        email: minted.user_email,
        plan: Some("Meta Muse Code".to_string()),
    })
}

fn secure_random_b64(byte_len: usize) -> Result<String> {
    let mut bytes = vec![0u8; byte_len];
    SystemRandom::new()
        .fill(&mut bytes)
        .map_err(|_| auth_error("could not read the system random source"))?;
    Ok(URL_SAFE_NO_PAD.encode(bytes))
}

fn jwt_claims(token: &str) -> Option<serde_json::Value> {
    let payload = token.split('.').nth(1)?;
    let bytes = URL_SAFE_NO_PAD.decode(payload).ok()?;
    serde_json::from_slice(&bytes).ok()
}

fn account_from_claims(claims: &serde_json::Value) -> OAuthAccount {
    OAuthAccount {
        account_id: account_id_from_claims(claims),
        email: claims
            .get("email")
            .and_then(|value| value.as_str())
            .map(str::to_string),
        plan: claims
            .get("https://api.openai.com/auth")
            .and_then(|auth| auth.get("chatgpt_plan_type"))
            .and_then(|value| value.as_str())
            .map(str::to_string),
    }
}

fn account_id_from_claims(claims: &serde_json::Value) -> Option<String> {
    let direct = claims
        .get("https://api.openai.com/auth")
        .and_then(|auth| auth.get("chatgpt_account_id"))
        .or_else(|| claims.get("chatgpt_account_id"))
        .or_else(|| {
            claims
                .get("https://api.openai.com/profile")
                .and_then(|profile| profile.get("chatgpt_account_id"))
        });
    if let Some(account_id) = direct.and_then(|value| value.as_str()) {
        return Some(account_id.to_string());
    }
    claims.as_object().and_then(|object| {
        object.values().find_map(|value| {
            value
                .as_object()
                .and_then(|inner| inner.get("chatgpt_account_id"))
                .and_then(|value| value.as_str())
                .map(str::to_string)
        })
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pkce_is_secure_url_safe_and_matches_s256() {
        let (verifier, challenge) = OAuthHandler::generate_pkce().unwrap();
        assert!((43..=128).contains(&verifier.len()));
        assert!(verifier
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || byte == b'-' || byte == b'_'));
        assert_eq!(
            challenge,
            URL_SAFE_NO_PAD.encode(Sha256::digest(verifier.as_bytes()))
        );
        let (other_verifier, _) = OAuthHandler::generate_pkce().unwrap();
        assert_ne!(verifier, other_verifier);
    }

    #[test]
    fn callback_decodes_code_and_accepts_matching_state() {
        let code = parse_callback_target(
            "/auth/callback?code=a%2Fb%2Bc&state=known-state",
            "/auth/callback",
            Some("known-state"),
        )
        .unwrap();
        assert_eq!(code, "a/b+c");
    }

    #[test]
    fn callback_rejects_missing_or_wrong_state() {
        let missing = parse_callback_target(
            "/auth/callback?code=abc",
            "/auth/callback",
            Some("expected"),
        );
        assert!(missing
            .unwrap_err()
            .to_string()
            .contains("state was missing"));

        let wrong = parse_callback_target(
            "/auth/callback?code=abc&state=wrong",
            "/auth/callback",
            Some("expected"),
        );
        assert!(wrong.unwrap_err().to_string().contains("did not match"));
    }

    #[test]
    fn callback_rejects_error_path_and_missing_code() {
        let refused = parse_callback_target(
            "/callback?error=access_denied&error_description=Not+allowed",
            "/callback",
            Some("unused"),
        );
        assert!(refused.unwrap_err().to_string().contains("Not allowed"));

        let wrong_path = parse_callback_target(
            "/callback/evil?code=abc&state=expected",
            "/callback",
            Some("expected"),
        );
        assert!(wrong_path
            .unwrap_err()
            .to_string()
            .contains("path did not match"));
    }

    #[test]
    fn verification_urls_require_provider_https_hosts() {
        assert!(validate_device_verification_uri(
            OAuthProvider::GithubCopilot,
            "https://github.com/login/device"
        )
        .is_ok());
        assert!(validate_device_verification_uri(
            OAuthProvider::GithubCopilot,
            "https://github.com.evil.example/login/device"
        )
        .is_err());
        assert!(validate_device_verification_uri(
            OAuthProvider::XaiOauth,
            "http://accounts.x.ai/device"
        )
        .is_err());
    }

    #[test]
    fn antigravity_is_a_known_provider_with_ephemeral_port_config() {
        assert_eq!(
            "antigravity".parse::<OAuthProvider>().unwrap(),
            OAuthProvider::Antigravity
        );
        let config = browser_config(OAuthProvider::Antigravity).unwrap();
        assert_eq!(
            config.port, 0,
            "Google's loopback flow must use an ephemeral port"
        );
        assert_eq!(config.callback_path, "/auth/callback");
        assert!(config.scope.contains("cloud-platform"));
    }

    #[test]
    fn antigravity_user_agent_names_the_ide_client() {
        let ua = antigravity_user_agent();
        assert!(ua.starts_with("antigravity/ide/"), "{ua}");
    }

    #[tokio::test]
    async fn ephemeral_loopback_bind_reports_the_real_port() {
        let (_listener, port) = bind_loopback_port(0).await.unwrap();
        assert_ne!(port, 0);
    }
}
