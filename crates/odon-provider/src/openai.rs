use std::collections::HashSet;
use std::sync::atomic::Ordering;

use async_trait::async_trait;
use eventsource_stream::Eventsource;
use futures_util::StreamExt;
use odon_core::{MessageRole, OdonError, Result};
use reqwest::header::{HeaderMap, HeaderValue, AUTHORIZATION, CONTENT_TYPE, USER_AGENT};
use serde_json::json;
use tokio::sync::mpsc::Sender;
use tokio_util::sync::CancellationToken;

use crate::anthropic::AnthropicProvider;
use crate::copilot::CopilotModelInfo;
use crate::multimodal;
use crate::stream::StreamEvent;
use crate::trait_def::{ChatRequest, ModelProvider};

/// Users frequently paste the full endpoint (or a trailing slash) into the
/// "Base URL" field. Normalize it so we always append `/chat/completions` once.
fn normalize_base_url(raw: &str) -> String {
    let mut base = raw.trim().trim_end_matches('/').to_string();
    for suffix in ["/chat/completions", "/completions", "/responses"] {
        if let Some(stripped) = base.strip_suffix(suffix) {
            base = stripped.trim_end_matches('/').to_string();
            break;
        }
    }
    if base.is_empty() {
        "http://localhost:11434/v1".to_string()
    } else {
        base
    }
}

pub struct OpenAiCompatibleProvider {
    client: reqwest::Client,
    base_url: String,
    api_key: std::sync::Mutex<(String, std::time::Instant)>,
    refresh_lock: tokio::sync::Mutex<()>,
    provider_label: String,
    copilot_responses_models: std::sync::Mutex<HashSet<String>>,
    copilot_models_cache: tokio::sync::Mutex<Option<(std::time::Instant, Vec<CopilotModelInfo>)>>,
    session_id: String,
    request_seq: std::sync::atomic::AtomicU64,
}

/// GitHub Copilot session tokens live ~30 minutes; re-mint before they lapse.
const COPILOT_TOKEN_MAX_AGE: std::time::Duration = std::time::Duration::from_secs(20 * 60);
const COPILOT_CATALOG_MAX_AGE: std::time::Duration = std::time::Duration::from_secs(5 * 60);
const COPILOT_EDITOR_VERSION: &str = "vscode/1.110.0";
const COPILOT_PLUGIN_VERSION: &str = "copilot-chat/0.38.0";

fn copilot_escalate(err: &OdonError) -> bool {
    let message = err.to_string().to_ascii_lowercase();
    message.contains("not accessible via the /chat/completions endpoint")
        || message.contains("the requested model is not supported")
        || message.contains("modelnotsupported")
        || message.contains("model_not_supported")
}

/// GPT-6 and the newer Codex models expose tool calling through the Responses
/// API; Chat Completions cannot carry the agent's tools reliably.
fn openai_prefers_responses(model: &str) -> bool {
    let model = model.to_ascii_lowercase();
    model.starts_with("gpt-6")
        || model.starts_with("gpt-5.6")
        || model.starts_with("gpt-5.5")
        || model.starts_with("gpt-5.4")
        || model.contains("codex")
}

