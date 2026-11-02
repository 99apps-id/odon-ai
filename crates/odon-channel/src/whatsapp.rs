//! WhatsApp Cloud API (Meta Graph API) relay.
//!
//! Inbound messages arrive through the daemon webhook
//! (`GET/POST /api/relays/whatsapp`) because the Cloud API is push-based.
//! Outbound replies use `POST /{phone_number_id}/messages`.

use async_trait::async_trait;
use base64::Engine;
use odon_core::{AgentEvent, MessageAttachment, OdonError, Result};
use reqwest::Client;
use serde_json::{json, Value};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use tokio::sync::mpsc;
use tracing::{info, warn};

use crate::adapter::{ChannelAdapter, IncomingChannelMessage};

const MAX_MESSAGE: usize = 3500;
const MAX_DOWNLOAD_BYTES: usize = 20 * 1024 * 1024;

#[derive(Clone)]
pub struct WhatsAppChannel {
    client: Client,
    access_token: Option<String>,
    phone_number_id: Option<String>,
    api_version: String,
    verify_token: Option<String>,
    app_secret: Option<String>,
    allowed_numbers: Vec<String>,
    is_online: Arc<AtomicBool>,
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

fn clean_recipient(recipient: &str) -> String {
    recipient
        .trim()
        .trim_start_matches("whatsapp:")
        .trim_start_matches('+')
        .chars()
        .filter(|c| c.is_ascii_digit())
        .collect()
}

fn looks_like_text_file(name: &str, mime: &str) -> bool {
    if mime.starts_with("text/") || mime == "application/json" {
        return true;
    }
    let lower = name.to_lowercase();
    [
        ".txt", ".md", ".json", ".csv", ".yaml", ".yml", ".py", ".rs", ".ts", ".js", ".html",
        ".css", ".sh", ".sql",
    ]
    .iter()
    .any(|ext| lower.ends_with(ext))
}

impl Default for WhatsAppChannel {
    fn default() -> Self {
        Self::from_env()
    }
}

impl WhatsAppChannel {
    pub fn new(access_token: Option<String>, phone_number_id: Option<String>) -> Self {
        let api_version =
            std::env::var("WHATSAPP_API_VERSION").unwrap_or_else(|_| "v21.0".to_string());
        let verify_token = std::env::var("WHATSAPP_VERIFY_TOKEN")
            .ok()
            .filter(|v| !v.is_empty());
        let app_secret = std::env::var("WHATSAPP_APP_SECRET")
            .ok()
            .filter(|v| !v.is_empty());
        let allowed_numbers = std::env::var("WHATSAPP_ALLOWED_NUMBERS")
            .ok()
            .map(|value| {
                value
                    .split(',')
                    .map(clean_recipient)
                    .filter(|item| !item.is_empty())
                    .collect::<Vec<_>>()
            })
            .unwrap_or_default();
        let client = Client::builder()
            .connect_timeout(std::time::Duration::from_secs(20))
            .timeout(std::time::Duration::from_secs(60))
            .build()
            .unwrap_or_else(|_| Client::new());
        Self {
            client,
            access_token: access_token.filter(|v| !v.trim().is_empty()),
            phone_number_id: phone_number_id.filter(|v| !v.trim().is_empty()),
            api_version,
            verify_token,
            app_secret,
            allowed_numbers,
            is_online: Arc::new(AtomicBool::new(false)),
        }
    }

    pub fn from_env() -> Self {
        Self::new(
            std::env::var("WHATSAPP_ACCESS_TOKEN").ok(),
            std::env::var("WHATSAPP_PHONE_NUMBER_ID").ok(),
        )
    }

    pub fn is_configured(&self) -> bool {
        self.access_token.is_some() && self.phone_number_id.is_some()
    }

    pub fn online_flag(&self) -> Arc<AtomicBool> {
        self.is_online.clone()
    }

    pub fn verify_token(&self) -> Option<&str> {
        self.verify_token.as_deref()
    }

    pub fn app_secret(&self) -> Option<&str> {
        self.app_secret.as_deref()
    }

    fn is_allowed(&self, from: &str) -> bool {
        if self.allowed_numbers.is_empty() {
            return std::env::var("WHATSAPP_ALLOW_ANYONE")
                .map(|v| matches!(v.trim().to_ascii_lowercase().as_str(), "1" | "true" | "yes"))
                .unwrap_or(false);
        }
        self.allowed_numbers.iter().any(|n| n == from)
    }

