pub mod artifact_gen;
pub mod fs;
pub mod git;
pub mod github;
pub mod harness;
pub mod mcp;
pub mod memory;
pub mod registry;
pub mod shell;
pub mod system;
pub mod web;

pub use registry::{Tool, ToolContext, ToolRegistry, ToolSchema};

pub fn build_full_tool_registry() -> ToolRegistry {
    let mut reg = ToolRegistry::new();

    // 1. Filesystem & Termigo Compatibility (17 tools)
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

    // 2. Harness & Planning (6 tools)
    reg.register(Box::new(harness::TodoWriteTool));
    reg.register(Box::new(harness::TodoReadTool));
    reg.register(Box::new(harness::TodoUpdateTool));
    reg.register(Box::new(harness::ThinkTool));
    reg.register(Box::new(harness::PlanModeTool));
    reg.register(Box::new(harness::RememberTool));

    // 3. Git Operations (14 tools)
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

    // 4. GitHub Integration (4 tools)
    reg.register(Box::new(github::GitHubGetRepoTool::new()));
    reg.register(Box::new(github::GitHubListIssuesTool::new()));
    reg.register(Box::new(github::GitHubCreateIssueTool::new()));
    reg.register(Box::new(github::GitHubListPullRequestsTool::new()));

    // 5. Shell & Execution (3 tools)
    reg.register(Box::new(shell::ShellExecTool));
    reg.register(Box::new(shell::BashRunTool));
    reg.register(Box::new(shell::TerminalExecTool));

    // 6. MCP Integration (2 tools)
    reg.register(Box::new(mcp::McpListToolsTool));
    reg.register(Box::new(mcp::McpCallTool));

    // 7. Web, Network & Security Audit (7 tools)
    reg.register(Box::new(web::WebSearchTool));
    reg.register(Box::new(web::WebFetchTool));
    reg.register(Box::new(web::ProbeUrlTool));
    reg.register(Box::new(web::HttpGetTool));
    reg.register(Box::new(web::HttpPostTool));
    reg.register(Box::new(web::DnsLookupTool));
    reg.register(Box::new(web::PortScanTool));

    // 8. System Environment & Evaluation (6 tools)
    reg.register(Box::new(system::SystemInfoTool));
    reg.register(Box::new(system::CalcEvalTool));
    reg.register(Box::new(system::EnvGetTool));
    reg.register(Box::new(system::EnvListTool));
    reg.register(Box::new(system::ProcessListTool));
    reg.register(Box::new(system::DiskUsageTool));

    // 9. Memory & Long-term Context (2 tools)
    reg.register(Box::new(memory::MemoryStoreTool));
    reg.register(Box::new(memory::MemoryRecallTool));

    // 10. Artifact Synthesis (4 tools)
    reg.register(Box::new(artifact_gen::ArtifactCreateCodeTool));
    reg.register(Box::new(artifact_gen::ArtifactCreateDocumentTool));
    reg.register(Box::new(artifact_gen::ArtifactCreateImageTool));
    reg.register(Box::new(artifact_gen::ArtifactUpdateTool));

    reg
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashSet;

    #[test]
    fn full_registry_has_over_fifty_tools_with_unique_names() {
        let reg = build_full_tool_registry();
        let schemas = reg.schemas();
        println!("Registered tools count: {}", schemas.len());
        assert!(schemas.len() >= 50, "Expected >= 50 tools, got {}", schemas.len());

        let mut names = HashSet::new();
        for s in &schemas {
            assert!(names.insert(&s.name), "Duplicate tool registered: {}", s.name);
            assert!(!s.description.is_empty(), "Tool {} missing description", s.name);
        }
    }
}

