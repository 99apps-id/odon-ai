use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::io::BufRead;
use std::path::PathBuf;
use std::sync::Arc;
use tokio::sync::RwLock;
use uuid::Uuid;

use crate::approval::RiskTier;

const MAX_MEMORY_EVENTS: usize = 2000;
const MAX_LOADED_EVENTS: usize = 1000;

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

/// Append-only audit trail. Events are kept in memory for fast reads and
/// mirrored to `~/.odon/audit.jsonl` so a daemon restart does not erase the
/// forensic history.
#[derive(Clone)]
pub struct AuditLogger {
    events: Arc<RwLock<Vec<AuditEvent>>>,
    path: PathBuf,
}

fn audit_home() -> PathBuf {
    std::env::var_os("ODON_HOME")
        .map(PathBuf::from)
        .or_else(|| std::env::var_os("USERPROFILE").map(PathBuf::from))
        .or_else(|| std::env::var_os("HOME").map(PathBuf::from))
        .unwrap_or_else(|| PathBuf::from("."))
        .join(".odon")
}

impl Default for AuditLogger {
    fn default() -> Self {
        Self::new()
    }
}

impl AuditLogger {
    pub fn new() -> Self {
        Self::with_path(audit_home().join("audit.jsonl"))
    }

    pub fn with_path(path: PathBuf) -> Self {
        let mut loaded = Vec::new();
        if let Ok(file) = std::fs::File::open(&path) {
            for line in std::io::BufReader::new(file).lines().map_while(Result::ok) {
                if let Ok(event) = serde_json::from_str::<AuditEvent>(&line) {
                    loaded.push(event);
                }
            }
            if loaded.len() > MAX_LOADED_EVENTS {
                loaded = loaded.split_off(loaded.len() - MAX_LOADED_EVENTS);
            }
            loaded.reverse();
        }
        Self {
            events: Arc::new(RwLock::new(loaded)),
            path,
        }
    }

    #[expect(
        clippy::too_many_arguments,
        reason = "audit calls mirror the persisted event fields at security boundaries"
    )]
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

        // Persist first so a crash cannot lose the latest event.
        if let Some(parent) = self.path.parent() {
            let _ = std::fs::create_dir_all(parent);
        }
        if let Ok(line) = serde_json::to_string(&event) {
            use std::io::Write;
            if let Ok(mut file) = std::fs::OpenOptions::new()
                .create(true)
                .append(true)
                .open(&self.path)
            {
                let _ = writeln!(file, "{line}");
                let _ = file.flush();
            }
            #[cfg(unix)]
            {
                use std::os::unix::fs::PermissionsExt;
                let _ =
                    std::fs::set_permissions(&self.path, std::fs::Permissions::from_mode(0o600));
            }
        }

        let mut events = self.events.write().await;
        events.insert(0, event.clone());
        if events.len() > MAX_MEMORY_EVENTS {
            events.truncate(MAX_MEMORY_EVENTS);
        }
        event
    }

    pub async fn get_history(&self, session_id: Option<&str>) -> Vec<AuditEvent> {
        let events = self.events.read().await;
        if let Some(sid) = session_id {
            events
                .iter()
                .filter(|e| e.session_id == sid)
                .cloned()
                .collect()
        } else {
            events.clone()
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn records_events_in_memory_and_on_disk() {
        let temp = std::env::temp_dir().join(format!("odon-audit-test-{}", Uuid::new_v4()));
        std::fs::create_dir_all(&temp).unwrap();
        let log_path = temp.join("audit.jsonl");
        let logger = AuditLogger::with_path(log_path.clone());
        let event = logger
            .record(
                "s1",
                "shell_exec",
                RiskTier::Tier2SystemMutating,
                "run",
                true,
                true,
                12,
            )
            .await;
        assert_eq!(event.tool_name, "shell_exec");
        assert_eq!(logger.get_history(Some("s1")).await.len(), 1);

        let content = std::fs::read_to_string(&log_path).unwrap();
        assert!(content.contains("shell_exec"));

        let _ = std::fs::remove_dir_all(&temp);
    }
}