    async fn send_text(&self, to: &str, text: &str) -> Result<()> {
        let Some(token) = self.access_token.as_ref() else {
            return Err(OdonError::Channel(
                "WhatsApp relay is not configured (missing WHATSAPP_ACCESS_TOKEN)".to_string(),
            ));
        };
        let Some(phone_id) = self.phone_number_id.as_ref() else {
            return Err(OdonError::Channel(
                "WhatsApp relay is not configured (missing WHATSAPP_PHONE_NUMBER_ID)".to_string(),
            ));
        };
        let recipient = clean_recipient(to);
        if recipient.is_empty() {
            return Err(OdonError::Channel(format!(
                "Invalid WhatsApp recipient: {to}"
            )));
        }

        for chunk in split_message(text, MAX_MESSAGE) {
            let url = format!(
                "https://graph.facebook.com/{}/{}/messages",
                self.api_version, phone_id
            );
            let resp = self
                .client
                .post(&url)
                .bearer_auth(token)
                .json(&json!({
                    "messaging_product": "whatsapp",
                    "recipient_type": "individual",
                    "to": recipient,
                    "type": "text",
                    "text": { "preview_url": false, "body": chunk }
                }))
                .send()
                .await
                .map_err(|e| OdonError::Channel(format!("WhatsApp send failed: {e}")))?;
            let status = resp.status();
            let body = resp.text().await.unwrap_or_default();
            if !status.is_success() {
                return Err(OdonError::Channel(format!(
                    "WhatsApp send HTTP {}: {}",
                    status.as_u16(),
                    body
                )));
            }
        }
        Ok(())
    }

    async fn download_media(&self, media_id: &str) -> Option<(Vec<u8>, String)> {
        let token = self.access_token.as_ref()?;
        let meta_url = format!(
            "https://graph.facebook.com/{}/{}",
            self.api_version, media_id
        );
        let meta = self
            .client
            .get(&meta_url)
            .bearer_auth(token)
            .send()
            .await
            .ok()?;
        if !meta.status().is_success() {
            warn!("WhatsApp media metadata failed: HTTP {}", meta.status());
            return None;
        }
        let meta_json: Value = meta.json().await.ok()?;
        let url = meta_json.get("url").and_then(|u| u.as_str())?;
        let mime = meta_json
            .get("mime_type")
            .and_then(|m| m.as_str())
            .unwrap_or("application/octet-stream")
            .to_string();

        let resp = self.client.get(url).bearer_auth(token).send().await.ok()?;
        if !resp.status().is_success() {
            warn!("WhatsApp media download failed: HTTP {}", resp.status());
            return None;
        }
        let bytes = resp.bytes().await.ok()?.to_vec();
        if bytes.len() > MAX_DOWNLOAD_BYTES {
            warn!("WhatsApp media exceeds size limit: {} bytes", bytes.len());
            return None;
        }
        Some((bytes, mime))
    }

