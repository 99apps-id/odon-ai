use std::sync::Arc;

use async_trait::async_trait;
use reqwest::Client;
use tokio::sync::mpsc::Sender;
use tokio_util::sync::CancellationToken;

use crate::stream::StreamEvent;
use crate::trait_def::{ChatRequest, ModelProvider};

const DEFAULT_HOST: &str = "https://antigravity-gemini.google.com";

pub struct AntigravityProvider {
    client: Client,
    base_url: String,
    access_token: String,
    account_id: Option<String>,
}

impl AntigravityProvider {
    pub fn new(client: Client, access_token: impl Into<String>, account_id: Option<String>) -> Self {
        Self {
            client,
            base_url: std::env::var("ANTIGRAVITY_API_HOST")
                .unwrap_or_else(|_| DEFAULT_HOST.to_string()),
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
    ) -> crate::Result<()> {
        let endpoint = format!(
            "{}/v1beta/chat/completions",
            self.base_url.trim_end_matches('/')
        );

        let mut headers = reqwest::header::HeaderMap::new();
        headers.insert(
            reqwest::header::CONTENT_TYPE,
            reqwest::header::HeaderValue::from_static("application/json"),
        );
        if let Ok(val) = reqwest::header::HeaderValue::from_str(&format!("Bearer {}", self.access_token)) {
            headers.insert(reqwest::header::AUTHORIZATION, val);
        }
        headers.insert(
            reqwest::header::USER_AGENT,
            reqwest::header::HeaderValue::from_static("antigravity/ide/cli"),
        );
        if let Some(acc) = &self.account_id {
            if let Ok(val) = reqwest::header::HeaderValue::from_str(acc) {
                headers.insert(reqwest::header::HeaderName::from_static("account"), val);
            }
        }

        let mut input_items = Vec::new();
        for msg in request.messages {
            let role_str = match msg.role {
                odon_core::MessageRole::User => "user",
                odon_core::MessageRole::Assistant => "assistant",
                odon_core::MessageRole::System => "system",
                odon_core::MessageRole::Tool => "tool",
            };
            input_items.push(serde_json::json!({
                "role": role_str,
                "content": msg.content.text,
            }));
        }

        let mut body = serde_json::json!({
            "model": request.model,
            "stream": true,
            "input": input_items,
        });

        if let Some(sys) = request.system_prompt {
            body["system_instruction"] = serde_json::json!({
                "parts": [{ "text": sys }]
            });
        }

        if let Some(effort) = request.effort {
            body["generationConfig"] = serde_json::json!({
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
            .map_err(|e| crate::OdonError::Provider {
                provider: "antigravity".to_string(),
                message: format!("Antigravity HTTP request failed: {}", e),
            })?;

        if resp.status() == 401 {
            return Err(crate::OdonError::Auth("Antigravity token expired or invalid".to_string()));
        }

        if !resp.status().is_success() {
            let err_body = resp.text().await.unwrap_or_default();
            return Err(crate::OdonError::Provider {
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
