use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum MessageRole {
    User,
    Assistant,
    System,
    Tool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OutputActionChip {
    pub id: String,
    pub label: String,
    pub icon: Option<String>,
    pub action_type: String, // e.g. "preview_run", "create_git_repo", "create_automation", "refine_prd"
    pub payload: Option<serde_json::Value>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolCallInfo {
    pub call_id: String,
    pub tool_name: String,
    pub arguments: serde_json::Value,
    pub output: Option<serde_json::Value>,
    pub is_error: bool,
    pub requires_approval: bool,
    pub is_approved: Option<bool>,
}

/// A binary attachment supplied with a user turn. `data_base64` is the raw
/// base64 payload without a `data:<mime>;base64,` prefix so every provider can
/// rebuild whichever multimodal envelope it needs.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct MessageAttachment {
    pub name: String,
    pub mime_type: String,
    pub data_base64: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MessageContent {
    pub text: String,
    pub thought: Option<String>,
    /// Opaque provider signature that must accompany `thought` when the
    /// history is replayed (Anthropic extended thinking, Gemini thought
    /// signatures). Providers that sign their reasoning refuse a replayed
    /// turn whose signature is missing or fabricated, so the runtime stores
    /// the value it received and the request builders either replay it
    /// verbatim or drop the reasoning block entirely.
    #[serde(default)]
    pub thought_signature: Option<String>,
    pub tool_calls: Vec<ToolCallInfo>,
    pub artifact_ids: Vec<String>,
    pub action_chips: Vec<OutputActionChip>,
    #[serde(default)]
    pub attachments: Vec<MessageAttachment>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Message {
    pub id: String,
    pub session_id: String,
    pub role: MessageRole,
    pub content: MessageContent,
    pub model: Option<String>,
    pub created_at: DateTime<Utc>,
}

impl Message {
    pub fn new_user(session_id: impl Into<String>, text: impl Into<String>) -> Self {
        Self::new_user_with_attachments(session_id, text, Vec::new())
    }

    pub fn new_user_with_attachments(
        session_id: impl Into<String>,
        text: impl Into<String>,
        attachments: Vec<MessageAttachment>,
    ) -> Self {
        Self {
            id: Uuid::new_v4().to_string(),
            session_id: session_id.into(),
            role: MessageRole::User,
            content: MessageContent {
                text: text.into(),
                thought: None,
                thought_signature: None,
                tool_calls: Vec::new(),
                artifact_ids: Vec::new(),
                action_chips: Vec::new(),
                attachments,
            },
            model: None,
            created_at: Utc::now(),
        }
    }

    pub fn new_assistant(
        session_id: impl Into<String>,
        text: impl Into<String>,
        thought: Option<String>,
        model: Option<String>,
    ) -> Self {
        Self::new_assistant_signed(session_id, text, thought, None, model)
    }

    /// [`Message::new_assistant`], carrying the provider signature that belongs
    /// with `thought` so a replaying provider can accept the reasoning block.
    pub fn new_assistant_signed(
        session_id: impl Into<String>,
        text: impl Into<String>,
        thought: Option<String>,
        thought_signature: Option<String>,
        model: Option<String>,
    ) -> Self {
        Self {
            id: Uuid::new_v4().to_string(),
            session_id: session_id.into(),
            role: MessageRole::Assistant,
            content: MessageContent {
                text: text.into(),
                thought,
                thought_signature,
                tool_calls: Vec::new(),
                artifact_ids: Vec::new(),
                action_chips: Vec::new(),
                attachments: Vec::new(),
            },
            model,
            created_at: Utc::now(),
        }
    }

    pub fn new_tool(
        session_id: impl Into<String>,
        call_id: impl Into<String>,
        content: impl Into<String>,
    ) -> Self {
        let cid = call_id.into();
        Self {
            id: Uuid::new_v4().to_string(),
            session_id: session_id.into(),
            role: MessageRole::Tool,
            content: MessageContent {
                text: content.into(),
                thought: None,
                thought_signature: None,
                tool_calls: vec![ToolCallInfo {
                    call_id: cid,
                    tool_name: String::new(),
                    arguments: serde_json::Value::Null,
                    output: None,
                    is_error: false,
                    requires_approval: false,
                    is_approved: Some(true),
                }],
                artifact_ids: Vec::new(),
                action_chips: Vec::new(),
                attachments: Vec::new(),
            },
            model: None,
            created_at: Utc::now(),
        }
    }

    pub fn with_artifact(mut self, artifact_id: impl Into<String>) -> Self {
        self.content.artifact_ids.push(artifact_id.into());
        self
    }

    pub fn with_chip(
        mut self,
        id: impl Into<String>,
        label: impl Into<String>,
        action_type: impl Into<String>,
        icon: Option<String>,
    ) -> Self {
        self.content.action_chips.push(OutputActionChip {
            id: id.into(),
            label: label.into(),
            action_type: action_type.into(),
            icon,
            payload: None,
        });
        self
    }
}
