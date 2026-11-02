use std::collections::HashMap;

use async_trait::async_trait;
use eventsource_stream::Eventsource;
use futures_util::StreamExt;
use odon_core::{MessageRole, OdonError, Result};
use reqwest::header::{HeaderMap, HeaderValue, AUTHORIZATION, CONTENT_TYPE, USER_AGENT};
use serde_json::{json, Value};
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
            base_url: base_url.unwrap_or("https://api.meta.ai/v1").to_string(),
            access_token: access_token.into(),
        }
    }
}

/// The Meta Model API serves `muse-spark-*` on the Responses API
/// (`POST /v1/responses`), not on `/v1/chat/completions`: the subscription key
/// minted by the Muse device flow is rejected on the chat endpoint while the
/// other OAuth providers in this crate keep their own transports untouched.
fn is_blank_text(text: &str) -> bool {
    text.trim().is_empty()
}

fn build_responses_body(request: &ChatRequest) -> Value {
    // Responses API input items. Tool calls/results must use the dedicated
    // function_call / function_call_output item types, not chat roles.
    //
    // The API rejects role items with empty content (`assistant message must
    // have content or toolcalls`), and stored history can contain blank
    // assistant/system turns, so blank items are skipped and every
    // function_call is guaranteed a non-empty call_id/name/arguments.
    let mut input_items = Vec::new();
    let mut anonymous_calls = 0usize;
    for msg in &request.messages {
        match msg.role {
            MessageRole::Tool => {
                let call_id = msg
                    .content
                    .tool_calls
                    .first()
                    .map(|t| t.call_id.clone())
                    .unwrap_or_else(|| msg.id.clone());
                let output = if is_blank_text(&msg.content.text) {
                    "(no output)".to_string()
                } else {
                    msg.content.text.clone()
                };
                input_items.push(json!({
                    "type": "function_call_output",
                    "call_id": call_id,
                    "output": output,
                }));
            }
            MessageRole::Assistant => {
                if !is_blank_text(&msg.content.text) {
                    input_items.push(json!({
                        "role": "assistant",
                        "content": msg.content.text,
                    }));
                }
                for tc in &msg.content.tool_calls {
                    let call_id = if tc.call_id.trim().is_empty() {
                        anonymous_calls += 1;
                        format!("call_anon_{anonymous_calls}")
                    } else {
                        tc.call_id.clone()
                    };
                    let name = if tc.tool_name.trim().is_empty() {
                        "unknown_tool".to_string()
                    } else {
                        tc.tool_name.clone()
                    };
                    let arguments = tc.arguments.as_object().map(|_| tc.arguments.to_string()).unwrap_or_else(|| "{}".to_string());
                    input_items.push(json!({
                        "type": "function_call",
                        "call_id": call_id,
                        "name": name,
                        "arguments": arguments,
                    }));
                }
            }
            MessageRole::User => {
                let content = multimodal::responses_content(&msg.content);
                let empty = content.as_str().map(str::is_empty).unwrap_or(false)
                    || content.as_array().map(|a| a.is_empty()).unwrap_or(false);
                if empty {
                    continue;
                }
                input_items.push(json!({
                    "role": "user",
                    "content": content,
                }));
            }
            MessageRole::System => {
                if is_blank_text(&msg.content.text) {
                    continue;
                }
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
            "effort": request.effort.as_deref().unwrap_or("medium")
        }
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

    if let Some(sys) = request.system_prompt.clone() {
        body["instructions"] = json!(sys);
    }

    body
}

/// One parsed Responses SSE event. Kept separate from [`StreamEvent`] so the
/// mapping below stays unit-testable without a channel.
#[derive(Debug, PartialEq)]
enum ParsedEvent {
    Text(String),
    Thought(String),
    ToolDelta {
        index: usize,
        id: Option<String>,
        name: Option<String>,
        arguments_delta: Option<String>,
    },
    Done,
    Error(String),
    Ignored,
}

/// Tracks tool-call indexes and which items already streamed their arguments,
/// so a final `output_item.done` never duplicates streamed argument chunks.
#[derive(Default)]
struct ResponsesAccumulator {
    next_index: usize,
    index_by_item: HashMap<String, usize>,
    streamed_args: HashMap<String, bool>,
}

impl ResponsesAccumulator {
    fn index_for(&mut self, key: &str, output_index: Option<usize>) -> usize {
        if let Some(i) = output_index {
            self.index_by_item.insert(key.to_string(), i);
            return i;
        }
        if let Some(&i) = self.index_by_item.get(key) {
            return i;
        }
        let i = self.next_index;
        self.next_index += 1;
        self.index_by_item.insert(key.to_string(), i);
        i
    }
}

fn item_key(item: &Value) -> Option<String> {
    item
        .get("id")
        .and_then(|v| v.as_str())
        .map(String::from)
        .or_else(|| {
            item
                .get("call_id")
                .and_then(|v| v.as_str())
                .map(String::from)
        })
}

fn parse_responses_event(val: &Value, acc: &mut ResponsesAccumulator) -> Vec<ParsedEvent> {
    let evt_type = val.get("type").and_then(|t| t.as_str()).unwrap_or_default();
    match evt_type {
        "response.output_text.delta" => match val.get("delta").and_then(|d| d.as_str()) {
            Some(delta) => vec![ParsedEvent::Text(delta.to_string())],
            None => vec![ParsedEvent::Ignored],
        },
        "response.reasoning_summary_text.delta" | "response.reasoning_text.delta" => {
            match val.get("delta").and_then(|d| d.as_str()) {
                Some(delta) => vec![ParsedEvent::Thought(delta.to_string())],
                None => vec![ParsedEvent::Ignored],
            }
        }
        "response.output_item.added" => {
            let Some(item) = val.get("item") else {
                return vec![ParsedEvent::Ignored];
            };
            if item.get("type").and_then(|t| t.as_str()) != Some("function_call") {
                return vec![ParsedEvent::Ignored];
            }
            let key = item_key(item).unwrap_or_else(|| format!("item-{}", acc.next_index));
            let output_index = val.get("output_index").and_then(|i| i.as_u64()).map(|i| i as usize);
            let index = acc.index_for(&key, output_index);
            acc.streamed_args.entry(key).or_insert(false);
            vec![ParsedEvent::ToolDelta {
                index,
                id: item
                    .get("call_id")
                    .and_then(|i| i.as_str())
                    .map(String::from),
                name: item.get("name").and_then(|n| n.as_str()).map(String::from),
                arguments_delta: None,
            }]
        }
        "response.function_call_arguments.delta" => {
            let delta = match val.get("delta").and_then(|d| d.as_str()) {
                Some(d) => d.to_string(),
                None => return vec![ParsedEvent::Ignored],
            };
            let key = val
                .get("item_id")
                .and_then(|v| v.as_str())
                .map(String::from)
                .unwrap_or_else(|| format!("item-{}", acc.next_index));
            let output_index = val.get("output_index").and_then(|i| i.as_u64()).map(|i| i as usize);
            let index = acc.index_for(&key, output_index);
            acc.streamed_args.insert(key, true);
            vec![ParsedEvent::ToolDelta {
                index,
                id: None,
                name: None,
                arguments_delta: Some(delta),
            }]
        }
        "response.output_item.done" => {
            let Some(item) = val.get("item") else {
                return vec![ParsedEvent::Ignored];
            };
            if item.get("type").and_then(|t| t.as_str()) != Some("function_call") {
                return vec![ParsedEvent::Ignored];
            }
            let key = item_key(item).unwrap_or_else(|| format!("item-{}", acc.next_index));
            let output_index = val.get("output_index").and_then(|i| i.as_u64()).map(|i| i as usize);
            let index = acc.index_for(&key, output_index);
            let already_streamed = acc.streamed_args.get(&key).copied().unwrap_or(false);
            if already_streamed {
                // Arguments arrived as deltas; only confirm identity, never
                // re-send arguments or the runtime would duplicate them.
                return vec![ParsedEvent::ToolDelta {
                    index,
                    id: item
                        .get("call_id")
                        .and_then(|i| i.as_str())
                        .map(String::from),
                    name: item.get("name").and_then(|n| n.as_str()).map(String::from),
                    arguments_delta: None,
                }];
            }
            acc.streamed_args.insert(key, true);
            vec![ParsedEvent::ToolDelta {
                index,
                id: item
                    .get("call_id")
                    .and_then(|i| i.as_str())
                    .map(String::from),
                name: item.get("name").and_then(|n| n.as_str()).map(String::from),
                arguments_delta: item
                    .get("arguments")
                    .and_then(|a| a.as_str())
                    .map(String::from),
            }]
        }
        "response.failed" | "error" => {
            let msg = val
                .pointer("/response/error/message")
                .or_else(|| val.pointer("/error/message"))
                .and_then(|m| m.as_str())
                .unwrap_or("Muse response error");
            vec![ParsedEvent::Error(msg.to_string())]
        }
        "response.completed" => vec![ParsedEvent::Done],
        _ => vec![ParsedEvent::Ignored],
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
        let endpoint = format!("{}/responses", self.base_url.trim_end_matches('/'));

        let mut headers = HeaderMap::new();
        headers.insert(CONTENT_TYPE, HeaderValue::from_static("application/json"));
        headers.insert(
            AUTHORIZATION,
            HeaderValue::from_str(&format!("Bearer {}", self.access_token))
                .map_err(|e| OdonError::Auth(e.to_string()))?,
        );
        headers.insert(USER_AGENT, HeaderValue::from_static(MUSE_USER_AGENT));

        let body = build_responses_body(&request);

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
                err_body.push('…');
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
        let mut acc = ResponsesAccumulator::default();

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
                            let Ok(val) = serde_json::from_str::<Value>(&event.data) else { continue };
                            let mut finished = false;
                            let mut failed = false;
                            for parsed in parse_responses_event(&val, &mut acc) {
                                match parsed {
                                    ParsedEvent::Text(content) => {
                                        let _ = event_sender.send(StreamEvent::TextDelta(content)).await;
                                    }
                                    ParsedEvent::Thought(thought) => {
                                        let _ = event_sender.send(StreamEvent::ThoughtDelta(thought)).await;
                                    }
                                    ParsedEvent::ToolDelta { index, id, name, arguments_delta } => {
                                        let _ = event_sender.send(StreamEvent::ToolCallDelta {
                                            index, id, name, arguments_delta,
                                        }).await;
                                    }
                                    ParsedEvent::Done => {
                                        let _ = event_sender.send(StreamEvent::Done).await;
                                        finished = true;
                                    }
                                    ParsedEvent::Error(message) => {
                                        let _ = event_sender.send(StreamEvent::Error(message)).await;
                                        failed = true;
                                    }
                                    ParsedEvent::Ignored => {}
                                }
                            }
                            if finished || failed {
                                break;
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::trait_def::ToolDefinition;
    use odon_core::{Message, MessageContent, MessageRole};

    fn text_message(role: MessageRole, text: &str) -> Message {
        Message {
            id: "m1".to_string(),
            session_id: "s1".to_string(),
            role,
            content: MessageContent {
                text: text.to_string(),
                thought: None,
                tool_calls: vec![],
                artifact_ids: vec![],
                action_chips: vec![],
                attachments: vec![],
            },
            model: None,
            created_at: chrono::Utc::now(),
        }
    }

    #[test]
    fn responses_body_uses_flat_tools_and_function_items() {
        let request = ChatRequest {
            messages: vec![text_message(MessageRole::User, "halo")],
            model: "muse-spark-1.3".to_string(),
            system_prompt: Some("sys".to_string()),
            tools: vec![ToolDefinition {
                name: "fs_read".to_string(),
                description: "read".to_string(),
                parameters: json!({"type": "object"}),
            }],
            temperature: None,
            max_tokens: None,
            effort: None,
        };
        let body = build_responses_body(&request);
        assert_eq!(body["model"], json!("muse-spark-1.3"));
        assert_eq!(body["stream"], json!(true));
        assert_eq!(body["store"], json!(false));
        assert_eq!(body["instructions"], json!("sys"));
        assert_eq!(body["tools"][0]["type"], json!("function"));
        assert_eq!(body["tools"][0]["name"], json!("fs_read"));
        assert_eq!(body["tool_choice"], json!("auto"));
        assert_eq!(body["input"][0]["role"], json!("user"));
    }

    #[test]
    fn responses_stream_text_and_completion_end_the_turn() {
        let mut acc = ResponsesAccumulator::default();
        let text: Value =
            serde_json::from_str(r#"{"type":"response.output_text.delta","delta":"hai"}"#).unwrap();
        assert_eq!(
            parse_responses_event(&text, &mut acc),
            vec![ParsedEvent::Text("hai".to_string())]
        );
        let done: Value = serde_json::from_str(r#"{"type":"response.completed"}"#).unwrap();
        assert_eq!(
            parse_responses_event(&done, &mut acc),
            vec![ParsedEvent::Done]
        );
    }

    #[test]
    fn streamed_tool_arguments_are_not_duplicated_by_done() {
        let mut acc = ResponsesAccumulator::default();
        let added: Value = serde_json::from_str(
            r#"{"type":"response.output_item.added","output_index":0,"item":{"type":"function_call","id":"fc1","call_id":"call_1","name":"fs_read"}}"#,
        )
        .unwrap();
        let events = parse_responses_event(&added, &mut acc);
        assert_eq!(events.len(), 1);
        let delta: Value = serde_json::from_str(
            r#"{"type":"response.function_call_arguments.delta","item_id":"fc1","output_index":0,"delta":"{\"path\":"}"#,
        )
        .unwrap();
        let events = parse_responses_event(&delta, &mut acc);
        assert!(matches!(
            events.as_slice(),
            [ParsedEvent::ToolDelta { arguments_delta: Some(_), .. }]
        ));
        let done: Value = serde_json::from_str(
            r#"{"type":"response.output_item.done","output_index":0,"item":{"type":"function_call","id":"fc1","call_id":"call_1","name":"fs_read","arguments":"{\"path\":\"x\"}"}}"#,
        )
        .unwrap();
        let events = parse_responses_event(&done, &mut acc);
        assert_eq!(events.len(), 1);
        match &events[0] {
            ParsedEvent::ToolDelta { arguments_delta, .. } => assert!(arguments_delta.is_none()),
            other => panic!("expected ToolDelta without arguments, got {other:?}"),
        }
    }

    #[test]
    fn non_streamed_done_still_carries_full_arguments() {
        let mut acc = ResponsesAccumulator::default();
        let done: Value = serde_json::from_str(
            r#"{"type":"response.output_item.done","output_index":0,"item":{"type":"function_call","id":"fc9","call_id":"call_9","name":"fs_list","arguments":"{}"}}"#,
        )
        .unwrap();
        let events = parse_responses_event(&done, &mut acc);
        assert!(matches!(
            events.as_slice(),
            [ParsedEvent::ToolDelta { arguments_delta: Some(_), .. }]
        ));
    }

    #[test]
    fn blank_history_items_are_never_sent() {
        use odon_core::ToolCallInfo;
        let mut assistant_blank = text_message(MessageRole::Assistant, "   ");
        assistant_blank.content.tool_calls = vec![ToolCallInfo {
            call_id: "".to_string(),
            tool_name: "".to_string(),
            arguments: serde_json::Value::Null,
            output: None,
            is_error: false,
            requires_approval: false,
            is_approved: None,
        }];
        let request = ChatRequest {
            messages: vec![
                assistant_blank,
                text_message(MessageRole::System, ""),
                text_message(MessageRole::User, ""),
            ],
            model: "muse-spark-1.3".to_string(),
            system_prompt: None,
            tools: vec![],
            temperature: None,
            max_tokens: None,
            effort: None,
        };
        let body = build_responses_body(&request);
        let input = body["input"].as_array().unwrap();
        // Only the repaired function_call survives: no blank assistant,
        // system, or user items that the API would reject.
        assert_eq!(input.len(), 1);
        assert_eq!(input[0]["type"], json!("function_call"));
        assert_eq!(input[0]["call_id"], json!("call_anon_1"));
        assert_eq!(input[0]["name"], json!("unknown_tool"));
        assert_eq!(input[0]["arguments"], json!("{}"));
    }

    #[test]
    fn blank_tool_output_gets_a_placeholder() {
        let request = ChatRequest {
            messages: vec![Message {
                id: "t1".to_string(),
                session_id: "s1".to_string(),
                role: MessageRole::Tool,
                content: odon_core::MessageContent {
                    text: "".to_string(),
                    thought: None,
                    tool_calls: vec![],
                    artifact_ids: vec![],
                    action_chips: vec![],
                    attachments: vec![],
                },
                model: None,
                created_at: chrono::Utc::now(),
            }],
            model: "muse-spark-1.3".to_string(),
            system_prompt: None,
            tools: vec![],
            temperature: None,
            max_tokens: None,
            effort: None,
        };
        let body = build_responses_body(&request);
        assert_eq!(
            body["input"][0]["output"],
            json!("(no output)")
        );
    }

    #[test]
    fn failed_responses_become_errors() {
        let mut acc = ResponsesAccumulator::default();
        let failed: Value = serde_json::from_str(
            r#"{"type":"response.failed","response":{"error":{"message":"boom"}}}"#,
        )
        .unwrap();
        assert_eq!(
            parse_responses_event(&failed, &mut acc),
            vec![ParsedEvent::Error("boom".to_string())]
        );
    }
}
