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

use crate::multimodal;
use crate::stream::StreamEvent;
use crate::trait_def::{ChatRequest, ModelProvider};

const CODE_ASSIST_HOST: &str = "https://daily-cloudcode-pa.googleapis.com";
const CODE_ASSIST_FALLBACK_HOST: &str = "https://cloudcode-pa.googleapis.com";
/// Google access tokens last ~1h; refresh a little early.
const TOKEN_MAX_AGE: std::time::Duration = std::time::Duration::from_secs(45 * 60);

pub struct AntigravityProvider {
    client: reqwest::Client,
    base_url: String,
    access_token: std::sync::Mutex<(String, std::time::Instant)>,
    refresh_lock: tokio::sync::Mutex<()>,
    project: tokio::sync::Mutex<Option<String>>,
    #[allow(dead_code)]
    account_id: Option<String>,
    signatures: std::sync::Arc<tokio::sync::Mutex<std::collections::HashMap<String, String>>>,
}

fn platform_code() -> i32 {
    if cfg!(target_os = "windows") {
        5
    } else if cfg!(target_os = "macos") {
        2
    } else {
        1
    }
}

fn random_hex(len: usize) -> String {
    let s = format!(
        "{}{}",
        uuid::Uuid::new_v4().simple(),
        uuid::Uuid::new_v4().simple()
    );
    s.chars().take(len * 2).collect()
}

fn new_session_id() -> String {
    let bytes = uuid::Uuid::new_v4().into_bytes();
    let num = u64::from_be_bytes(bytes[..8].try_into().unwrap()) & 0x7fff_ffff_ffff_ffff;
    num.to_string()
}

fn antigravity_request_id(message_count: usize) -> String {
    let step = (message_count * 2).max(1) - 1;
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis();
    format!(
        "agent/{}/{}/{}/{}",
        random_hex(8),
        now,
        random_hex(8),
        step.max(1)
    )
}

fn extract_project_id(raw: &Value) -> Option<String> {
    if let Some(s) = raw.as_str() {
        let trimmed = s.trim().trim_start_matches("projects/").trim();
        if !trimmed.is_empty() {
            return Some(trimmed.to_string());
        }
    }
    if let Some(obj) = raw.as_object() {
        for key in ["id", "projectId", "project", "name"] {
            if let Some(val) = obj.get(key).and_then(|v| v.as_str()) {
                let trimmed = val.trim().trim_start_matches("projects/").trim();
                if !trimmed.is_empty() {
                    return Some(trimmed.to_string());
                }
            }
        }
    }
    None
}

impl AntigravityProvider {
    pub fn new(
        access_token: impl Into<String>,
        account_id: Option<String>,
        custom_base: Option<&str>,
    ) -> Self {
        Self {
            client: reqwest::Client::new(),
            base_url: custom_base
                .unwrap_or(CODE_ASSIST_HOST)
                .trim_end_matches('/')
                .to_string(),
            access_token: std::sync::Mutex::new((access_token.into(), std::time::Instant::now())),
            refresh_lock: tokio::sync::Mutex::new(()),
            project: tokio::sync::Mutex::new(None),
            account_id,
            signatures: std::sync::Arc::new(tokio::sync::Mutex::new(
                std::collections::HashMap::new(),
            )),
        }
    }

    async fn remember_signature(&self, key: &str, signature: &str) {
        if !signature.is_empty() {
            let mut sigs = self.signatures.lock().await;
            sigs.insert(key.to_string(), signature.to_string());
        }
    }

    async fn get_signature(&self, key: &str) -> Option<String> {
        let sigs = self.signatures.lock().await;
        sigs.get(key).cloned()
    }

    async fn token(&self) -> String {
        let read = || {
            let guard = self.access_token.lock().unwrap_or_else(|e| e.into_inner());
            (guard.0.clone(), guard.1.elapsed())
        };
        let (tok, age) = read();
        if age <= TOKEN_MAX_AGE {
            return tok;
        }
        // Serialise refreshes. Google rotates the refresh token on use, so two
        // concurrent refreshes would make the loser fail and leave the provider
        // caching a token that is already dead.
        let _refresh = self.refresh_lock.lock().await;
        let (tok, age) = read();
        if age <= TOKEN_MAX_AGE {
            return tok;
        }
        if let Some(fresh) = crate::host_creds::discover_antigravity().await {
            let mut guard = self.access_token.lock().unwrap_or_else(|e| e.into_inner());
            *guard = (fresh.access_token.clone(), std::time::Instant::now());
            return fresh.access_token;
        }
        tok
    }

