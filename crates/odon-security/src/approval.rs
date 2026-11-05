use chrono::{DateTime, Utc};
use odon_core::{AgentEvent, EventSender};
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};
use std::path::PathBuf;
use std::sync::Arc;
use tokio::sync::{oneshot, RwLock};
use uuid::Uuid;

use crate::policy::{ExecutionMode, SecurityPolicy};
use std::time::Duration;

fn approval_timeout() -> Duration {
    let seconds = std::env::var("ODON_APPROVAL_TIMEOUT_SECS")
        .ok()
        .and_then(|value| value.trim().parse::<u64>().ok())
        .unwrap_or(120);
    Duration::from_secs(seconds.clamp(5, 3600))
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub enum RiskTier {
    Tier0ReadOnly = 0,
    Tier1WorkspaceMutating = 1,
    Tier2SystemMutating = 2,
    Tier3NetworkAction = 3,
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
    ApprovedAll,
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
        ApprovalDecision::ApprovedAll => "approved_all",
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
    /// Root that mutating calls are expected to stay inside. Always allocated
    /// so the root can be attached after construction (workspace switches);
    /// the inner `None` means no root was declared, in which case containment
    /// cannot be judged and no escalation is applied.
    workspace_root: Arc<RwLock<Option<PathBuf>>>,
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
            for line in std::io::BufReader::new(file).lines().map_while(Result::ok) {
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
            workspace_root: Arc::new(RwLock::new(None)),
        }
    }

    /// Declare the workspace root so out-of-workspace targets can be escalated
    /// instead of silently auto-approved. Chainable at construction:
    /// `ApprovalEngine::new(policy).with_workspace_root(root)`.
    pub fn with_workspace_root(mut self, root: impl Into<PathBuf>) -> Self {
        self.workspace_root = Arc::new(RwLock::new(Some(root.into())));
        self
    }

    /// Re-point the containment root at runtime (workspace switch, project
    /// attach). The previous root is replaced, never merged.
    pub async fn set_workspace_root(&self, root: impl Into<PathBuf>) {
        *self.workspace_root.write().await = Some(root.into());
    }

    /// The declared containment root, if any.
    pub async fn workspace_root(&self) -> Option<PathBuf> {
        self.workspace_root.read().await.clone()
    }

    /// Re-point the persisted policy and audit files, reloading both from
    /// the new location. `new` reads the operator's real `~/.odon` directory,
    /// so anything that must not inherit (tests most of all) has to call
    /// this before it relies on a clean slate — otherwise a stored `*`
    /// grant silently answers every request and nothing is ever asked.
    pub fn with_paths(mut self, policy_path: PathBuf, audit_path: PathBuf) -> Self {
        let always_allowed = std::fs::read_to_string(&policy_path)
            .ok()
            .and_then(|data| serde_json::from_str::<HashSet<String>>(&data).ok())
            .unwrap_or_default();
        self.always_allowed = Arc::new(RwLock::new(always_allowed));
        self.audit_history = Arc::new(RwLock::new(Vec::new()));
        self.policy_path = policy_path;
        self.audit_path = audit_path;
        self
    }

    /// Lexically resolve `.` and `..` components without touching the disk.
    fn normalize_lexically(p: &std::path::Path) -> std::path::PathBuf {
        use std::path::{Component, PathBuf};
        let mut out = PathBuf::new();
        for component in p.components() {
            match component {
                Component::CurDir => {}
                Component::ParentDir => {
                    out.pop();
                }
                Component::Prefix(prefix) => out.push(prefix.as_os_str()),
                Component::RootDir => out.push(std::path::MAIN_SEPARATOR.to_string()),
                Component::Normal(part) => out.push(part),
            }
        }
        out
    }

    fn canonical_root(workspace_root: &std::path::Path) -> std::path::PathBuf {
        Self::strip_verbatim(
            workspace_root
                .canonicalize()
                .unwrap_or_else(|_| Self::normalize_lexically(workspace_root)),
        )
    }

    /// Canonicalise the deepest ancestor that exists, then re-append the
    /// components that do not exist yet.
    ///
    /// A target file is normally still absent when approval is requested, so
    /// `canonicalize` fails outright. Falling back to a purely lexical join
    /// keeps whatever spelling the caller used, which does not match the
    /// canonical root (8.3 short names on Windows, symlinked temp dirs on
    /// macOS) and makes in-workspace paths look like escapes.
    fn canonicalize_existing_ancestor(path: &std::path::Path) -> Option<std::path::PathBuf> {
        let mut probe = path.to_path_buf();
        let mut tail: Vec<std::ffi::OsString> = Vec::new();
        loop {
            if let Ok(mut resolved) = probe.canonicalize() {
                for part in tail.iter().rev() {
                    resolved.push(part);
                }
                return Some(resolved);
            }
            let name = probe.file_name()?.to_os_string();
            tail.push(name);
            if !probe.pop() {
                return None;
            }
        }
    }

    /// Drop the Windows verbatim prefix (`\\?\`, and `\\?\UNC\` for shares).
    ///
    /// `fs::canonicalize` always returns a verbatim path on Windows, while a
    /// path that does not exist yet is only normalised lexically and carries no
    /// prefix. Comparing the two spellings never matches, which made every
    /// target look like it escaped the workspace.
    fn strip_verbatim(path: std::path::PathBuf) -> std::path::PathBuf {
        let text = path.to_string_lossy();
        if let Some(rest) = text.strip_prefix(r"\\?\UNC\") {
            return std::path::PathBuf::from(format!(r"\\{rest}"));
        }
        if let Some(rest) = text.strip_prefix(r"\\?\") {
            return std::path::PathBuf::from(rest);
        }
        path
    }

    /// Whether `candidate` sits inside `root`.
    ///
    /// Windows paths are case-insensitive and one side is often a lexical path
    /// (the target may not exist yet) while the other was canonicalised, so a
    /// byte-wise `starts_with` is not enough there.
    fn path_is_inside(candidate: &std::path::Path, root: &std::path::Path) -> bool {
        if candidate.starts_with(root) {
            return true;
        }
        #[cfg(windows)]
        {
            let norm = |p: &std::path::Path| p.to_string_lossy().replace('/', "\\").to_lowercase();
            let candidate = norm(candidate);
            let mut root = norm(root);
            while root.ends_with('\\') && root.len() > 3 {
                root.pop();
            }
            if candidate == root {
                return true;
            }
            if !root.ends_with('\\') {
                root.push('\\');
            }
            candidate.starts_with(&root)
        }
        #[cfg(not(windows))]
        {
            false
        }
    }

    /// Check if target parameters point outside the workspace.
    pub fn is_outside_workspace(
        parameters: &serde_json::Value,
        workspace_root: &std::path::Path,
    ) -> bool {
        Self::outside_workspace_target(parameters, workspace_root).is_some()
    }

    /// First target parameter that escapes `workspace_root`, spelled as the
    /// caller wrote it (for the approval prompt).
    pub fn outside_workspace_target(
        parameters: &serde_json::Value,
        workspace_root: &std::path::Path,
    ) -> Option<String> {
        let root = Self::canonical_root(workspace_root);
        for key in ["path", "cwd", "destination", "source", "file"] {
            if let Some(p_str) = parameters.get(key).and_then(|v| v.as_str()) {
                let raw = std::path::Path::new(p_str);
                // Join relative paths onto the workspace root; absolute paths
                // are taken as-is. Never trust the raw spelling — `..`
                // components must be resolved before containment is decided.
                //
                // Join onto the *canonical* root, not the caller's spelling:
                // `std::env::temp_dir()` can hand back an 8.3 short path (for
                // example `RUNNER~1`) while `canonicalize` resolves the long
                // form. Mixing the two made every in-workspace target look
                // like it escaped.
                let joined = if raw.is_absolute() {
                    raw.to_path_buf()
                } else {
                    root.join(raw)
                };
                let normalized = Self::strip_verbatim(
                    joined
                        .canonicalize()
                        .ok()
                        .or_else(|| Self::canonicalize_existing_ancestor(&joined))
                        .unwrap_or_else(|| Self::normalize_lexically(&joined)),
                );
                if !Self::path_is_inside(&normalized, &root) {
                    return Some(p_str.to_string());
                }
            }
        }
        None
    }

    /// The containment target for these parameters, when a root is declared.
    async fn escaping_target(&self, parameters: &serde_json::Value) -> Option<String> {
        let root = self.workspace_root().await?;
        Self::outside_workspace_target(parameters, &root)
    }

    /// Publish approval lifecycle events (requested / resolved) on the event bus.
    pub fn with_events(mut self, events: EventSender) -> Self {
        self.events = Some(events);
        self
    }

    /// Borrow the event-bus sender when one is attached (e.g. so the agent
    /// runtime can publish child-session lifecycle events).
    pub fn event_sender(&self) -> Option<&EventSender> {
        self.events.as_ref()
    }

    fn emit(&self, event: AgentEvent) {
        if let Some(tx) = &self.events {
            let _ = tx.send(event);
        }
    }

    pub async fn check_risk(&self, tool_name: &str) -> RiskTier {
        match tool_name {
            "fs_read"
            | "fs_list"
            | "fs_search"
            | "fs_glob"
            | "fs_stat"
            | "fs_readlink"
            | "fs_diff"
            | "fs_tree"
            | "fs_du"
            | "fs_find"
            | "termigo_copy_selection"
            | "editor_open"
            | "editor_reveal"
            | "editor_reload"
            | "read_file"
            | "fs_owner_check"
            | "list_directory"
            | "grep"
            | "glob"
            | "code_search"
            | "review_diff"
            | "review_file"
            | "review_security"
            | "review_session_changes"
            | "change_summary"
            | "git_status"
            | "git_diff"
            | "git_log"
            | "git_show"
            | "git_stash_list"
            | "git_blame"
            | "git_log_file"
            | "git_ls_files"
            | "github_list_issues"
            | "github_get_repo"
            | "github_list_prs"
            | "github_list_comments"
            | "github_workflow_list"
            | "github_workflow_runs"
            | "github_workflow_jobs"
            | "github_workflow_logs"
            | "github_pr_files"
            | "web_search"
            | "web_fetch"
            | "web_search_news"
            | "web_search_code"
            | "web_scrape"
            | "probe_url"
            | "http_get"
            | "security_audit_headers"
            | "security_audit_cert"
            | "url_preview"
            | "dns_lookup"
            | "system_info"
            | "calc_eval"
            | "env_get"
            | "env_list"
            | "process_list"
            | "app_list"
            | "exec_stream_stdout"
            | "exec_background_list"
            | "disk_usage"
            | "file_hash"
            | "file_checksum"
            | "terminal_info"
            | "memory_recall"
            | "memory_search"
            | "todo_write"
            | "todo_read"
            | "think"
            | "plan_mode"
            | "remember"
            | "mcp_list_tools"
            | "mcp_list_servers"
            | "approval_list"
            | "approval_history"
            | "audit_search"
            | "session_list"
            | "session_get"
            | "session_messages"
            | "session_export"
            | "relay_status"
            | "automation_list"
            | "automation_logs"
            | "provider_list"
            | "daemon_status"
            | "daemon_metrics"
            | "tool_diagnostics"
            | "doctor"
            | "workspace_list"
            | "project_detect"
            | "project_scripts"
            | "dependency_audit"
            | "browser_session_text"
            | "browser_session_wait"
            | "ssh_profile_test"
            | "settings_get" => RiskTier::Tier0ReadOnly,
            "fs_write"
            | "fs_edit"
            | "fs_mkdir"
            | "fs_move"
            | "fs_copy"
            | "fs_chmod"
            | "fs_symlink_create"
            | "fs_patch"
            | "fs_copy_recursive"
            | "fs_move_recursive"
            | "fs_zip"
            | "fs_unzip"
            | "termigo_multi_paste"
            | "editor_save"
            | "multi_edit"
            | "write_file"
            | "edit"
            | "create_directory"
            | "move_file"
            | "copy_file"
            | "git_commit"
            | "git_branch"
            | "git_add"
            | "git_checkout"
            | "git_stash"
            | "git_stash_pop"
            | "git_merge"
            | "git_rebase"
            | "git_cherry_pick"
            | "git_revert"
            | "git_tag"
            | "git_remote_add"
            | "git_remote_set_url"
            | "github_create_issue"
            | "github_create_review"
            | "http_download"
            | "http_upload"
            | "code_index"
            | "app_focus"
            | "clipboard_read"
            | "termigo_paste_buffer_history"
            | "notification_send"
            | "port_scan"
            | "network_traceroute"
            | "artifact_create_code"
            | "artifact_create_document"
            | "artifact_create_image"
            | "artifact_update"
            | "artifact_delete"
            | "artifact_export"
            | "memory_store"
            | "todo_update"
            | "memory_forget"
            | "mcp_call"
            | "mcp_add_server"
            | "mcp_toggle_server"
            | "approval_decide"
            | "approval_policy_revoke"
            | "automation_trigger"
            | "automation_update"
            | "automation_delete"
            | "provider_test"
            | "provider_set_default"
            | "workspace_add"
            | "workspace_remove"
            | "browser_session_type"
            | "browser_session_select"
            | "browser_session_screenshot" => RiskTier::Tier1WorkspaceMutating,
            _ => RiskTier::Tier2SystemMutating, // shell_exec, bash_run, terminal_exec, fs_delete, git_push, git_pull, git_reset, git_reset_hard, git_clean, github_merge_pr, github_workflow_run, process_kill, exec_background, http_delete, ssh_exec
        }
    }

    pub async fn evaluate_and_request(
        &self,
        session_id: &str,
        tool_name: &str,
        parameters: &serde_json::Value,
        description: &str,
    ) -> Result<ApprovalDecision, String> {
        self.evaluate_and_request_with_risk(session_id, tool_name, None, parameters, description)
            .await
    }

    /// [`evaluate_and_request`], seeded with the risk tier the tool declares.
    ///
    /// The name-based table in [`check_risk`] is a second source of truth for
    /// the same fact, and the two drift: a tool missing from the table falls
    /// through to `Tier2SystemMutating`, while a tool that declares `Tier1` but
    /// is not listed is treated as system-mutating. The effective tier is the
    /// stricter of the two, so neither source can downgrade a call the other
    /// considers dangerous, and the gate always sees the tool's own claim.
    pub async fn evaluate_and_request_with_risk(
        &self,
        session_id: &str,
        tool_name: &str,
        declared: Option<RiskTier>,
        parameters: &serde_json::Value,
        description: &str,
    ) -> Result<ApprovalDecision, String> {
        let by_name = self.check_risk(tool_name).await;
        let risk = match declared {
            Some(declared) => by_name.max(declared),
            None => by_name,
        };
        let policy_key = Self::policy_key(tool_name, parameters);
        let exact_key = Self::exact_policy_key(tool_name, parameters);
        let wildcard_key = Self::wildcard_policy_key(parameters);
        // A mutating call aimed outside the declared workspace is never
        // auto-approved: not by the Autonomous Tier-1 shortcut, and not by a
        // cached `always allow` grant. Those grants are recorded while the
        // operator is looking at in-workspace work, so a stored wildcard `*`
        // must not quietly extend itself to a path they never saw. Such a call
        // always reaches the interactive prompt, flagged with the escaping
        // path, which is what `is_outside_workspace` was written for.
        let escaping = if risk > RiskTier::Tier0ReadOnly {
            self.escaping_target(parameters).await
        } else {
            None
        };
        let description = match &escaping {
            Some(target) => {
                format!("WARNING: target path '{target}' is outside the workspace. {description}")
            }
            None => description.to_string(),
        };
        let description = description.as_str();
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

        // Autonomous mode auto-approves Tier 1 — but never when the target
        // escapes the workspace, which must be seen by a human.
        if policy.mode == ExecutionMode::Autonomous
            && risk == RiskTier::Tier1WorkspaceMutating
            && escaping.is_none()
        {
            return Ok(ApprovalDecision::Approved);
        }

        // `always_allowed` may only skip the interactive prompt for tools that
        // are already permitted by mode/tier enforcement above. This prevents
        // an `ApprovedAll` wildcard from bypassing execution-mode restrictions.
        // A cached grant is also scoped to the workspace it was granted in, so
        // an out-of-workspace target bypasses the cache entirely.
        if escaping.is_none() {
            let allowed = self.always_allowed.read().await;
            if allowed.contains(&policy_key)
                || allowed.contains(&exact_key)
                || allowed.contains(&wildcard_key)
            {
                return Ok(ApprovalDecision::Approved);
            }
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
            pending.insert(
                id.clone(),
                PendingRequest {
                    request: req,
                    sender: tx,
                },
            );
        }
        drop(policy);
        self.emit(AgentEvent::ApprovalRequested {
            session_id: session_id.to_string(),
            approval_id: id.clone(),
            tool_name: tool_name.to_string(),
            risk_tier: risk as u8,
            description: description.to_string(),
            parameters: parameters.clone(),
        });

        // Wait for decision or timeout
        match tokio::time::timeout(approval_timeout(), rx).await {
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
        let key = Self::exact_policy_key(tool_name, parameters);
        self.always_allowed.write().await.insert(key);
    }

    pub async fn submit_decision(&self, approval_id: &str, decision: ApprovalDecision) -> bool {
        let mut pending = self.pending.write().await;
        if let Some(entry) = pending.remove(approval_id) {
            let grant_key = match &decision {
                // "Always" sticks per tool in one workspace scope.
                ApprovalDecision::ApprovedAlways => Some(Self::policy_key(
                    &entry.request.tool_name,
                    &entry.request.parameters,
                )),
                // "Allow all" sticks for every tool in one workspace scope.
                ApprovalDecision::ApprovedAll => {
                    Some(Self::wildcard_policy_key(&entry.request.parameters))
                }
                _ => None,
            };
            if let Some(key) = grant_key {
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

            // "Allow all" resolves the current request as a plain approval:
            // the lasting part is the stored wildcard grant, and no caller
            // needs a new decision variant to handle.
            let send_decision = match decision {
                ApprovalDecision::ApprovedAll => ApprovalDecision::Approved,
                other => other,
            };
            let _ = entry.sender.send(send_decision);
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

    fn scope_of(parameters: &serde_json::Value) -> &str {
        parameters
            .get("path")
            .or_else(|| parameters.get("cwd"))
            .or_else(|| parameters.get("destination"))
            .or_else(|| parameters.get("id"))
            .and_then(|value| value.as_str())
            .unwrap_or("workspace")
    }

    /// Scope that "Always allow" binds to: the tool within one workspace
    /// target. Users who choose Always expect later calls of the same tool in
    /// the same place to stop prompting, so the full parameters must NOT be
    /// part of this key (an earlier exact-hash key made every new command
    /// prompt again and users reported "already Always, still asked").
    fn policy_key(tool_name: &str, parameters: &serde_json::Value) -> String {
        format!("{tool_name}|{}", Self::scope_of(parameters))
    }

    /// Exact-invocation key, used only by [`Self::pre_approve`] (CLI `--yes`)
    /// so a one-shot opt-in never widens into a lasting grant.
    fn exact_policy_key(tool_name: &str, parameters: &serde_json::Value) -> String {
        let scope = Self::scope_of(parameters);
        use sha2::{Digest, Sha256};
        let digest = Sha256::digest(parameters.to_string().as_bytes());
        format!("{tool_name}|{scope}|{:x}", digest)
    }

    /// Stored marker for "Allow All": stored as a bare `*` so one explicit
    /// grant covers every tool and scope until the user revokes it (it shows
    /// up as `*` in the policies list). Distinct from Always, which stays
    /// scoped per tool and target.
    fn wildcard_policy_key(_parameters: &serde_json::Value) -> String {
        "*".to_string()
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

    /// Best-effort scrub of token-like substrings before writing to disk.
    fn redact_secrets(text: &str) -> String {
        use regex::Regex;
        use std::sync::OnceLock;
        static PATTERNS: OnceLock<Vec<Regex>> = OnceLock::new();
        let patterns = PATTERNS.get_or_init(|| {
            [
                r"sk-[A-Za-z0-9_\-]{8,}",
                r"gh[pousr]_[A-Za-z0-9]{8,}",
                r"github_pat_[A-Za-z0-9_]{8,}",
                r"xox[baprs]-[A-Za-z0-9\-]{8,}",
                r"AKIA[0-9A-Z]{8,}",
                r"(?i)bearer\s+[A-Za-z0-9._\-]{8,}",
                r#"(?i)(api[_-]?key|access[_-]?token|refresh[_-]?token|client[_-]?secret|password)(\\?"?\s*[:=]\s*\\?"?)[^",\s&]{4,}"#,
            ]
            .iter()
            .filter_map(|p| Regex::new(p).ok())
            .collect()
        });
        let mut out = text.to_string();
        for pattern in patterns {
            out = pattern.replace_all(&out, "***REDACTED***").into_owned();
        }
        out
    }

    fn persist_audit_record(record: &AuditRecord, path: &PathBuf) {
        if let Ok(line) = serde_json::to_string(record) {
            let line = Self::redact_secrets(&line);
            let mut options = std::fs::OpenOptions::new();
            options.create(true).append(true);
            #[cfg(unix)]
            {
                use std::os::unix::fs::OpenOptionsExt;
                options.mode(0o600);
            }
            let _ = options.open(path).and_then(|mut f| {
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
        let engine = ApprovalEngine::new(SecurityPolicy::default());
        // Never write test records into the operator's real ~/.odon directory.
        let tmp = std::env::temp_dir().join(format!(
            "odon-approval-test-{}",
            uuid::Uuid::new_v4().simple()
        ));
        let engine = engine
            .with_paths(
                tmp.join("approval-policies.json"),
                tmp.join("approval-audit.jsonl"),
            )
            .with_events(tx);

        let waiter = {
            let engine = engine.clone();
            tokio::spawn(async move {
                engine
                    .evaluate_and_request("s1", "shell_exec", &serde_json::json!({}), "run")
                    .await
            })
        };

        let id = match rx.recv().await.unwrap().event {
            AgentEvent::ApprovalRequested {
                approval_id,
                tool_name,
                risk_tier,
                ..
            } => {
                assert_eq!(tool_name, "shell_exec");
                assert_eq!(risk_tier, 2);
                approval_id
            }
            other => panic!("expected ApprovalRequested, got {other:?}"),
        };
        assert!(
            engine
                .submit_decision(&id, ApprovalDecision::Approved)
                .await
        );

        match rx.recv().await.unwrap().event {
            AgentEvent::ApprovalResolved {
                approval_id,
                decision,
                ..
            } => {
                assert_eq!(approval_id, id);
                assert_eq!(decision, "approved");
            }
            other => panic!("expected ApprovalResolved, got {other:?}"),
        }
        assert!(matches!(
            waiter.await.unwrap().unwrap(),
            ApprovalDecision::Approved
        ));
    }

    /// Engine whose policy/audit files live in a throwaway temp dir so tests
    /// never touch the operator's real `approval-policies.json`.
    fn test_engine() -> (ApprovalEngine, odon_core::events::EventReceiver) {
        let (tx, rx) = odon_core::events::create_event_bus(64);
        let engine = ApprovalEngine::new(SecurityPolicy::default());
        let tmp = std::env::temp_dir().join(format!(
            "odon-approval-test-{}",
            uuid::Uuid::new_v4().simple()
        ));
        let engine = engine
            .with_paths(
                tmp.join("approval-policies.json"),
                tmp.join("approval-audit.jsonl"),
            )
            .with_events(tx);
        (engine, rx)
    }

    async fn next_requested_id(rx: &mut odon_core::events::EventReceiver) -> String {
        loop {
            match rx.recv().await.unwrap().event {
                AgentEvent::ApprovalRequested { approval_id, .. } => return approval_id,
                _ => continue,
            }
        }
    }

    #[tokio::test]
    async fn always_allow_sticks_for_later_different_parameters() {
        let (engine, mut rx) = test_engine();

        let first = {
            let engine = engine.clone();
            tokio::spawn(async move {
                engine
                    .evaluate_and_request(
                        "s1",
                        "shell_exec",
                        &serde_json::json!({"command": "git status", "cwd": "C:/work"}),
                        "run",
                    )
                    .await
            })
        };
        let id = next_requested_id(&mut rx).await;
        assert!(
            engine
                .submit_decision(&id, ApprovalDecision::ApprovedAlways)
                .await
        );
        assert!(matches!(
            first.await.unwrap().unwrap(),
            ApprovalDecision::ApprovedAlways
        ));

        // A later, different command with the same tool and scope must be
        // approved instantly instead of prompting again.
        let second = engine
            .evaluate_and_request(
                "s1",
                "shell_exec",
                &serde_json::json!({"command": "git diff", "cwd": "C:/work"}),
                "run",
            )
            .await;
        assert!(matches!(second, Ok(ApprovalDecision::Approved)));

        // The same tool in a different scope still prompts, proven by the
        // granted key being scope-bound.
        assert!(engine
            .get_always_allowed()
            .await
            .iter()
            .any(|key| key == "shell_exec|C:/work"));
    }

    #[tokio::test]
    async fn approve_all_covers_other_tools_until_revoked() {
        let (engine, mut rx) = test_engine();

        let first = {
            let engine = engine.clone();
            tokio::spawn(async move {
                engine
                    .evaluate_and_request(
                        "s1",
                        "fs_delete",
                        &serde_json::json!({"path": "C:/work/old.txt"}),
                        "delete",
                    )
                    .await
            })
        };
        let id = next_requested_id(&mut rx).await;
        assert!(
            engine
                .submit_decision(&id, ApprovalDecision::ApprovedAll)
                .await
        );
        // The current request itself resolves as a plain approval.
        assert!(matches!(
            first.await.unwrap().unwrap(),
            ApprovalDecision::Approved
        ));

        // A different tool name is covered by the stored wildcard grant.
        let second = engine
            .evaluate_and_request(
                "s1",
                "shell_exec",
                &serde_json::json!({"command": "git status", "cwd": "C:/work"}),
                "run",
            )
            .await;
        assert!(matches!(second, Ok(ApprovalDecision::Approved)));

        // Revoking the wildcard removes the cover again.
        assert!(engine.revoke_allowed("*").await);
        assert!(!engine
            .get_always_allowed()
            .await
            .iter()
            .any(|key| key == "*"));
    }

    #[test]
    fn always_and_exact_keys_have_different_granularity() {
        let a = serde_json::json!({"command": "git status", "cwd": "C:/work"});
        let b = serde_json::json!({"command": "rm -rf x", "cwd": "C:/work"});
        assert_eq!(
            ApprovalEngine::policy_key("shell_exec", &a),
            ApprovalEngine::policy_key("shell_exec", &b),
        );
        assert_ne!(
            ApprovalEngine::exact_policy_key("shell_exec", &a),
            ApprovalEngine::exact_policy_key("shell_exec", &b),
        );
        assert_eq!(ApprovalEngine::wildcard_policy_key(&a), "*".to_string());
    }

    #[tokio::test]
    async fn approved_all_does_not_bypass_locked_mode() {
        let tmp =
            std::env::temp_dir().join(format!("odon-locked-{}", uuid::Uuid::new_v4().simple()));
        let policy = SecurityPolicy {
            mode: ExecutionMode::Locked,
            ..SecurityPolicy::default()
        };
        let engine = ApprovalEngine::new(policy)
            .with_paths(tmp.join("approval-policies.json"), tmp.join("audit.jsonl"));
        // A real wildcard grant, stored the way `ApprovedAll` stores it. (The
        // previous version recorded nothing — `submit_decision` on an unknown
        // id is a no-op — so the assertion passed without testing a bypass.)
        engine.always_allowed.write().await.insert("*".to_string());

        let decision = engine
            .evaluate_and_request("s1", "shell_exec", &serde_json::json!({}), "bash")
            .await
            .unwrap();
        assert!(
            matches!(decision, ApprovalDecision::Rejected { .. }),
            "Locked mode must refuse a mutation even with a `*` grant stored, got {decision:?}"
        );
    }

    fn temp_dir(prefix: &str) -> std::path::PathBuf {
        let dir =
            std::env::temp_dir().join(format!("odon-{prefix}-{}", uuid::Uuid::new_v4().simple()));
        std::fs::create_dir_all(&dir).expect("create temp dir");
        dir
    }

    /// A target that does not exist yet must still be resolved against the
    /// canonical ancestor, not against whatever spelling the caller used.
    /// `temp_dir()` can return an 8.3 short path on Windows while
    /// `canonicalize` resolves the long one, which made every in-workspace
    /// file look like an escape.
    #[test]
    fn missing_target_resolves_against_the_canonical_root() {
        let root = temp_dir("ws-missing");
        std::fs::create_dir_all(root.join("nested")).expect("create nested");
        let canonical = ApprovalEngine::canonical_root(&root);

        let resolved = ApprovalEngine::strip_verbatim(
            ApprovalEngine::canonicalize_existing_ancestor(&root.join("nested").join("new.txt"))
                .expect("existing ancestor resolves"),
        );
        assert_eq!(resolved, canonical.join("nested").join("new.txt"));
        assert!(resolved.starts_with(&canonical));

        // The containment answer must not depend on the root's spelling.
        let spelled = root.join(".");
        assert_eq!(
            ApprovalEngine::outside_workspace_target(
                &serde_json::json!({ "path": "nested/new.txt" }),
                &spelled
            ),
            None,
            "in-workspace target is inside for a non-canonical root spelling"
        );

        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn outside_workspace_targets_are_detected_after_normalisation() {
        let root = temp_dir("ws-root");
        std::fs::create_dir_all(root.join("nested")).expect("create nested");

        assert_eq!(
            ApprovalEngine::outside_workspace_target(
                &serde_json::json!({ "path": "nested/file.txt" }),
                &root
            ),
            None,
            "a relative in-workspace path is fine"
        );
        assert!(
            ApprovalEngine::outside_workspace_target(
                &serde_json::json!({ "path": "nested/../../escape.txt" }),
                &root
            )
            .is_some(),
            "`..` must be resolved before containment is decided"
        );

        // A real directory outside the root, so the check is platform-neutral.
        let elsewhere = temp_dir("ws-elsewhere");
        let escaped = elsewhere.join("hosts");
        assert_eq!(
            ApprovalEngine::outside_workspace_target(
                &serde_json::json!({ "destination": escaped.to_string_lossy() }),
                &root
            ),
            Some(escaped.to_string_lossy().to_string()),
            "an absolute path elsewhere is reported verbatim for the prompt"
        );
        assert!(!ApprovalEngine::is_outside_workspace(
            &serde_json::json!({ "path": "nested/file.txt" }),
            &root
        ));

        let _ = std::fs::remove_dir_all(root);
        let _ = std::fs::remove_dir_all(elsewhere);
    }

    /// A symlink planted inside the workspace must not become a trampoline out
    /// of it. The ancestor is canonicalised before the prefix check, so this
    /// holds even when the leaf file does not exist yet.
    #[test]
    fn symlink_inside_the_workspace_cannot_escape_it() {
        let root = temp_dir("ws-link-root");
        let outside = temp_dir("ws-link-outside");
        std::fs::write(outside.join("secret.txt"), "secret").expect("write outside file");

        let link = root.join("escape");
        let linked = {
            #[cfg(unix)]
            {
                std::os::unix::fs::symlink(&outside, &link).is_ok()
            }
            #[cfg(windows)]
            {
                std::os::windows::fs::symlink_dir(&outside, &link).is_ok()
            }
        };
        if !linked {
            // Creating symlinks needs a privilege this runner may not have.
            let _ = std::fs::remove_dir_all(root);
            let _ = std::fs::remove_dir_all(outside);
            return;
        }

        for target in ["escape/secret.txt", "escape/new-file.txt"] {
            assert!(
                ApprovalEngine::outside_workspace_target(
                    &serde_json::json!({ "path": target }),
                    &root
                )
                .is_some(),
                "a target reached through an escaping symlink must be reported: {target}"
            );
        }
        // The root's spelling must not change the answer.
        assert!(
            ApprovalEngine::outside_workspace_target(
                &serde_json::json!({ "path": "escape/new-file.txt" }),
                &root.join(".")
            )
            .is_some(),
            "a non-canonical root spelling must not hide the escape"
        );

        let _ = std::fs::remove_dir_all(root);
        let _ = std::fs::remove_dir_all(outside);
    }

    /// Absolute targets are resolved (`..` included) before containment is
    /// decided, so an absolute in-workspace path is not treated as an escape.
    #[test]
    fn absolute_targets_are_resolved_before_containment() {
        let root = temp_dir("ws-absolute");
        std::fs::create_dir_all(root.join("nested")).expect("create nested");
        let inside = root.join("nested").join("..").join("file.txt");

        assert_eq!(
            ApprovalEngine::outside_workspace_target(
                &serde_json::json!({ "path": inside.to_string_lossy() }),
                &root
            ),
            None,
            "an absolute path that resolves inside the workspace is fine"
        );

        let elsewhere = temp_dir("ws-absolute-out");
        assert!(
            ApprovalEngine::outside_workspace_target(
                &serde_json::json!({ "path": elsewhere.to_string_lossy() }),
                &root
            )
            .is_some(),
            "an absolute path outside the workspace must be reported"
        );

        let _ = std::fs::remove_dir_all(root);
        let _ = std::fs::remove_dir_all(elsewhere);
    }

    /// An out-of-workspace target must reach a human even in Autonomous mode,
    /// where Tier-1 calls are otherwise auto-approved, and even when a cached
    /// grant exists for exactly these arguments.
    #[tokio::test]
    async fn autonomous_mode_does_not_auto_approve_outside_the_workspace() {
        let root = temp_dir("ws-grant");
        let tmp = temp_dir("ws-policy");
        let (tx, mut rx) = odon_core::events::create_event_bus(64);
        let policy = SecurityPolicy {
            mode: ExecutionMode::Autonomous,
            ..SecurityPolicy::default()
        };
        let engine = ApprovalEngine::new(policy)
            .with_paths(tmp.join("approval-policies.json"), tmp.join("audit.jsonl"))
            .with_events(tx)
            .with_workspace_root(root.clone());

        let outside = serde_json::json!({ "path": "../nope/escape.txt" });
        // A cached grant for this exact invocation must not cover it.
        engine
            .always_allowed
            .write()
            .await
            .insert(ApprovalEngine::exact_policy_key(
                "create_directory",
                &outside,
            ));

        let handle = {
            let engine = engine.clone();
            tokio::spawn(async move {
                engine
                    .evaluate_and_request("s1", "create_directory", &outside, "mkdir")
                    .await
                    .unwrap()
            })
        };

        let approval_id = next_requested_id(&mut rx).await;
        assert!(
            engine
                .submit_decision(
                    &approval_id,
                    ApprovalDecision::Rejected {
                        reason: "no".to_string()
                    }
                )
                .await,
            "the request must be pending, i.e. the operator was actually asked"
        );
        let decision = handle.await.expect("evaluate task");
        assert!(
            matches!(decision, ApprovalDecision::Rejected { .. }),
            "an outside-workspace mutation must be decided by a human, got {decision:?}"
        );

        // Inside the workspace the Autonomous Tier-1 shortcut still applies.
        let decision = engine
            .evaluate_and_request(
                "s1",
                "create_directory",
                &serde_json::json!({ "path": "sub/dir" }),
                "mkdir",
            )
            .await
            .unwrap();
        assert!(
            matches!(decision, ApprovalDecision::Approved),
            "in-workspace Tier-1 work must stay auto-approved, got {decision:?}"
        );

        let _ = std::fs::remove_dir_all(root);
        let _ = std::fs::remove_dir_all(tmp);
    }

    /// The tool-declared tier and the name-based table must not disagree in a
    /// way that weakens the gate: the effective tier is the stricter one.
    #[tokio::test]
    async fn a_declared_tier_can_only_raise_the_name_table_risk() {
        let tmp = temp_dir("ws-risk");
        let (tx, mut rx) = odon_core::events::create_event_bus(64);
        let engine = ApprovalEngine::new(SecurityPolicy {
            mode: ExecutionMode::Autonomous,
            ..SecurityPolicy::default()
        })
        .with_paths(tmp.join("approval-policies.json"), tmp.join("audit.jsonl"))
        .with_events(tx);

        // The name table calls `create_directory` Tier 1, which Autonomous mode
        // auto-approves. A tool that declares Tier 2 must still be asked about.
        let outside_workspace_free = serde_json::json!({ "path": "sub/dir" });
        let handle = {
            let engine = engine.clone();
            let params = outside_workspace_free.clone();
            tokio::spawn(async move {
                engine
                    .evaluate_and_request_with_risk(
                        "s1",
                        "create_directory",
                        Some(RiskTier::Tier2SystemMutating),
                        &params,
                        "mkdir",
                    )
                    .await
                    .unwrap()
            })
        };
        let approval_id = next_requested_id(&mut rx).await;
        assert!(
            engine
                .submit_decision(
                    &approval_id,
                    ApprovalDecision::Rejected {
                        reason: "declared tier2".to_string()
                    }
                )
                .await
        );
        assert!(matches!(
            handle.await.expect("evaluate task"),
            ApprovalDecision::Rejected { .. }
        ));

        // A tool under-declaring read-only cannot talk its way out of the
        // name table's Tier 2 either.
        let handle = {
            let engine = engine.clone();
            tokio::spawn(async move {
                engine
                    .evaluate_and_request_with_risk(
                        "s1",
                        "shell_exec",
                        Some(RiskTier::Tier0ReadOnly),
                        &serde_json::json!({ "command": "echo hi" }),
                        "run",
                    )
                    .await
                    .unwrap()
            })
        };
        let approval_id = next_requested_id(&mut rx).await;
        assert!(
            engine
                .submit_decision(
                    &approval_id,
                    ApprovalDecision::Rejected {
                        reason: "under-declared".to_string()
                    }
                )
                .await
        );
        assert!(matches!(
            handle.await.expect("evaluate task"),
            ApprovalDecision::Rejected { .. }
        ));

        // A Tier 1 declaration that agrees with the table keeps the shortcut.
        let decision = engine
            .evaluate_and_request_with_risk(
                "s1",
                "create_directory",
                Some(RiskTier::Tier1WorkspaceMutating),
                &serde_json::json!({ "path": "sub/dir" }),
                "mkdir",
            )
            .await
            .unwrap();
        assert!(matches!(decision, ApprovalDecision::Approved));

        let _ = std::fs::remove_dir_all(tmp);
    }
}
