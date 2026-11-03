use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};

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
use crate::trait_def::{ChatRequest, ModelProvider, ToolDefinition};

const MUSE_USER_AGENT: &str = "muse-code/1.0.2";

pub struct MuseProvider {
    client: reqwest::Client,
    base_url: String,
    access_token: String,
    // Set once Meta refuses a native tools field for this credential (a
    // Muse Code subscription key). From then on this provider runs the
    // tools over the text tool protocol below instead of asking Meta to
    // register them, so the refusal is paid once per provider, not on
    // every agent step.
    text_tools: AtomicBool,
    call_seq: AtomicU64,
}

impl MuseProvider {
    pub fn new(access_token: impl Into<String>, base_url: Option<&str>) -> Self {
        Self {
            client: reqwest::Client::new(),
            base_url: base_url.unwrap_or("https://api.meta.ai/v1").to_string(),
            access_token: access_token.into(),
            text_tools: AtomicBool::new(false),
            call_seq: AtomicU64::new(0),
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
                    let arguments = tc
                        .arguments
                        .as_object()
                        .map(|_| tc.arguments.to_string())
                        .unwrap_or_else(|| "{}".to_string());
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

/// Reports whether an error response is Meta's client-attestation
/// refusal, matched on the provider's own error code rather than prose
/// alone. Meta returns it (403) for any request that registers tools from
/// a Muse Code subscription key outside the official client.
fn is_attestation_denial(status: u16, body: &str) -> bool {
    if status != 403 {
        return false;
    }
    if let Ok(parsed) = serde_json::from_str::<Value>(body) {
        if parsed.pointer("/error/code").and_then(|v| v.as_str())
            == Some("client_attestation_denied")
        {
            return true;
        }
        let message = parsed
            .pointer("/error/message")
            .and_then(|v| v.as_str())
            .unwrap_or_default()
            .to_lowercase();
        if message.contains("attestation") || message.contains("muse code subscription") {
            return true;
        }
    }
    let lower = body.to_lowercase();
    lower.contains("client_attestation_denied") || lower.contains("attestation")
}

/// Builds the Responses body for the text tool protocol: the same input
/// shape as a plain request, with the tool catalogue and the calling
/// convention folded into the instructions and no tools field for Meta
/// to refuse. Stored tool calls and results render as `tool_call` and
/// `tool_result` blocks, so the history needs no function_call items
/// either.
fn build_text_tools_body(request: &ChatRequest) -> Value {
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
                let output = if is_blank_text(&msg.content.text) {
                    "(no output)".to_string()
                } else {
                    msg.content.text.clone()
                };
                let block = json!({ "call_id": call_id, "output": output }).to_string();
                input_items.push(json!({
                    "role": "user",
                    "content": format!("```tool_result\n{block}\n```"),
                }));
            }
            MessageRole::Assistant => {
                let mut parts: Vec<String> = Vec::new();
                if !is_blank_text(&msg.content.text) {
                    parts.push(msg.content.text.clone());
                }
                for tc in &msg.content.tool_calls {
                    if tc.tool_name.trim().is_empty() {
                        continue;
                    }
                    let arguments = if tc.arguments.is_null() {
                        json!({})
                    } else {
                        tc.arguments.clone()
                    };
                    let block = json!({ "name": tc.tool_name, "arguments": arguments }).to_string();
                    parts.push(format!("```tool_call\n{block}\n```"));
                }
                if parts.is_empty() {
                    continue;
                }
                input_items.push(json!({
                    "role": "assistant",
                    "content": parts.join("\n\n"),
                }));
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

    let mut instructions = request.system_prompt.clone().unwrap_or_default();
    if !request.tools.is_empty() {
        if !instructions.trim().is_empty() {
            instructions.push_str("\n\n");
        }
        instructions.push_str(&text_tool_protocol(&request.tools));
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
    if !instructions.trim().is_empty() {
        body["instructions"] = json!(instructions);
    }
    body
}

/// Teaches the model the text tool calling convention and lists the tools
/// Odon will execute for it.
fn text_tool_protocol(tools: &[ToolDefinition]) -> String {
    let mut out = String::new();
    out.push_str("Tool use (text protocol): the tools below are available, but they are not registered with this API request. Odon executes them on the operator's machine and returns their output to you.\n");
    out.push_str("To call a tool, end your reply with one block per call, in exactly this shape and with no text after the last block:\n");
    out.push_str("```tool_call\n{\"name\":\"tool_name\",\"arguments\":{\"arg\":value}}\n```\n");
    out.push_str("Tool outputs come back as tool_result blocks; read them and continue. When the task is finished, answer in plain text with no tool_call block.\n\nAvailable tools:\n");
    for tool in tools {
        out.push_str(&format!(
            "- {}: {}\n  arguments JSON schema: {}\n",
            tool.name, tool.description, tool.parameters
        ));
    }
    out
}

/// Splits a model reply into the prose around the `tool_call` blocks and
/// the calls themselves, as (name, arguments JSON) pairs. A block that
/// does not parse is not a call: the whole reply then stays prose, so a
/// model that talks about the protocol in a code sample is not mistaken
/// for one using it.
fn parse_text_tool_calls(text: &str) -> (String, Vec<(String, String)>) {
    const FENCE: &str = "```tool_call";
    let mut prose = String::new();
    let mut calls: Vec<(String, String)> = Vec::new();
    let mut rest = text;
    loop {
        let Some(start) = rest.find(FENCE) else {
            prose.push_str(rest);
            break;
        };
        let tail = &rest[start + FENCE.len()..];
        let Some(end) = tail.find("```") else {
            return (text.to_string(), Vec::new());
        };
        let raw = tail[..end].trim();
        let Ok(parsed) = serde_json::from_str::<Value>(raw) else {
            return (text.to_string(), Vec::new());
        };
        let Some(name) = parsed
            .get("name")
            .and_then(|v| v.as_str())
            .map(|s| s.trim())
            .filter(|n| !n.is_empty())
        else {
            return (text.to_string(), Vec::new());
        };
        let arguments = match parsed.get("arguments") {
            Some(Value::String(s)) => s.clone(),
            Some(v) => v.to_string(),
            None => "{}".to_string(),
        };
        if serde_json::from_str::<Value>(&arguments).is_err() {
            return (text.to_string(), Vec::new());
        }
        prose.push_str(&rest[..start]);
        calls.push((name.to_string(), arguments));
        rest = &tail[end + "```".len()..];
    }
    if calls.is_empty() {
        (text.to_string(), Vec::new())
    } else {
        (prose, calls)
    }
}

/// Truncates an error body for display, on a UTF-8 boundary: cutting at a
/// fixed byte offset can land inside a multi-byte character, and
/// `String::truncate` would panic there inside the provider task.
fn truncate_body(body: &str) -> String {
    if body.len() <= 2000 {
        return body.to_string();
    }
    let mut end = 2000;
    while end > 0 && !body.is_char_boundary(end) {
        end -= 1;
    }
    format!("{}…", &body[..end])
}

/// The HTTP failure every Muse path reports, policy refusals aside.
fn http_error(status: u16, model: &str, body: &str) -> OdonError {
    let trimmed = truncate_body(body.trim());
    OdonError::Provider {
        provider: "muse".to_string(),
        message: format!(
            "HTTP {} from Muse (model: {}).{}",
            status,
            model,
            if trimmed.is_empty() {
                String::new()
            } else {
                format!(" Response: {}", trimmed)
            }
        ),
    }
}

/// The terminal attestation error: it only surfaces when even a request
/// without a native tools field was refused, because a tools refusal on
/// its own switches the provider to the text tool protocol. The refusal
/// is policy, not a bad key, so the message names the next step.
fn attestation_error(model: &str, body: &str) -> OdonError {
    OdonError::Provider {
        provider: "muse".to_string(),
        message: format!(
            "HTTP 403 from Muse (model: {}): Meta requires the official Muse Code client's attestation for this Muse Code subscription key, and refused the request even without native tools. Use a pay-as-you-go Model API key from https://dev.meta.ai, or keep this key in the official Muse Code client. Response: {}",
            model,
            truncate_body(body.trim())
        ),
    }
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
    item.get("id")
        .and_then(|v| v.as_str())
        .map(String::from)
        .or_else(|| {
            item.get("call_id")
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
            let output_index = val
                .get("output_index")
                .and_then(|i| i.as_u64())
                .map(|i| i as usize);
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
            let output_index = val
                .get("output_index")
                .and_then(|i| i.as_u64())
                .map(|i| i as usize);
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
            let output_index = val
                .get("output_index")
                .and_then(|i| i.as_u64())
                .map(|i| i as usize);
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

/// The outcome of one Responses POST: a streaming success, or the status
/// and body of a failure (kept whole so the caller can tell a policy
/// refusal from an ordinary error before formatting either).
enum ResponsesSend {
    Success(reqwest::Response),
    Failure { status: u16, body: String },
}

impl MuseProvider {
    async fn post_responses(&self, body: &Value) -> Result<ResponsesSend> {
        let endpoint = format!("{}/responses", self.base_url.trim_end_matches('/'));
        let mut headers = HeaderMap::new();
        headers.insert(CONTENT_TYPE, HeaderValue::from_static("application/json"));
        headers.insert(
            AUTHORIZATION,
            HeaderValue::from_str(&format!("Bearer {}", self.access_token))
                .map_err(|e| OdonError::Auth(e.to_string()))?,
        );
        headers.insert(USER_AGENT, HeaderValue::from_static(MUSE_USER_AGENT));

        let resp = self
            .client
            .post(&endpoint)
            .headers(headers)
            .json(body)
            .send()
            .await
            .map_err(|e| OdonError::Provider {
                provider: "muse".to_string(),
                message: format!("Muse HTTP request failed: {}", e),
            })?;

        if resp.status().is_success() {
            return Ok(ResponsesSend::Success(resp));
        }
        let status = resp.status().as_u16();
        let body = resp.text().await.unwrap_or_default();
        Ok(ResponsesSend::Failure { status, body })
    }

    /// Runs one step on the text tool protocol after Meta has refused
    /// native tool use for this credential.
    async fn stream_text_tools(
        &self,
        request: ChatRequest,
        event_sender: Sender<StreamEvent>,
        cancellation: CancellationToken,
    ) -> Result<()> {
        let body = build_text_tools_body(&request);
        match self.post_responses(&body).await? {
            ResponsesSend::Success(resp) => {
                self.stream_text_response(resp, event_sender, cancellation)
                    .await
            }
            ResponsesSend::Failure { status, body } => {
                if is_attestation_denial(status, &body) {
                    Err(attestation_error(&request.model, &body))
                } else {
                    Err(http_error(status, &request.model, &body))
                }
            }
        }
    }

    /// Consumes a native Responses stream, forwarding text, thoughts and
    /// tool call deltas as they arrive.
    async fn stream_native_response(
        &self,
        resp: reqwest::Response,
        event_sender: Sender<StreamEvent>,
        cancellation: CancellationToken,
    ) -> Result<()> {
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

    /// Consumes a text-protocol Responses stream. Text is buffered rather
    /// than forwarded, because a `tool_call` block is only a tool call
    /// once its closing fence has arrived; thoughts still stream as they
    /// come. At the end the parsed calls go out as ordinary ToolCallDelta
    /// events, so the agent loop executes them locally exactly as it does
    /// for a native call.
    async fn stream_text_response(
        &self,
        resp: reqwest::Response,
        event_sender: Sender<StreamEvent>,
        cancellation: CancellationToken,
    ) -> Result<()> {
        let mut stream = resp.bytes_stream().eventsource();
        let mut acc = ResponsesAccumulator::default();
        let mut text = String::new();
        let mut done = false;

        while !done {
            tokio::select! {
                _ = cancellation.cancelled() => {
                    let _ = event_sender.send(StreamEvent::Done).await;
                    return Ok(());
                }
                item = stream.next() => {
                    match item {
                        Some(Ok(event)) => {
                            if event.data == "[DONE]" {
                                done = true;
                                continue;
                            }
                            let Ok(val) = serde_json::from_str::<Value>(&event.data) else { continue };
                            for parsed in parse_responses_event(&val, &mut acc) {
                                match parsed {
                                    ParsedEvent::Text(content) => text.push_str(&content),
                                    ParsedEvent::Thought(thought) => {
                                        let _ = event_sender.send(StreamEvent::ThoughtDelta(thought)).await;
                                    }
                                    ParsedEvent::ToolDelta { index, id, name, arguments_delta } => {
                                        let _ = event_sender.send(StreamEvent::ToolCallDelta {
                                            index, id, name, arguments_delta,
                                        }).await;
                                    }
                                    ParsedEvent::Done => done = true,
                                    ParsedEvent::Error(message) => {
                                        let _ = event_sender.send(StreamEvent::Error(message)).await;
                                        return Ok(());
                                    }
                                    ParsedEvent::Ignored => {}
                                }
                            }
                        }
                        Some(Err(e)) => {
                            let _ = event_sender.send(StreamEvent::Error(format!("Muse SSE error: {}", e))).await;
                            return Ok(());
                        }
                        None => done = true,
                    }
                }
            }
        }

        let (prose, calls) = parse_text_tool_calls(&text);
        if calls.is_empty() {
            if !text.is_empty() {
                let _ = event_sender.send(StreamEvent::TextDelta(text)).await;
            }
        } else {
            if !prose.trim().is_empty() {
                let _ = event_sender
                    .send(StreamEvent::TextDelta(prose.trim().to_string()))
                    .await;
            }
            for (index, (name, arguments)) in calls.iter().enumerate() {
                let seq = self.call_seq.fetch_add(1, Ordering::Relaxed) + 1;
                let _ = event_sender
                    .send(StreamEvent::ToolCallDelta {
                        index,
                        id: Some(format!("call_muse_{seq}")),
                        name: Some(name.clone()),
                        arguments_delta: Some(arguments.clone()),
                    })
                    .await;
            }
        }
        let _ = event_sender.send(StreamEvent::Done).await;
        Ok(())
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
        if self.text_tools.load(Ordering::Relaxed) {
            return self
                .stream_text_tools(request, event_sender, cancellation)
                .await;
        }
        let body = build_responses_body(&request);
        match self.post_responses(&body).await? {
            ResponsesSend::Success(resp) => {
                self.stream_native_response(resp, event_sender, cancellation)
                    .await
            }
            ResponsesSend::Failure { status, body } => {
                // A client-attestation refusal is a policy answer about
                // the native tools field, not a stale key: switch this
                // provider to the text tool protocol and run the step
                // that way instead of failing a login that works.
                if is_attestation_denial(status, &body) {
                    self.text_tools.store(true, Ordering::Relaxed);
                    return self
                        .stream_text_tools(request, event_sender, cancellation)
                        .await;
                }
                Err(http_error(status, &request.model, &body))
            }
        }
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
            [ParsedEvent::ToolDelta {
                arguments_delta: Some(_),
                ..
            }]
        ));
        let done: Value = serde_json::from_str(
            r#"{"type":"response.output_item.done","output_index":0,"item":{"type":"function_call","id":"fc1","call_id":"call_1","name":"fs_read","arguments":"{\"path\":\"x\"}"}}"#,
        )
        .unwrap();
        let events = parse_responses_event(&done, &mut acc);
        assert_eq!(events.len(), 1);
        match &events[0] {
            ParsedEvent::ToolDelta {
                arguments_delta, ..
            } => assert!(arguments_delta.is_none()),
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
            [ParsedEvent::ToolDelta {
                arguments_delta: Some(_),
                ..
            }]
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
        assert_eq!(body["input"][0]["output"], json!("(no output)"));
    }

    #[test]
    fn text_tools_body_registers_no_tools_and_renders_history_blocks() {
        use odon_core::ToolCallInfo;
        let mut assistant = text_message(MessageRole::Assistant, "");
        assistant.content.tool_calls = vec![ToolCallInfo {
            call_id: "call_1".to_string(),
            tool_name: "fs_read".to_string(),
            arguments: json!({"path": "a.rs"}),
            output: None,
            is_error: false,
            requires_approval: false,
            is_approved: None,
        }];
        let mut tool_result = text_message(MessageRole::Tool, "file contents");
        tool_result.id = "call_1".to_string();
        let request = ChatRequest {
            messages: vec![
                text_message(MessageRole::User, "read a.rs"),
                assistant,
                tool_result,
            ],
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
        let body = build_text_tools_body(&request);
        assert!(
            body.get("tools").is_none(),
            "the text protocol must not register a native tools field: {body}"
        );
        let instructions = body["instructions"].as_str().unwrap_or_default();
        assert!(instructions.contains("tool_call"), "{instructions}");
        assert!(instructions.contains("fs_read"), "{instructions}");
        let rendered = body["input"].to_string();
        assert!(rendered.contains("tool_call"), "{rendered}");
        assert!(rendered.contains("fs_read"), "{rendered}");
        assert!(rendered.contains("tool_result"), "{rendered}");
        assert!(rendered.contains("file contents"), "{rendered}");
    }

    #[test]
    fn parse_text_tool_calls_extracts_calls_and_keeps_prose() {
        let (prose, calls) = parse_text_tool_calls(
            "Checking.\n```tool_call\n{\"name\":\"bash_run\",\"arguments\":{\"command\":\"ls\"}}\n```",
        );
        assert_eq!(calls.len(), 1);
        assert_eq!(calls[0].0, "bash_run");
        assert_eq!(calls[0].1, "{\"command\":\"ls\"}");
        assert!(prose.contains("Checking."), "{prose}");
        assert!(!prose.contains("tool_call"), "{prose}");
    }

    #[test]
    fn parse_text_tool_calls_keeps_a_malformed_block_as_prose() {
        let text = "Example:\n```tool_call\nnot json\n```";
        let (prose, calls) = parse_text_tool_calls(text);
        assert!(calls.is_empty());
        assert_eq!(prose, text);
    }

    #[test]
    fn attestation_denial_is_matched_on_the_provider_code() {
        let body = r#"{"error":{"code":"client_attestation_denied","message":"This API key is billed to a Muse Code subscription.","type":"permission_error"}}"#;
        assert!(is_attestation_denial(403, body));
        assert!(!is_attestation_denial(401, body));
        assert!(!is_attestation_denial(
            403,
            r#"{"error":{"code":"model_not_found","message":"no"}}"#
        ));
    }

    // End-to-end at the provider seam: a fake Meta server refuses the
    // native request with client_attestation_denied and answers the
    // text-protocol retry with a tool_call block. The provider must emit
    // a real ToolCallDelta, and the next step must go straight to the
    // text protocol instead of paying the refusal again.
    #[tokio::test]
    async fn attestation_switches_to_text_tools_and_sticks() {
        use std::io::{Read, Write};
        use std::sync::{Arc, Mutex};

        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let addr = listener.local_addr().unwrap();
        let bodies: Arc<Mutex<Vec<String>>> = Arc::new(Mutex::new(Vec::new()));
        let seen = bodies.clone();
        let server = std::thread::spawn(move || {
            for _ in 0..3 {
                let Ok((mut stream, _)) = listener.accept() else {
                    return;
                };
                stream
                    .set_read_timeout(Some(std::time::Duration::from_secs(10)))
                    .unwrap();
                let mut raw: Vec<u8> = Vec::new();
                let mut chunk = [0u8; 4096];
                let header_len = loop {
                    match stream.read(&mut chunk) {
                        Ok(0) | Err(_) => return,
                        Ok(n) => {
                            raw.extend_from_slice(&chunk[..n]);
                            if let Some(pos) = find_subsequence(&raw, b"\r\n\r\n") {
                                break pos + 4;
                            }
                        }
                    }
                };
                let headers = String::from_utf8_lossy(&raw[..header_len]).to_string();
                let length: usize = headers
                    .lines()
                    .find_map(|line| {
                        line.to_ascii_lowercase()
                            .strip_prefix("content-length:")
                            .and_then(|v| v.trim().parse().ok())
                    })
                    .unwrap_or(0);
                while raw.len() < header_len + length {
                    match stream.read(&mut chunk) {
                        Ok(0) | Err(_) => return,
                        Ok(n) => raw.extend_from_slice(&chunk[..n]),
                    }
                }
                let body =
                    String::from_utf8_lossy(&raw[header_len..header_len + length]).to_string();
                let refused = body.contains("\"tools\":[");
                seen.lock().unwrap().push(body);
                let (status, payload) = if refused {
                    (
                        "403 Forbidden",
                        r#"{"error":{"code":"client_attestation_denied","message":"This API key is billed to a Muse Code subscription.","type":"permission_error"}}"#.to_string(),
                    )
                } else {
                    let delta = serde_json::to_string(
                        "```tool_call\n{\"name\":\"bash_run\",\"arguments\":{\"command\":\"ls\"}}\n```",
                    )
                    .unwrap();
                    (
                        "200 OK",
                        format!(
                            "data: {{\"type\":\"response.output_text.delta\",\"delta\":{delta}}}\n\ndata: {{\"type\":\"response.completed\"}}\n\n"
                        ),
                    )
                };
                let content_type = if refused {
                    "application/json"
                } else {
                    "text/event-stream"
                };
                let response = format!(
                    "HTTP/1.1 {status}\r\ncontent-type: {content_type}\r\ncontent-length: {}\r\nconnection: close\r\n\r\n{payload}",
                    payload.len()
                );
                let _ = stream.write_all(response.as_bytes());
            }
        });

        fn find_subsequence(haystack: &[u8], needle: &[u8]) -> Option<usize> {
            haystack
                .windows(needle.len())
                .position(|window| window == needle)
        }

        let base_url = format!("http://{addr}/v1");
        let provider = MuseProvider::new("key-subscription", Some(&base_url));
        let make_request = || ChatRequest {
            messages: vec![text_message(MessageRole::User, "list files")],
            model: "muse-spark-1.3".to_string(),
            system_prompt: Some("sys".to_string()),
            tools: vec![ToolDefinition {
                name: "bash_run".to_string(),
                description: "Run a command.".to_string(),
                parameters: json!({"type": "object"}),
            }],
            temperature: None,
            max_tokens: None,
            effort: None,
        };

        for step in 0..2 {
            let (tx, mut rx) = tokio::sync::mpsc::channel(64);
            provider
                .stream_chat(make_request(), tx, CancellationToken::new())
                .await
                .unwrap_or_else(|e| panic!("step {step} stream_chat: {e}"));
            let mut tool_events: Vec<(Option<String>, Option<String>, Option<String>)> = Vec::new();
            let mut done = false;
            while let Some(event) = rx.recv().await {
                match event {
                    StreamEvent::ToolCallDelta {
                        id,
                        name,
                        arguments_delta,
                        ..
                    } => tool_events.push((id, name, arguments_delta)),
                    StreamEvent::Done => {
                        done = true;
                        break;
                    }
                    StreamEvent::Error(message) => panic!("step {step} error event: {message}"),
                    _ => {}
                }
            }
            assert!(done, "step {step} must end with Done");
            assert_eq!(tool_events.len(), 1, "step {step} events: {tool_events:?}");
            assert_eq!(tool_events[0].1.as_deref(), Some("bash_run"));
            assert_eq!(tool_events[0].2.as_deref(), Some("{\"command\":\"ls\"}"));
            assert!(tool_events[0].0.is_some(), "the call needs an id");
        }

        server.join().unwrap();
        let bodies = bodies.lock().unwrap();
        assert_eq!(bodies.len(), 3, "bodies: {bodies:?}");
        assert!(bodies[0].contains("\"tools\":["), "{}", bodies[0]);
        assert!(!bodies[1].contains("\"tools\":["), "{}", bodies[1]);
        assert!(!bodies[2].contains("\"tools\":["), "{}", bodies[2]);
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
