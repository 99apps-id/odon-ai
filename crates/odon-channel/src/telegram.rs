use async_trait::async_trait;
use base64::Engine;
use odon_core::{AgentEvent, MessageAttachment, OdonError, Result};
use reqwest::Client;
use serde_json::{json, Value};
use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use tokio::sync::mpsc;
use tracing::{error, info, warn};

use crate::adapter::{ChannelAdapter, IncomingChannelMessage};

/// Telegram Bot API rejects messages longer than 4096 UTF-16 code units.
const TELEGRAM_MAX_MESSAGE: usize = 4000;
/// Bot API `getFile` only serves files up to 20 MB.
const MAX_DOWNLOAD_BYTES: usize = 20 * 1024 * 1024;

#[derive(Clone)]
pub struct TelegramRelay {
    client: Client,
    token: Option<String>,
    is_online: Arc<AtomicBool>,
    allowed_users: Vec<String>,
    sessions: Arc<Mutex<HashMap<String, String>>>,
}

fn value_to_id(value: &Value) -> String {
    match value {
        Value::String(s) => s.clone(),
        Value::Number(n) => n.to_string(),
        other => other.to_string(),
    }
}

fn split_message(text: &str, max: usize) -> Vec<String> {
    if text.len() <= max {
        return vec![text.to_string()];
    }
    let mut chunks = Vec::new();
    let mut current = String::new();
    for line in text.split_inclusive('\n') {
        if current.len() + line.len() > max && !current.is_empty() {
            chunks.push(std::mem::take(&mut current));
        }
        if line.len() > max {
            let mut rest = line;
            while rest.len() > max {
                let mut cut = max;
                while cut > 0 && !rest.is_char_boundary(cut) {
                    cut -= 1;
                }
                chunks.push(rest[..cut].to_string());
                rest = &rest[cut..];
            }
            current.push_str(rest);
        } else {
            current.push_str(line);
        }
    }
    if !current.is_empty() {
        chunks.push(current);
    }
    chunks
}

fn looks_like_text_file(name: &str, mime: &str) -> bool {
    if mime.starts_with("text/") || mime == "application/json" || mime == "application/xml" {
        return true;
    }
    let lower = name.to_lowercase();
    [
        ".txt", ".md", ".json", ".csv", ".yaml", ".yml", ".toml", ".ini", ".py", ".rs",
        ".ts", ".tsx", ".js", ".html", ".css", ".sh", ".ps1", ".sql", ".go", ".java",
    ]
    .iter()
    .any(|ext| lower.ends_with(ext))
}

impl TelegramRelay {
    pub fn new(token: Option<String>) -> Self {
        let allowed_users = std::env::var("TELEGRAM_ALLOWED_USERS")
            .ok()
            .map(|value| {
                value
                    .split(',')
                    .map(|item| item.trim().trim_start_matches('@').to_lowercase())
                    .filter(|item| !item.is_empty())
                    .collect::<Vec<_>>()
            })
            .unwrap_or_default();
        Self::with_allowlist(token, allowed_users)
    }

    pub fn with_allowlist(token: Option<String>, allowed_users: Vec<String>) -> Self {
        let allowed_users = allowed_users
            .into_iter()
            .map(|item| item.trim().trim_start_matches('@').to_lowercase())
            .filter(|item| !item.is_empty())
            .collect::<Vec<_>>();
        let client = Client::builder()
            .connect_timeout(std::time::Duration::from_secs(20))
            // Long polling keeps the request open for `timeout` seconds.
            .timeout(std::time::Duration::from_secs(60))
            .build()
            .unwrap_or_else(|_| Client::new());
        Self {
            client,
            token,
            is_online: Arc::new(AtomicBool::new(false)),
            allowed_users,
            sessions: Arc::new(Mutex::new(HashMap::new())),
        }
    }

    pub fn online_flag(&self) -> Arc<AtomicBool> {
        self.is_online.clone()
    }

    pub fn is_connected(&self) -> bool {
        self.is_online.load(Ordering::SeqCst)
    }

    pub fn stop(&self) {
        self.is_online.store(false, Ordering::SeqCst);
    }

