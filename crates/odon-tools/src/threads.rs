//! Official Threads API tools (Meta Graph API).
//!
//! Publishing is a two-step flow, mirroring Instagram's: create a media
//! container (`POST /{user-id}/threads`), then publish it
//! (`POST /{user-id}/threads_publish`). Replies use the same two calls; the
//! only difference is the `reply_to_id` value — a post id for replying to a
//! post, or the comment (reply) id itself for replying to a reply. Nesting is
//! preserved, so a reply chain is: publish the root, then publish each next
//! container with `reply_to_id` pointing at the previous published id.
//!
//! Scopes: `threads_basic`, `threads_content_publish`, `threads_read_replies`.
//! The access token comes from `THREADS_ACCESS_TOKEN` (or an explicit
//! `access_token` argument) and is never logged.

use async_trait::async_trait;
use odon_core::{OdonError, Result};
use odon_security::RiskTier;
use serde_json::{json, Value};

use crate::registry::{Tool, ToolContext};

const BASE: &str = "https://graph.threads.net/v1.0";
/// Threads caps a post's text at 500 characters.
const MAX_TEXT_CHARS: usize = 500;

fn tool_error(tool: &str, message: impl Into<String>) -> OdonError {
    OdonError::Tool {
        tool: tool.to_string(),
        message: message.into(),
    }
}

fn token_from(args: &Value, tool: &str) -> Result<String> {
    let from_arg = args
        .get("access_token")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty());
    if let Some(token) = from_arg {
        return Ok(token.to_string());
    }
    for name in ["THREADS_ACCESS_TOKEN", "THREAD_API_TOKEN"] {
        if let Ok(value) = std::env::var(name) {
            let value = value.trim();
            if !value.is_empty() {
                return Ok(value.to_string());
            }
        }
    }
    Err(tool_error(
        tool,
        "Missing Threads access token: set THREADS_ACCESS_TOKEN (scopes: threads_basic, \
         threads_content_publish, threads_read_replies) or pass access_token",
    ))
}

fn user_id_from(args: &Value) -> String {
    args.get("user_id")
        .and_then(Value::as_str)
        .filter(|value| !value.trim().is_empty())
        .unwrap_or("me")
        .trim()
        .to_string()
}

/// Graph error extraction; never echoes the token.
fn graph_error(tool: &str, status: reqwest::StatusCode, body: &Value) -> OdonError {
    let message = body
        .get("error")
        .and_then(|error| error.get("message"))
        .and_then(Value::as_str)
        .unwrap_or("Threads request failed");
    let code = body
        .get("error")
        .and_then(|error| error.get("code"))
        .and_then(Value::as_u64)
        .map(|code| format!(" (code {code})"))
        .unwrap_or_default();
    let detail: String = message.chars().take(300).collect();
    tool_error(
        tool,
        format!("Threads API returned HTTP {status}{code}: {detail}"),
    )
}

fn client() -> reqwest::Client {
    reqwest::Client::new()
}

async fn get_json(tool: &str, url: String, token: &str) -> Result<Value> {
    let url = format!("{url}?access_token={token}");
    let response = client()
        .get(url)
        .send()
        .await
        .map_err(|error| tool_error(tool, format!("Threads request failed: {error}")))?;
    let status = response.status();
    let body: Value = response.json().await.unwrap_or(json!({}));
    if !status.is_success() {
        return Err(graph_error(tool, status, &body));
    }
    Ok(body)
}

async fn post_form(
    tool: &str,
    url: String,
    fields: &[(&str, String)],
    token: &str,
) -> Result<Value> {
    let url = format!("{url}?access_token={token}");
    let response = client()
        .post(url)
        .form(fields)
        .send()
        .await
        .map_err(|error| tool_error(tool, format!("Threads request failed: {error}")))?;
    let status = response.status();
    let body: Value = response.json().await.unwrap_or(json!({}));
    if !status.is_success() {
        return Err(graph_error(tool, status, &body));
    }
    Ok(body)
}

