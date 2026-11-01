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

pub struct OpenAiCompatibleProvider {
    client: reqwest::Client,
    base_url: String,
    api_key: String,
    provider_label: String,
}

impl OpenAiCompatibleProvider {
    pub fn new(
        base_url: impl Into<String>,
        api_key: impl Into<String>,
        label: Option<&str>,
    ) -> Self {
        Self {
            client: reqwest::Client::new(),
            base_url: base_url.into(),
            api_key: api_key.into(),
            provider_label: label.unwrap_or("openai-compatible").to_string(),
        }
    }

    pub fn new_deepseek(api_key: impl Into<String>) -> Self {
        Self::new("https://api.deepseek.com/v1", api_key, Some("deepseek"))
    }

    pub fn new_qwen(api_key: impl Into<String>) -> Self {
        Self::new("https://dashscope-intl.aliyuncs.com/compatible-mode/v1", api_key, Some("qwen"))
    }

    pub fn new_alibaba(api_key: impl Into<String>) -> Self {
        Self::new("https://dashscope.aliyuncs.com/compatible-mode/v1", api_key, Some("alibaba"))
    }

    pub fn new_stepfun(api_key: impl Into<String>) -> Self {
        Self::new("https://api.stepfun.com/v1", api_key, Some("stepfun"))
    }
}

#[async_trait]
impl ModelProvider for OpenAiCompatibleProvider {
    fn provider_name(&self) -> &str {
        &self.provider_label
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
        if !self.api_key.is_empty() {
            let auth_val = format!("Bearer {}", self.api_key);
            if let Ok(val) = HeaderValue::from_str(&auth_val) {
                headers.insert(AUTHORIZATION, val);
            }
        }

        if self.provider_label == "github-copilot" {
            headers.insert("Editor-Version", HeaderValue::from_static("vscode/1.90.0"));
            headers.insert("Editor-Plugin-Version", HeaderValue::from_static("copilot-chat/0.38.0"));
            headers.insert(USER_AGENT, HeaderValue::from_static("GitHubCopilotChat/0.38.0"));
        }

        // Format messages
        let mut messages_payload = Vec::new();
        if let Some(sys) = request.system_prompt {
            messages_payload.push(json!({
                "role": "system",
                "content": sys,
            }));
        }

        for msg in request.messages {
            match msg.role {
                MessageRole::User => {
                    messages_payload.push(json!({
                        "role": "user",
                        "content": msg.content.text,
                    }));
                }
                MessageRole::System => {
                    messages_payload.push(json!({
                        "role": "system",
                        "content": msg.content.text,
                    }));
                }
                MessageRole::Assistant => {
                    let mut obj = json!({
                        "role": "assistant",
                        "content": if msg.content.text.is_empty() { serde_json::Value::Null } else { json!(msg.content.text) },
                    });
                    if !msg.content.tool_calls.is_empty() {
                        let tcs: Vec<_> = msg.content.tool_calls.iter().map(|tc| {
                            json!({
                                "id": tc.call_id,
                                "type": "function",
                                "function": {
                                    "name": tc.tool_name,
                                    "arguments": tc.arguments.to_string(),
                                }
                            })
                        }).collect();
                        obj["tool_calls"] = json!(tcs);
                    }
                    messages_payload.push(obj);
                }
                MessageRole::Tool => {
                    messages_payload.push(json!({
                        "role": "tool",
                        "tool_call_id": msg.id,
                        "content": msg.content.text,
                    }));
                }
            }
        }

        let model_str = if request.model == "odon-auto" || request.model == "auto" || request.model.is_empty() {
            match self.provider_label.as_str() {
                "deepseek" => "deepseek-chat",
                "qwen" => "qwen-max-latest",
                "mistral" => "mistral-large-latest",
                _ => "gpt-4o",
            }
        } else if self.provider_label == "github-copilot" {
            request.model.strip_prefix("copilot-").unwrap_or(&request.model)
        } else {
            &request.model
        };

        let mut body = json!({
            "model": model_str,
            "messages": messages_payload,
            "stream": true,
        });

        if !request.tools.is_empty() {
            let tools_json: Vec<_> = request
                .tools
                .into_iter()
                .map(|t| {
                    json!({
                        "type": "function",
                        "function": {
                            "name": t.name,
                            "description": t.description,
                            "parameters": t.parameters,
                        }
                    })
                })
                .collect();
            body["tools"] = json!(tools_json);
        }

        let resp = self
            .client
            .post(&endpoint)
            .headers(headers)
            .json(&body)
            .send()
            .await
            .map_err(|e| OdonError::Provider {
                provider: self.provider_label.clone(),
                message: format!("HTTP request failed: {}", e),
            })?;

        if !resp.status().is_success() {
            let err_body = resp.text().await.unwrap_or_default();
            return Err(OdonError::Provider {
                provider: self.provider_label.clone(),
                message: format!("HTTP status error: {}", err_body),
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
                                if let Some(choices) = val.get("choices").and_then(|c| c.as_array()) {
                                    if let Some(first) = choices.first() {
                                        let delta = first.get("delta");
                                        if let Some(content) = delta.and_then(|d| d.get("content")).and_then(|c| c.as_str()) {
                                            let _ = event_sender.send(StreamEvent::TextDelta(content.to_string())).await;
                                        }
                                        if let Some(thought) = delta.and_then(|d| d.get("reasoning_content")).and_then(|c| c.as_str()) {
                                            let _ = event_sender.send(StreamEvent::ThoughtDelta(thought.to_string())).await;
                                        }
                                        if let Some(tool_calls) = delta.and_then(|d| d.get("tool_calls")).and_then(|t| t.as_array()) {
                                            for tc in tool_calls {
                                                let idx = tc.get("index").and_then(|i| i.as_u64()).unwrap_or(0) as usize;
                                                let id = tc.get("id").and_then(|s| s.as_str()).map(|s| s.to_string());
                                                let func = tc.get("function");
                                                let name = func.and_then(|f| f.get("name")).and_then(|n| n.as_str()).map(|s| s.to_string());
                                                let args = func.and_then(|f| f.get("arguments")).and_then(|a| a.as_str()).map(|s| s.to_string());

                                                let _ = event_sender.send(StreamEvent::ToolCallDelta {
                                                    index: idx,
                                                    id,
                                                    name,
                                                    arguments_delta: args,
                                                }).await;
                                            }
                                        }
                                    }
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
