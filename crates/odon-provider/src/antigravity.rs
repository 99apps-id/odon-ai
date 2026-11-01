use async_trait::async_trait;
use eventsource_stream::Eventsource;
use futures_util::StreamExt;
use odon_core::{MessageRole, OdonError, Result};
use reqwest::header::{HeaderMap, HeaderValue, AUTHORIZATION, CONTENT_TYPE, USER_AGENT};
use serde_json::json;
use tokio::sync::mpsc::Sender;
use tokio_util::sync::CancellationToken;

use crate::stream::StreamEvent;
use crate::trait_def::{ChatRequest, ModelProvider};

pub struct AntigravityProvider {
    client: reqwest::Client,
    access_token: String,
    project_id: Option<String>,
}

impl AntigravityProvider {
    pub fn new(access_token: impl Into<String>, project_id: Option<String>) -> Self {
        Self {
            client: reqwest::Client::new(),
            access_token: access_token.into(),
            project_id,
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
        let endpoint = "https://cloudcode-pa.googleapis.com/v1internal:streamGenerateContent?alt=sse";

        let mut headers = HeaderMap::new();
        headers.insert(CONTENT_TYPE, HeaderValue::from_static("application/json"));
        headers.insert(
            AUTHORIZATION,
            HeaderValue::from_str(&format!("Bearer {}", self.access_token))
                .map_err(|e| OdonError::Auth(e.to_string()))?,
        );
        headers.insert(
            USER_AGENT,
            HeaderValue::from_static("antigravity-client/1.0"),
        );

        let mut contents = Vec::new();
        for msg in request.messages {
            let role = match msg.role {
                MessageRole::User => "user",
                _ => "model",
            };
            contents.push(json!({
                "role": role,
                "parts": [{ "text": msg.content.text }]
            }));
        }

        let project = self.project_id.clone().unwrap_or_else(|| "odon-default".to_string());

        let body = json!({
            "project": project,
            "request": {
                "contents": contents,
                "generationConfig": {
                    "temperature": request.temperature.unwrap_or(0.7),
                    "maxOutputTokens": request.max_tokens.unwrap_or(4096),
                }
            }
        });

        let resp = self
            .client
            .post(endpoint)
            .headers(headers)
            .json(&body)
            .send()
            .await
            .map_err(|e| OdonError::Provider {
                provider: "antigravity".to_string(),
                message: format!("Antigravity HTTP request failed: {}", e),
            })?;

        if !resp.status().is_success() {
            let err_body = resp.text().await.unwrap_or_default();
            return Err(OdonError::Provider {
                provider: "antigravity".to_string(),
                message: format!("Antigravity HTTP status error: {}", err_body),
            });
        }

        let mut stream = resp.bytes_stream().eventsource();

        loop {
            tokio::select! {
                _ = cancellation.cancelled() => {
                    let _ = event_sender.send(StreamEvent::Done).await;
                    return Ok(());
                }
                item = stream.next() => {
                    match item {
                        Some(Ok(event)) => {
                            if let Ok(val) = serde_json::from_str::<serde_json::Value>(&event.data) {
                                // Antigravity envelope: {"response": {"candidates": [{"content": {"parts": [{"text": ...}]}}]}}
                                let target = val.get("response").unwrap_or(&val);
                                if let Some(candidates) = target.get("candidates").and_then(|c| c.as_array()) {
                                    if let Some(cand) = candidates.first() {
                                        if let Some(parts) = cand.get("content").and_then(|c| c.get("parts")).and_then(|p| p.as_array()) {
                                            for part in parts {
                                                if let Some(text) = part.get("text").and_then(|t| t.as_str()) {
                                                    let _ = event_sender.send(StreamEvent::TextDelta(text.to_string())).await;
                                                }
                                            }
                                        }
                                    }
                                }
                            }
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