    async fn api_post(&self, method: &str, body: Value) -> Result<Value> {
        let token = self
            .token
            .as_ref()
            .ok_or_else(|| OdonError::Channel("Telegram bot token not configured".to_string()))?;
        let url = format!("https://api.telegram.org/bot{}/{}", token, method);
        let resp = self
            .client
            .post(&url)
            .json(&body)
            .send()
            .await
            .map_err(|e| OdonError::Channel(format!("Telegram {method} failed: {e}")))?;
        let status = resp.status();
        let text = resp
            .text()
            .await
            .map_err(|e| OdonError::Channel(format!("Telegram {method} read failed: {e}")))?;
        let val: Value = serde_json::from_str(&text)
            .map_err(|e| OdonError::Channel(format!("Telegram {method} returned invalid JSON: {e}: {text}")))?;
        if !status.is_success() {
            return Err(OdonError::Channel(format!(
                "Telegram {method} HTTP {}: {}",
                status.as_u16(),
                val.get("description").and_then(|d| d.as_str()).unwrap_or(&text)
            )));
        }
        Ok(val)
    }

    fn is_allowed(&self, message: &Value) -> bool {
        if self.allowed_users.is_empty() {
            // Secure default: without an allowlist the bot is not open to the
            // world. Operators must opt in explicitly.
            return std::env::var("TELEGRAM_ALLOW_ANYONE")
                .map(|v| matches!(v.trim().to_ascii_lowercase().as_str(), "1" | "true" | "yes"))
                .unwrap_or(false);
        }
        let from = message.get("from");
        let id = from
            .and_then(|f| f.get("id"))
            .map(value_to_id)
            .unwrap_or_default();
        let username = from
            .and_then(|f| f.get("username"))
            .and_then(|u| u.as_str())
            .unwrap_or("")
            .to_lowercase();
        self.allowed_users
            .iter()
            .any(|allowed| allowed == &id || allowed == &username)
    }

    async fn send_chat_action(&self, chat_id: &str, action: &str) {
        let _ = self
            .api_post(
                "sendChatAction",
                json!({ "chat_id": chat_id, "action": action }),
            )
            .await;
    }

    /// Send Markdown when the model output happens to be valid Telegram
    /// Markdown, but always fall back to plain text so a parse error never
    /// swallows the reply.
    async fn send_text(&self, chat_id: &str, text: &str) -> Result<()> {
        for chunk in split_message(text, TELEGRAM_MAX_MESSAGE) {
            let with_markdown = json!({
                "chat_id": chat_id,
                "text": chunk,
                "parse_mode": "Markdown",
                "disable_web_page_preview": true,
            });
            let mut delivered = false;
            if let Ok(val) = self.api_post("sendMessage", with_markdown).await {
                delivered = val.get("ok").and_then(|o| o.as_bool()).unwrap_or(false);
            }
            if !delivered {
                let plain = json!({
                    "chat_id": chat_id,
                    "text": chunk,
                    "disable_web_page_preview": true,
                });
                self.api_post("sendMessage", plain).await?;
            }
        }
        Ok(())
    }

    async fn download_file(&self, file_id: &str) -> Option<Vec<u8>> {
        let token = self.token.as_ref()?;
        let info = self.api_post("getFile", json!({ "file_id": file_id })).await.ok()?;
        let file_path = info
            .pointer("/result/file_path")
            .and_then(|p| p.as_str())?;
        let url = format!("https://api.telegram.org/file/bot{}/{}", token, file_path);
        let resp = self.client.get(&url).send().await.ok()?;
        if !resp.status().is_success() {
            warn!("Telegram file download failed: HTTP {}", resp.status());
            return None;
        }
        let bytes = resp.bytes().await.ok()?;
        if bytes.len() > MAX_DOWNLOAD_BYTES {
            warn!("Telegram file is too large: {} bytes", bytes.len());
            return None;
        }
        Some(bytes.to_vec())
    }

    async fn collect_attachments(
        &self,
        message: &Value,
        extra_text: &mut String,
    ) -> Vec<MessageAttachment> {
        let mut attachments = Vec::new();

        if let Some(photos) = message.get("photo").and_then(|p| p.as_array()) {
            if let Some(photo) = photos.last() {
                if let Some(file_id) = photo.get("file_id").and_then(|f| f.as_str()) {
                    if let Some(bytes) = self.download_file(file_id).await {
                        attachments.push(MessageAttachment {
                            name: format!(
                                "telegram-photo-{}.jpg",
                                chrono::Utc::now().timestamp()
                            ),
                            mime_type: "image/jpeg".to_string(),
                            data_base64: base64::engine::general_purpose::STANDARD.encode(bytes),
                        });
                    }
                }
            }
        }

        if let Some(document) = message.get("document") {
            let file_name = document
                .get("file_name")
                .and_then(|f| f.as_str())
                .unwrap_or("document");
            let mime = document
                .get("mime_type")
                .and_then(|m| m.as_str())
                .unwrap_or("application/octet-stream");
            if let Some(file_id) = document.get("file_id").and_then(|f| f.as_str()) {
                if let Some(bytes) = self.download_file(file_id).await {
                    if mime.starts_with("image/") || mime == "application/pdf" {
                        attachments.push(MessageAttachment {
                            name: file_name.to_string(),
                            mime_type: mime.to_string(),
                            data_base64: base64::engine::general_purpose::STANDARD.encode(bytes),
                        });
                    } else if looks_like_text_file(file_name, mime) {
                        let text = String::from_utf8_lossy(&bytes);
                        let text = if text.len() > 200_000 {
                            format!("{}...(truncated)", &text[..200_000])
                        } else {
                            text.to_string()
                        };
                        extra_text.push_str(&format!(
                            "\n\n[Attached document: {}]\n```\n{}\n```",
                            file_name, text
                        ));
                    } else {
                        extra_text.push_str(&format!(
                            "\n\n[Attached binary file: {} ({}, {} bytes) — this format cannot be read directly]",
                            file_name,
                            mime,
                            bytes.len()
                        ));
                    }
                }
            }
        }

        if message.get("voice").is_some() || message.get("audio").is_some() {
            extra_text.push_str(
                "\n\n[Voice/audio message received. Audio transcription is not enabled; please send text.]",
            );
        }
        if message.get("video").is_some() {
            extra_text.push_str("\n\n[Video message received. Video input is not supported by the text model.]");
        }

        attachments
    }

