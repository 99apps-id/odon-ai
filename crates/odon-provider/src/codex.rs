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

const CODEX_CLI_VERSION: &str = "0.159.0";

pub struct OpenAiCodexProvider {
    client: reqwest::Client,
    base_url: String,
    access_token: String,
    account_id: Option<String>,
}

impl OpenAiCodexProvider {
    pub fn new(
        access_token: impl Into<String>,
        account_id: Option<String>,
        custom_base: Option<&str>,
    ) -> Self {
        Self {
            client: reqwest::Client::new(),
            base_url: custom_base
                .unwrap_or("https://chatgpt.com/backend-api")
                .to_string(),
            access_token: access_token.into(),
            account_id,
        }
    }
}

#[async_trait]
impl ModelProvider for OpenAiCodexProvider {
    fn provider_name(&self) -> &str {
        "openai-codex"
    }

    async fn stream_chat(
        &self,
        request: ChatRequest,
        event_sender: Sender<StreamEvent>,
        cancellation: CancellationToken,
    ) -> Result<()> {
        let endpoint = format!("{}/responses", self.base_url.trim_end_matches('/'));

        let mut headers = HeaderMap::new();
        headers.insert(CONTENT_TYPE, HeaderValue::from_static("application/json"));
        headers.insert(
            AUTHORIZATION,
            HeaderValue::from_str(&format!("Bearer {}", self.access_token))
                .map_err(|e| OdonError::Auth(e.to_string()))?,
        );
        headers.insert(
            USER_AGENT,
            HeaderValue::from_static("codex_cli_rs/0.159.0"),
        );
        headers.insert("originator", HeaderValue::from_static("codex_cli_rs"));
        headers.insert("version", HeaderValue::from_static(CODEX_CLI_VERSION));

        if let Some(acc) = &self.account_id {
            if let Ok(val) = HeaderValue::from_str(acc) {
                headers.insert("ChatGPT-Account-ID", val);
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

        let mut body = json!({
            "model": request.model,
            "stream": true,
            "store": false,
            "input": input_items,
            "reasoning": {
                "summary": "auto",
                "effort": "high"
            },
            "include": ["reasoning.encrypted_content"]
        });

        if let Some(sys) = request.system_prompt {
            body["instructions"] = json!(sys);
        }

        let resp = self
            .client
            .post(&endpoint)
            .headers(headers)
            .json(&body)
            .send()
            .await
            .map_err(|e| OdonError::Provider {
                provider: "openai-codex".to_string(),
                message: format!("Codex HTTP request failed: {}", e),
            })?;

        if !resp.status().is_success() {
            let err_body = resp.text().await.unwrap_or_default();
            return Err(OdonError::Provider {
                provider: "openai-codex".to_string(),
                message: format!("Codex HTTP status error: {}", err_body),
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
                            if event.data == "[DONE]" {
                                let _ = event_sender.send(StreamEvent::Done).await;
                                break;
                            }
                            if let Ok(val) = serde_json::from_str::<serde_json::Value>(&event.data) {
                                let evt_type = val.get("type").and_then(|t| t.as_str()).unwrap_or_default();
                                match evt_type {
                                    "response.output_item.delta" | "response.text.delta" => {
                                        if let Some(delta) = val.get("delta").and_then(|d| d.get("text")).and_then(|t| t.as_str()) {
                                            let _ = event_sender.send(StreamEvent::TextDelta(delta.to_string())).await;
                                        }
                                    }
                                    "response.reasoning.delta" => {
                                        if let Some(delta) = val.get("delta").and_then(|d| d.get("text")).and_then(|t| t.as_str()) {
                                            let _ = event_sender.send(StreamEvent::ThoughtDelta(delta.to_string())).await;
                                        }
                                    }
                                    "response.completed" => {
                                        let _ = event_sender.send(StreamEvent::Done).await;
                                        break;
                                    }
                                    _ => {}
                                }
                            }
                        }
                        Some(Err(e)) => {
                            let _ = event_sender.send(StreamEvent::Error(format!("SSE error: {}", e))).await;
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
