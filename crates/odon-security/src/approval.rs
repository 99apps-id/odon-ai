use chrono::{DateTime, Utc};
use odon_core::{AgentEvent, EventSender};
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};
use std::path::PathBuf;
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
    ApprovedAlways,
    Rejected { reason: String },
    Expired,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AuditRecord {
    pub id: String,
    pub session_id: String,
    pub tool_name: String,
    pub risk_tier: RiskTier,
    pub description: String,
    pub parameters: serde_json::Value,
    pub decision: String,
    pub reason: Option<String>,
    pub created_at: DateTime<Utc>,
    pub resolved_at: DateTime<Utc>,
}

struct PendingRequest {
    request: ApprovalRequest,
    sender: oneshot::Sender<ApprovalDecision>,
}

fn decision_label(decision: &ApprovalDecision) -> &'static str {
    match decision {
        ApprovalDecision::Approved => "approved",
        ApprovalDecision::ApprovedAlways => "approved_always",
        ApprovalDecision::Rejected { .. } => "rejected",
        ApprovalDecision::Expired => "expired",
    }
}

#[derive(Clone)]
pub struct ApprovalEngine {
    policy: Arc<RwLock<SecurityPolicy>>,
    pending: Arc<RwLock<HashMap<String, PendingRequest>>>,
    always_allowed: Arc<RwLock<HashSet<String>>>,
    audit_history: Arc<RwLock<Vec<AuditRecord>>>,
    policy_path: PathBuf,
    audit_path: PathBuf,
    events: Option<EventSender>,
}

impl ApprovalEngine {
    pub fn new(policy: SecurityPolicy) -> Self {
        let base = std::env::var_os("ODON_HOME")
            .map(PathBuf::from)
            .or_else(|| std::env::var_os("USERPROFILE").map(PathBuf::from))
            .or_else(|| std::env::var_os("HOME").map(PathBuf::from))
            .unwrap_or_else(|| PathBuf::from("."));
        let policy_path = base.join(".odon").join("approval-policies.json");
        let always_allowed = std::fs::read_to_string(&policy_path)
            .ok()
            .and_then(|data| serde_json::from_str::<HashSet<String>>(&data).ok())
            .unwrap_or_default();

        let audit_path = base.join(".odon").join("approval-audit.jsonl");
        let mut loaded_audit = Vec::new();
        if let Ok(file) = std::fs::File::open(&audit_path) {
            use std::io::BufRead;
            for line in std::io::BufReader::new(file).lines().flatten() {
                if let Ok(rec) = serde_json::from_str::<AuditRecord>(&line) {
                    loaded_audit.push(rec);
                }
            }
            if loaded_audit.len() > 100 {
                loaded_audit = loaded_audit.split_off(loaded_audit.len() - 100);
            }
            loaded_audit.reverse();
        }

        Self {
            policy: Arc::new(RwLock::new(policy)),
            pending: Arc::new(RwLock::new(HashMap::new())),
            always_allowed: Arc::new(RwLock::new(always_allowed)),
            audit_history: Arc::new(RwLock::new(loaded_audit)),
            policy_path,
            audit_path,
            events: None,
        }
    }

