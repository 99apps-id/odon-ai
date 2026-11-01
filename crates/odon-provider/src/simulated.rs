use async_trait::async_trait;
use odon_core::{MessageRole, Result};
use tokio::sync::mpsc::Sender;
use tokio_util::sync::CancellationToken;

use crate::stream::StreamEvent;
use crate::trait_def::{ChatRequest, ModelProvider};

pub struct SimulatedProvider {
    name: String,
}

impl Default for SimulatedProvider {
    fn default() -> Self {
        Self::new("odon-simulated")
    }
}

impl SimulatedProvider {
    pub fn new(name: impl Into<String>) -> Self {
        Self { name: name.into() }
    }
}

#[async_trait]
impl ModelProvider for SimulatedProvider {
    fn provider_name(&self) -> &str {
        &self.name
    }

    async fn stream_chat(
        &self,
        request: ChatRequest,
        event_sender: Sender<StreamEvent>,
        cancellation: CancellationToken,
    ) -> Result<()> {
        let last_user_msg = request
            .messages
            .iter()
            .rev()
            .find(|m| m.role == MessageRole::User)
            .map(|m| m.content.text.to_lowercase())
            .unwrap_or_default();

        let has_tool_results = request.messages.iter().any(|m| m.role == MessageRole::Tool);

        // Simulate short thinking delay
        let _ = event_sender
            .send(StreamEvent::ThoughtDelta(
                "Analyzing user intent, evaluating tools and synthesizing response...".to_string(),
            ))
            .await;
        tokio::time::sleep(std::time::Duration::from_millis(50)).await;

        if cancellation.is_cancelled() {
            let _ = event_sender.send(StreamEvent::Done).await;
            return Ok(());
        }

        // If the user asked to inspect files, system, or run tools and we haven't executed a tool yet:
        if !has_tool_results && (last_user_msg.contains("system") || last_user_msg.contains("sistem") || last_user_msg.contains("environment") || last_user_msg.contains("info")) {
            let call_id = format!("call_sys_info_{}", uuid::Uuid::new_v4().simple());
            let _ = event_sender.send(StreamEvent::ToolCallDelta {
                index: 0,
                id: Some(call_id),
                name: Some("system_info".to_string()),
                arguments_delta: Some("{}".to_string()),
            }).await;
            let _ = event_sender.send(StreamEvent::Done).await;
            return Ok(());
        }

        if !has_tool_results && (last_user_msg.contains("list file") || last_user_msg.contains("dir") || last_user_msg.contains("check files") || last_user_msg.contains("berkas") || last_user_msg.contains("daftar file")) {
            let call_id = format!("call_fs_list_{}", uuid::Uuid::new_v4().simple());
            let _ = event_sender.send(StreamEvent::ToolCallDelta {
                index: 0,
                id: Some(call_id),
                name: Some("fs_list".to_string()),
                arguments_delta: Some(r#"{"path":"."}"#.to_string()),
            }).await;
            let _ = event_sender.send(StreamEvent::Done).await;
            return Ok(());
        }

        let _latest_tool_output = request
            .messages
            .iter()
            .rev()
            .find(|m| m.role == MessageRole::Tool)
            .map(|m| m.content.text.clone());

        // Simulated provider is intentionally a stub. Return a clear guidance message instead of fake content.
        let response_text = "ODON is currently in **Simulated Mode** because no real provider is configured.\n\n\
        To get real AI responses, do one of the following:\n\n\
        1. Open **Settings → Providers** and connect an OAuth provider (GitHub Copilot, Antigravity, xAI, Meta Muse, Claude, or ChatGPT).\n\
        2. Or add a **BYOK API key** for OpenAI, Anthropic, Google Gemini, DeepSeek, Qwen, Mistral, or any OpenAI-compatible endpoint.\n\n\
        After that, select the model from the **model picker** and send your message again.";

        // Stream response in small words
        for chunk in response_text.split_inclusive(' ') {
            if cancellation.is_cancelled() {
                break;
            }
            let _ = event_sender.send(StreamEvent::TextDelta(chunk.to_string())).await;
            tokio::time::sleep(std::time::Duration::from_millis(15)).await;
        }

        let _ = event_sender.send(StreamEvent::Done).await;
        Ok(())
    }
}