async fn container_status(tool: &str, creation_id: &str, token: &str) -> Result<String> {
    let body = get_json(
        tool,
        format!("{BASE}/{creation_id}?fields=status,error_message"),
        token,
    )
    .await?;
    let status = body
        .get("status")
        .and_then(Value::as_str)
        .unwrap_or("UNKNOWN")
        .to_string();
    if let Some(error) = body.get("error_message").and_then(Value::as_str) {
        if status != "FINISHED" && !error.is_empty() {
            return Err(tool_error(
                tool,
                format!("Threads container failed: {error}"),
            ));
        }
    }
    Ok(status)
}

pub struct ThreadsPublishTool;

#[async_trait]
impl Tool for ThreadsPublishTool {
    fn name(&self) -> &str {
        "threads_publish"
    }

    fn description(&self) -> &str {
        "Publish a post or a reply on Threads via the official API. One endpoint handles both: set `reply_to_id` to a post id to reply to that post, or to a comment id to reply to that comment. Returns the published post id."
    }

    fn parameters_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "text": { "type": "string", "description": "The text to post or reply with (max 500 characters)" },
                "reply_to_id": { "type": "string", "description": "Optional: post id to reply to, or comment id to reply to that comment. Omit for a new root post" },
                "image_url": { "type": "string", "description": "Optional publicly accessible image URL to attach" },
                "reply_control": { "type": "string", "enum": ["everyone", "mentioned_only", "profiles_you_follow", "none"], "description": "Who may reply (default everyone)" },
                "user_id": { "type": "string", "description": "Threads user id (default: me)" },
                "access_token": { "type": "string", "description": "Threads access token (default: THREADS_ACCESS_TOKEN)" }
            },
            "required": ["text"]
        })
    }

    fn risk_tier(&self) -> RiskTier {
        // Publishing publicly is hard to undo, so it stays approval-gated.
        RiskTier::Tier2SystemMutating
    }

    async fn execute(&self, _ctx: &ToolContext, args: Value) -> Result<Value> {
        let tool = self.name();
        let token = token_from(&args, tool)?;
        let text = args
            .get("text")
            .and_then(Value::as_str)
            .ok_or_else(|| tool_error(tool, "Missing 'text' parameter"))?
            .to_string();
        if text.trim().is_empty() {
            return Err(tool_error(tool, "Text must not be empty"));
        }
        if text.chars().count() > MAX_TEXT_CHARS {
            return Err(tool_error(
                tool,
                format!(
                    "Text is {} characters; Threads allows at most {MAX_TEXT_CHARS}",
                    text.chars().count()
                ),
            ));
        }
        let user = user_id_from(&args);
        let reply_control = args
            .get("reply_control")
            .and_then(Value::as_str)
            .unwrap_or("everyone")
            .to_string();
        let image_url = args
            .get("image_url")
            .and_then(Value::as_str)
            .filter(|value| !value.trim().is_empty())
            .map(str::to_string);

        let mut fields: Vec<(&str, String)> = vec![
            (
                "media_type",
                if image_url.is_some() {
                    "IMAGE".to_string()
                } else {
                    "TEXT".to_string()
                },
            ),
            ("text", text.clone()),
            ("reply_control", reply_control),
        ];
        if let Some(url) = image_url {
            fields.push(("image_url", url));
        }
        if let Some(reply_to) = args
            .get("reply_to_id")
            .and_then(Value::as_str)
            .filter(|value| !value.trim().is_empty())
        {
            fields.push(("reply_to_id", reply_to.trim().to_string()));
        }

        let created = post_form(tool, format!("{BASE}/{user}/threads"), &fields, &token).await?;
        let creation_id = created
            .get("id")
            .and_then(Value::as_str)
            .ok_or_else(|| {
                tool_error(
                    tool,
                    format!("Threads did not return a container id: {created}"),
                )
            })?
            .to_string();

        // Wait for the media pipeline (Meta recommends ~30s for media; text is
        // usually finished quickly). Bounded so a stuck container cannot spin.
        let mut status = "IN_PROGRESS".to_string();
        for _ in 0..15 {
            status = container_status(tool, &creation_id, &token).await?;
            if status == "FINISHED" {
                break;
            }
            if status == "ERROR" {
                break;
            }
            tokio::time::sleep(std::time::Duration::from_secs(3)).await;
        }
        if status != "FINISHED" {
            return Ok(json!({
                "creation_id": creation_id,
                "status": status,
                "published": false,
                "message": "Container is not ready yet; poll with threads_status, then publish."
            }));
        }

        let published = post_form(
            tool,
            format!("{BASE}/{user}/threads_publish"),
            &[("creation_id", creation_id.clone())],
            &token,
        )
        .await?;
        let post_id = published
            .get("id")
            .and_then(Value::as_str)
            .unwrap_or_default()
            .to_string();

        Ok(json!({
            "creation_id": creation_id,
            "published": !post_id.is_empty(),
            "post_id": post_id,
            "status": status,
            "reply_to_id": args.get("reply_to_id").and_then(Value::as_str),
        }))
    }
}

