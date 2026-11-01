use async_trait::async_trait;
use odon_core::{AgentEvent, Result};
use tokio::sync::mpsc;
use tracing::info;

use crate::adapter::{ChannelAdapter, IncomingChannelMessage};

/// WhatsApp Channel Adapter Stub.
/// Per engineering requirements:
/// "Do not implement WhatsApp, video generation, complex automation scheduling,
/// or multi-tenant hosting before the MVP foundation passes integration tests."
pub struct WhatsAppChannel {
    phone_number: Option<String>,
}

impl Default for WhatsAppChannel {
    fn default() -> Self {
        Self::new(None)
    }
}

impl WhatsAppChannel {
    pub fn new(phone_number: Option<String>) -> Self {
        Self { phone_number }
    }

    pub fn is_configured(&self) -> bool {
        self.phone_number.is_some()
    }
}

#[async_trait]
impl ChannelAdapter for WhatsAppChannel {
    fn channel_name(&self) -> &str {
        "whatsapp"
    }

    async fn start(
        &self,
        _incoming_tx: mpsc::Sender<IncomingChannelMessage>,
    ) -> Result<()> {
        info!("WhatsApp channel interface ready (deferred until post-MVP integration tests pass)");
        Ok(())
    }

    async fn broadcast_event(&self, _event: &AgentEvent) -> Result<()> {
        Ok(())
    }

    async fn send_direct_message(&self, _recipient: &str, _content: &str) -> Result<()> {
        Ok(())
    }
}
