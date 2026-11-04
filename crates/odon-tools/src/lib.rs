pub mod artifact_gen;
pub mod browser_bridge;
pub mod browser_session;
pub mod coding;
pub mod daemon_ops;
pub mod dev_server;
pub mod fs;
pub mod fs_ext;
pub mod git;
pub mod git_ext;
pub mod github;
pub mod gmaps;
pub mod harness;
pub mod local_app;
pub mod mcp;
pub mod media;
pub mod memory;
pub mod net_guard;
pub mod ops_ext;
pub mod project_ops;
pub mod provider_ops;
pub mod quality;
pub mod registry;
pub mod review;
pub mod runtime;
pub mod sftp;
pub mod shell;
pub mod shell_ext;
pub mod skills;
pub mod system;
pub mod unified;
pub mod web;
pub mod web_ext;
pub mod workspace_path;

pub use registry::{Tool, ToolContext, ToolRegistry, ToolSchema};

pub fn build_full_tool_registry() -> ToolRegistry {
    let mut reg = ToolRegistry::new();

    // 1. Filesystem & Termigo Compatibility
    reg.register(Box::new(fs::FsReadTool));
    reg.register(Box::new(fs::FsWriteTool));
    reg.register(Box::new(fs::FsEditTool));
    reg.register(Box::new(fs::FsListTool));
    reg.register(Box::new(fs::FsSearchTool));
    reg.register(Box::new(fs::FsDeleteTool));
    reg.register(Box::new(fs::FsMkdirTool));
    reg.register(Box::new(fs::FsMoveTool));
    reg.register(Box::new(fs::FsCopyTool));
    reg.register(Box::new(fs::FsGlobTool));
    reg.register(Box::new(fs::MultiEditTool));
    reg.register(Box::new(fs::TermigoReadFileTool));
    reg.register(Box::new(fs::TermigoWriteFileTool));
    reg.register(Box::new(fs::TermigoEditTool));
    reg.register(Box::new(fs::TermigoListDirTool));
    reg.register(Box::new(fs::TermigoDeleteFileTool));
    reg.register(Box::new(fs::TermigoGrepTool));

    // Extended Filesystem & Editor Tools
    reg.register(Box::new(fs_ext::FsStatTool));
    reg.register(Box::new(fs_ext::FsChmodTool));
    reg.register(Box::new(fs_ext::FsOwnerCheckTool));
    reg.register(Box::new(fs_ext::FsSymlinkCreateTool));
    reg.register(Box::new(fs_ext::FsReadlinkTool));
    reg.register(Box::new(fs_ext::FsDiffTool));
    reg.register(Box::new(fs_ext::FsPatchTool));
    reg.register(Box::new(fs_ext::FsTreeTool));
    reg.register(Box::new(fs_ext::FsDuTool));
    reg.register(Box::new(fs_ext::FsCopyRecursiveTool));
    reg.register(Box::new(fs_ext::FsMoveRecursiveTool));
    reg.register(Box::new(fs_ext::FsFindTool));
    reg.register(Box::new(fs_ext::FsZipTool));
    reg.register(Box::new(fs_ext::FsUnzipTool));
    reg.register(Box::new(fs_ext::TermigoMultiPasteTool));
    reg.register(Box::new(fs_ext::TermigoCopySelectionTool));
    reg.register(Box::new(fs_ext::TermigoPasteBufferHistoryTool));
    reg.register(Box::new(fs_ext::EditorOpenTool));
    reg.register(Box::new(fs_ext::EditorRevealTool));
    reg.register(Box::new(fs_ext::EditorSaveTool));
    reg.register(Box::new(fs_ext::EditorReloadTool));

    // 2. Planning & Harness
    reg.register(Box::new(harness::TodoWriteTool));
    reg.register(Box::new(harness::TodoReadTool));
    reg.register(Box::new(harness::TodoUpdateTool));
    reg.register(Box::new(harness::ThinkTool));
    reg.register(Box::new(harness::PlanModeTool));
    reg.register(Box::new(harness::RememberTool));

    // 3. Git Operations
    reg.register(Box::new(git::GitStatusTool));
    reg.register(Box::new(git::GitDiffTool));
    reg.register(Box::new(git::GitCommitTool));
    reg.register(Box::new(git::GitLogTool));
    reg.register(Box::new(git::GitBranchTool));
    reg.register(Box::new(git::GitAddTool));
    reg.register(Box::new(git::GitCheckoutTool));
    reg.register(Box::new(git::GitShowTool));
    reg.register(Box::new(git::GitStashTool));
    reg.register(Box::new(git::GitStashPopTool));
    reg.register(Box::new(git::GitStashListTool));
    reg.register(Box::new(git::GitPullTool));
    reg.register(Box::new(git::GitPushTool));
    reg.register(Box::new(git::GitResetTool));

    // Extended Git Operations
    reg.register(Box::new(git_ext::GitMergeTool));
    reg.register(Box::new(git_ext::GitRebaseTool));
    reg.register(Box::new(git_ext::GitCherryPickTool));
    reg.register(Box::new(git_ext::GitRevertTool));
    reg.register(Box::new(git_ext::GitResetHardTool));
    reg.register(Box::new(git_ext::GitCleanTool));
    reg.register(Box::new(git_ext::GitTagTool));
    reg.register(Box::new(git_ext::GitRemoteAddTool));
    reg.register(Box::new(git_ext::GitRemoteSetUrlTool));
    reg.register(Box::new(git_ext::GitBlameTool));
    reg.register(Box::new(git_ext::GitLogFileTool));
    reg.register(Box::new(git_ext::GitLsFilesTool));

    // 4. GitHub Operations
    reg.register(Box::new(github::GitHubGetRepoTool::new()));
    reg.register(Box::new(github::GitHubListIssuesTool::new()));
    reg.register(Box::new(github::GitHubCreateIssueTool::new()));
    reg.register(Box::new(github::GitHubListPullRequestsTool::new()));

    // Extended GitHub Operations
    reg.register(Box::new(git_ext::GitHubCreateReviewTool::new()));
    reg.register(Box::new(git_ext::GitHubListCommentsTool::new()));
    reg.register(Box::new(git_ext::GitHubMergePrTool::new()));
    reg.register(Box::new(git_ext::GitHubWorkflowListTool::new()));
    reg.register(Box::new(git_ext::GitHubWorkflowRunTool::new()));
    reg.register(Box::new(git_ext::GitHubWorkflowRunsTool::new()));
    reg.register(Box::new(git_ext::GitHubWorkflowJobsTool::new()));
    reg.register(Box::new(git_ext::GitHubWorkflowLogsTool::new()));
    reg.register(Box::new(git_ext::GitHubPrFilesTool::new()));

    // 4b. Code Intelligence & Quality
    reg.register(Box::new(coding::CodeIndexTool));
    reg.register(Box::new(coding::CodeSearchTool));
    reg.register(Box::new(quality::FormatCodeTool));
    reg.register(Box::new(quality::RunChecksTool));
    reg.register(Box::new(quality::TestFileTool));
    reg.register(Box::new(review::ReviewDiffTool));
    reg.register(Box::new(review::ReviewFileTool));
    reg.register(Box::new(review::ReviewSecurityTool));
    reg.register(Box::new(review::ReviewSessionChangesTool));
    reg.register(Box::new(review::ChangeSummaryTool));

    // 5. Shell & Execution
    reg.register(Box::new(shell::ShellExecTool));
    reg.register(Box::new(shell::BashRunTool));
    reg.register(Box::new(shell::TerminalExecTool));

    // 5a. SFTP via scp
    reg.register(Box::new(sftp::SftpUploadTool));
    reg.register(Box::new(sftp::SftpDownloadTool));
    reg.register(Box::new(sftp::SftpListTool));
    reg.register(Box::new(sftp::SshExecTool));
    reg.register(Box::new(sftp::SshProfileTestTool));

    // Extended Shell & Process Management
    reg.register(Box::new(shell_ext::ProcessKillTool));
    reg.register(Box::new(shell_ext::ExecBackgroundTool));
    reg.register(Box::new(shell_ext::ExecStreamStdoutTool));
    reg.register(Box::new(shell_ext::ExecBackgroundListTool));
    reg.register(Box::new(shell_ext::ExecBackgroundKillTool));
    reg.register(Box::new(shell_ext::ProjectTaskTool));
    reg.register(Box::new(shell_ext::CronAddTool));
    reg.register(Box::new(shell_ext::CronListTool));
    reg.register(Box::new(shell_ext::CronRemoveTool));
    reg.register(Box::new(shell_ext::FileHashTool));
    reg.register(Box::new(shell_ext::FileChecksumTool));
    reg.register(Box::new(shell_ext::ClipboardReadTool));
    reg.register(Box::new(shell_ext::ClipboardWriteTool));
    reg.register(Box::new(shell_ext::NotificationSendTool));
    reg.register(Box::new(shell_ext::TerminalInfoTool));
    reg.register(Box::new(shell_ext::ProcessWaitTool));
    reg.register(Box::new(runtime::DevServerStatusTool));
    reg.register(Box::new(runtime::BrowserStatusTool));
    reg.register(Box::new(dev_server::DevServerStartTool));
    reg.register(Box::new(dev_server::DevServerStopTool));
    reg.register(Box::new(dev_server::DevServerLogsTool));

    // 6. MCP Integration
    reg.register(Box::new(mcp::McpListToolsTool));
    reg.register(Box::new(mcp::McpCallTool));
    reg.register(Box::new(mcp::McpCallStreamTool));
    reg.register(Box::new(mcp::McpRemoveServerTool));
    reg.register(Box::new(mcp::McpToggleServerTool));
    reg.register(Box::new(ops_ext::McpAddServerTool));
    reg.register(Box::new(ops_ext::McpListServersTool));

    // 7. Web, Network & Security Audit
    reg.register(Box::new(web::WebSearchTool));
    reg.register(Box::new(web::WebFetchTool));
    reg.register(Box::new(web::ProbeUrlTool));
    reg.register(Box::new(web::HttpGetTool));
    reg.register(Box::new(web::HttpPostTool));
    reg.register(Box::new(web::DnsLookupTool));
    reg.register(Box::new(web::PortScanTool));
    reg.register(Box::new(gmaps::GmapsSearchTool));

    // Extended Web Tools
    reg.register(Box::new(web_ext::HttpPutTool));
    reg.register(Box::new(web_ext::HttpPatchTool));
    reg.register(Box::new(web_ext::HttpDeleteTool));
    reg.register(Box::new(web_ext::HttpDownloadTool));
    reg.register(Box::new(web_ext::HttpUploadTool));
    reg.register(Box::new(web_ext::WebSearchNewsTool));
    reg.register(Box::new(web_ext::WebSearchCodeTool));
    reg.register(Box::new(web_ext::SecurityAuditHeadersTool));
    reg.register(Box::new(web_ext::SecurityAuditCertTool));
    reg.register(Box::new(web_ext::NetworkTracerouteTool));
    reg.register(Box::new(web_ext::WebScrapeTool));
    reg.register(Box::new(web_ext::UrlPreviewTool));
    reg.register(Box::new(web::BrowserOpenTool));
    reg.register(Box::new(web::BrowserNavigateTool));
    reg.register(Box::new(web::BrowserScreenshotTool));
    reg.register(Box::new(web::BrowserClickTool));
    reg.register(Box::new(web::BrowserDownloadTool));
    reg.register(Box::new(local_app::LocalAppListTool));
    reg.register(Box::new(local_app::LocalAppLaunchTool));
    reg.register(Box::new(local_app::LocalAppFocusTool));
    reg.register(Box::new(local_app::LocalAppCloseTool));
    reg.register(Box::new(local_app::LocalAppInputTool));
    reg.register(Box::new(media::SendDocumentTool));
    reg.register(Box::new(media::SendImageTool));
    reg.register(Box::new(media::SendMediaTool));
    reg.register(Box::new(browser_session::BrowserSessionCreateTool));
    reg.register(Box::new(browser_session::BrowserSessionListTool));
    reg.register(Box::new(browser_session::BrowserSessionResumeTool));
    reg.register(Box::new(browser_session::BrowserSessionCloseTool));
    reg.register(Box::new(browser_session::BrowserSessionNavigateTool));
    reg.register(Box::new(browser_session::BrowserSessionClickTool));
    reg.register(Box::new(browser_session::BrowserSessionTextTool));
    reg.register(Box::new(browser_session::BrowserSessionTypeTool));
    reg.register(Box::new(browser_session::BrowserSessionWaitTool));
    reg.register(Box::new(browser_session::BrowserSessionScreenshotTool));
    reg.register(Box::new(browser_session::BrowserSessionSelectTool));

    // Provider operations
    reg.register(Box::new(provider_ops::ProviderListTool));
    reg.register(Box::new(provider_ops::ProviderTestTool));
    reg.register(Box::new(provider_ops::ProviderSetDefaultTool));
    reg.register(Box::new(provider_ops::ProviderDeleteTool));

    // Daemon operations
    reg.register(Box::new(daemon_ops::DaemonStatusTool));
    reg.register(Box::new(daemon_ops::DaemonMetricsTool));
    reg.register(Box::new(daemon_ops::ToolDiagnosticsTool));
    reg.register(Box::new(daemon_ops::DoctorTool));
    reg.register(Box::new(daemon_ops::WorkspaceAddTool));
    reg.register(Box::new(daemon_ops::WorkspaceRemoveTool));
    reg.register(Box::new(daemon_ops::WorkspaceListTool));

    // Project operations
    reg.register(Box::new(project_ops::ProjectDetectTool));
    reg.register(Box::new(project_ops::ProjectScriptsTool));
    reg.register(Box::new(project_ops::DependencyAuditTool));

    // Autonomous Skills (.odon/skills)
    reg.register(Box::new(skills::SkillListTool));
    reg.register(Box::new(skills::SkillCreateTool));
    reg.register(Box::new(skills::SkillRunTool));

    // 8. System Environment & Evaluation
    reg.register(Box::new(system::SystemInfoTool));
    reg.register(Box::new(system::CalcEvalTool));
    reg.register(Box::new(system::EnvGetTool));
    reg.register(Box::new(system::EnvListTool));
    reg.register(Box::new(shell_ext::ProcessListTool));
    reg.register(Box::new(system::DiskUsageTool));

    // 9. Memory & Long-term Context
    reg.register(Box::new(memory::MemoryStoreTool));
    reg.register(Box::new(memory::MemoryRecallTool));
    reg.register(Box::new(memory::MemoryForgetTool));
    reg.register(Box::new(memory::MemorySearchTool));

    // 10. Artifact Synthesis
    reg.register(Box::new(artifact_gen::ArtifactCreateCodeTool));
    reg.register(Box::new(artifact_gen::ArtifactCreateDocumentTool));
    reg.register(Box::new(artifact_gen::ArtifactCreateChartTool));
    reg.register(Box::new(artifact_gen::ArtifactCreateImageTool));
    reg.register(Box::new(artifact_gen::ArtifactUpdateTool));
    reg.register(Box::new(artifact_gen::ArtifactImportTool));
    reg.register(Box::new(ops_ext::ArtifactDeleteTool));
    reg.register(Box::new(ops_ext::ArtifactExportTool));

    // 11. Approvals, Sessions & Relays
    reg.register(Box::new(ops_ext::ApprovalListTool));
    reg.register(Box::new(ops_ext::ApprovalHistoryTool));
    reg.register(Box::new(ops_ext::ApprovalPolicyRevokeTool));
    reg.register(Box::new(ops_ext::ApprovalDecideTool));
    reg.register(Box::new(ops_ext::AuditSearchTool));
    reg.register(Box::new(ops_ext::SessionListTool));
    reg.register(Box::new(ops_ext::SessionGetTool));
    reg.register(Box::new(ops_ext::SessionMessagesTool));
    reg.register(Box::new(ops_ext::SessionDeleteTool));
    reg.register(Box::new(ops_ext::SessionSwitchTool));
    reg.register(Box::new(ops_ext::SessionExportTool));
    reg.register(Box::new(ops_ext::RelayStatusTool));
    reg.register(Box::new(ops_ext::AutomationListTool));
    reg.register(Box::new(ops_ext::AutomationTriggerTool));
    reg.register(Box::new(ops_ext::AutomationLogsTool));
    reg.register(Box::new(ops_ext::AutomationUpdateTool));
    reg.register(Box::new(ops_ext::AutomationDeleteTool));
    reg.register(Box::new(ops_ext::SettingsGetTool));
    reg.register(Box::new(ops_ext::SettingsSetTool));

    reg
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::registry::ToolAvailability;
    use crate::registry::ToolVisibility;
    use std::collections::HashSet;

    #[test]
    fn full_registry_contract_is_explicit_and_valid() {
        let reg = build_full_tool_registry();
        assert!(reg.contains("termigopastebuffer_history"));
        let schemas = reg.schemas();
        let all_schemas = reg.all_schemas();
        let compatibility = all_schemas
            .iter()
            .filter(|schema| schema.visibility == ToolVisibility::Compatibility)
            .map(|schema| schema.name.as_str())
            .collect::<HashSet<_>>();
        assert_eq!(
            schemas.len(),
            212,
            "Tool surface changed; review the addition or removal and update this contract intentionally"
        );
        assert_eq!(
            compatibility,
            HashSet::from([
                "fs_read",
                "fs_write",
                "fs_edit",
                "fs_list",
                "fs_search",
                "fs_delete",
                "bash_run",
                "terminal_exec",
                "cron_add",
                "cron_list",
                "cron_remove",
                // Security perimeter: these mutate approval policy, widen the
                // workspace, or add MCP servers, so they are never advertised
                // to the model. The UI drives them over HTTP instead.
                "approval_decide",
                "settings_set",
                "workspace_add",
                "mcp_add_server",
            ]),
            "Compatibility tools must be reviewed explicitly"
        );

        let mut names = HashSet::new();
        let mut unavailable = HashSet::new();
        for schema in &schemas {
            assert!(
                names.insert(&schema.name),
                "Duplicate tool registered: {}",
                schema.name
            );
            assert!(
                !schema.description.trim().is_empty(),
                "Tool {} missing description",
                schema.name
            );
            assert!(
                schema.risk_tier <= 3,
                "Tool {} has invalid risk tier {}",
                schema.name,
                schema.risk_tier
            );
            assert_eq!(
                schema
                    .parameters
                    .get("type")
                    .and_then(serde_json::Value::as_str),
                Some("object"),
                "Tool {} must use an object parameter schema",
                schema.name
            );
            if let ToolAvailability::Unavailable { reason } = &schema.availability {
                assert!(
                    !reason.trim().is_empty(),
                    "Tool {} has no unavailability reason",
                    schema.name
                );
                unavailable.insert(schema.name.as_str());
            }
        }

        let unavailable = all_schemas
            .iter()
            .filter(|schema| matches!(schema.availability, ToolAvailability::Unavailable { .. }))
            .map(|schema| schema.name.as_str())
            .collect::<HashSet<_>>();

        assert_eq!(
            unavailable,
            HashSet::from([
                "session_switch",
                "mcp_call_stream",
                "editor_open",
                "editor_reveal",
                "security_audit_cert",
            ]),
            "Unavailable capabilities must be reviewed explicitly"
        );

        let diagnostics = reg.diagnostics();
        assert_eq!(diagnostics.len(), all_schemas.len());
        assert_eq!(diagnostics.iter().filter(|item| item.ready).count(), 219);
    }

    #[test]
    fn golden_manifest_matches_registry() {
        let manifest = std::fs::read_to_string(
            std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
                .join("src")
                .join("golden_manifest.json"),
        )
        .expect("golden_manifest.json must exist");
        let manifest_items: Vec<serde_json::Value> =
            serde_json::from_str(&manifest).expect("manifest must be valid JSON");
        let reg = build_full_tool_registry();
        let schemas = reg.all_schemas();
        let diagnostics = reg.diagnostics();

        let by_name: std::collections::HashMap<_, _> =
            schemas.iter().map(|s| (s.name.as_str(), s)).collect();
        let diagnostic_by_name: std::collections::HashMap<_, _> =
            diagnostics.iter().map(|d| (d.name.as_str(), d)).collect();

        assert_eq!(
            manifest_items.len(),
            schemas.len(),
            "manifest length must match registry length"
        );

        for item in &manifest_items {
            let name = item
                .get("name")
                .and_then(|v| v.as_str())
                .expect("manifest item must have name");
            let schema = by_name.get(name).expect("manifest references unknown tool");
            let diagnostic = diagnostic_by_name
                .get(name)
                .expect("manifest references tool without diagnostic");

            assert_eq!(
                schema.availability.label(),
                item.get("availability")
                    .and_then(|v| v.get("status"))
                    .and_then(|v| v.as_str())
                    .unwrap(),
                "availability mismatch for {name}"
            );
            assert_eq!(
                diagnostic.ready,
                item.get("ready").and_then(|v| v.as_bool()).unwrap(),
                "readiness mismatch for {name}"
            );
        }
    }
}
