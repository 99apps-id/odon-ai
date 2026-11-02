use crate::registry::{Tool, ToolContext};
use async_trait::async_trait;
use odon_core::error::{OdonError, Result};
use odon_security::RiskTier;
use serde_json::{json, Value};
use std::path::{Path, PathBuf};

fn tool_error(name: &str, message: impl Into<String>) -> OdonError {
    OdonError::Tool { tool: name.to_string(), message: message.into() }
}

fn file_path(ctx: &ToolContext, raw: &str, tool: &str) -> Result<PathBuf> {
    let path = Path::new(raw);
    let absolute = if path.is_absolute() { path.to_path_buf() } else { Path::new(&ctx.workspace_root).join(path) };
    let canonical = std::fs::canonicalize(&absolute).map_err(|e| tool_error(tool, format!("cannot access media file: {e}")))?;
    let root = std::fs::canonicalize(&ctx.workspace_root).map_err(|e| tool_error(tool, format!("cannot resolve workspace: {e}")))?;
    if !canonical.starts_with(&root) {
        return Err(tool_error(tool, "media file must be inside the workspace"));
    }
    if !canonical.is_file() { return Err(tool_error(tool, "media path is not a regular file")); }
    Ok(canonical)
}

async fn send_telegram(ctx: &ToolContext, tool: &str, args: Value, method: &str) -> Result<Value> {
    let token = std::env::var("TELEGRAM_BOT_TOKEN").map_err(|_| tool_error(tool, "TELEGRAM_BOT_TOKEN is not configured"))?;
    let chat_id = args.get("chat_id").and_then(Value::as_str).ok_or_else(|| tool_error(tool, "chat_id is required"))?;
    let raw = args.get("path").and_then(Value::as_str).ok_or_else(|| tool_error(tool, "path is required"))?;
    let path = file_path(ctx, raw, tool)?;
    let bytes = tokio::fs::read(&path).await.map_err(|e| tool_error(tool, format!("cannot read media file: {e}")))?;
    let filename = path.file_name().and_then(|x| x.to_str()).unwrap_or("media.bin").to_string();
    let part = reqwest::multipart::Part::bytes(bytes).file_name(filename.clone());
    let form = reqwest::multipart::Form::new().text("chat_id", chat_id.to_string()).part("document", part);
    let url = format!("https://api.telegram.org/bot{token}/{method}");
    let response = reqwest::Client::new().post(url).multipart(form).send().await.map_err(|e| tool_error(tool, format!("Telegram request failed: {e}")))?;
    let status = response.status();
    let body: Value = response.json().await.map_err(|e| tool_error(tool, format!("Telegram response was not JSON: {e}")))?;
    if !status.is_success() || body.get("ok") != Some(&Value::Bool(true)) {
        return Err(tool_error(tool, format!("Telegram rejected media: {body}")));
    }
    Ok(json!({"sent": true, "channel": "telegram", "method": method, "chat_id": chat_id, "path": path, "response": body}))
}

pub struct SendDocumentTool;
#[async_trait]
impl Tool for SendDocumentTool {
    fn name(&self) -> &str { "send_document" }
    fn description(&self) -> &str { "Send a workspace file as a Telegram document" }
    fn parameters_schema(&self) -> Value { json!({"type":"object","required":["chat_id","path"],"properties":{"chat_id":{"type":"string"},"path":{"type":"string"},"caption":{"type":"string"}}}) }
    fn risk_tier(&self) -> RiskTier { RiskTier::Tier2SystemMutating }
fn requirements(&self) -> crate::registry::ToolRequirements { crate::registry::ToolRequirements { binaries: Vec::new(), environment: vec!["TELEGRAM_BOT_TOKEN".to_string()], ..Default::default() } }
async fn execute(&self, ctx: &ToolContext, args: Value) -> Result<Value> { send_telegram(ctx, self.name(), args, "sendDocument").await }
}

pub struct SendImageTool;
#[async_trait]
impl Tool for SendImageTool {
    fn name(&self) -> &str { "send_image" }
    fn description(&self) -> &str { "Send a workspace image as a Telegram photo" }
    fn parameters_schema(&self) -> Value { json!({"type":"object","required":["chat_id","path"],"properties":{"chat_id":{"type":"string"},"path":{"type":"string"},"caption":{"type":"string"}}}) }
    fn risk_tier(&self) -> RiskTier { RiskTier::Tier2SystemMutating }
fn requirements(&self) -> crate::registry::ToolRequirements { crate::registry::ToolRequirements { binaries: Vec::new(), environment: vec!["TELEGRAM_BOT_TOKEN".to_string()], ..Default::default() } }
async fn execute(&self, ctx: &ToolContext, args: Value) -> Result<Value> { send_telegram(ctx, self.name(), args, "sendPhoto").await }
}

pub struct SendMediaTool;
#[async_trait]
impl Tool for SendMediaTool {
    fn name(&self) -> &str { "send_media" }
    fn description(&self) -> &str { "Send a workspace media file through Telegram using its media type" }
    fn parameters_schema(&self) -> Value { json!({"type":"object","required":["chat_id","path","media_type"],"properties":{"chat_id":{"type":"string"},"path":{"type":"string"},"media_type":{"type":"string","enum":["document","photo","audio","video"]},"caption":{"type":"string"}}}) }
    fn risk_tier(&self) -> RiskTier { RiskTier::Tier2SystemMutating }
fn requirements(&self) -> crate::registry::ToolRequirements { crate::registry::ToolRequirements { binaries: Vec::new(), environment: vec!["TELEGRAM_BOT_TOKEN".to_string()], ..Default::default() } }
async fn execute(&self, ctx: &ToolContext, args: Value) -> Result<Value> {
        let media_type = args.get("media_type").and_then(Value::as_str).ok_or_else(|| tool_error(self.name(), "media_type is required"))?;
        let method = match media_type { "photo" => "sendPhoto", "audio" => "sendAudio", "video" => "sendVideo", "document" => "sendDocument", _ => return Err(tool_error(self.name(), "unsupported media_type")) };
        send_telegram(ctx, self.name(), args, method).await
    }
}
