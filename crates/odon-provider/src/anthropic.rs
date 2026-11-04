use async_trait::async_trait;
use eventsource_stream::Eventsource;
use futures_util::StreamExt;
use odon_core::{MessageRole, OdonError, Result};
use reqwest::header::{HeaderMap, HeaderValue, AUTHORIZATION, CONTENT_TYPE};
use serde_json::json;
use tokio::sync::mpsc::Sender;
use tokio_util::sync::CancellationToken;

use crate::multimodal;
use crate::stream::StreamEvent;
use crate::trait_def::{ChatRequest, ModelProvider};

#[derive(Clone, Copy, PartialEq, Eq)]
enum AuthMode {
    ApiKey,
    Bearer,
}

pub struct AnthropicProvider {
    client: reqwest::Client,
    api_key: String,
    base_url: String,
    label: String,
    auth_mode: AuthMode,
    extra_headers: HeaderMap,
    default_max_tokens: u32,
}

impl AnthropicProvider {
    pub fn new(api_key: impl Into<String>) -> Self {
        Self {
            client: reqwest::Client::new(),
            api_key: api_key.into(),
            base_url: "https://api.anthropic.com/v1".to_string(),
            label: "anthropic".to_string(),
            auth_mode: AuthMode::ApiKey,
            extra_headers: HeaderMap::new(),
            default_max_tokens: 4096,
        }
    }

    pub fn with_base_url(api_key: impl Into<String>, base_url: impl Into<String>) -> Self {
        Self {
            client: reqwest::Client::new(),
            api_key: api_key.into(),
            base_url: base_url.into(),
            label: "anthropic".to_string(),
            auth_mode: AuthMode::ApiKey,
            extra_headers: HeaderMap::new(),
            default_max_tokens: 4096,
        }
    }

    /// Anthropic-compatible endpoints such as GitHub Copilot's `/v1/messages`
    /// authenticate with a bearer token instead of `x-api-key`.
    pub fn with_bearer_auth(api_key: impl Into<String>, base_url: impl Into<String>) -> Self {
        let mut provider = Self::with_base_url(api_key, base_url);
        provider.auth_mode = AuthMode::Bearer;
        provider
    }

    pub fn with_label(mut self, label: impl Into<String>) -> Self {
        self.label = label.into();
        self
    }

    pub fn with_extra_headers(mut self, headers: HeaderMap) -> Self {
        for (name, value) in headers.iter() {
            self.extra_headers.insert(name.clone(), value.clone());
        }
        self
    }

    pub fn with_default_max_tokens(mut self, max_tokens: u32) -> Self {
        self.default_max_tokens = max_tokens;
        self
    }
}

