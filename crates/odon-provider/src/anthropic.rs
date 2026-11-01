use async_trait::async_trait;
use eventsource_stream::Eventsource;
use futures_util::StreamExt;
use odon_core::{MessageRole, OdonError, Result};
use reqwest::header::{HeaderMap, HeaderValue, CONTENT_TYPE};
use serde_json::json;
use tokio::sync::mpsc::Sender;
use tokio_util::sync::CancellationToken;

use crate::stream::StreamEvent;
use crate::trait_def::{ChatRequest, ModelProvider};

pub struct AnthropicProvider {
    client: reqwest::Client,
    api_key: String,
    base_url: String,
}

impl AnthropicProvider {
    pub fn new(api_key: impl Into<String>) -> Self {
        Self {
            client: reqwest::Client::new(),
            api_key: api_key.into(),
            base_url: "https://api.anthropic.com/v1".to_string(),
        }
    }

    pub fn with_base_url(api_key: impl Into<String>, base_url: impl Into<String>) -> Self {
        Self {
            client: reqwest::Client::new(),
            api_key: api_key.into(),
            base_url: base_url.into(),
        }
    }
}

#[async_trait]
impl ModelProvider for AnthropicProvider {
    fn provider_name(&self) -> &str {
        "anthropic"
    }

    async fn stream_chat(
        &self,
        request: ChatRequest,
        event_sender: Sender<StreamEvent>,
        cancellation: CancellationToken,
    ) -> Result<()> {
        let endpoint = format!("{}/messages", self.base_url.trim_end_matches('/'));

        let mut headers = HeaderMap::new();
        headers.insert(CONTENT_TYPE, HeaderValue::from_static("application/json"));
        headers.insert(
            "x-api-key",
            HeaderValue::from_str(&self.api_key)
                .map_err(|e| OdonError::Auth(format!("Invalid Anthropic API key format: {e}")))?,
        );
        headers.insert("anthropic-version", HeaderValue::from_static("2023-06-01"));

        let mut messages_payload = Vec::new();
        for msg in request.messages {
            match msg.role {
                MessageRole::User => {
                    messages_payload.push(json!({
                        "role": "user",
                        "content": msg.content.text,
                    }));
                }
                MessageRole::Assistant => {
                    messages_payload.push(json!({
                        "role": "assistant",
                        "content": msg.content.text,
                    }));
                }
                _ => {}
            }
        }

        let mut body = json!({
            "model": request.model,
            "messages": messages_payload,
            "max_tokens": request.max_tokens.unwrap_or(4096),
            "stream": true,
        });

        if let Some(sys) = request.system_prompt {
            body["system"] = json!(sys);
        }

        let resp = self
            .client
            .post(&endpoint)
            .headers(headers)
            .json(&body)
            .send()
            .await
            .map_err(|e| OdonError::Provider {
                provider: "anthropic".to_string(),
                message: format!("Anthropic request failed: {}", e),
            })?;

        if !resp.status().is_success() {
            let err_body = resp.text().await.unwrap_or_default();
            return Err(OdonError::Provider {
                provider: "anthropic".to_string(),
                message: format!("Anthropic API error: {}", err_body),
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
                            if event.event == "message_stop" {
                                let _ = event_sender.send(StreamEvent::Done).await;
                                break;
                            }
                            if let Ok(val) = serde_json::from_str::<serde_json::Value>(&event.data) {
                                let evt_type = val.get("type").and_then(|t| t.as_str()).unwrap_or_default();
                                match evt_type {
                                    "content_block_delta" => {
                                        if let Some(delta) = val.get("delta") {
                                            if let Some(text) = delta.get("text").and_then(|t| t.as_str()) {
                                                let _ = event_sender.send(StreamEvent::TextDelta(text.to_string())).await;
                                            }
                                        }
                                    }
                                    "message_delta" => {
                                        if let Some(stop) = val.get("delta").and_then(|d| d.get("stop_reason")).and_then(|s| s.as_str()) {
                                            if stop == "end_turn" || stop == "stop_sequence" {
                                                let _ = event_sender.send(StreamEvent::Done).await;
                                                break;
                                            }
                                        }
                                    }
                                    "error" => {
                                        let msg = val.get("error").and_then(|e| e.get("message")).and_then(|m| m.as_str()).unwrap_or("Anthropic error");
                                        let _ = event_sender.send(StreamEvent::Error(msg.to_string())).await;
                                        break;
                                    }
                                    _ => {}
                                }
                            }
                        }
                        Some(Err(e)) => {
                            let _ = event_sender.send(StreamEvent::Error(format!("Anthropic stream error: {}", e))).await;
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
