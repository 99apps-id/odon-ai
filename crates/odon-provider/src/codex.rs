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

const CODEX_CLI_VERSION: &str = "0.159.0";

pub struct OpenAiCodexProvider {
    client: reqwest::Client,
    base_url: String,
    access_token: String,
    account_id: Option<String>,
    session_id: String,
    models_cache: tokio::sync::Mutex<
        Option<(
            std::time::Instant,
            Vec<crate::codex_catalog::CodexModelInfo>,
        )>,
    >,
}

impl OpenAiCodexProvider {
    pub fn new(
        access_token: impl Into<String>,
        account_id: Option<String>,
        custom_base: Option<&str>,
    ) -> Self {
        let session_id = format!(
            "odon-{:x}",
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|duration| duration.as_nanos())
                .unwrap_or(0)
        );
        Self {
            client: reqwest::Client::new(),
            base_url: custom_base
                .unwrap_or("https://chatgpt.com/backend-api/codex")
                .to_string(),
            access_token: access_token.into(),
            account_id,
            session_id,
            models_cache: tokio::sync::Mutex::new(None),
        }
    }

    async fn codex_models(&self) -> Option<Vec<crate::codex_catalog::CodexModelInfo>> {
        if !self.base_url.contains("chatgpt.com") {
            return None;
        }
        {
            let cache = self.models_cache.lock().await;
            if let Some((fetched_at, models)) = cache.as_ref() {
                if fetched_at.elapsed() < std::time::Duration::from_secs(5 * 60) {
                    return Some(models.clone());
                }
            }
        }
        let models = crate::codex_catalog::fetch_models_with_token(
            &self.access_token,
            self.account_id.as_deref(),
        )
        .await?;
        *self.models_cache.lock().await = Some((std::time::Instant::now(), models.clone()));
        Some(models)
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
        let requested = request.model.trim().to_string();
        if !requested.is_empty() && requested != "auto" && requested != "odon-auto" {
            if let Some(models) = self.codex_models().await {
                if !models.iter().any(|model| model.id == requested) {
                    let available = models
                        .iter()
                        .map(|model| model.id.as_str())
                        .collect::<Vec<_>>()
                        .join(", ");
                    return Err(OdonError::Provider {
                        provider: "openai-codex".to_string(),
                        message: format!(
                            "Model '{}' is not available for this ChatGPT account. Pick another model. Available: {}",
                            requested, available
                        ),
                    });
                }
            }
        }

        let endpoint = format!("{}/responses", self.base_url.trim_end_matches('/'));

        let mut headers = HeaderMap::new();
        headers.insert(CONTENT_TYPE, HeaderValue::from_static("application/json"));
        headers.insert(
            AUTHORIZATION,
            HeaderValue::from_str(&format!("Bearer {}", self.access_token))
                .map_err(|e| OdonError::Auth(e.to_string()))?,
        );
        headers.insert(USER_AGENT, HeaderValue::from_static("codex_cli_rs/0.159.0"));
        headers.insert("originator", HeaderValue::from_static("codex_cli_rs"));
        headers.insert("version", HeaderValue::from_static(CODEX_CLI_VERSION));
        headers.insert(
            "session_id",
            HeaderValue::from_str(&self.session_id).map_err(|e| OdonError::Provider {
                provider: "openai-codex".to_string(),
                message: format!("Invalid Codex session id: {e}"),
            })?,
        );

        if let Some(acc) = &self.account_id {
            if let Ok(val) = HeaderValue::from_str(acc) {
                headers.insert("ChatGPT-Account-ID", val);
            }
        }

        // Responses API input items. Tool calls/results must use the dedicated
        // function_call / function_call_output item types, not chat roles.
        let mut input_items = Vec::new();
        for msg in &request.messages {
            match msg.role {
                MessageRole::Tool => {
                    let call_id = msg
                        .content
                        .tool_calls
                        .first()
                        .map(|t| t.call_id.clone())
                        .unwrap_or_else(|| msg.id.clone());
                    input_items.push(json!({
                        "type": "function_call_output",
                        "call_id": call_id,
                        "output": msg.content.text,
                    }));
                }
                MessageRole::Assistant => {
                    if !msg.content.text.is_empty() {
                        input_items.push(json!({
                            "role": "assistant",
                            "content": msg.content.text,
                        }));
                    }
                    for tc in &msg.content.tool_calls {
                        input_items.push(json!({
                            "type": "function_call",
                            "call_id": tc.call_id,
                            "name": tc.tool_name,
                            "arguments": tc.arguments.to_string(),
                        }));
                    }
                }
                MessageRole::User => {
                    input_items.push(json!({
                        "role": "user",
                        "content": multimodal::responses_content(&msg.content),
                    }));
                }
                MessageRole::System => {
                    input_items.push(json!({
                        "role": "system",
                        "content": msg.content.text,
                    }));
                }
            }
        }

        let mut body = json!({
            "model": request.model,
            "stream": true,
            "store": false,
            "input": input_items,
            "reasoning": {
                "summary": "auto",
                "effort": request.effort.as_deref().unwrap_or("medium")
            },
            "include": ["reasoning.encrypted_content"]
        });

        if !request.tools.is_empty() {
            let tools: Vec<_> = request
                .tools
                .iter()
                .map(|t| {
                    json!({
                        "type": "function",
                        "name": t.name,
                        "description": t.description,
                        "parameters": t.parameters,
                    })
                })
                .collect();
            body["tools"] = json!(tools);
            body["tool_choice"] = json!("auto");
        }

        if let Some(sys) = request.system_prompt {
            body["instructions"] = json!(sys);
        }

        let resp = crate::send_with_retry("openai-codex", || {
            self.client
                .post(&endpoint)
                .headers(headers.clone())
                .json(&body)
        })
        .await?;

        if !resp.status().is_success() {
            let status = resp.status();
            let err_body = resp.text().await.unwrap_or_default();
            return Err(OdonError::Provider {
                provider: "openai-codex".to_string(),
                message: format!(
                    "Codex HTTP status error {}: {}",
                    status,
                    crate::truncate_str_boundary(&err_body, 2000)
                ),
            });
        }

        let mut stream = resp.bytes_stream().eventsource();
        let mut tool_index = 0usize;

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
                            let Ok(val) = serde_json::from_str::<serde_json::Value>(&event.data) else { continue };
                            let evt_type = val.get("type").and_then(|t| t.as_str()).unwrap_or_default();
                            match evt_type {
                                "response.output_text.delta" => {
                                    if let Some(delta) = val.get("delta").and_then(|d| d.as_str()) {
                                        let _ = event_sender.send(StreamEvent::TextDelta(delta.to_string())).await;
                                    }
                                }
                                "response.reasoning_summary_text.delta"
                                | "response.reasoning_text.delta" => {
                                    if let Some(delta) = val.get("delta").and_then(|d| d.as_str()) {
                                        let _ = event_sender.send(StreamEvent::ThoughtDelta(delta.to_string())).await;
                                    }
                                }
                                "response.output_item.done" => {
                                    if let Some(item) = val.get("item") {
                                        if item.get("type").and_then(|t| t.as_str()) == Some("function_call") {
                                            let call_id = item
                                                .get("call_id")
                                                .or_else(|| item.get("id"))
                                                .and_then(|i| i.as_str())
                                                .map(String::from);
                                            let name = item.get("name").and_then(|n| n.as_str()).map(String::from);
                                            let arguments = item.get("arguments").and_then(|a| a.as_str()).map(String::from);
                                            let _ = event_sender.send(StreamEvent::ToolCallDelta {
                                                index: tool_index,
                                                id: call_id,
                                                name,
                                                arguments_delta: arguments,
                                            }).await;
                                            tool_index += 1;
                                        }
                                    }
                                }
                                "response.failed" | "error" => {
                                    let msg = val
                                        .pointer("/response/error/message")
                                        .or_else(|| val.pointer("/error/message"))
                                        .and_then(|m| m.as_str())
                                        .unwrap_or("Codex response error");
                                    let _ = event_sender.send(StreamEvent::Error(msg.to_string())).await;
                                    break;
                                }
                                // The Responses API reports a budget-cut reply as
                                // an `incomplete` response instead of a normal
                                // one. Without surfacing it the runtime persists
                                // a mid-sentence answer as final and never runs
                                // the bounded continuation.
                                "response.incomplete" => {
                                    let _ = event_sender.send(StreamEvent::OutputTruncated).await;
                                    let _ = event_sender.send(StreamEvent::Done).await;
                                    break;
                                }
                                "response.completed" => {
                                    let status = val
                                        .pointer("/response/status")
                                        .and_then(|s| s.as_str());
                                    let reason = val
                                        .pointer("/response/incomplete_details/reason")
                                        .and_then(|s| s.as_str());
                                    if status == Some("incomplete")
                                        || matches!(reason, Some("max_output_tokens" | "max_tokens" | "length"))
                                    {
                                        let _ = event_sender.send(StreamEvent::OutputTruncated).await;
                                    }
                                    let _ = event_sender.send(StreamEvent::Done).await;
                                    break;
                                }
                                _ => {}
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
