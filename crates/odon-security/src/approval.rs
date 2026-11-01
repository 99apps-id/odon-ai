use chrono::{DateTime, Utc};
use odon_core::{AgentEvent, EventSender};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::{oneshot, RwLock};
use uuid::Uuid;

use crate::policy::{ExecutionMode, SecurityPolicy};

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub enum RiskTier {
    Tier0ReadOnly = 0,
    Tier1WorkspaceMutating = 1,
    Tier2SystemMutating = 2,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ApprovalRequest {
    pub id: String,
    pub session_id: String,
    pub tool_name: String,
    pub risk_tier: RiskTier,
    pub description: String,
    pub parameters: serde_json::Value,
    pub created_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum ApprovalDecision {
    Approved,
    Rejected { reason: String },
    Expired,
}

struct PendingRequest {
    request: ApprovalRequest,
    sender: oneshot::Sender<ApprovalDecision>,
}

fn decision_label(decision: &ApprovalDecision) -> &'static str {
    match decision {
        ApprovalDecision::Approved => "approved",
        ApprovalDecision::Rejected { .. } => "rejected",
        ApprovalDecision::Expired => "expired",
    }
}

#[derive(Clone)]
pub struct ApprovalEngine {
    policy: Arc<RwLock<SecurityPolicy>>,
    pending: Arc<RwLock<HashMap<String, PendingRequest>>>,
    events: Option<EventSender>,
}

impl ApprovalEngine {
    pub fn new(policy: SecurityPolicy) -> Self {
        Self {
            policy: Arc::new(RwLock::new(policy)),
            pending: Arc::new(RwLock::new(HashMap::new())),
            events: None,
        }
    }

    /// Publish approval lifecycle events (requested / resolved) on the event bus.
    pub fn with_events(mut self, events: EventSender) -> Self {
        self.events = Some(events);
        self
    }

    fn emit(&self, event: AgentEvent) {
        if let Some(tx) = &self.events {
            let _ = tx.send(event);
        }
    }

    pub async fn check_risk(&self, tool_name: &str) -> RiskTier {
        match tool_name {
            "fs_read" | "fs_list" | "fs_search" | "git_status" | "git_diff" | "git_log" | "github_list_issues" | "github_get_repo" | "mcp_list_tools" | "web_search" | "web_fetch" | "system_info" | "calc_eval" | "memory_recall" | "todo_write" | "todo_read" | "think" | "plan_mode" => {
                RiskTier::Tier0ReadOnly
            }
            "fs_write" | "fs_edit" | "git_commit" | "git_branch" | "artifact_create_code" | "artifact_create_document" | "artifact_create_image" | "github_create_issue" | "memory_store" => {
                RiskTier::Tier1WorkspaceMutating
            }
            _ => RiskTier::Tier2SystemMutating, // shell_exec, fs_delete, git_push, destructive actions
        }
    }

    pub async fn evaluate_and_request(
        &self,
        session_id: &str,
        tool_name: &str,
        parameters: &serde_json::Value,
        description: &str,
    ) -> Result<ApprovalDecision, String> {
        let risk = self.check_risk(tool_name).await;
        let policy = self.policy.read().await;

        // In locked mode, all mutating calls rejected
        if policy.mode == ExecutionMode::Locked && risk > RiskTier::Tier0ReadOnly {
            return Ok(ApprovalDecision::Rejected {
                reason: "Workspace is in Locked mode: all mutating actions are refused".to_string(),
            });
        }

        // Auto approve Tier 0 read-only
        if risk == RiskTier::Tier0ReadOnly {
            return Ok(ApprovalDecision::Approved);
        }

        // Autonomous mode auto-approves Tier 1
        if policy.mode == ExecutionMode::Autonomous && risk == RiskTier::Tier1WorkspaceMutating {
            return Ok(ApprovalDecision::Approved);
        }

        // Otherwise requires interactive approval (Tier 2 in Autonomous, or Tier 1 & 2 in Supervised)
        let id = Uuid::new_v4().to_string();
        let (tx, rx) = oneshot::channel();

        let req = ApprovalRequest {
            id: id.clone(),
            session_id: session_id.to_string(),
            tool_name: tool_name.to_string(),
            risk_tier: risk,
            description: description.to_string(),
            parameters: parameters.clone(),
            created_at: Utc::now(),
        };

        {
            let mut pending = self.pending.write().await;
            pending.insert(id.clone(), PendingRequest { request: req, sender: tx });
        }
        drop(policy);
        self.emit(AgentEvent::ApprovalRequested {
            session_id: session_id.to_string(),
            approval_id: id.clone(),
            tool_name: tool_name.to_string(),
            risk_tier: risk as u8,
            description: description.to_string(),
        });

        // Wait for decision or timeout
        match tokio::time::timeout(std::time::Duration::from_secs(300), rx).await {
            Ok(Ok(decision)) => Ok(decision),
            Ok(Err(_)) => Ok(ApprovalDecision::Rejected {
                reason: "Approval channel closed".to_string(),
            }),
            Err(_) => {
                let mut pending = self.pending.write().await;
                pending.remove(&id);
                drop(pending);
                self.emit(AgentEvent::ApprovalResolved {
                    session_id: session_id.to_string(),
                    approval_id: id.clone(),
                    decision: "expired".to_string(),
                });
                Ok(ApprovalDecision::Expired)
            }
        }
    }

    pub async fn submit_decision(&self, approval_id: &str, decision: ApprovalDecision) -> bool {
        let mut pending = self.pending.write().await;
        if let Some(entry) = pending.remove(approval_id) {
            let session_id = entry.request.session_id.clone();
            let label = decision_label(&decision);
            let _ = entry.sender.send(decision);
            drop(pending);
            self.emit(AgentEvent::ApprovalResolved {
                session_id,
                approval_id: approval_id.to_string(),
                decision: label.to_string(),
            });
            true
        } else {
            false
        }
    }

    pub async fn list_pending(&self) -> Vec<ApprovalRequest> {
        let pending = self.pending.read().await;
        pending.values().map(|p| p.request.clone()).collect()
    }

    pub async fn get_policy(&self) -> SecurityPolicy {
        self.policy.read().await.clone()
    }

    pub async fn update_mode(&self, mode: ExecutionMode) {
        let mut policy = self.policy.write().await;
        policy.mode = mode;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn approval_publishes_requested_then_resolved_events() {
        let (tx, mut rx) = odon_core::events::create_event_bus(16);
        let engine = ApprovalEngine::new(SecurityPolicy::default()).with_events(tx);

        let waiter = {
            let engine = engine.clone();
            tokio::spawn(async move {
                engine
                    .evaluate_and_request("s1", "shell_exec", &serde_json::json!({}), "run")
                    .await
            })
        };

        let id = match rx.recv().await.unwrap() {
            AgentEvent::ApprovalRequested { approval_id, tool_name, risk_tier, .. } => {
                assert_eq!(tool_name, "shell_exec");
                assert_eq!(risk_tier, 2);
                approval_id
            }
            other => panic!("expected ApprovalRequested, got {other:?}"),
        };
        assert!(engine.submit_decision(&id, ApprovalDecision::Approved).await);

        match rx.recv().await.unwrap() {
            AgentEvent::ApprovalResolved { approval_id, decision, .. } => {
                assert_eq!(approval_id, id);
                assert_eq!(decision, "approved");
            }
            other => panic!("expected ApprovalResolved, got {other:?}"),
        }
        assert!(matches!(waiter.await.unwrap().unwrap(), ApprovalDecision::Approved));
    }
}
