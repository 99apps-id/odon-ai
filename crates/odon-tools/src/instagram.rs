//! Official Instagram Graph API tools (Meta).
//!
//! Publishing is a two-step container flow against `graph.facebook.com`:
//! create an IG container (`POST /{ig-user-id}/media` with `image_url` and
//! `caption`), poll it until `status_code` is `FINISHED`, then publish it
//! (`POST /{ig-user-id}/media_publish` with `creation_id`).
//!
//! Requirements: an Instagram **professional** (Business or Creator) account
//! linked to a Facebook Page, with `instagram_basic` and
//! `instagram_content_publish` scopes. The `image_url` must be a publicly
//! accessible HTTPS URL that Meta's servers can fetch — a local URL will not
//! work. The token and the numeric IG user id come from
//! `INSTAGRAM_ACCESS_TOKEN` / `INSTAGRAM_USER_ID` (or explicit arguments) and
//! are never logged.

use async_trait::async_trait;
use odon_core::{OdonError, Result};
use odon_security::RiskTier;
use serde_json::{json, Value};

use crate::registry::{Tool, ToolContext};

const BASE: &str = "https://graph.facebook.com/v21.0";

fn tool_error(tool: &str, message: impl Into<String>) -> OdonError {
    OdonError::Tool {
        tool: tool.to_string(),
        message: message.into(),
    }
}

fn token_from(args: &Value, tool: &str) -> Result<String> {
    if let Some(token) = args
        .get("access_token")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
    {
        return Ok(token.to_string());
    }
    for name in ["INSTAGRAM_ACCESS_TOKEN", "IG_ACCESS_TOKEN"] {
        if let Ok(value) = std::env::var(name) {
            let value = value.trim();
            if !value.is_empty() {
                return Ok(value.to_string());
            }
        }
    }
    Err(tool_error(
        tool,
        "Missing Instagram access token: set INSTAGRAM_ACCESS_TOKEN (scopes: instagram_basic, \
         instagram_content_publish) or pass access_token",
    ))
}

fn user_id_from(args: &Value, tool: &str) -> Result<String> {
    if let Some(id) = args
        .get("user_id")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
    {
        return Ok(id.to_string());
    }
    for name in ["INSTAGRAM_USER_ID", "IG_USER_ID"] {
        if let Ok(value) = std::env::var(name) {
            let value = value.trim();
            if !value.is_empty() {
                let _ = tool;
                return Ok(value.to_string());
            }
        }
    }
    Err(tool_error(
        tool,
        "Missing Instagram user id: set INSTAGRAM_USER_ID (the numeric id of your professional \
         account) or pass user_id",
    ))
}

fn graph_error(tool: &str, status: reqwest::StatusCode, body: &Value) -> OdonError {
    let message = body
        .get("error")
        .and_then(|error| error.get("message"))
        .and_then(Value::as_str)
        .unwrap_or("Instagram request failed");
    let detail: String = message.chars().take(300).collect();
    tool_error(
        tool,
        format!("Instagram API returned HTTP {status}: {detail}"),
    )
}

async fn get_json(tool: &str, url: String, token: &str) -> Result<Value> {
    let url = format!("{url}?access_token={token}");
    let response = reqwest::Client::new()
        .get(url)
        .send()
        .await
        .map_err(|error| tool_error(tool, format!("Instagram request failed: {error}")))?;
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
    let response = reqwest::Client::new()
        .post(url)
        .form(fields)
        .send()
        .await
        .map_err(|error| tool_error(tool, format!("Instagram request failed: {error}")))?;
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
        format!("{BASE}/{creation_id}?fields=status_code,error_message"),
        token,
    )
    .await?;
    let status = body
        .get("status_code")
        .and_then(Value::as_str)
        .unwrap_or("UNKNOWN")
        .to_string();
    if status == "ERROR" {
        let detail = body
            .get("error_message")
            .and_then(Value::as_str)
            .unwrap_or("media processing failed");
        return Err(tool_error(
            tool,
            format!("Instagram container failed: {detail}"),
        ));
    }
    Ok(status)
}

fn require_https_public_url(tool: &str, value: &str, label: &str) -> Result<()> {
    let trimmed = value.trim();
    if !trimmed.starts_with("https://") {
        return Err(tool_error(
            tool,
            format!("{label} must be a public https:// URL that Meta can fetch; got {trimmed}"),
        ));
    }
    Ok(())
}

pub struct InstagramPublishTool;

#[async_trait]
impl Tool for InstagramPublishTool {
    fn name(&self) -> &str {
        "instagram_publish"
    }

    fn description(&self) -> &str {
        "Publish an image or video to Instagram via the official Graph API (professional account). Returns the published media id and permalink."
    }

