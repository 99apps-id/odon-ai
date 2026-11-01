use async_trait::async_trait;
use futures_util::StreamExt;
use odon_core::{MessageRole, OdonError, Result};
use reqwest::header::{HeaderMap, HeaderValue, AUTHORIZATION, CONTENT_TYPE, USER_AGENT};
use serde_json::json;
use tokio::sync::mpsc::Sender;
use tokio_util::sync::CancellationToken;

use crate::stream::StreamEvent;
use crate::trait_def::{ChatRequest, ModelProvider};

const ANTIGRAVITY_HOST: &str = "https://antigravity-gemini.google.com";

pub struct AntigravityProvider {
    client: reqwest::Client,
    base_url: String,
    access_token: String,
    account_id: Option<String>,
}

impl AntigravityProvider {
    pub fn new(
        access_token: impl Into<String>,
        account_id: Option<String>,
        custom_base: Option<&str>,
    ) -> Self {
        Self {
            client: reqwest::Client::new(),
            base_url: custom_base
                .unwrap_or(ANTIGRAVITY_HOST)
                .to_string(),
            access_token: access_token.into(),
            account_id,
        }
    }
}

#[async_trait]
impl ModelProvider for AntigravityProvider {
    fn provider_name(&self) -> &str {
        "antigravity"
    }

    async fn stream_chat(
        &self,
        request: ChatRequest,
        event_sender: Sender<StreamEvent>,
        cancellation: CancellationToken,
    ) -> Result<()> {
        let endpoint = format!(
            "{}/v1beta/chat/completions",
            self.base_url.trim_end_matches('/')
        );

        let mut headers = HeaderMap::new();
        headers.insert(CONTENT_TYPE, HeaderValue::from_static("application/json"));
        headers.insert(
            AUTHORIZATION,
            HeaderValue::from_str(&format!("Bearer {}", self.access_token))
                .map_err(|e| OdonError::Auth(e.to_string()))?,
        );
        headers.insert(
            USER_AGENT,
            HeaderValue::from_static("antigravity/ide/cli"),
        );
        if let Some(acc) = &self.account_id {
            if let Ok(val) = HeaderValue::from_str(acc) {
                headers.insert("account", val);
            }
        }

        let mut input_items = Vec::new();
        for msg in request.messages {
            let role_str = match msg.role {
                MessageRole::User => "user",
                MessageRole::Assistant => "assistant",
                MessageRole::System => "system",
                MessageRole::Tool => "tool",
            };
            input_items.push(json!({
                "role": role_str,
                "content": msg.content.text,
            }));
        }

        let model_str = if request.model == "odon-auto" || request.model == "auto" || request.model.is_empty() {
            "gemini-2.0-flash"
        } else {
            &request.model
        };

        let mut body = json!({
            "model": model_str,
            "stream": true,
            "input": input_items,
        });

        if let Some(sys) = request.system_prompt {
            body["system_instruction"] = json!({
                "parts": [{ "text": sys }]
            });
        }

        if let Some(effort) = request.effort {
            body["generationConfig"] = json!({
                "thinkingConfig": {
                    "thinkingBudget": match effort.as_str() {
                        "low" => 2048,
                        "medium" => 8192,
                        "high" => 32768,
                        "xhigh" => 65536,
                        _ => 8192,
                    }
                }
            });
        }

        let resp = self
            .client
            .post(&endpoint)
            .headers(headers)
            .json(&body)
            .send()
            .await
            .map_err(|e| OdonError::Provider {
                provider: "antigravity".to_string(),
                message: format!("Antigravity HTTP request failed: {}", e),
            })?;

        if resp.status() == 401 {
            return Err(OdonError::Auth("Antigravity token expired or invalid".to_string()));
        }

        if !resp.status().is_success() {
            let err_body = resp.text().await.unwrap_or_default();
            return Err(OdonError::Provider {
                provider: "antigravity".to_string(),
                message: format!("Antigravity HTTP status error: {}", err_body),
            });
        }

        let mut stream = resp.bytes_stream();

        loop {
            tokio::select! {
                _ = cancellation.cancelled() => {
                    let _ = event_sender.send(StreamEvent::Done).await;
                    return Ok(());
                }
                item = stream.next() => {
                    match item {
                        Some(Ok(bytes)) => {
                            let text = String::from_utf8_lossy(&bytes).to_string();
                            let _ = event_sender.send(StreamEvent::TextDelta(text)).await;
                        }
                        Some(Err(e)) => {
                            let _ = event_sender.send(StreamEvent::Error(format!("Antigravity stream error: {}", e))).await;
                            break;
                        }
                        None => {
                            let _ = event_sender.send(StreamEvent::Done).await;
                            break;
                        }
                    }
                }
            }
        }

        Ok(())
    }
}
