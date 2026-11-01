use async_trait::async_trait;
use odon_core::{AgentEvent, OdonError, Result};
use reqwest::Client;
use serde_json::{json, Value};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use tokio::sync::mpsc;
use tracing::{error, info};

use crate::adapter::{ChannelAdapter, IncomingChannelMessage};

pub struct TelegramRelay {
    client: Client,
    token: Option<String>,
    is_online: Arc<AtomicBool>,
}

impl TelegramRelay {
    pub fn new(token: Option<String>) -> Self {
        Self {
            client: Client::new(),
            token,
            is_online: Arc::new(AtomicBool::new(false)),
        }
    }

    pub fn is_connected(&self) -> bool {
        self.is_online.load(Ordering::SeqCst)
    }

    async fn api_post(&self, method: &str, body: Value) -> Result<Value> {
        let token = self.token.as_ref().ok_or_else(|| {
            OdonError::Channel("Telegram bot token not configured".to_string())
        })?;

        let url = format!("https://api.telegram.org/bot{}/{}", token, method);
        let resp = self
            .client
            .post(&url)
            .json(&body)
            .send()
            .await
            .map_err(|e| OdonError::Channel(format!("Telegram API call failed: {}", e)))?;

        let val = resp
            .json::<Value>()
            .await
            .map_err(|e| OdonError::Channel(format!("Failed to parse Telegram response: {}", e)))?;

        Ok(val)
    }
}

#[async_trait]
impl ChannelAdapter for TelegramRelay {
    fn channel_name(&self) -> &str {
        "telegram"
    }

    async fn start(
        &self,
        incoming_tx: mpsc::Sender<IncomingChannelMessage>,
    ) -> Result<()> {
        if self.token.is_none() {
            info!("Telegram relay disabled (no bot token provided)");
            return Ok(());
        }

        self.is_online.store(true, Ordering::SeqCst);
        let token = self.token.clone().unwrap();
        let client = self.client.clone();
        let online = self.is_online.clone();

        tokio::spawn(async move {
            info!("Telegram relay polling loop started");
            let mut offset = 0i64;

            while online.load(Ordering::SeqCst) {
                let url = format!(
                    "https://api.telegram.org/bot{}/getUpdates?offset={}&timeout=30",
                    token, offset
                );

                match client.get(&url).send().await {
                    Ok(resp) => {
                        if let Ok(val) = resp.json::<Value>().await {
                            if let Some(results) = val.get("result").and_then(|r| r.as_array()) {
                                for update in results {
                                    if let Some(update_id) = update.get("update_id").and_then(|u| u.as_i64()) {
                                        offset = update_id + 1;
                                    }
                                    if let Some(msg) = update.get("message") {
                                        if let (Some(chat_id), Some(text)) = (
                                            msg.get("chat").and_then(|c| c.get("id")).map(|id| id.to_string()),
                                            msg.get("text").and_then(|t| t.as_str()),
                                        ) {
                                            let _ = incoming_tx.send(IncomingChannelMessage {
                                                channel: "telegram".to_string(),
                                                sender_id: chat_id,
                                                session_id: None,
                                                text: text.to_string(),
                                            }).await;
                                        }
                                    }
                                }
                            }
                        }
                    }
                    Err(e) => {
                        error!("Telegram polling error: {}", e);
                        tokio::time::sleep(tokio::time::Duration::from_secs(5)).await;
                    }
                }
            }
        });

        Ok(())
    }

    async fn broadcast_event(&self, _event: &AgentEvent) -> Result<()> {
        // Can format and forward major events to active paired chat
        Ok(())
    }

    async fn send_direct_message(&self, recipient: &str, content: &str) -> Result<()> {
        self.api_post(
            "sendMessage",
            json!({
                "chat_id": recipient,
                "text": content,
                "parse_mode": "Markdown",
            }),
        )
        .await?;
        Ok(())
    }
}
