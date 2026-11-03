use serde::{Deserialize, Serialize};
use tokio::sync::broadcast;

use crate::artifact::Artifact;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum AgentState {
    Idle,
    Thinking,
    CallingTool,
    AwaitingApproval,
    ExecutingTool,
    GeneratingArtifact,
    Completed,
    Failed,
    /// The turn hit its configured step cap with work still in flight.
    /// Everything so far is persisted, and a follow-up message resumes
    /// the same session from that history.
    Paused,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", content = "payload", rename_all = "snake_case")]
pub enum AgentEvent {
    StateChanged {
        session_id: String,
        state: AgentState,
    },
    TextDelta {
        session_id: String,
        delta: String,
    },
    ThoughtDelta {
        session_id: String,
        delta: String,
    },
    ToolCallStarted {
        session_id: String,
        call_id: String,
        tool_name: String,
        arguments: serde_json::Value,
    },
    ToolCallFinished {
        session_id: String,
        call_id: String,
        output: serde_json::Value,
        is_error: bool,
    },
    ArtifactCreated {
        session_id: String,
        artifact: Artifact,
    },
    ApprovalRequested {
        session_id: String,
        approval_id: String,
        tool_name: String,
        risk_tier: u8,
        description: String,
        parameters: serde_json::Value,
    },
    ApprovalResolved {
        session_id: String,
        approval_id: String,
        /// "approved" | "rejected" | "expired"
        decision: String,
    },
    SteerApplied {
        session_id: String,
        steer_id: String,
        text: String,
    },
    RelayLog {
        channel: String, // "telegram", "whatsapp", "desktop"
        message: String,
        is_ok: bool,
        timestamp: i64,
    },
    Error {
        session_id: String,
        message: String,
    },
}

pub type EventSender = broadcast::Sender<AgentEvent>;
pub type EventReceiver = broadcast::Receiver<AgentEvent>;

pub fn create_event_bus(capacity: usize) -> (EventSender, EventReceiver) {
    broadcast::channel(capacity)
}