    /// Check if target parameters point outside the workspace.
    pub fn is_outside_workspace(parameters: &serde_json::Value, workspace_root: &std::path::Path) -> bool {
        for key in ["path", "cwd", "destination", "source", "file"] {
            if let Some(p_str) = parameters.get(key).and_then(|v| v.as_str()) {
                let p = std::path::Path::new(p_str);
                if p.is_absolute() {
                    if !p.starts_with(workspace_root) {
                        return true;
                    }
                } else if p.components().any(|c| matches!(c, std::path::Component::ParentDir)) {
                    let normalized = workspace_root.join(p);
                    if let Ok(canonical) = normalized.canonicalize() {
                        if !canonical.starts_with(workspace_root) {
                            return true;
                        }
                    } else {
                        return true;
                    }
                }
            }
        }
        false
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
            "fs_read" | "fs_list" | "fs_search" | "fs_glob" | "fs_stat" | "fs_readlink" | "fs_diff" | "fs_tree" | "fs_du" | "fs_find"
            | "termigo_copy_selection" | "termigo_paste_buffer_history" | "editor_open" | "editor_reveal" | "editor_reload"
            | "read_file" | "list_directory" | "grep" | "glob"
            | "git_status" | "git_diff" | "git_log" | "git_show" | "git_stash_list" | "git_blame" | "git_log_file" | "git_ls_files"
            | "github_list_issues" | "github_get_repo" | "github_list_prs" | "github_list_comments" | "github_workflow_list" | "github_workflow_runs" | "github_workflow_jobs" | "github_workflow_logs" | "github_pr_files"
            | "web_search" | "web_fetch" | "web_search_news" | "web_search_code" | "web_scrape" | "probe_url" | "http_get" | "security_audit_headers" | "security_audit_cert" | "network_traceroute" | "url_preview" | "dns_lookup" | "port_scan"
            | "system_info" | "calc_eval" | "env_get" | "env_list" | "process_list" | "exec_stream_stdout" | "exec_background_list" | "disk_usage" | "file_hash" | "file_checksum" | "clipboard_read" | "clipboard_write" | "notification_send" | "terminal_info"
            | "memory_recall" | "memory_search" | "todo_write" | "todo_read" | "todo_update" | "think" | "plan_mode" | "remember"
            | "mcp_list_tools" | "mcp_list_servers" | "approval_list" | "session_list" | "session_export" | "relay_status" | "automation_list" | "settings_get" => {
                RiskTier::Tier0ReadOnly
            }
            "fs_write" | "fs_edit" | "fs_mkdir" | "fs_move" | "fs_copy" | "fs_chmod" | "fs_chown" | "fs_symlink_create" | "fs_patch" | "fs_copy_recursive" | "fs_move_recursive" | "fs_zip" | "fs_unzip" | "termigo_multi_paste" | "editor_save" | "multi_edit" | "write_file" | "edit" | "create_directory" | "move_file" | "copy_file"
            | "git_commit" | "git_branch" | "git_add" | "git_checkout" | "git_stash" | "git_stash_pop" | "git_merge" | "git_rebase" | "git_cherry_pick" | "git_revert" | "git_tag" | "git_remote_add" | "git_remote_set_url"
            | "github_create_issue" | "github_create_review"
            | "http_post" | "http_put" | "http_patch" | "http_download" | "http_upload"
            | "artifact_create_code" | "artifact_create_document" | "artifact_create_image" | "artifact_update" | "artifact_delete" | "artifact_export"
            | "memory_store" | "memory_forget" | "mcp_call" | "mcp_add_server" | "approval_decide" | "cron_add" | "cron_remove" | "automation_trigger" | "settings_set" => {
                RiskTier::Tier1WorkspaceMutating
            }
            _ => RiskTier::Tier2SystemMutating, // shell_exec, bash_run, terminal_exec, fs_delete, git_push, git_pull, git_reset, git_reset_hard, git_clean, github_merge_pr, github_workflow_run, process_kill, exec_background, http_delete
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
        let policy_key = Self::policy_key(tool_name, parameters);
        if self.always_allowed.read().await.contains(&policy_key) {
            return Ok(ApprovalDecision::Approved);
        }

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

    /// Pre-approve a single tool invocation for this process. Used by the CLI
    /// `tools run --yes` path after the user explicitly opts in.
    pub async fn pre_approve(&self, tool_name: &str, parameters: &serde_json::Value) {
        let key = Self::policy_key(tool_name, parameters);
        self.always_allowed.write().await.insert(key);
    }

    pub async fn submit_decision(&self, approval_id: &str, decision: ApprovalDecision) -> bool {
        let mut pending = self.pending.write().await;
        if let Some(entry) = pending.remove(approval_id) {
            if matches!(decision, ApprovalDecision::ApprovedAlways) {
                let key = Self::policy_key(&entry.request.tool_name, &entry.request.parameters);
                let policies = {
                    let mut policies = self.always_allowed.write().await;
                    policies.insert(key);
                    policies.clone()
                };
                if let Some(parent) = self.policy_path.parent() {
                    let _ = std::fs::create_dir_all(parent);
                }
                if let Ok(data) = serde_json::to_vec_pretty(&policies) {
                    let _ = std::fs::write(&self.policy_path, data);
                }
            }
            let session_id = entry.request.session_id.clone();
            let label = decision_label(&decision);
            let reason = match &decision {
                ApprovalDecision::Rejected { reason } => Some(reason.clone()),
                _ => None,
            };

            // Record into audit history
            {
                let mut audit = self.audit_history.write().await;
                audit.insert(
                    0,
                    AuditRecord {
                        id: entry.request.id.clone(),
                        session_id: session_id.clone(),
                        tool_name: entry.request.tool_name.clone(),
                        risk_tier: entry.request.risk_tier,
                        description: entry.request.description.clone(),
                        parameters: entry.request.parameters.clone(),
                        decision: label.to_string(),
                        reason,
                        created_at: entry.request.created_at,
                        resolved_at: Utc::now(),
                    },
                );
                if audit.len() > 500 {
                    audit.truncate(500);
                }
                Self::persist_audit_record(&audit[0], &self.audit_path);
            }

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

    fn policy_key(tool_name: &str, parameters: &serde_json::Value) -> String {
        let scope = parameters
            .get("path")
            .or_else(|| parameters.get("cwd"))
            .or_else(|| parameters.get("destination"))
            .and_then(|value| value.as_str())
            .unwrap_or("workspace");
        format!("{tool_name}|{scope}")
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

    pub async fn get_audit_history(&self) -> Vec<AuditRecord> {
        self.audit_history.read().await.clone()
    }

    fn persist_audit_record(record: &AuditRecord, path: &PathBuf) {
        if let Ok(line) = serde_json::to_string(record) {
            let _ = std::fs::OpenOptions::new()
                .create(true)
                .append(true)
                .open(path)
                .and_then(|mut f| {
                    use std::io::Write;
                    writeln!(f, "{line}")
                });
        }
    }

    pub async fn get_always_allowed(&self) -> Vec<String> {
        self.always_allowed.read().await.iter().cloned().collect()
    }

    pub async fn revoke_allowed(&self, key: &str) -> bool {
        let mut allowed = self.always_allowed.write().await;
        let removed = allowed.remove(key);
        if removed {
            let policies = allowed.clone();
            drop(allowed);
            if let Ok(data) = serde_json::to_vec_pretty(&policies) {
                let _ = std::fs::write(&self.policy_path, data);
            }
        }
        removed
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