pub struct ThreadsStatusTool;

#[async_trait]
impl Tool for ThreadsStatusTool {
    fn name(&self) -> &str {
        "threads_status"
    }

    fn description(&self) -> &str {
        "Check the processing status of a Threads media container"
    }

    fn parameters_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "creation_id": { "type": "string" },
                "access_token": { "type": "string" }
            },
            "required": ["creation_id"]
        })
    }

    fn risk_tier(&self) -> RiskTier {
        RiskTier::Tier0ReadOnly
    }

    async fn execute(&self, _ctx: &ToolContext, args: Value) -> Result<Value> {
        let tool = self.name();
        let token = token_from(&args, tool)?;
        let creation_id = args
            .get("creation_id")
            .and_then(Value::as_str)
            .ok_or_else(|| tool_error(tool, "Missing 'creation_id' parameter"))?;
        let body = get_json(
            tool,
            format!("{BASE}/{creation_id}?fields=status,error_message"),
            &token,
        )
        .await?;
        Ok(json!({
            "creation_id": creation_id,
            "status": body.get("status").and_then(Value::as_str).unwrap_or("UNKNOWN"),
            "error_message": body.get("error_message").cloned().unwrap_or(Value::Null),
        }))
    }
}

pub struct ThreadsReplyListTool;

#[async_trait]
impl Tool for ThreadsReplyListTool {
    fn name(&self) -> &str {
        "threads_reply_list"
    }

    fn description(&self) -> &str {
        "Read replies to a Threads post or comment using threads_read_replies"
    }

    fn parameters_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "media_id": { "type": "string", "description": "Post or comment id to read replies for" },
                "limit": { "type": "integer", "minimum": 1, "maximum": 100 },
                "access_token": { "type": "string" }
            },
            "required": ["media_id"]
        })
    }

    fn risk_tier(&self) -> RiskTier {
        RiskTier::Tier0ReadOnly
    }

    async fn execute(&self, _ctx: &ToolContext, args: Value) -> Result<Value> {
        let tool = self.name();
        let token = token_from(&args, tool)?;
        let media_id = args
            .get("media_id")
            .and_then(Value::as_str)
            .ok_or_else(|| tool_error(tool, "Missing 'media_id' parameter"))?;
        let limit = args
            .get("limit")
            .and_then(Value::as_u64)
            .unwrap_or(25)
            .clamp(1, 100);
        let body = get_json(
            tool,
            format!(
                "{BASE}/{media_id}/conversation?fields=id,text,username,timestamp,like_count&limit={limit}"
            ),
            &token,
        )
        .await?;
        Ok(body)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn user_id_defaults_to_me_and_honours_an_override() {
        assert_eq!(user_id_from(&json!({})), "me");
        assert_eq!(
            user_id_from(&json!({ "user_id": " 12345 " })),
            "12345",
            "surrounding whitespace is trimmed"
        );
        assert_eq!(
            user_id_from(&json!({ "user_id": "   " })),
            "me",
            "a blank override falls back to me"
        );
    }

    #[test]
    fn graph_errors_extract_the_message_without_the_body() {
        let body = json!({ "error": { "message": "bad", "code": 190 } });
        let error = graph_error("threads_publish", reqwest::StatusCode::BAD_REQUEST, &body);
        let text = format!("{error}");
        assert!(text.contains("bad"), "message kept: {text}");
        assert!(text.contains("190"), "code kept: {text}");
        assert!(
            !text.contains("access_token"),
            "no token may leak into errors: {text}"
        );
    }
}