#[async_trait]
impl ModelProvider for AnthropicProvider {
    fn provider_name(&self) -> &str {
        &self.label
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
        match self.auth_mode {
            AuthMode::ApiKey => {
                headers.insert(
                    "x-api-key",
                    HeaderValue::from_str(&self.api_key).map_err(|e| {
                        OdonError::Auth(format!("Invalid Anthropic API key format: {e}"))
                    })?,
                );
            }
            AuthMode::Bearer => {
                headers.insert(
                    AUTHORIZATION,
                    HeaderValue::from_str(&format!("Bearer {}", self.api_key)).map_err(|e| {
                        OdonError::Auth(format!("Invalid bearer token format: {e}"))
                    })?,
                );
            }
        }
        headers.insert("anthropic-version", HeaderValue::from_static("2023-06-01"));
        for (name, value) in self.extra_headers.iter() {
            headers.insert(name.clone(), value.clone());
        }
        if request.messages.iter().any(|m| {
            m.content
                .attachments
                .iter()
                .any(|a| a.mime_type == "application/pdf")
        }) {
            headers.insert(
                "anthropic-beta",
                HeaderValue::from_static("pdfs-2024-09-25"),
            );
        }

        // Anthropic wants: assistant turns carry `tool_use` blocks, and the matching
        // results come back as `tool_result` blocks inside a single user turn.
        let mut messages_payload: Vec<serde_json::Value> = Vec::new();
        for msg in request.messages {
            match msg.role {
                MessageRole::User => {
                    messages_payload.push(json!({
                        "role": "user",
                        "content": multimodal::anthropic_blocks(&msg.content),
                    }));
                }
                MessageRole::Assistant => {
                    let mut blocks = Vec::new();
                    if !msg.content.text.is_empty() {
                        blocks.push(json!({ "type": "text", "text": msg.content.text }));
                    }
                    for tc in &msg.content.tool_calls {
                        let input = if tc.arguments.is_object() {
                            tc.arguments.clone()
                        } else {
                            json!({})
                        };
                        blocks.push(json!({
                            "type": "tool_use", "id": tc.call_id, "name": tc.tool_name, "input": input
                        }));
                    }
                    if blocks.is_empty() {
                        continue;
                    }
                    messages_payload.push(json!({ "role": "assistant", "content": blocks }));
                }
                MessageRole::Tool => {
                    let call_id = msg
                        .content
                        .tool_calls
                        .first()
                        .map(|t| t.call_id.clone())
                        .unwrap_or_default();
                    let block = json!({
                        "type": "tool_result", "tool_use_id": call_id, "content": msg.content.text
                    });
                    let merge = messages_payload
                        .last()
                        .map(|m| m["role"] == "user" && m["content"].is_array())
                        .unwrap_or(false);
                    if merge {
                        if let Some(arr) = messages_payload
                            .last_mut()
                            .and_then(|m| m["content"].as_array_mut())
                        {
                            arr.push(block);
                        }
                    } else {
                        messages_payload.push(json!({ "role": "user", "content": [block] }));
                    }
                }
                MessageRole::System => {}
            }
        }

        let model_str = if self.label == "github-copilot" {
            request.model.as_str()
        } else {
            match request.model.as_str() {
                "odon-auto" | "auto" | "" | "claude-3-7-sonnet" => "claude-3-7-sonnet-20250219",
                "claude-3-5-sonnet" => "claude-3-5-sonnet-20241022",
                "claude-3-5-haiku" => "claude-3-5-haiku-20241022",
                other => other,
            }
        };

        let mut body = json!({
            "model": model_str,
            "messages": messages_payload,
            "max_tokens": request.max_tokens.unwrap_or(self.default_max_tokens),
            "stream": true,
        });

        if !request.tools.is_empty() {
            let tools: Vec<_> = request
                .tools
                .iter()
                .map(|t| json!({ "name": t.name, "description": t.description, "input_schema": t.parameters }))
                .collect();
            body["tools"] = json!(tools);
        }

        if let Some(sys) = request.system_prompt {
            body["system"] = json!(sys);
        }

        // Anthropic answers 429 while rate limiting and 529/500/503 while the
        // upstream is saturated. A single transient answer must not end a chat
        // turn, so retry briefly before surfacing the error. This mirrors the
        // openai-compatible path so both providers behave the same way.
        let mut attempt = 0u32;
        let resp = loop {
            let sent = self
                .client
                .post(&endpoint)
                .headers(headers.clone())
                .json(&body)
                .send()
                .await;

            match sent {
                Ok(response) => {
                    let status = response.status();
                    let retryable = status.is_server_error()
                        || status.as_u16() == 429
                        || status.as_u16() == 408;
                    if retryable && attempt < 2 {
                        attempt += 1;
                        tracing::warn!(
                            "Anthropic returned HTTP {} - retry {}/2",
                            status,
                            attempt
                        );
                        tokio::time::sleep(std::time::Duration::from_millis(600 * attempt as u64))
                            .await;
                        continue;
                    }
                    break response;
                }
                Err(e) if attempt < 2 && (e.is_timeout() || e.is_connect()) => {
                    attempt += 1;
                    tracing::warn!("Anthropic request failed ({}): retry {}/2", e, attempt);
                    tokio::time::sleep(std::time::Duration::from_millis(600 * attempt as u64))
                        .await;
                }
                Err(e) => {
                    return Err(OdonError::Provider {
                        provider: self.label.clone(),
                        message: format!("Anthropic request failed: {}", e),
                    });
                }
            }
        };

        if !resp.status().is_success() {
            let status = resp.status();
            let err_body =
                crate::truncate_str_boundary(&resp.text().await.unwrap_or_default(), 2000);
            return Err(OdonError::Provider {
                provider: self.label.clone(),
                message: format!(
                    "HTTP {} from Anthropic (model: {}).{}",
                    status.as_u16(),
                    model_str,
                    if err_body.trim().is_empty() {
                        String::new()
                    } else {
                        format!(" Response: {}", err_body.trim())
                    }
                ),
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
                                    "content_block_start" => {
                                        if let Some(cb) = val.get("content_block") {
                                            if cb.get("type").and_then(|t| t.as_str()) == Some("tool_use") {
                                                let idx = val.get("index").and_then(|i| i.as_u64()).unwrap_or(0) as usize;
                                                let _ = event_sender.send(StreamEvent::ToolCallDelta {
                                                    index: idx,
                                                    id: cb.get("id").and_then(|s| s.as_str()).map(String::from),
                                                    name: cb.get("name").and_then(|s| s.as_str()).map(String::from),
                                                    arguments_delta: None,
                                                }).await;
                                            }
                                        }
                                    }
                                    "content_block_delta" => {
                                        if let Some(delta) = val.get("delta") {
                                            if let Some(text) = delta.get("text").and_then(|t| t.as_str()) {
                                                let _ = event_sender.send(StreamEvent::TextDelta(text.to_string())).await;
                                            } else if let Some(th) = delta.get("thinking").and_then(|t| t.as_str()) {
                                                let _ = event_sender.send(StreamEvent::ThoughtDelta(th.to_string())).await;
                                            } else if let Some(pj) = delta.get("partial_json").and_then(|t| t.as_str()) {
                                                let idx = val.get("index").and_then(|i| i.as_u64()).unwrap_or(0) as usize;
                                                let _ = event_sender.send(StreamEvent::ToolCallDelta {
                                                    index: idx,
                                                    id: None,
                                                    name: None,
                                                    arguments_delta: Some(pj.to_string()),
                                                }).await;
                                            }
                                        }
                                    }
                                    "message_delta" => {
                                        if let Some(reason) = val
                                            .get("delta")
                                            .and_then(|d| d.get("stop_reason"))
                                            .and_then(|r| r.as_str())
                                        {
                                            if reason == "max_tokens" {
                                                let _ =
                                                    event_sender.send(StreamEvent::OutputTruncated).await;
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
