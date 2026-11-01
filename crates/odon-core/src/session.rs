use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum SessionMode {
    General,
    Code,
    Create,
    Documents,
    Agents,
    Automations,
}

impl Default for SessionMode {
    fn default() -> Self {
        Self::General
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum SessionStatus {
    Active,
    Archived,
    Idle,
    Running,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Session {
    pub id: String,
    pub title: String,
    pub mode: SessionMode,
    pub status: SessionStatus,
    pub active_model: String,
    pub workspace_root: Option<String>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

impl Session {
    pub fn new(title: impl Into<String>, mode: SessionMode, active_model: impl Into<String>) -> Self {
        let now = Utc::now();
        Self {
            id: Uuid::new_v4().to_string(),
            title: title.into(),
            mode,
            status: SessionStatus::Active,
            active_model: active_model.into(),
            workspace_root: None,
            created_at: now,
            updated_at: now,
        }
    }
}