impl OpenAiCompatibleProvider {
    pub fn new(
        base_url: impl Into<String>,
        api_key: impl Into<String>,
        label: Option<&str>,
    ) -> Self {
        let client = reqwest::Client::builder()
            .connect_timeout(std::time::Duration::from_secs(20))
            .build()
            .unwrap_or_else(|_| reqwest::Client::new());
        let session_id = format!(
            "odon-{:x}",
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|duration| duration.as_nanos())
                .unwrap_or(0)
        );
        Self {
            client,
            base_url: normalize_base_url(&base_url.into()),
            api_key: std::sync::Mutex::new((api_key.into(), std::time::Instant::now())),
            refresh_lock: tokio::sync::Mutex::new(()),
            provider_label: label.unwrap_or("openai-compatible").to_string(),
            copilot_responses_models: std::sync::Mutex::new(HashSet::new()),
            copilot_models_cache: tokio::sync::Mutex::new(None),
            session_id,
            request_seq: std::sync::atomic::AtomicU64::new(0),
        }
    }

    /// Returns a usable bearer token, refreshing the Copilot session token when stale.
    async fn current_token(&self) -> String {
        let stale = |age: std::time::Duration| {
            self.provider_label == "github-copilot" && age > COPILOT_TOKEN_MAX_AGE
        };
        let read = || {
            let guard = self.api_key.lock().unwrap_or_else(|e| e.into_inner());
            (guard.0.clone(), guard.1.elapsed())
        };
        let (token, age) = read();
        if !stale(age) {
            return token;
        }
        // Serialise refreshes. Copilot session tokens rotate, so two concurrent
        // refreshes race and the loser can cache a token that is already dead.
        let _refresh = self.refresh_lock.lock().await;
        let (token, age) = read();
        if !stale(age) {
            return token;
        }
        if let Some(fresh) = crate::host_creds::discover_github_copilot().await {
            let mut guard = self.api_key.lock().unwrap_or_else(|e| e.into_inner());
            *guard = (fresh.access_token.clone(), std::time::Instant::now());
            return fresh.access_token;
        }
        token
    }

    /// Headers used by the VS Code Copilot Chat client. Copilot rejects some
    /// requests with `modelnotsupported` when these are missing or stale.
    fn copilot_headers(&self, token: &str) -> Result<HeaderMap> {
        let mut headers = HeaderMap::new();
        headers.insert(CONTENT_TYPE, HeaderValue::from_static("application/json"));
        headers.insert(
            AUTHORIZATION,
            HeaderValue::from_str(&format!("Bearer {token}"))
                .map_err(|e| OdonError::Auth(format!("Invalid Copilot token: {e}")))?,
        );
        headers.insert(
            "copilot-integration-id",
            HeaderValue::from_static("vscode-chat"),
        );
        headers.insert(
            "editor-version",
            HeaderValue::from_static(COPILOT_EDITOR_VERSION),
        );
        headers.insert(
            "editor-plugin-version",
            HeaderValue::from_static(COPILOT_PLUGIN_VERSION),
        );
        headers.insert(
            USER_AGENT,
            HeaderValue::from_static("GitHubCopilotChat/0.38.0"),
        );
        headers.insert(
            "openai-intent",
            HeaderValue::from_static("conversation-panel"),
        );
        headers.insert(
            "x-github-api-version",
            HeaderValue::from_static("2025-04-01"),
        );
        headers.insert(
            "x-vscode-user-agent-library-version",
            HeaderValue::from_static("electron-fetch"),
        );
        headers.insert("X-Initiator", HeaderValue::from_static("user"));
        let request_id = format!(
            "copilot-{}-{}",
            self.session_id,
            self.request_seq.fetch_add(1, Ordering::Relaxed)
        );
        headers.insert(
            "x-request-id",
            HeaderValue::from_str(&request_id).map_err(|e| OdonError::Provider {
                provider: self.provider_label.clone(),
                message: format!("Invalid Copilot request id: {e}"),
            })?,
        );
        headers.insert(
            "session_id",
            HeaderValue::from_str(&self.session_id).map_err(|e| OdonError::Provider {
                provider: self.provider_label.clone(),
                message: format!("Invalid Copilot session id: {e}"),
            })?,
        );
        Ok(headers)
    }

    async fn copilot_models(&self) -> Option<Vec<CopilotModelInfo>> {
        {
            let cache = self.copilot_models_cache.lock().await;
            if let Some((fetched_at, models)) = cache.as_ref() {
                if fetched_at.elapsed() < COPILOT_CATALOG_MAX_AGE {
                    return Some(models.clone());
                }
            }
        }
        let token = self.current_token().await;
        let models = crate::copilot::fetch_models_with_token(&token).await?;
        if models.is_empty() {
            return None;
        }
        *self.copilot_models_cache.lock().await = Some((std::time::Instant::now(), models.clone()));
        Some(models)
    }

    fn is_copilot_responses_model(&self, model: &str) -> bool {
        self.copilot_responses_models
            .lock()
            .map(|models| models.contains(model))
            .unwrap_or(false)
    }

    fn mark_copilot_responses(&self, model: &str) {
        if let Ok(mut models) = self.copilot_responses_models.lock() {
            models.insert(model.to_string());
        }
    }

    /// Route a Copilot request the way the working VS Code client does:
    /// Claude goes to `/v1/messages`, models that reject `/chat/completions`
    /// escalate to `/responses`, everything else uses `/chat/completions`.
    async fn stream_copilot(
        &self,
        mut request: ChatRequest,
        event_sender: Sender<StreamEvent>,
        cancellation: CancellationToken,
    ) -> Result<()> {
        let token = self.current_token().await;
        let requested = request
            .model
            .strip_prefix("copilot-")
            .unwrap_or(&request.model)
            .trim()
            .to_string();
        let models = self.copilot_models().await;

        let model = if requested == "odon-auto" || requested == "auto" || requested.is_empty() {
            models
                .as_ref()
                .and_then(|models| {
                    models
                        .iter()
                        .find(|model| model.id == "gpt-6-luna")
                        .or_else(|| models.first())
                })
                .map(|model| model.id.clone())
                .ok_or_else(|| OdonError::Provider {
                    provider: "github-copilot".to_string(),
                    message: "No GitHub Copilot model is selected. Choose one in the model picker."
                        .to_string(),
                })?
        } else {
            requested
        };

        if let Some(models) = models.as_ref() {
            if !models.iter().any(|available| available.id == model) {
                let available = models
                    .iter()
                    .take(8)
                    .map(|model| model.id.as_str())
                    .collect::<Vec<_>>()
                    .join(", ");
                return Err(OdonError::Provider {
                    provider: "github-copilot".to_string(),
                    message: format!(
                        "Model '{}' is not available for this GitHub Copilot account (disabled or retired). Pick another model in the picker. Available: {}",
                        model, available
                    ),
                });
            }
        }

        request.model = model.clone();
        let headers = self.copilot_headers(&token)?;
        let lower = model.to_ascii_lowercase();

        if lower.contains("claude") {
            return self
                .stream_copilot_messages(&token, request, event_sender, cancellation)
                .await;
        }
        if self.is_copilot_responses_model(&model) {
            return self
                .stream_copilot_responses(&headers, &request, event_sender, cancellation)
                .await;
        }

        let result = self
            .stream_copilot_chat(
                &headers,
                &model,
                &request,
                event_sender.clone(),
                cancellation.clone(),
            )
            .await;
        if let Err(err) = &result {
            if copilot_escalate(err) && !lower.contains("gemini") {
                self.mark_copilot_responses(&model);
                return self
                    .stream_copilot_responses(&headers, &request, event_sender, cancellation)
                    .await;
            }
        }
        result
    }

    async fn stream_copilot_messages(
        &self,
        token: &str,
        request: ChatRequest,
        event_sender: Sender<StreamEvent>,
        cancellation: CancellationToken,
    ) -> Result<()> {
        let headers = self.copilot_headers(token)?;
        let base_url = format!("{}/v1", self.base_url.trim_end_matches('/'));
        AnthropicProvider::with_bearer_auth(token.to_string(), base_url)
            .with_label("github-copilot")
            .with_extra_headers(headers)
            .with_default_max_tokens(8192)
            .stream_chat(request, event_sender, cancellation)
            .await
    }

    async fn stream_copilot_chat(
        &self,
        headers: &HeaderMap,
        model: &str,
        request: &ChatRequest,
        event_sender: Sender<StreamEvent>,
        cancellation: CancellationToken,
    ) -> Result<()> {
        let endpoint = format!("{}/chat/completions", self.base_url.trim_end_matches('/'));
        let mut messages_payload = Vec::new();
        if let Some(system) = &request.system_prompt {
            messages_payload.push(json!({ "role": "system", "content": system }));
        }
        for msg in &request.messages {
            match msg.role {
                MessageRole::User => {
                    messages_payload.push(json!({
                        "role": "user",
                        "content": multimodal::openai_content(&msg.content),
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
                        let calls: Vec<_> = msg
                            .content
                            .tool_calls
                            .iter()
                            .map(|call| {
                                json!({
                                    "id": call.call_id,
                                    "type": "function",
                                    "function": {
                                        "name": call.tool_name,
                                        "arguments": call.arguments.to_string(),
                                    }
                                })
                            })
                            .collect();
                        obj["tool_calls"] = json!(calls);
                    }
                    messages_payload.push(obj);
                }
                MessageRole::Tool => {
                    let call_id = msg
                        .content
                        .tool_calls
                        .first()
                        .map(|call| call.call_id.clone())
                        .unwrap_or_else(|| msg.id.clone());
                    messages_payload.push(json!({
                        "role": "tool",
                        "tool_call_id": call_id,
                        "content": msg.content.text,
                    }));
                }
            }
        }

        let mut body = json!({
            "model": model,
            "messages": messages_payload,
            "stream": true,
        });
        if let Some(temperature) = request.temperature {
            body["temperature"] = json!(temperature);
        }
        if !request.tools.is_empty() {
            let tools: Vec<_> = request
                .tools
                .iter()
                .map(|tool| {
                    json!({
                        "type": "function",
                        "function": {
                            "name": tool.name,
                            "description": tool.description,
                            "parameters": tool.parameters,
                        }
                    })
                })
                .collect();
            body["tools"] = json!(tools);
        }

        let response = self
            .client
            .post(&endpoint)
            .headers(headers.clone())
            .json(&body)
            .send()
            .await
            .map_err(|e| OdonError::Provider {
                provider: "github-copilot".to_string(),
                message: format!("Could not reach GitHub Copilot: {e}"),
            })?;

        if !response.status().is_success() {
            let status = response.status();
            let err_body =
                crate::truncate_str_boundary(&response.text().await.unwrap_or_default(), 2000);
            return Err(OdonError::Provider {
                provider: "github-copilot".to_string(),
                message: format!(
                    "HTTP {} from {} (model: {}). Response: {}",
                    status.as_u16(),
                    endpoint,
                    model,
                    err_body.trim()
                ),
            });
        }

        let mut stream = response.bytes_stream().eventsource();
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
                                if let Some(choices) = val.get("choices").and_then(|choices| choices.as_array()) {
                                    if let Some(first) = choices.first() {
                                        if let Some(reason) = first.get("finish_reason").and_then(|reason| reason.as_str()) {
                                            if matches!(reason, "length" | "max_tokens") {
                                                let _ = event_sender.send(StreamEvent::OutputTruncated).await;
                                            }
                                        }
                                        let delta = first.get("delta");
                                        if let Some(content) = delta.and_then(|delta| delta.get("content")).and_then(|content| content.as_str()) {
                                            let _ = event_sender.send(StreamEvent::TextDelta(content.to_string())).await;
                                        }
                                        if let Some(thought) = delta.and_then(|delta| delta.get("reasoning_content")).and_then(|content| content.as_str()) {
                                            let _ = event_sender.send(StreamEvent::ThoughtDelta(thought.to_string())).await;
                                        }
                                        if let Some(tool_calls) = delta.and_then(|delta| delta.get("tool_calls")).and_then(|calls| calls.as_array()) {
                                            for call in tool_calls {
                                                let index = call.get("index").and_then(|index| index.as_u64()).unwrap_or(0) as usize;
                                                let id = call.get("id").and_then(|id| id.as_str()).map(str::to_string);
                                                let function = call.get("function");
                                                let name = function.and_then(|function| function.get("name")).and_then(|name| name.as_str()).map(str::to_string);
                                                let arguments = function.and_then(|function| function.get("arguments")).and_then(|arguments| arguments.as_str()).map(str::to_string);
                                                let _ = event_sender.send(StreamEvent::ToolCallDelta {
                                                    index,
                                                    id,
                                                    name,
                                                    arguments_delta: arguments,
                                                }).await;
                                            }
                                        }
                                    }
                                }
                            }
                        }
                        Some(Err(e)) => {
                            let _ = event_sender.send(StreamEvent::Error(format!("SSE error: {e}"))).await;
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

    async fn stream_responses_request(
        &self,
        endpoint: &str,
        headers: &HeaderMap,
        request: &ChatRequest,
        event_sender: Sender<StreamEvent>,
        cancellation: CancellationToken,
    ) -> Result<()> {
        let mut input_items = Vec::new();
        for msg in &request.messages {
            match msg.role {
                MessageRole::Tool => {
                    let call_id = msg
                        .content
                        .tool_calls
                        .first()
                        .map(|call| call.call_id.clone())
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
                    for call in &msg.content.tool_calls {
                        input_items.push(json!({
                            "type": "function_call",
                            "call_id": call.call_id,
                            "name": call.tool_name,
                            "arguments": call.arguments.to_string(),
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
        });
        if let Some(system) = &request.system_prompt {
            body["instructions"] = json!(system);
        }
        if let Some(effort) = &request.effort {
            body["reasoning"] = json!({ "effort": effort, "summary": "auto" });
        }
        if !request.tools.is_empty() {
            let tools: Vec<_> = request
                .tools
                .iter()
                .map(|tool| {
                    json!({
                        "type": "function",
                        "name": tool.name,
                        "description": tool.description,
                        "parameters": tool.parameters,
                    })
                })
                .collect();
            body["tools"] = json!(tools);
            body["tool_choice"] = json!("auto");
        }

        let response = self
            .client
            .post(endpoint)
            .headers(headers.clone())
            .json(&body)
            .send()
            .await
            .map_err(|e| OdonError::Provider {
                provider: self.provider_label.clone(),
                message: format!("Could not reach {}: {e}", self.provider_label),
            })?;

        if !response.status().is_success() {
            let status = response.status();
            let err_body =
                crate::truncate_str_boundary(&response.text().await.unwrap_or_default(), 2000);
            return Err(OdonError::Provider {
                provider: self.provider_label.clone(),
                message: format!(
                    "HTTP {} from {} (model: {}). Response: {}",
                    status.as_u16(),
                    endpoint,
                    request.model,
                    err_body.trim()
                ),
            });
        }

        let mut stream = response.bytes_stream().eventsource();
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
                            let Ok(val) = serde_json::from_str::<serde_json::Value>(&event.data) else {
                                continue;
                            };
                            let event_type = val.get("type").and_then(|kind| kind.as_str()).unwrap_or_default();
                            match event_type {
                                "response.output_text.delta" => {
                                    if let Some(delta) = val.get("delta").and_then(|delta| delta.as_str()) {
                                        let _ = event_sender.send(StreamEvent::TextDelta(delta.to_string())).await;
                                    }
                                }
                                "response.reasoning_summary_text.delta"
                                | "response.reasoning_text.delta" => {
                                    if let Some(delta) = val.get("delta").and_then(|delta| delta.as_str()) {
                                        let _ = event_sender.send(StreamEvent::ThoughtDelta(delta.to_string())).await;
                                    }
                                }
                                "response.output_item.done" => {
                                    if let Some(item) = val.get("item") {
                                        if item.get("type").and_then(|kind| kind.as_str()) == Some("function_call") {
                                            let call_id = item
                                                .get("call_id")
                                                .or_else(|| item.get("id"))
                                                .and_then(|id| id.as_str())
                                                .map(str::to_string);
                                            let name = item.get("name").and_then(|name| name.as_str()).map(str::to_string);
                                            let arguments = item.get("arguments").and_then(|arguments| arguments.as_str()).map(str::to_string);
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
                                    let message = val
                                        .pointer("/response/error/message")
                                        .or_else(|| val.pointer("/error/message"))
                                        .and_then(|message| message.as_str())
                                        .unwrap_or("Responses API error");
                                    let _ = event_sender.send(StreamEvent::Error(message.to_string())).await;
                                    break;
                                }
                                "response.completed" => {
                                    let _ = event_sender.send(StreamEvent::Done).await;
                                    break;
                                }
                                _ => {}
                            }
                        }
                        Some(Err(e)) => {
                            let _ = event_sender.send(StreamEvent::Error(format!("SSE error: {e}"))).await;
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

    async fn stream_copilot_responses(
        &self,
        headers: &HeaderMap,
        request: &ChatRequest,
        event_sender: Sender<StreamEvent>,
        cancellation: CancellationToken,
    ) -> Result<()> {
        let endpoint = format!("{}/responses", self.base_url.trim_end_matches('/'));
        self.stream_responses_request(&endpoint, headers, request, event_sender, cancellation)
            .await
    }

    pub fn new_deepseek(api_key: impl Into<String>) -> Self {
        Self::new("https://api.deepseek.com/v1", api_key, Some("deepseek"))
    }

    pub fn new_qwen(api_key: impl Into<String>) -> Self {
        Self::new(
            "https://dashscope-intl.aliyuncs.com/compatible-mode/v1",
            api_key,
            Some("qwen"),
        )
    }

    pub fn new_alibaba(api_key: impl Into<String>) -> Self {
        Self::new(
            "https://dashscope.aliyuncs.com/compatible-mode/v1",
            api_key,
            Some("alibaba"),
        )
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
        if self.provider_label == "github-copilot" {
            return self
                .stream_copilot(request, event_sender, cancellation)
                .await;
        }

        if self.provider_label == "openai" && openai_prefers_responses(&request.model) {
            let token = self.current_token().await;
            let mut headers = HeaderMap::new();
            headers.insert(CONTENT_TYPE, HeaderValue::from_static("application/json"));
            if !token.is_empty() {
                if let Ok(auth) = HeaderValue::from_str(&format!("Bearer {token}")) {
                    headers.insert(AUTHORIZATION, auth);
                }
            }
            let endpoint = format!("{}/responses", self.base_url.trim_end_matches('/'));
            return self
                .stream_responses_request(&endpoint, &headers, &request, event_sender, cancellation)
                .await;
        }

        let endpoint = format!("{}/chat/completions", self.base_url.trim_end_matches('/'));

        let mut headers = HeaderMap::new();
        headers.insert(CONTENT_TYPE, HeaderValue::from_static("application/json"));
        let bearer = self.current_token().await;
        if !bearer.is_empty() {
            let auth_val = format!("Bearer {}", bearer);
            if let Ok(val) = HeaderValue::from_str(&auth_val) {
                headers.insert(AUTHORIZATION, val);
            }
        }

        if self.provider_label == "github-copilot" {
            headers.insert("Editor-Version", HeaderValue::from_static("vscode/1.90.0"));
            headers.insert(
                "Editor-Plugin-Version",
                HeaderValue::from_static("copilot-chat/0.38.0"),
            );
            headers.insert(
                USER_AGENT,
                HeaderValue::from_static("GitHubCopilotChat/0.38.0"),
            );
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
                        "content": multimodal::openai_content(&msg.content),
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
                        let tcs: Vec<_> = msg
                            .content
                            .tool_calls
                            .iter()
                            .map(|tc| {
                                json!({
                                    "id": tc.call_id,
                                    "type": "function",
                                    "function": {
                                        "name": tc.tool_name,
                                        "arguments": tc.arguments.to_string(),
                                    }
                                })
                            })
                            .collect();
                        obj["tool_calls"] = json!(tcs);
                    }
                    messages_payload.push(obj);
                }
                MessageRole::Tool => {
                    // The tool result must reference the assistant's tool_call id,
                    // which Message::new_tool stores in tool_calls[0].call_id.
                    let call_id = msg
                        .content
                        .tool_calls
                        .first()
                        .map(|t| t.call_id.clone())
                        .unwrap_or_else(|| msg.id.clone());
                    messages_payload.push(json!({
                        "role": "tool",
                        "tool_call_id": call_id,
                        "content": msg.content.text,
                    }));
                }
            }
        }

        let model_str = if request.model == "odon-auto"
            || request.model == "auto"
            || request.model.is_empty()
        {
            match self.provider_label.as_str() {
                "deepseek" => "deepseek-chat",
                "qwen" => "qwen-max-latest",
                "mistral" => "mistral-large-latest",
                "openai" => "gpt-4o",
                // A custom gateway has no meaningful default: sending a made-up
                // model id is what turns into opaque 404/502 upstream errors.
                "openai-compatible" | "openai_compatible" | "ollama" => {
                    return Err(OdonError::Provider {
                        provider: self.provider_label.clone(),
                        message: "No model selected. Add a model ID under Settings → Providers → OpenAI Compatible (Custom / Gateway), then select it in the model menu.".to_string(),
                    });
                }
                _ => "gpt-4o",
            }
        } else if self.provider_label == "github-copilot" {
            request
                .model
                .strip_prefix("copilot-")
                .unwrap_or(&request.model)
        } else {
            &request.model
        };

        if model_str.trim().is_empty() {
            return Err(OdonError::Provider {
                provider: self.provider_label.clone(),
                message: "The configured model ID is empty. Choose a model in the model menu."
                    .to_string(),
            });
        }

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

        // Gateways frequently answer 502/503/504 (or 429) while an upstream
        // model is cold-starting or rate limited. Retry briefly before surfacing
        // the error so transient provider hiccups do not break a chat turn.
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
                            "{} returned HTTP {} — retry {}/2",
                            self.provider_label,
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
                    tracing::warn!(
                        "{} request failed ({}): retry {}/2",
                        self.provider_label,
                        e,
                        attempt
                    );
                    tokio::time::sleep(std::time::Duration::from_millis(600 * attempt as u64))
                        .await;
                }
                Err(e) => {
                    return Err(OdonError::Provider {
                        provider: self.provider_label.clone(),
                        message: format!(
                            "Could not reach {} at {}. Check the Base URL and that the gateway is running. ({})",
                            self.provider_label, self.base_url, e
                        ),
                    });
                }
            }
        };

        if !resp.status().is_success() {
            let status = resp.status();
            let err_body =
                crate::truncate_str_boundary(&resp.text().await.unwrap_or_default(), 2000);
            let hint = match status.as_u16() {
                401 | 403 => " Check the API key for this provider.",
                404 => " Check the Base URL and model ID.",
                429 => " The provider is rate limiting requests; retry shortly.",
                502..=504 => {
                    " The upstream gateway/model is unavailable. Verify the model ID and that the gateway can reach the upstream provider."
                }
                _ => "",
            };
            let detail = if err_body.trim().is_empty() {
                String::new()
            } else {
                format!(" Response: {}", err_body.trim())
            };
            return Err(OdonError::Provider {
                provider: self.provider_label.clone(),
                message: format!(
                    "HTTP {} from {} (model: {}).{}{}",
                    status.as_u16(),
                    endpoint,
                    model_str,
                    detail,
                    hint
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
                            if let Ok(val) = serde_json::from_str::<serde_json::Value>(&event.data) {
                                if let Some(choices) = val.get("choices").and_then(|c| c.as_array()) {
                                    if let Some(first) = choices.first() {
                                        if let Some(reason) = first.get("finish_reason").and_then(|r| r.as_str()) {
                                            if matches!(reason, "length" | "max_tokens") {
                                                let _ = event_sender.send(StreamEvent::OutputTruncated).await;
                                            }
                                        }
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn copilot_escalates_only_on_endpoint_or_model_errors() {
        let unsupported = OdonError::Provider {
            provider: "github-copilot".to_string(),
            message: "HTTP 400 from https://api.githubcopilot.com/chat/completions. Response: {\"error\":{\"code\":\"modelnotsupported\"}}".to_string(),
        };
        assert!(copilot_escalate(&unsupported));

        let endpoint = OdonError::Provider {
            provider: "github-copilot".to_string(),
            message: "The requested model is not accessible via the /chat/completions endpoint."
                .to_string(),
        };
        assert!(copilot_escalate(&endpoint));

        let auth = OdonError::Provider {
            provider: "github-copilot".to_string(),
            message: "HTTP 401 unauthorized".to_string(),
        };
        assert!(!copilot_escalate(&auth));
    }

    #[test]
    fn openai_gpt6_uses_responses() {
        assert!(openai_prefers_responses("gpt-6-astra"));
        assert!(openai_prefers_responses("gpt-6.1-sol"));
        assert!(openai_prefers_responses("gpt-5.6-terra"));
        assert!(openai_prefers_responses("gpt-5.3-codex"));
        assert!(!openai_prefers_responses("gpt-4o"));
    }

    #[test]
    fn copilot_headers_match_the_vscode_client() {
        let provider = OpenAiCompatibleProvider::new(
            "https://api.githubcopilot.com",
            "test-token",
            Some("github-copilot"),
        );
        let headers = provider.copilot_headers("test-token").unwrap();
        let value = |name: &str| headers.get(name).unwrap().to_str().unwrap().to_string();

        assert_eq!(value("authorization"), "Bearer test-token");
        assert_eq!(value("copilot-integration-id"), "vscode-chat");
        assert_eq!(value("editor-version"), "vscode/1.110.0");
        assert_eq!(value("editor-plugin-version"), "copilot-chat/0.38.0");
        assert_eq!(value("user-agent"), "GitHubCopilotChat/0.38.0");
        assert_eq!(value("openai-intent"), "conversation-panel");
        assert_eq!(value("x-github-api-version"), "2025-04-01");
        assert_eq!(
            value("x-vscode-user-agent-library-version"),
            "electron-fetch"
        );
        assert_eq!(value("x-initiator"), "user");
        assert!(!value("session_id").is_empty());
        assert!(value("x-request-id").starts_with("copilot-"));
    }
}
