use serde::{Deserialize, Serialize};
use tokio::sync::broadcast;

use crate::artifact::Artifact;

// Session id bound to the async task that is currently running a turn. The
// agent runtime scopes one turn in this value; `EventSender::send` reads it to
// stamp the events that turn emits, so no event callsite and no `AgentEvent`
// variant has to carry the routing id. Task-locals do not cross
// `tokio::spawn`: a task that must attribute its events has to run inside
// `ACTIVE_SESSION_ID.scope(..)`.
tokio::task_local! {
    pub static ACTIVE_SESSION_ID: String;
}

/// The session id bound to the current task, or `None` outside a turn scope.
pub fn current_session_id() -> Option<String> {
    ACTIVE_SESSION_ID.try_with(|id| id.clone()).ok()
}

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
    /// Live progress of a delegated child session, published on the **parent**
    /// session so the parent's UI is not silent while the child works.
    ///
    /// `session_id` is the parent; `subagent_session_id` names the child.
    /// `phase` is `started`, `working`, `finished` or `failed`.
    SubagentProgress {
        session_id: String,
        subagent_session_id: String,
        phase: String,
        detail: String,
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

/// An [`AgentEvent`] plus the session it was emitted for.
///
/// The event is flattened next to the routing id, so the wire shape stays flat:
/// `{"type": "...", "payload": {...}, "session_id": "..."}`. An event emitted
/// outside any turn scope omits `session_id` entirely, which keeps its JSON
/// identical to the pre-envelope format.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AgentEventEnvelope {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub session_id: Option<String>,
    #[serde(flatten)]
    pub event: AgentEvent,
}

/// Fan-out sender for [`AgentEvent`]s.
///
/// A thin clone-able wrapper over `broadcast::Sender<AgentEventEnvelope>` that
/// stamps every outgoing event with the session bound to the current task
/// ([`ACTIVE_SESSION_ID`]). `send` takes a bare [`AgentEvent`], so callsites are
/// unchanged from the days when this was a plain broadcast sender.
#[derive(Clone, Debug)]
pub struct EventSender {
    tx: broadcast::Sender<AgentEventEnvelope>,
}

impl EventSender {
    pub fn new(tx: broadcast::Sender<AgentEventEnvelope>) -> Self {
        Self { tx }
    }

    /// Publishes `event`, stamped with the session id of the current task.
    // Mirrors `broadcast::Sender::send`, including its error, which hands the
    // whole envelope back on the (never hit in practice) no-subscriber path.
    #[allow(clippy::result_large_err)]
    pub fn send(
        &self,
        event: AgentEvent,
    ) -> std::result::Result<usize, broadcast::error::SendError<AgentEventEnvelope>> {
        self.tx.send(AgentEventEnvelope {
            session_id: current_session_id(),
            event,
        })
    }

    /// A new subscriber. Each item is an [`AgentEventEnvelope`].
    pub fn subscribe(&self) -> EventReceiver {
        self.tx.subscribe()
    }
}

/// Incoming end of the event bus: [`AgentEvent`]s wrapped in their envelope.
pub type EventReceiver = broadcast::Receiver<AgentEventEnvelope>;

pub fn create_event_bus(capacity: usize) -> (EventSender, EventReceiver) {
    let (tx, rx) = broadcast::channel(capacity);
    (EventSender::new(tx), rx)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn state_changed(session_id: &str) -> AgentEvent {
        AgentEvent::StateChanged {
            session_id: session_id.to_string(),
            state: AgentState::Thinking,
        }
    }

    /// Two turns running at once must not leak each other's session id: the
    /// stamp comes from the task the event was sent on, never from shared
    /// global state.
    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn concurrent_scopes_keep_distinct_session_ids() {
        let (tx, mut rx) = create_event_bus(16);

        let mut turns = Vec::new();
        for session_id in ["session-a", "session-b"] {
            let tx = tx.clone();
            turns.push(tokio::spawn(async move {
                ACTIVE_SESSION_ID
                    .scope(session_id.to_string(), async move {
                        tx.send(state_changed(session_id)).unwrap();
                    })
                    .await;
            }));
        }
        for turn in turns {
            turn.await.unwrap();
        }

        let mut envelopes = Vec::new();
        for _ in 0..2 {
            envelopes.push(rx.recv().await.unwrap());
        }
        envelopes.sort_by(|a, b| a.session_id.cmp(&b.session_id));

        assert_eq!(envelopes[0].session_id.as_deref(), Some("session-a"));
        assert_eq!(envelopes[1].session_id.as_deref(), Some("session-b"));
        // The routing stamp must agree with the id inside the event payload.
        for envelope in &envelopes {
            match &envelope.event {
                AgentEvent::StateChanged { session_id, .. } => {
                    assert_eq!(Some(session_id.as_str()), envelope.session_id.as_deref());
                }
                other => panic!("expected StateChanged, got {other:?}"),
            }
        }
    }

    #[tokio::test]
    async fn events_sent_outside_a_scope_carry_no_session_id() {
        let (tx, mut rx) = create_event_bus(4);

        tx.send(AgentEvent::RelayLog {
            channel: "telegram".to_string(),
            message: "relay note".to_string(),
            is_ok: true,
            timestamp: 0,
        })
        .unwrap();

        let envelope = rx.recv().await.unwrap();
        assert_eq!(envelope.session_id, None);
        assert!(matches!(envelope.event, AgentEvent::RelayLog { .. }));
    }

    /// SSE consumers key off a top-level `type` field, so the envelope must add
    /// `session_id` beside the event rather than nesting the event.
    #[test]
    fn envelope_serializes_flat_with_an_optional_session_id() {
        let scoped = AgentEventEnvelope {
            session_id: Some("session-a".to_string()),
            event: state_changed("session-a"),
        };
        let json = serde_json::to_value(&scoped).unwrap();
        assert_eq!(json["type"], "state_changed");
        assert_eq!(json["payload"]["session_id"], "session-a");
        assert_eq!(json["session_id"], "session-a");
        assert!(json.get("event").is_none(), "event must stay flat: {json}");

        let unscoped = AgentEventEnvelope {
            session_id: None,
            event: state_changed("session-b"),
        };
        let json = serde_json::to_value(&unscoped).unwrap();
        assert!(
            json.get("session_id").is_none(),
            "an unscoped event keeps the pre-envelope shape: {json}"
        );
    }

    #[test]
    fn envelope_round_trips_and_accepts_legacy_payloads() {
        let scoped = AgentEventEnvelope {
            session_id: Some("session-a".to_string()),
            event: state_changed("session-a"),
        };
        let json = serde_json::to_string(&scoped).unwrap();
        let parsed: AgentEventEnvelope = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed.session_id.as_deref(), Some("session-a"));
        assert!(matches!(parsed.event, AgentEvent::StateChanged { .. }));

        // A payload written before the envelope existed still deserializes.
        let legacy: AgentEventEnvelope = serde_json::from_str(
            r#"{"type":"state_changed","payload":{"session_id":"s1","state":"idle"}}"#,
        )
        .unwrap();
        assert_eq!(legacy.session_id, None);
        assert!(matches!(
            legacy.event,
            AgentEvent::StateChanged {
                state: AgentState::Idle,
                ..
            }
        ));
    }
}
