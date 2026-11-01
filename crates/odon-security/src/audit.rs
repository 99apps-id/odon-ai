use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use tokio::sync::RwLock;
use uuid::Uuid;

use crate::approval::RiskTier;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AuditEvent {
    pub id: String,
    pub session_id: String,
    pub tool_name: String,
    pub risk_tier: RiskTier,
    pub description: String,
    pub approved: bool,
    pub success: bool,
    pub execution_duration_ms: u64,
    pub timestamp: DateTime<Utc>,
}

#[derive(Clone, Default)]
pub struct AuditLogger {
    events: Arc<RwLock<Vec<AuditEvent>>>,
}

impl AuditLogger {
    pub fn new() -> Self {
        Self {
            events: Arc::new(RwLock::new(Vec::new())),
        }
    }

    pub async fn record(
        &self,
        session_id: &str,
        tool_name: &str,
        risk_tier: RiskTier,
        description: &str,
        approved: bool,
        success: bool,
        duration_ms: u64,
    ) -> AuditEvent {
        let event = AuditEvent {
            id: Uuid::new_v4().to_string(),
            session_id: session_id.to_string(),
            tool_name: tool_name.to_string(),
            risk_tier,
            description: description.to_string(),
            approved,
            success,
            execution_duration_ms: duration_ms,
            timestamp: Utc::now(),
        };

        let mut events = self.events.write().await;
        events.push(event.clone());
        event
    }

    pub async fn get_history(&self, session_id: Option<&str>) -> Vec<AuditEvent> {
        let events = self.events.read().await;
        if let Some(sid) = session_id {
            events.iter().filter(|e| e.session_id == sid).cloned().collect()
        } else {
            events.clone()
        }
    }
}