    fn parameters_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "image_url": { "type": "string", "description": "Public https URL of the image (for media_type IMAGE)" },
                "video_url": { "type": "string", "description": "Public https URL of the video (for media_type VIDEO)" },
                "cover_image_url": { "type": "string", "description": "Optional poster image for a video" },
                "caption": { "type": "string", "description": "Caption text" },
                "user_id": { "type": "string", "description": "Numeric Instagram user id (default: INSTAGRAM_USER_ID)" },
                "access_token": { "type": "string", "description": "Instagram access token (default: INSTAGRAM_ACCESS_TOKEN)" }
            }
        })
    }

    fn risk_tier(&self) -> RiskTier {
        // Posting publicly is hard to undo, so it stays approval-gated.
        RiskTier::Tier2SystemMutating
    }

    async fn execute(&self, _ctx: &ToolContext, args: Value) -> Result<Value> {
        let tool = self.name();
        let token = token_from(&args, tool)?;
        let user = user_id_from(&args, tool)?;
        let caption = args
            .get("caption")
            .and_then(Value::as_str)
            .unwrap_or("")
            .to_string();
        let image_url = args
            .get("image_url")
            .and_then(Value::as_str)
            .filter(|value| !value.trim().is_empty())
            .map(str::to_string);
        let video_url = args
            .get("video_url")
            .and_then(Value::as_str)
            .filter(|value| !value.trim().is_empty())
            .map(str::to_string);

        let mut fields: Vec<(&str, String)> = Vec::new();
        if let Some(url) = image_url {
            require_https_public_url(tool, &url, "image_url")?;
            fields.push(("media_type", "IMAGE".to_string()));
            fields.push(("image_url", url));
        } else if let Some(url) = video_url {
            require_https_public_url(tool, &url, "video_url")?;
            fields.push(("media_type", "VIDEO".to_string()));
            fields.push(("video_url", url));
            if let Some(cover) = args
                .get("cover_image_url")
                .and_then(Value::as_str)
                .filter(|value| !value.trim().is_empty())
            {
                require_https_public_url(tool, cover, "cover_image_url")?;
                fields.push(("cover_image_url", cover.trim().to_string()));
            }
        } else {
            return Err(tool_error(
                tool,
                "Provide image_url or video_url (a public https URL Meta can fetch; local URLs do not work)",
            ));
        }
        if !caption.is_empty() {
            fields.push(("caption", caption));
        }

        let created = post_form(tool, format!("{BASE}/{user}/media"), &fields, &token).await?;
        let creation_id = created
            .get("id")
            .and_then(Value::as_str)
            .ok_or_else(|| {
                tool_error(
                    tool,
                    format!("Instagram did not return a container id: {created}"),
                )
            })?
            .to_string();

        // Wait for Instagram to finish fetching/transcoding the media.
        let mut status = "IN_PROGRESS".to_string();
        for _ in 0..20 {
            status = container_status(tool, &creation_id, &token).await?;
            if status == "FINISHED" {
                break;
            }
            tokio::time::sleep(std::time::Duration::from_secs(3)).await;
        }
        if status != "FINISHED" {
            return Ok(json!({
                "creation_id": creation_id,
                "status": status,
                "published": false,
                "message": "Container is not ready yet; poll with instagram_status, then publish."
            }));
        }

        let published = post_form(
            tool,
            format!("{BASE}/{user}/media_publish"),
            &[("creation_id", creation_id.clone())],
            &token,
        )
        .await?;
        let media_id = published
            .get("id")
            .and_then(Value::as_str)
            .unwrap_or_default()
            .to_string();

        let permalink = if media_id.is_empty() {
            String::new()
        } else {
            match get_json(tool, format!("{BASE}/{media_id}?fields=permalink"), &token).await {
                Ok(body) => body
                    .get("permalink")
                    .and_then(Value::as_str)
                    .unwrap_or_default()
                    .to_string(),
                Err(_) => String::new(),
            }
        };

        Ok(json!({
            "creation_id": creation_id,
            "published": !media_id.is_empty(),
            "media_id": media_id,
            "permalink": permalink,
            "status": status,
        }))
    }
}

pub struct InstagramStatusTool;

#[async_trait]
impl Tool for InstagramStatusTool {
    fn name(&self) -> &str {
        "instagram_status"
    }

    fn description(&self) -> &str {
        "Check the processing status of an Instagram media container"
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
            format!("{BASE}/{creation_id}?fields=status_code,error_message"),
            &token,
        )
        .await?;
        Ok(json!({
            "creation_id": creation_id,
            "status_code": body.get("status_code").and_then(Value::as_str).unwrap_or("UNKNOWN"),
            "error_message": body.get("error_message").cloned().unwrap_or(Value::Null),
        }))
    }
}

pub struct InstagramMediaGetTool;

#[async_trait]
impl Tool for InstagramMediaGetTool {
    fn name(&self) -> &str {
        "instagram_media_get"
    }

    fn description(&self) -> &str {
        "Fetch an Instagram media object (permalink, caption, timestamp) by id"
    }

    fn parameters_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "media_id": { "type": "string" },
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
        get_json(
            tool,
            format!("{BASE}/{media_id}?fields=id,caption,permalink,timestamp,media_type,like_count,comments_count"),
            &token,
        )
        .await
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn public_urls_must_be_https() {
        assert!(
            require_https_public_url("t", "https://cdn.example.com/a.jpg", "image_url").is_ok()
        );
        assert!(require_https_public_url("t", "http://localhost/a.jpg", "image_url").is_err());
        assert!(require_https_public_url("t", "file:///tmp/a.jpg", "image_url").is_err());
    }
}
