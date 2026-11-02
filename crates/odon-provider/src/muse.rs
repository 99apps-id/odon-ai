use async_trait::async_trait;
use eventsource_stream::Eventsource;
use futures_util::StreamExt;
use odon_core::{MessageRole, OdonError, Result};
use reqwest::header::{HeaderMap, HeaderValue, AUTHORIZATION, CONTENT_TYPE, USER_AGENT};
use serde_json::json;
use tokio::sync::mpsc::Sender;
use tokio_util::sync::CancellationToken;

use crate::multimodal;
use crate::stream::StreamEvent;
use crate::trait_def::{ChatRequest, ModelProvider};

const MUSE_USER_AGENT: &str = "muse-code/1.0.2";

pub struct MuseProvider {
    client: reqwest::Client,
    base_url: String,
    access_token: String,
}

impl MuseProvider {
    pub fn new(access_token: impl Into<String>, base_url: Option<&str>) -> Self {
        Self {
            client: reqwest::Client::new(),
            base_url: base_url.unwrap_or("https://api.muse.ai/v1").to_string(),
            access_token: access_token.into(),
        }
    }

    /// Rewrites raw SSE JSON, normalizing chunks that lack choices or send null choices to empty array
    pub fn sanitize_sse_payload(payload: &str) -> String {
        if let Ok(mut val) = serde_json::from_str::<serde_json::Value>(payload) {
            if let Some(obj) = val.as_object_mut() {
                if !obj.contains_key("choices") || obj.get("choices").map(|c| c.is_null()).unwrap_or(false) {
                    obj.insert("choices".to_string(), json!([]));
                    return serde_json::to_string(&val).unwrap_or_else(|_| payload.to_string());
                }
            }
        }
        payload.to_string()
    }
}

#[async_trait]
impl ModelProvider for MuseProvider {
    fn provider_name(&self) -> &str {
        "muse"
    }

    async fn stream_chat(
        &self,
        request: ChatRequest,
        event_sender: Sender<StreamEvent>,
        cancellation: CancellationToken,
    ) -> Result<()> {
        let endpoint = format!("{}/chat/completions", self.base_url.trim_end_matches('/'));

        let mut headers = HeaderMap::new();
        headers.insert(CONTENT_TYPE, HeaderValue::from_static("application/json"));
        headers.insert(
            AUTHORIZATION,
            HeaderValue::from_str(&format!("Bearer {}", self.access_token))
                .map_err(|e| OdonError::Auth(e.to_string()))?,
        );
        headers.insert(USER_AGENT, HeaderValue::from_static(MUSE_USER_AGENT));

        let mut messages_payload = Vec::new();
        if let Some(sys) = &request.system_prompt {
            messages_payload.push(json!({ "role": "system", "content": sys }));
        }
        for msg in request.messages {
            let role_str = match msg.role {
                MessageRole::User => "user",
                MessageRole::Assistant => "assistant",
                MessageRole::System => "system",
                MessageRole::Tool => "tool",
            };
            let mut payload = json!({
                "role": role_str,
                "content": if matches!(msg.role, MessageRole::User) {
                    multimodal::openai_content(&msg.content)
                } else if msg.content.text.is_empty() {
                    serde_json::Value::Null
                } else {
                    json!(msg.content.text)
                },
            });
            if matches!(msg.role, MessageRole::Assistant) && !msg.content.tool_calls.is_empty() {
                let calls: Vec<_> = msg
                    .content
                    .tool_calls
                    .iter()
                    .map(|tc| {
                        json!({
                            "id": tc.call_id,
                            "type": "function",
                            "function": {
                                "name": tc.tool_name,
                                "arguments": tc.arguments.to_string()
                            }
                        })
                    })
                    .collect();
                payload["tool_calls"] = json!(calls);
            }
            if matches!(msg.role, MessageRole::Tool) {
                payload["tool_call_id"] = json!(msg
                    .content
                    .tool_calls
                    .first()
                    .map(|t| t.call_id.clone())
                    .unwrap_or_else(|| msg.id.clone()));
            }
            messages_payload.push(payload);
        }

        let mut body = json!({
            "model": request.model,
            "messages": messages_payload,
            "stream": true,
        });

        if !request.tools.is_empty() {
            let tools: Vec<_> = request
                .tools
                .iter()
                .map(|t| {
                    json!({
                        "type": "function",
                        "function": {
                            "name": t.name,
                            "description": t.description,
                            "parameters": t.parameters
                        }
                    })
                })
                .collect();
            body["tools"] = json!(tools);
        }

        let resp = self
            .client
            .post(&endpoint)
            .headers(headers)
            .json(&body)
            .send()
            .await
            .map_err(|e| OdonError::Provider {
                provider: "muse".to_string(),
                message: format!("Muse HTTP request failed: {}", e),
            })?;

        if !resp.status().is_success() {
            let status = resp.status();
            let mut err_body = resp.text().await.unwrap_or_default();
            if err_body.len() > 2000 {
                err_body.truncate(2000);
                err_body.push_str("…");
            }
            return Err(OdonError::Provider {
                provider: "muse".to_string(),
                message: format!(
                    "HTTP {} from Muse (model: {}).{}",
                    status.as_u16(),
                    request.model,
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
                            if event.data == "[DONE]" {
                                let _ = event_sender.send(StreamEvent::Done).await;
                                break;
                            }
                            let sanitized = Self::sanitize_sse_payload(&event.data);
                            if let Ok(val) = serde_json::from_str::<serde_json::Value>(&sanitized) {
                                if let Some(choices) = val.get("choices").and_then(|c| c.as_array()) {
                                    if let Some(first) = choices.first() {
                                        if let Some(delta) = first.get("delta") {
                                            if let Some(content) = delta.get("content").and_then(|c| c.as_str()) {
                                                let _ = event_sender.send(StreamEvent::TextDelta(content.to_string())).await;
                                            }
                                            if let Some(thought) = delta.get("reasoning_content").and_then(|c| c.as_str()) {
                                                let _ = event_sender.send(StreamEvent::ThoughtDelta(thought.to_string())).await;
                                            }
                                            if let Some(calls) = delta.get("tool_calls").and_then(|t| t.as_array()) {
                                                for tc in calls {
                                                    let index = tc.get("index").and_then(|i| i.as_u64()).unwrap_or(0) as usize;
                                                    let func = tc.get("function");
                                                    let _ = event_sender.send(StreamEvent::ToolCallDelta {
                                                        index,
                                                        id: tc.get("id").and_then(|i| i.as_str()).map(String::from),
                                                        name: func.and_then(|f| f.get("name")).and_then(|n| n.as_str()).map(String::from),
                                                        arguments_delta: func.and_then(|f| f.get("arguments")).and_then(|a| a.as_str()).map(String::from),
                                                    }).await;
                                                }
                                            }
                                        }
                                    }
                                }
                            }
                        }
                        Some(Err(e)) => {
                            let _ = event_sender.send(StreamEvent::Error(format!("Muse SSE error: {}", e))).await;
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