    fn headers(token: &str, session: Option<&str>) -> Result<HeaderMap> {
        let mut h = HeaderMap::new();
        h.insert(CONTENT_TYPE, HeaderValue::from_static("application/json"));
        h.insert(
            AUTHORIZATION,
            HeaderValue::from_str(&format!("Bearer {token}"))
                .map_err(|e| OdonError::Auth(e.to_string()))?,
        );
        let platform = if cfg!(target_os = "windows") {
            "windows/x64"
        } else if cfg!(target_os = "macos") {
            "darwin/arm64"
        } else {
            "linux/x64"
        };
        h.insert(
            USER_AGENT,
            HeaderValue::from_str(&format!("antigravity/ide/2.11.0 {platform}")).unwrap_or(
                HeaderValue::from_static("antigravity/ide/2.11.0 windows/x64"),
            ),
        );
        if let Some(s) = session {
            if let Ok(v) = HeaderValue::from_str(s) {
                h.insert("x-machine-session-id", v);
            }
        }
        Ok(h)
    }

    async fn project_id(&self, token: &str) -> Result<String> {
        let mut cached = self.project.lock().await;
        if let Some(p) = cached.as_ref() {
            if !p.starts_with("termixgo-") {
                return Ok(p.clone());
            }
        }
        if let Some(acc) = &self.account_id {
            let trimmed = acc.trim().trim_start_matches("projects/").trim();
            if !trimmed.is_empty() {
                *cached = Some(trimmed.to_string());
                return Ok(trimmed.to_string());
            }
        }

        let metadata = json!({
            "ideType": 9,
            "platform": platform_code(),
            "pluginType": 2
        });

        // 1. Query loadCodeAssist with IDE metadata (matching Termigo)
        let load_url = format!("{}/v1internal:loadCodeAssist", CODE_ASSIST_FALLBACK_HOST);
        let resp = self
            .client
            .post(&load_url)
            .headers(Self::headers(token, None)?)
            .json(&json!({ "metadata": metadata }))
            .send()
            .await;

        if let Ok(res) = resp {
            if res.status() == 401 {
                return Err(OdonError::Auth(
                    "Antigravity token expired or invalid".into(),
                ));
            }
            if res.status().is_success() {
                let val: Value = res.json().await.unwrap_or(Value::Null);
                tracing::info!("Antigravity loadCodeAssist raw response: {:?}", val);
                if let Some(proj) = val
                    .get("cloudaicompanionProject")
                    .and_then(extract_project_id)
                    .or_else(|| val.get("project").and_then(extract_project_id))
                    .or_else(|| val.get("projectId").and_then(extract_project_id))
                {
                    tracing::info!("Antigravity companion project found: {}", proj);
                    *cached = Some(proj.clone());
                    return Ok(proj);
                }

                // If not yet onboarded, check default tier from allowedTiers
                let mut tier = "legacy-tier".to_string();
                if let Some(tiers) = val.get("allowedTiers").and_then(|t| t.as_array()) {
                    for candidate in tiers {
                        if candidate
                            .get("isDefault")
                            .and_then(|d| d.as_bool())
                            .unwrap_or(false)
                        {
                            if let Some(id) = candidate.get("id").and_then(|i| i.as_str()) {
                                tier = id.to_string();
                                break;
                            }
                        }
                    }
                }

                // 2. Call onboardUser (matching Termigo)
                let onboard_url = format!("{}/v1internal:onboardUser", CODE_ASSIST_FALLBACK_HOST);
                for _ in 0..3 {
                    let onboard_resp = self
                        .client
                        .post(&onboard_url)
                        .headers(Self::headers(token, None)?)
                        .json(&json!({ "tierId": tier, "metadata": metadata }))
                        .send()
                        .await;

                    if let Ok(onboard_res) = onboard_resp {
                        if onboard_res.status().is_success() {
                            let onboard_val: Value =
                                onboard_res.json().await.unwrap_or(Value::Null);
                            let from_onboard = onboard_val
                                .get("response")
                                .and_then(|r| r.get("cloudaicompanionProject"))
                                .and_then(extract_project_id)
                                .or_else(|| {
                                    onboard_val
                                        .get("cloudaicompanionProject")
                                        .and_then(extract_project_id)
                                })
                                .or_else(|| {
                                    onboard_val.get("response").and_then(extract_project_id)
                                });

                            if let Some(proj) = from_onboard {
                                *cached = Some(proj.clone());
                                return Ok(proj);
                            }
                            if onboard_val
                                .get("done")
                                .and_then(|d| d.as_bool())
                                .unwrap_or(false)
                            {
                                break;
                            }
                        }
                    }
                    tokio::time::sleep(tokio::time::Duration::from_secs(2)).await;
                }
            }
        }

        // 3. Fallback project format matching Termigo
        let fallback = format!("termixgo-{}", random_hex(4));
        *cached = Some(fallback.clone());
        Ok(fallback)
    }
}

