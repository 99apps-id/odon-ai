//! Google Antigravity / Gemini Code Assist provider.
//!
//! Speaks the Cloud Code Assist `v1internal` protocol that the Antigravity and
//! Gemini CLI clients use with a Google OAuth bearer token:
//!   1. `:loadCodeAssist` resolves the companion project for the account.
//!   2. `:streamGenerateContent?alt=sse` streams Gemini candidates (text,
//!      thoughts and native function calls).

use async_trait::async_trait;
use eventsource_stream::Eventsource;
use futures_util::StreamExt;
use odon_core::{MessageRole, OdonError, Result};
use reqwest::header::{HeaderMap, HeaderValue, AUTHORIZATION, CONTENT_TYPE, USER_AGENT};
use serde_json::{json, Value};
use tokio::sync::mpsc::Sender;
use tokio_util::sync::CancellationToken;

use crate::stream::StreamEvent;
use crate::trait_def::{ChatRequest, ModelProvider};

const CODE_ASSIST_HOST: &str = "https://cloudcode-pa.googleapis.com";
/// Google access tokens last ~1h; refresh a little early.
const TOKEN_MAX_AGE: std::time::Duration = std::time::Duration::from_secs(45 * 60);

pub struct AntigravityProvider {
    client: reqwest::Client,
    base_url: String,
    access_token: std::sync::Mutex<(String, std::time::Instant)>,
    project: tokio::sync::Mutex<Option<String>>,
    #[allow(dead_code)]
    account_id: Option<String>,
}

impl AntigravityProvider {
    pub fn new(
        access_token: impl Into<String>,
        account_id: Option<String>,
        custom_base: Option<&str>,
    ) -> Self {
        Self {
            client: reqwest::Client::new(),
            base_url: custom_base.unwrap_or(CODE_ASSIST_HOST).trim_end_matches('/').to_string(),
            access_token: std::sync::Mutex::new((access_token.into(), std::time::Instant::now())),
            project: tokio::sync::Mutex::new(None),
            account_id,
        }
    }

    async fn token(&self) -> String {
        let (tok, age) = {
            let g = self.access_token.lock().unwrap();
            (g.0.clone(), g.1.elapsed())
        };
        if age > TOKEN_MAX_AGE {
            if let Some(fresh) = crate::host_creds::discover_antigravity().await {
                let mut g = self.access_token.lock().unwrap();
                *g = (fresh.access_token.clone(), std::time::Instant::now());
                return fresh.access_token;
            }
        }
        tok
    }

    fn headers(token: &str) -> Result<HeaderMap> {
        let mut h = HeaderMap::new();
        h.insert(CONTENT_TYPE, HeaderValue::from_static("application/json"));
        h.insert(
            AUTHORIZATION,
            HeaderValue::from_str(&format!("Bearer {token}")).map_err(|e| OdonError::Auth(e.to_string()))?,
        );
        h.insert(USER_AGENT, HeaderValue::from_static("antigravity/ide/2.11.0 windows/x64"));
        Ok(h)
    }

    async fn project_id(&self, token: &str) -> Result<String> {
        let mut cached = self.project.lock().await;
        if let Some(p) = cached.as_ref() {
            return Ok(p.clone());
        }
        let body = json!({
            "metadata": { "ideType": "IDE_UNSPECIFIED", "platform": "PLATFORM_UNSPECIFIED", "pluginType": "GEMINI" }
        });
        let resp = self
            .client
            .post(format!("{}/v1internal:loadCodeAssist", self.base_url))
            .headers(Self::headers(token)?)
            .json(&body)
            .send()
            .await
            .map_err(|e| OdonError::Provider { provider: "antigravity".into(), message: format!("loadCodeAssist failed: {e}") })?;
        if resp.status() == 401 {
            return Err(OdonError::Auth("Antigravity token expired or invalid".into()));
        }
        let status = resp.status();
        let val: Value = resp.json().await.unwrap_or(Value::Null);
        if !status.is_success() {
            return Err(OdonError::Provider { provider: "antigravity".into(), message: format!("loadCodeAssist HTTP {status}: {val}") });
        }
        let project = val
            .get("cloudaicompanionProject")
            .and_then(|p| p.as_str().map(String::from).or_else(|| p.get("id").and_then(|i| i.as_str()).map(String::from)))
            .ok_or_else(|| OdonError::Provider {
                provider: "antigravity".into(),
                message: format!("No Code Assist project available for this account: {val}"),
            })?;
        *cached = Some(project.clone());
        Ok(project)
    }
}

