use chrono::{DateTime, Utc};
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
    _request: ApprovalRequest,
    sender: oneshot::Sender<ApprovalDecision>,
}

#[derive(Clone)]
pub struct ApprovalEngine {
    policy: Arc<RwLock<SecurityPolicy>>,
    pending: Arc<RwLock<HashMap<String, PendingRequest>>>,
}

impl ApprovalEngine {
    pub fn new(policy: SecurityPolicy) -> Self {
        Self {
            policy: Arc::new(RwLock::new(policy)),
            pending: Arc::new(RwLock::new(HashMap::new())),
        }
    }

    pub async fn check_risk(&self, tool_name: &str) -> RiskTier {
        match tool_name {
            "fs_read" | "fs_list" | "git_status" | "git_diff" | "git_log" | "github_list_issues" | "github_get_repo" | "mcp_list_tools" => {
                RiskTier::Tier0ReadOnly
            }
            "fs_write" | "fs_edit" | "git_commit" | "git_branch" | "artifact_create_code" | "artifact_create_document" | "artifact_create_image" | "github_create_issue" => {
                RiskTier::Tier1WorkspaceMutating
            }
            _ => RiskTier::Tier2SystemMutating, // shell_exec, delete_file, git_push, destructive actions
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
            pending.insert(id.clone(), PendingRequest { _request: req, sender: tx });
        }

        // Wait for decision or timeout
        match tokio::time::timeout(std::time::Duration::from_secs(300), rx).await {
            Ok(Ok(decision)) => Ok(decision),
            Ok(Err(_)) => Ok(ApprovalDecision::Rejected {
                reason: "Approval channel closed".to_string(),
            }),
            Err(_) => {
                let mut pending = self.pending.write().await;
                pending.remove(&id);
                Ok(ApprovalDecision::Expired)
            }
        }
    }

    pub async fn submit_decision(&self, approval_id: &str, decision: ApprovalDecision) -> bool {
        let mut pending = self.pending.write().await;
        if let Some(entry) = pending.remove(approval_id) {
            let _ = entry.sender.send(decision);
            true
        } else {
            false
        }
    }
}