    async fn handle_message(&self, message: &Value, incoming_tx: &mpsc::Sender<IncomingChannelMessage>) {
        let Some(chat_id) = message
            .get("chat")
            .and_then(|c| c.get("id"))
            .map(value_to_id)
        else {
            return;
        };

        if !self.is_allowed(message) {
            warn!("Rejected Telegram message from unauthorized user in chat {chat_id}");
            let _ = self
                .send_text(&chat_id, "⛔ Unauthorized. Add your Telegram user ID or @username to TELEGRAM_ALLOWED_USERS.")
                .await;
            return;
        }

        let mut text = message
            .get("text")
            .and_then(|t| t.as_str())
            .or_else(|| message.get("caption").and_then(|c| c.as_str()))
            .unwrap_or("")
            .to_string();

        // Commands (allow the /cmd@botname form Telegram uses in groups).
        let command = text
            .trim()
            .split_whitespace()
            .next()
            .unwrap_or("")
            .split('@')
            .next()
            .unwrap_or("")
            .to_lowercase();
        match command.as_str() {
            "/start" | "/help" => {
                let _ = self
                    .send_text(
                        &chat_id,
                        "🤖 *ODON is online.*\n\nSend any message to run an autonomous agent turn in your workspace.\n\nCommands:\n/new — start a fresh session\n/status — show relay status\n/help — show this help",
                    )
                    .await;
                return;
            }
            "/new" | "/reset" => {
                let session = format!("tg_{}_{}", chat_id, chrono::Utc::now().timestamp());
                if let Ok(mut map) = self.sessions.lock() {
                    map.insert(chat_id.clone(), session);
                }
                let _ = self
                    .send_text(&chat_id, "🆕 New ODON session started for this chat.")
                    .await;
                return;
            }
            "/status" => {
                let _ = self
                    .send_text(
                        &chat_id,
                        "✅ ODON daemon is online and the Telegram relay is polling for updates.",
                    )
                    .await;
                return;
            }
            _ => {}
        }

        let mut extra_text = String::new();
        let attachments = self.collect_attachments(message, &mut extra_text).await;
        text.push_str(&extra_text);

        if text.trim().is_empty() && attachments.is_empty() {
            let _ = self
                .send_text(&chat_id, "Unsupported message type. Send text, an image, or a document.")
                .await;
            return;
        }

        let session_id = self
            .sessions
            .lock()
            .ok()
            .and_then(|map| map.get(&chat_id).cloned())
            .unwrap_or_else(|| format!("tg_{chat_id}"));

        self.send_chat_action(&chat_id, "typing").await;
        let _ = incoming_tx
            .send(IncomingChannelMessage {
                channel: "telegram".to_string(),
                sender_id: chat_id,
                session_id: Some(session_id),
                text,
                attachments,
            })
            .await;
    }

