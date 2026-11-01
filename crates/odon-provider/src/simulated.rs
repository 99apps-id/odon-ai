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
        if !has_tool_results && (last_user_msg.contains("system") || last_user_msg.contains("environment") || last_user_msg.contains("info")) {
            let _ = event_sender.send(StreamEvent::ToolCallDelta {
                index: 0,
                id: Some("call_sys_info_1".to_string()),
                name: Some("system_info".to_string()),
                arguments_delta: Some("{}".to_string()),
            }).await;
            let _ = event_sender.send(StreamEvent::Done).await;
            return Ok(());
        }

        if !has_tool_results && (last_user_msg.contains("list file") || last_user_msg.contains("dir") || last_user_msg.contains("check files")) {
            let _ = event_sender.send(StreamEvent::ToolCallDelta {
                index: 0,
                id: Some("call_fs_list_1".to_string()),
                name: Some("fs_list".to_string()),
                arguments_delta: Some(r#"{"path":"."}"#.to_string()),
            }).await;
            let _ = event_sender.send(StreamEvent::Done).await;
            return Ok(());
        }

        // If the user requested code, app, or personal knowledge hub:
        let response_text = if last_user_msg.contains("knowledge hub") || last_user_msg.contains("build") || last_user_msg.contains("react") {
            "I'm on it! I will build the **Personal Knowledge Hub** project end-to-end.\n\n\
            Here is what has been assembled for your workspace:\n\n\
            1. **Core Application**: A lightweight, responsive React 19 note-taking app with tagging, full-text search, and local persistence.\n\
            2. **Product Requirement Document (PRD)**: Comprehensive specification covering goals, persona workflows, API contracts, and non-functional requirements.\n\
            3. **Visual Mockups**: High-resolution interface preview and hero branding cards.\n\n\
            You can preview, modify, or download the generated artifacts in the Artifacts panel to the right."
        } else if last_user_msg.contains("search") || last_user_msg.contains("cari") {
            "Here are the findings based on your inquiry. The parameters have been checked against the active workspace knowledge base and online documentation. Let me know if you would like me to drill down further or write a tailored script."
        } else if has_tool_results {
            "Tool execution completed successfully. The environment and workspace indicators have been recorded into the context session."
        } else {
            "Hello! I am **ODON (Open Digital Orchestration Network)**, your autonomous AI desktop assistant.\n\n\
            I can help you:\n\
            - **Code & Build**: Generate fullstack web apps, scripts, or debug existing codebases.\n\
            - **Research & Write**: Produce PRDs, architecture specifications, and technical reports.\n\
            - **Automate**: Run background shell operations, git workflows, and connect to relays (CLI, Telegram, WhatsApp).\n\n\
            What would you like to work on today?"
        };

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
