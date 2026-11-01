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

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MessageContent {
    pub text: String,
    pub thought: Option<String>,
    pub tool_calls: Vec<ToolCallInfo>,
    pub artifact_ids: Vec<String>,
    pub action_chips: Vec<OutputActionChip>,
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
        Self {
            id: Uuid::new_v4().to_string(),
            session_id: session_id.into(),
            role: MessageRole::User,
            content: MessageContent {
                text: text.into(),
                thought: None,
                tool_calls: Vec::new(),
                artifact_ids: Vec::new(),
                action_chips: Vec::new(),
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
        Self {
            id: Uuid::new_v4().to_string(),
            session_id: session_id.into(),
            role: MessageRole::Assistant,
            content: MessageContent {
                text: text.into(),
                thought,
                tool_calls: Vec::new(),
                artifact_ids: Vec::new(),
                action_chips: Vec::new(),
            },
            model,
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
