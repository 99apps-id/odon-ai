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
    // Filesystem
    reg.register(Box::new(fs::FsReadTool));
    reg.register(Box::new(fs::FsWriteTool));
    reg.register(Box::new(fs::FsEditTool));
    reg.register(Box::new(fs::FsListTool));
    reg.register(Box::new(fs::FsSearchTool));
    reg.register(Box::new(fs::FsDeleteTool));

    // Harness
    reg.register(Box::new(harness::TodoWriteTool));
    reg.register(Box::new(harness::TodoReadTool));
    reg.register(Box::new(harness::ThinkTool));
    reg.register(Box::new(harness::PlanModeTool));

    // Git
    reg.register(Box::new(git::GitStatusTool));
    reg.register(Box::new(git::GitDiffTool));
    reg.register(Box::new(git::GitCommitTool));
    reg.register(Box::new(git::GitLogTool));
    reg.register(Box::new(git::GitBranchTool));

    // GitHub
    reg.register(Box::new(github::GitHubGetRepoTool::new()));
    reg.register(Box::new(github::GitHubListIssuesTool::new()));

    // Shell
    reg.register(Box::new(shell::ShellExecTool));

    // MCP
    reg.register(Box::new(mcp::McpListToolsTool));

    // Web
    reg.register(Box::new(web::WebSearchTool));
    reg.register(Box::new(web::WebFetchTool));

    // System & Math
    reg.register(Box::new(system::SystemInfoTool));
    reg.register(Box::new(system::CalcEvalTool));

    // Memory & Recall
    reg.register(Box::new(memory::MemoryStoreTool));
    reg.register(Box::new(memory::MemoryRecallTool));

    // Artifact Synthesis
    reg.register(Box::new(artifact_gen::ArtifactCreateCodeTool));
    reg.register(Box::new(artifact_gen::ArtifactCreateDocumentTool));
    reg.register(Box::new(artifact_gen::ArtifactCreateImageTool));

    reg
}