/// Gemini function declarations reject several JSON-Schema keywords and require that
/// every field in `required` exists in `properties`.
fn sanitize_schema(v: &Value) -> Value {
    match v {
        Value::Object(m) => {
            let mut out = serde_json::Map::new();
            for (k, val) in m {
                if matches!(
                    k.as_str(),
                    "default"
                        | "additionalProperties"
                        | "$schema"
                        | "$id"
                        | "$ref"
                        | "$defs"
                        | "definitions"
                        | "examples"
                        | "title"
                        | "optional"
                        | "deprecated"
                        | "readOnly"
                        | "writeOnly"
                ) {
                    continue;
                }
                out.insert(k.clone(), sanitize_schema(val));
            }

            // Prune required fields that are not in properties
            if let Some(req_val) = out.get("required") {
                if let Some(req_arr) = req_val.as_array() {
                    let props = out.get("properties").and_then(|p| p.as_object());
                    let filtered: Vec<Value> = req_arr
                        .iter()
                        .filter(|item| {
                            if let Some(prop_name) = item.as_str() {
                                if let Some(p) = props {
                                    return p.contains_key(prop_name);
                                }
                            }
                            false
                        })
                        .cloned()
                        .collect();

                    if filtered.is_empty() {
                        out.remove("required");
                    } else {
                        out.insert("required".to_string(), Value::Array(filtered));
                    }
                }
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
                MessageRole::User => contents.push(json!({
                    "role": "user",
                    "parts": multimodal::gemini_parts(&msg.content),
                })),
                MessageRole::Assistant => {
                    let mut parts = Vec::new();
                    if !msg.content.text.is_empty() {
                        parts.push(json!({ "text": msg.content.text }));
                    }
                    for tc in &msg.content.tool_calls {
                        let args = if tc.arguments.is_object() {
                            tc.arguments.clone()
                        } else {
                            json!({})
                        };
                        let clean_name = tc
                            .tool_name
                            .trim_start_matches("defaultapi:")
                            .trim_start_matches("default_api:");

                        // A thought signature must be the one the provider
                        // issued for this call. Fabricating one used to paper
                        // over a miss, but the API rejects an invented value
                        // exactly as it rejects a missing one, so the field is
                        // omitted instead: the call is then replayed as an
                        // ordinary function call.
                        let sig = msg
                            .content
                            .thought_signature
                            .clone()
                            .or(self.get_signature(&tc.call_id).await)
                            .or(self.get_signature(&tc.tool_name).await)
                            .or(self.get_signature(clean_name).await);

                        let mut part = json!({
                            "functionCall": {
                                "id": tc.call_id,
                                "name": clean_name,
                                "args": args
                            }
                        });
                        if let Some(sig) = sig {
                            part["thoughtSignature"] = json!(sig);
                        }
                        parts.push(part);
                    }
                    if !parts.is_empty() {
                        contents.push(json!({ "role": "model", "parts": parts }));
                    }
                }
                MessageRole::Tool => {
                    let raw_name = msg
                        .content
                        .tool_calls
                        .first()
                        .map(|t| t.tool_name.clone())
                        .unwrap_or_default();
                    let clean_name = raw_name
                        .trim_start_matches("defaultapi:")
                        .trim_start_matches("default_api:");
                    let call_id = msg
                        .content
                        .tool_calls
                        .first()
                        .map(|t| t.call_id.clone())
                        .unwrap_or_default();

                    let output: Value = serde_json::from_str(&msg.content.text)
                        .unwrap_or_else(|_| json!(msg.content.text));
                    let mut resp_obj = json!({
                        "name": clean_name,
                        "response": { "output": output }
                    });
                    if !call_id.is_empty() {
                        resp_obj["id"] = json!(call_id);
                    }
                    let part = json!({ "functionResponse": resp_obj });
                    let merge = contents
                        .last()
                        .map(|c| {
                            c["role"] == "user"
                                && c["parts"]
                                    .as_array()
                                    .map(|p| p.iter().all(|x| x.get("functionResponse").is_some()))
                                    .unwrap_or(false)
                        })
                        .unwrap_or(false);
                    if merge {
                        if let Some(arr) =
                            contents.last_mut().and_then(|c| c["parts"].as_array_mut())
                        {
                            arr.push(part);
                        }
                    } else {
                        contents.push(json!({ "role": "user", "parts": [part] }));
                    }
                }
                MessageRole::System => {}
            }
        }

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
        let session_id = new_session_id();
        let wire_model = match request.model.as_str() {
            // Legacy aliases kept so saved sessions keep working.
            "gemini-4-argon"
            | "gemini-3.1-pro"
            | "gemini-3.8-flash"
            | "gemini-3.8-flash-medium" => "gemini-2.5-pro",
            "gemini-3.8-flash-high" | "odon-auto" | "auto" | "" => "gemini-2.5-flash",
            "claude-sonnet-5-5" | "claude-sonnet-4-6" => "claude-sonnet-4-20250514",
            "claude-opus-5-5" => "claude-opus-4-20250514",
            "claude-3-5-sonnet" | "claude-3-5-sonnet-20241022" => "claude-3-5-sonnet-20241022",
            other => other,
        };

        let mut gen = json!({
            "maxOutputTokens": 64000,
            "thinkingConfig": {
                "includeThoughts": true,
                "thinkingLevel": "medium"
            }
        });
        if let Some(t) = request.temperature {
            gen["temperature"] = json!(t);
        }
        if let Some(effort) = &request.effort {
            gen["thinkingConfig"]["thinkingBudget"] = json!(match effort.as_str() {
                "low" => 2048,
                "medium" => 8192,
                "high" => 24576,
                "xhigh" => 32768,
                _ => 8192,
            });
        }

        let mut inner = json!({
            "contents": contents,
            "sessionId": session_id,
            "generationConfig": gen,
            "safetySettings": [
                { "category": "HARM_CATEGORY_HARASSMENT", "threshold": "BLOCK_NONE" },
                { "category": "HARM_CATEGORY_HATE_SPEECH", "threshold": "BLOCK_NONE" },
                { "category": "HARM_CATEGORY_SEXUALLY_EXPLICIT", "threshold": "BLOCK_NONE" },
                { "category": "HARM_CATEGORY_DANGEROUS_CONTENT", "threshold": "BLOCK_NONE" },
                { "category": "HARM_CATEGORY_CIVIC_INTEGRITY", "threshold": "BLOCK_NONE" }
            ]
        });

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
            inner["toolConfig"] = json!({ "functionCallingConfig": { "mode": "VALIDATED" } });
        }

        let body = json!({
            "project": project,
            "model": wire_model,
            "userAgent": "antigravity",
            "requestId": antigravity_request_id(request.messages.len()),
            "request": inner,
        });

        tracing::info!(
            "Antigravity stream_chat request: project={}, model={}, wire_model={}",
            project,
            request.model,
            wire_model
        );

        let endpoint = format!("{}/v1internal:streamGenerateContent?alt=sse", self.base_url);
        let mut resp = self
            .client
            .post(&endpoint)
            .headers(Self::headers(&token, Some(&session_id))?)
            .json(&body)
            .send()
            .await
            .map_err(|e| OdonError::Provider {
                provider: "antigravity".into(),
                message: format!("Antigravity request failed: {e}"),
            })?;

        if resp.status().is_server_error() && self.base_url.contains("daily-cloudcode-pa") {
            let fallback_url = format!(
                "{}/v1internal:streamGenerateContent?alt=sse",
                CODE_ASSIST_FALLBACK_HOST
            );
            if let Ok(retry) = self
                .client
                .post(&fallback_url)
                .headers(Self::headers(&token, Some(&session_id))?)
                .json(&body)
                .send()
                .await
            {
                resp = retry;
            }
        }

        if resp.status() == 401 {
            return Err(OdonError::Auth(
                "Antigravity token expired or invalid".into(),
            ));
        }
        if !resp.status().is_success() {
            let status = resp.status();
            let err_body = resp.text().await.unwrap_or_default();
            return Err(OdonError::Provider {
                provider: "antigravity".into(),
                message: format!(
                    "Antigravity HTTP {status}: {}",
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
                item = stream.next() => match item {
                    Some(Ok(ev)) => {
                        let Ok(val) = serde_json::from_str::<Value>(&ev.data) else { continue };
                        let root = val.get("response").unwrap_or(&val);
                        // Gemini reports a cut answer through finishReason, not
                        // an OpenAI-style finish_reason. Surface it so the
                        // runtime's continuation logic can resume the answer.
                        let finish_reason = root
                            .get("candidates")
                            .and_then(|c| c.get(0))
                            .and_then(|c| c.get("finishReason"))
                            .and_then(Value::as_str);
                        if finish_reason == Some("MAX_TOKENS") {
                            let _ = event_sender.send(StreamEvent::OutputTruncated).await;
                        }
                        let parts = root
                            .get("candidates")
                            .and_then(|c| c.get(0))
                            .and_then(|c| c.get("content"))
                            .and_then(|c| c.get("parts"))
                            .and_then(|p| p.as_array());
                        for part in parts.into_iter().flatten() {
                            if let Some(fc) = part.get("functionCall") {
                                let raw_name = fc.get("name").and_then(|n| n.as_str()).unwrap_or_default().to_string();
                                let name = raw_name
                                    .trim_start_matches("defaultapi:")
                                    .trim_start_matches("default_api:")
                                    .to_string();
                                let args = fc.get("args").cloned().unwrap_or_else(|| json!({}));
                                let call_id = fc.get("id").and_then(|i| i.as_str())
                                    .map(String::from)
                                    .unwrap_or_else(|| format!("call_{}_{}", name, uuid::Uuid::new_v4().simple()));

                                let sig = part.get("thoughtSignature")
                                    .or_else(|| part.get("thought_signature"))
                                    .or_else(|| fc.get("thoughtSignature"))
                                    .or_else(|| fc.get("thought_signature"))
                                    .and_then(|s| s.as_str())
                                    .unwrap_or_default();

                                if !sig.is_empty() {
                                    self.remember_signature(&call_id, sig).await;
                                    self.remember_signature(&name, sig).await;
                                    self.remember_signature(&raw_name, sig).await;
                                }

                                let _ = event_sender.send(StreamEvent::ToolCallDelta {
                                    index: tool_index,
                                    id: Some(call_id),
                                    name: Some(name),
                                    arguments_delta: Some(args.to_string()),
                                }).await;
                                tool_index += 1;
                            } else if let Some(text) = part.get("text").and_then(|t| t.as_str()) {
                                // A signature can ride on a thought part or on
                                // the closing text part; either way it must be
                                // forwarded so the runtime can store it with
                                // the message and replay it later.
                                let part_sig = part
                                    .get("thoughtSignature")
                                    .or_else(|| part.get("thought_signature"))
                                    .and_then(|s| s.as_str())
                                    .unwrap_or_default();
                                if !part_sig.is_empty() {
                                    self.remember_signature("last", part_sig).await;
                                    let _ = event_sender
                                        .send(StreamEvent::ThoughtSignature(part_sig.to_string()))
                                        .await;
                                }
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