/// Gemini function declarations reject several JSON-Schema keywords.
fn sanitize_schema(v: &Value) -> Value {
    match v {
        Value::Object(m) => {
            let mut out = serde_json::Map::new();
            for (k, val) in m {
                if matches!(k.as_str(), "default" | "additionalProperties" | "$schema" | "examples" | "title") {
                    continue;
                }
                out.insert(k.clone(), sanitize_schema(val));
            }
            Value::Object(out)
        }
        Value::Array(a) => Value::Array(a.iter().map(sanitize_schema).collect()),
        other => other.clone(),
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
        let token = self.token().await;
        let project = self.project_id(&token).await?;

        let mut contents: Vec<Value> = Vec::new();
        for msg in &request.messages {
            match msg.role {
                MessageRole::User => contents.push(json!({ "role": "user", "parts": [{ "text": msg.content.text }] })),
                MessageRole::Assistant => {
                    let mut parts = Vec::new();
                    if !msg.content.text.is_empty() {
                        parts.push(json!({ "text": msg.content.text }));
                    }
                    for tc in &msg.content.tool_calls {
                        let args = if tc.arguments.is_object() { tc.arguments.clone() } else { json!({}) };
                        parts.push(json!({ "functionCall": { "name": tc.tool_name, "args": args } }));
                    }
                    if !parts.is_empty() {
                        contents.push(json!({ "role": "model", "parts": parts }));
                    }
                }
                MessageRole::Tool => {
                    let name = msg.content.tool_calls.first().map(|t| t.tool_name.clone()).unwrap_or_default();
                    let output: Value = serde_json::from_str(&msg.content.text).unwrap_or_else(|_| json!(msg.content.text));
                    let part = json!({ "functionResponse": { "name": name, "response": { "output": output } } });
                    let merge = contents
                        .last()
                        .map(|c| c["role"] == "user" && c["parts"].as_array().map(|p| p.iter().all(|x| x.get("functionResponse").is_some())).unwrap_or(false))
                        .unwrap_or(false);
                    if merge {
                        if let Some(arr) = contents.last_mut().and_then(|c| c["parts"].as_array_mut()) {
                            arr.push(part);
                        }
                    } else {
                        contents.push(json!({ "role": "user", "parts": [part] }));
                    }
                }
                MessageRole::System => {}
            }
        }

        let model = match request.model.as_str() {
            "odon-auto" | "auto" | "" => "gemini-2.5-flash",
            m => m,
        };

        let mut inner = json!({ "contents": contents });
        if let Some(sys) = &request.system_prompt {
            inner["systemInstruction"] = json!({ "role": "user", "parts": [{ "text": sys }] });
        }
        if !request.tools.is_empty() {
            let decls: Vec<Value> = request
                .tools
                .iter()
                .map(|t| json!({ "name": t.name, "description": t.description, "parameters": sanitize_schema(&t.parameters) }))
                .collect();
            inner["tools"] = json!([{ "functionDeclarations": decls }]);
        }
        let mut gen = json!({});
        if let Some(t) = request.temperature {
            gen["temperature"] = json!(t);
        }
        if let Some(effort) = &request.effort {
            gen["thinkingConfig"] = json!({
                "includeThoughts": true,
                "thinkingBudget": match effort.as_str() { "low" => 2048, "medium" => 8192, "high" => 24576, "xhigh" => 32768, _ => 8192 }
            });
        }
        inner["generationConfig"] = gen;

        let body = json!({
            "model": model,
            "project": project,
            "user_prompt_id": uuid::Uuid::new_v4().to_string(),
            "request": inner,
        });

        let resp = self
            .client
            .post(format!("{}/v1internal:streamGenerateContent?alt=sse", self.base_url))
            .headers(Self::headers(&token)?)
            .json(&body)
            .send()
            .await
            .map_err(|e| OdonError::Provider { provider: "antigravity".into(), message: format!("Antigravity request failed: {e}") })?;

        if resp.status() == 401 {
            return Err(OdonError::Auth("Antigravity token expired or invalid".into()));
        }
        if !resp.status().is_success() {
            let status = resp.status();
            let err_body = resp.text().await.unwrap_or_default();
            return Err(OdonError::Provider { provider: "antigravity".into(), message: format!("Antigravity HTTP {status}: {err_body}") });
        }

        let mut stream = resp.bytes_stream().eventsource();
        let mut tool_index = 0usize;
        loop {
            tokio::select! {
                _ = cancellation.cancelled() => {
                    let _ = event_sender.send(StreamEvent::Done).await;
                    return Ok(());
                }
                item = stream.next() => match item {
                    Some(Ok(ev)) => {
                        let Ok(val) = serde_json::from_str::<Value>(&ev.data) else { continue };
                        let root = val.get("response").unwrap_or(&val);
                        let parts = root
                            .get("candidates")
                            .and_then(|c| c.get(0))
                            .and_then(|c| c.get("content"))
                            .and_then(|c| c.get("parts"))
                            .and_then(|p| p.as_array());
                        for part in parts.into_iter().flatten() {
                            if let Some(fc) = part.get("functionCall") {
                                let name = fc.get("name").and_then(|n| n.as_str()).unwrap_or_default().to_string();
                                let args = fc.get("args").cloned().unwrap_or_else(|| json!({}));
                                let _ = event_sender.send(StreamEvent::ToolCallDelta {
                                    index: tool_index,
                                    id: Some(format!("call_{}_{}", name, uuid::Uuid::new_v4().simple())),
                                    name: Some(name),
                                    arguments_delta: Some(args.to_string()),
                                }).await;
                                tool_index += 1;
                            } else if let Some(text) = part.get("text").and_then(|t| t.as_str()) {
                                if part.get("thought").and_then(|t| t.as_bool()).unwrap_or(false) {
                                    let _ = event_sender.send(StreamEvent::ThoughtDelta(text.to_string())).await;
                                } else {
                                    let _ = event_sender.send(StreamEvent::TextDelta(text.to_string())).await;
                                }
                            }
                        }
                    }
                    Some(Err(e)) => {
                        let _ = event_sender.send(StreamEvent::Error(format!("Antigravity stream error: {e}"))).await;
                        break;
                    }
                    None => {
                        let _ = event_sender.send(StreamEvent::Done).await;
                        break;
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

    #[tokio::test]
    async fn test_antigravity_project_resolution() {
        if let Some(token) = crate::host_creds::discover_antigravity().await {
            let prov = AntigravityProvider::new(token.access_token, None, None);
            let proj = prov.project_id(&prov.token().await).await;
            println!("RESOLVED ANTIGRAVITY PROJECT: {:?}", proj);
        } else {
            println!("NO ANTIGRAVITY TOKEN DISCOVERED");
        }
    }
}
