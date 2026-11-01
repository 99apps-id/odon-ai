use async_trait::async_trait;
use odon_core::{Message, Result};
use serde::{Deserialize, Serialize};
use tokio_util::sync::CancellationToken;

use crate::stream::StreamEvent;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolDefinition {
    pub name: String,
    pub description: String,
    pub parameters: serde_json::Value,
}

#[derive(Debug, Clone)]
pub struct ChatRequest {
    pub messages: Vec<Message>,
    pub model: String,
    pub system_prompt: Option<String>,
    pub tools: Vec<ToolDefinition>,
    pub temperature: Option<f32>,
    pub max_tokens: Option<u32>,
    /// Reasoning effort: low | medium | high | xhigh. None = provider default.
    pub effort: Option<String>,
}

#[async_trait]
pub trait ModelProvider: Send + Sync {
    fn provider_name(&self) -> &str;
    
    async fn stream_chat(
        &self,
        request: ChatRequest,
        event_sender: tokio::sync::mpsc::Sender<StreamEvent>,
        cancellation: CancellationToken,
    ) -> Result<()>;
}
