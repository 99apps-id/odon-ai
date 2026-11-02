use async_trait::async_trait;
use odon_core::{AgentEvent, MessageAttachment, Result};
use serde::{Deserialize, Serialize};
use tokio::sync::mpsc;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct IncomingChannelMessage {
    pub channel: String,
    pub sender_id: String,
    pub session_id: Option<String>,
    pub text: String,
    #[serde(default)]
    pub attachments: Vec<MessageAttachment>,
}

#[async_trait]
pub trait ChannelAdapter: Send + Sync {
    fn channel_name(&self) -> &str;

    async fn start(
        &self,
        incoming_tx: mpsc::Sender<IncomingChannelMessage>,
    ) -> Result<()>;

    async fn broadcast_event(&self, event: &AgentEvent) -> Result<()>;

    async fn send_direct_message(&self, recipient: &str, content: &str) -> Result<()>;
}
