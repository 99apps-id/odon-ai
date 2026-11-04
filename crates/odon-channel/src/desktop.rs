use async_trait::async_trait;
use odon_core::{AgentEvent, EventSender, Result};
use std::sync::Arc;
use tokio::sync::{mpsc, RwLock};

use crate::adapter::{ChannelAdapter, IncomingChannelMessage};

pub struct DesktopChannel {
    event_tx: EventSender,
    incoming_tx: Arc<RwLock<Option<mpsc::Sender<IncomingChannelMessage>>>>,
}

impl DesktopChannel {
    pub fn new(event_tx: EventSender) -> Self {
        Self {
            event_tx,
            incoming_tx: Arc::new(RwLock::new(None)),
        }
    }

    pub async fn submit_user_message(&self, session_id: &str, text: &str) -> Result<()> {
        let lock = self.incoming_tx.read().await;
        if let Some(tx) = lock.as_ref() {
            let _ = tx
                .send(IncomingChannelMessage {
                    channel: "desktop".to_string(),
                    sender_id: "desktop_user".to_string(),
                    session_id: Some(session_id.to_string()),
                    text: text.to_string(),
                    attachments: Vec::new(),
                })
                .await;
        }
        Ok(())
    }
}

#[async_trait]
impl ChannelAdapter for DesktopChannel {
    fn channel_name(&self) -> &str {
        "desktop"
    }

    async fn start(&self, incoming_tx: mpsc::Sender<IncomingChannelMessage>) -> Result<()> {
        let mut lock = self.incoming_tx.write().await;
        *lock = Some(incoming_tx);
        Ok(())
    }

    async fn broadcast_event(&self, event: &AgentEvent) -> Result<()> {
        let _ = self.event_tx.send(event.clone());
        Ok(())
    }

    async fn send_direct_message(&self, _recipient: &str, _content: &str) -> Result<()> {
        Ok(())
    }
}