    async fn poll_loop(&self, incoming_tx: mpsc::Sender<IncomingChannelMessage>) {
        let Some(token) = self.token.clone() else { return };
        let mut offset = 0i64;
        let mut backoff = 1u64;
        info!("Telegram relay polling loop started");

        while self.is_online.load(Ordering::SeqCst) {
            let url = format!(
                "https://api.telegram.org/bot{}/getUpdates?offset={}&timeout=25&allowed_updates=%5B%22message%22%5D",
                token, offset
            );
            match self.client.get(&url).send().await {
                Ok(resp) => {
                    let val: Value = match resp.json().await {
                        Ok(v) => v,
                        Err(e) => {
                            error!("Telegram getUpdates parse error: {e}");
                            tokio::time::sleep(std::time::Duration::from_secs(backoff)).await;
                            backoff = (backoff * 2).min(30);
                            continue;
                        }
                    };
                    if val.get("ok").and_then(|o| o.as_bool()) != Some(true) {
                        let description = val
                            .get("description")
                            .and_then(|d| d.as_str())
                            .unwrap_or("unknown error");
                        error!("Telegram getUpdates rejected: {description}");
                        tokio::time::sleep(std::time::Duration::from_secs(backoff)).await;
                        backoff = (backoff * 2).min(30);
                        continue;
                    }
                    backoff = 1;
                    if let Some(results) = val.get("result").and_then(|r| r.as_array()) {
                        for update in results {
                            if let Some(update_id) = update.get("update_id").and_then(|u| u.as_i64()) {
                                offset = offset.max(update_id + 1);
                            }
                            if let Some(message) = update.get("message") {
                                self.handle_message(message, &incoming_tx).await;
                            }
                        }
                    }
                }
                Err(e) => {
                    error!("Telegram polling error: {e}");
                    tokio::time::sleep(std::time::Duration::from_secs(backoff)).await;
                    backoff = (backoff * 2).min(30);
                }
            }
        }
        info!("Telegram relay polling loop stopped");
    }
}

#[async_trait]
impl ChannelAdapter for TelegramRelay {
    fn channel_name(&self) -> &str {
        "telegram"
    }

    async fn start(&self, incoming_tx: mpsc::Sender<IncomingChannelMessage>) -> Result<()> {
        if self.token.is_none() {
            info!("Telegram relay disabled (no bot token provided)");
            return Ok(());
        }
        if self.allowed_users.is_empty()
            && !std::env::var("TELEGRAM_ALLOW_ANYONE")
                .map(|v| matches!(v.trim().to_ascii_lowercase().as_str(), "1" | "true" | "yes"))
                .unwrap_or(false)
        {
            warn!(
                "TELEGRAM_ALLOWED_USERS is empty: all Telegram messages will be rejected. Set the allowlist or TELEGRAM_ALLOW_ANYONE=1."
            );
        }

        // A webhook blocks getUpdates with HTTP 409. Bound the startup checks so
        // an unreachable network cannot stall daemon boot.
        let _ = tokio::time::timeout(
            std::time::Duration::from_secs(10),
            self.api_post("deleteWebhook", json!({ "drop_pending_updates": false })),
        )
        .await;

        let probe = tokio::time::timeout(
            std::time::Duration::from_secs(15),
            self.api_post("getMe", json!({})),
        )
        .await;
        match probe {
            Ok(Ok(val)) if val.get("ok").and_then(|o| o.as_bool()) == Some(true) => {
                let username = val
                    .pointer("/result/username")
                    .and_then(|u| u.as_str())
                    .unwrap_or("unknown");
                info!("Telegram bot @{username} authenticated");
            }
            Ok(Ok(val)) => {
                let description = val
                    .get("description")
                    .and_then(|d| d.as_str())
                    .unwrap_or("invalid token");
                return Err(OdonError::Channel(format!(
                    "Telegram getMe failed: {description}"
                )));
            }
            Ok(Err(e)) => {
                // Keep the daemon alive: the network may come back later.
                warn!("Telegram getMe could not complete: {e}");
            }
            Err(_) => {
                warn!("Telegram getMe timed out; polling will retry in the background");
            }
        }

        self.is_online.store(true, Ordering::SeqCst);
        let relay = self.clone();
        tokio::spawn(async move {
            relay.poll_loop(incoming_tx).await;
        });
        Ok(())
    }

    async fn broadcast_event(&self, event: &AgentEvent) -> Result<()> {
        // Replies are sent from the daemon's relay dispatcher once a turn
        // completes; no per-token streaming to Telegram.
        if let AgentEvent::Error { message, .. } = event {
            warn!("Agent error during Telegram turn: {message}");
        }
        Ok(())
    }

    async fn send_direct_message(&self, recipient: &str, content: &str) -> Result<()> {
        self.send_text(recipient, content).await
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn splits_long_messages_on_boundaries() {
        let text = "a".repeat(5000);
        let chunks = split_message(&text, 1000);
        assert_eq!(chunks.len(), 5);
        assert!(chunks.iter().all(|c| c.len() <= 1000));
    }

    #[test]
    fn value_ids_do_not_gain_quotes() {
        assert_eq!(value_to_id(&json!(12345)), "12345");
        assert_eq!(value_to_id(&json!("abc")), "abc");
    }
}
