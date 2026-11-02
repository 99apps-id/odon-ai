pub mod artifact_gen;
pub mod coding;
pub mod quality;
pub mod fs;
pub mod fs_ext;
pub mod git;
pub mod git_ext;
pub mod github;
pub mod harness;
pub mod mcp;
pub mod net_guard;
pub mod memory;
pub mod ops_ext;
pub mod registry;
pub mod shell;
pub mod shell_ext;
pub mod sftp;
pub mod skills;
pub mod system;
pub mod web;
pub mod web_ext;

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
    reg.register(Box::new(fs_ext::FsChownTool));
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

    // 5. Shell & Execution
    reg.register(Box::new(shell::ShellExecTool));
    reg.register(Box::new(shell::BashRunTool));
    reg.register(Box::new(shell::TerminalExecTool));

    // 5a. SFTP via scp
    reg.register(Box::new(sftp::SftpUploadTool));
    reg.register(Box::new(sftp::SftpDownloadTool));
    reg.register(Box::new(sftp::SftpListTool));

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

    // 6. MCP Integration
    reg.register(Box::new(mcp::McpListToolsTool));
    reg.register(Box::new(mcp::McpCallTool));
    reg.register(Box::new(mcp::McpCallStreamTool));
    reg.register(Box::new(mcp::McpRemoveServerTool));
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
    reg.register(Box::new(ops_ext::ApprovalDecideTool));
    reg.register(Box::new(ops_ext::SessionListTool));
    reg.register(Box::new(ops_ext::SessionSwitchTool));
    reg.register(Box::new(ops_ext::SessionExportTool));
    reg.register(Box::new(ops_ext::RelayStatusTool));
    reg.register(Box::new(ops_ext::AutomationListTool));
    reg.register(Box::new(ops_ext::AutomationTriggerTool));
    reg.register(Box::new(ops_ext::SettingsGetTool));
    reg.register(Box::new(ops_ext::SettingsSetTool));

    reg
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::registry::ToolAvailability;
    use std::collections::HashSet;

    #[test]
    fn full_registry_contract_is_explicit_and_valid() {
        let reg = build_full_tool_registry();
        let schemas = reg.schemas();
        assert_eq!(
            schemas.len(),
            169,
            "Tool surface changed; review the addition or removal and update this contract intentionally"
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

        assert_eq!(
            unavailable,
            HashSet::from(["browser_click", "session_switch"]),
            "Unavailable capabilities must be reviewed explicitly"
        );

        let diagnostics = reg.diagnostics();
        assert_eq!(diagnostics.len(), schemas.len());
        assert_eq!(diagnostics.iter().filter(|item| item.ready).count(), 167);
    }
}