    async fn collect_attachments(
        &self,
        message: &Value,
        extra_text: &mut String,
    ) -> Vec<MessageAttachment> {
        let mut attachments = Vec::new();

        for kind in ["image", "document"] {
            let Some(media) = message.get(kind) else {
                continue;
            };
            let Some(media_id) = media.get("id").and_then(|i| i.as_str()) else {
                continue;
            };
            let Some((bytes, mime)) = self.download_media(media_id).await else {
                extra_text.push_str(&format!(
                    "\n\n[Attached {kind} could not be downloaded from WhatsApp]"
                ));
                continue;
            };
            let file_name = media
                .get("filename")
                .and_then(|f| f.as_str())
                .unwrap_or_else(|| {
                    if kind == "image" {
                        "whatsapp-image.jpg"
                    } else {
                        "whatsapp-document"
                    }
                })
                .to_string();

            if mime.starts_with("image/") || mime == "application/pdf" {
                attachments.push(MessageAttachment {
                    name: file_name,
                    mime_type: mime,
                    data_base64: base64::engine::general_purpose::STANDARD.encode(bytes),
                });
            } else if looks_like_text_file(&file_name, &mime) {
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

        if message.get("audio").is_some() || message.get("voice").is_some() {
            extra_text.push_str(
                "\n\n[Voice/audio message received. Audio transcription is not enabled; please send text.]",
            );
        }
        attachments
    }

    /// Parse a Meta webhook payload into channel messages. Status callbacks and
    /// reactions are ignored.
    pub async fn parse_webhook(&self, payload: &Value) -> Vec<IncomingChannelMessage> {
        let mut out = Vec::new();
        let Some(entries) = payload.get("entry").and_then(|e| e.as_array()) else {
            return out;
        };
        for entry in entries {
            let Some(changes) = entry.get("changes").and_then(|c| c.as_array()) else {
                continue;
            };
            for change in changes {
                let Some(value) = change.get("value") else {
                    continue;
                };
                if value.get("messaging_product").and_then(|m| m.as_str()) != Some("whatsapp") {
                    continue;
                }
                let Some(messages) = value.get("messages").and_then(|m| m.as_array()) else {
                    continue;
                };
                for message in messages {
                    let Some(from) = message.get("from").and_then(|f| f.as_str()) else {
                        continue;
                    };
                    if !self.is_allowed(from) {
                        warn!("Rejected WhatsApp message from unauthorized number {from}");
                        continue;
                    }

                    let mut text = message
                        .pointer("/text/body")
                        .and_then(|t| t.as_str())
                        .or_else(|| message.pointer("/button/text").and_then(|t| t.as_str()))
                        .or_else(|| {
                            message
                                .pointer("/interactive/button_reply/title")
                                .and_then(|t| t.as_str())
                        })
                        .or_else(|| {
                            message
                                .pointer("/interactive/list_reply/title")
                                .and_then(|t| t.as_str())
                        })
                        .or_else(|| message.get("caption").and_then(|c| c.as_str()))
                        .unwrap_or("")
                        .to_string();

                    let mut extra_text = String::new();
                    let attachments = self.collect_attachments(message, &mut extra_text).await;
                    text.push_str(&extra_text);
                    if text.trim().is_empty() && attachments.is_empty() {
                        continue;
                    }

                    out.push(IncomingChannelMessage {
                        channel: "whatsapp".to_string(),
                        sender_id: from.to_string(),
                        session_id: Some(format!("wa_{from}")),
                        text,
                        attachments,
                    });
                }
            }
        }
        out
    }
}

#[async_trait]
impl ChannelAdapter for WhatsAppChannel {
    fn channel_name(&self) -> &str {
        "whatsapp"
    }

    async fn start(&self, _incoming_tx: mpsc::Sender<IncomingChannelMessage>) -> Result<()> {
        if !self.is_configured() {
            info!(
                "WhatsApp relay disabled (set WHATSAPP_ACCESS_TOKEN and WHATSAPP_PHONE_NUMBER_ID)"
            );
            return Ok(());
        }
        if self.allowed_numbers.is_empty()
            && !std::env::var("WHATSAPP_ALLOW_ANYONE")
                .map(|v| matches!(v.trim().to_ascii_lowercase().as_str(), "1" | "true" | "yes"))
                .unwrap_or(false)
        {
            warn!(
                "WHATSAPP_ALLOWED_NUMBERS is empty: all inbound WhatsApp messages will be rejected. Set the allowlist or WHATSAPP_ALLOW_ANYONE=1."
            );
        }
        self.is_online.store(true, Ordering::SeqCst);
        info!(
            "WhatsApp Cloud API relay ready (phone_number_id={}, webhook=/api/relays/whatsapp)",
            self.phone_number_id.as_deref().unwrap_or("?")
        );
        Ok(())
    }

    async fn broadcast_event(&self, _event: &AgentEvent) -> Result<()> {
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
    fn normalizes_phone_recipients() {
        assert_eq!(clean_recipient("+62 812-3456-7890"), "6281234567890");
        assert_eq!(clean_recipient("whatsapp:6281234567890"), "6281234567890");
    }

    #[test]
    fn splits_long_text() {
        let chunks = split_message(&"x".repeat(8000), 3000);
        assert!(chunks.len() >= 3);
        assert!(chunks.iter().all(|c| c.len() <= 3000));
    }

    #[tokio::test]
    async fn parses_text_webhook_payload() {
        let channel = WhatsAppChannel {
            client: Client::new(),
            access_token: None,
            phone_number_id: None,
            api_version: "v21.0".to_string(),
            verify_token: None,
            app_secret: None,
            allowed_numbers: vec!["628111222333".to_string()],
            is_online: Arc::new(AtomicBool::new(false)),
        };
        let payload = json!({
            "entry": [{
                "changes": [{
                    "value": {
                        "messaging_product": "whatsapp",
                        "messages": [{
                            "from": "628111222333",
                            "type": "text",
                            "text": { "body": "hello odon" }
                        }]
                    }
                }]
            }]
        });
        let messages = channel.parse_webhook(&payload).await;
        assert_eq!(messages.len(), 1);
        assert_eq!(messages[0].channel, "whatsapp");
        assert_eq!(messages[0].sender_id, "628111222333");
        assert_eq!(messages[0].session_id.as_deref(), Some("wa_628111222333"));
        assert_eq!(messages[0].text, "hello odon");
        assert!(messages[0].attachments.is_empty());
    }
}
