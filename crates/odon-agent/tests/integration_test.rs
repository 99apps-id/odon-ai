use async_trait::async_trait;
use odon_agent::AgentRuntime;
use odon_core::events::create_event_bus;
use odon_core::session::{Session, SessionMode};
use odon_core::{AgentEvent, ArtifactKind, Result};
use odon_provider::{ChatRequest, ModelProvider, StreamEvent};
use odon_security::{ApprovalDecision, ApprovalEngine, AuditLogger, RiskTier, SecurityPolicy};
use odon_storage::StorageEngine;
use odon_tools::artifact_gen::{ArtifactCreateCodeTool, ArtifactCreateDocumentTool, ArtifactCreateImageTool};
use odon_tools::fs::{FsListTool, FsReadTool, FsWriteTool};
use odon_tools::git::{GitDiffTool, GitStatusTool};
use odon_tools::github::GitHubGetRepoTool;
use odon_tools::mcp::McpListToolsTool;
use odon_tools::ToolRegistry;
use std::sync::Arc;
use tokio::sync::mpsc::Sender;
use tokio_util::sync::CancellationToken;

/// Mock provider that simulates a natural ChatGPT/Codex conversational response
/// and creates artifacts and contextual action chips.
struct MockConversationalProvider;

#[async_trait]
impl ModelProvider for MockConversationalProvider {
    fn provider_name(&self) -> &str {
        "mock-conversational"
    }

    async fn stream_chat(
        &self,
        _request: ChatRequest,
        event_sender: Sender<StreamEvent>,
        _cancellation: CancellationToken,
    ) -> Result<()> {
        let _ = event_sender
            .send(StreamEvent::ThoughtDelta(
                "Analyzing user request for Personal Knowledge Hub and planning components...".to_string(),
            ))
            .await;

        let _ = event_sender
            .send(StreamEvent::TextDelta(
                "Got it! I'll help you build the 'Personal Knowledge Hub' end-to-end \
                 — including the code (React 19), a product requirements document (PRD), \
                 and high-quality project images."
                    .to_string(),
            ))
            .await;

        // Simulate tool call delta to create code artifact
        let _ = event_sender
            .send(StreamEvent::ToolCallDelta {
                index: 0,
                id: Some("call_art_code".to_string()),
                name: Some("artifact_create_code".to_string()),
                arguments_delta: Some(serde_json::to_string(&serde_json::json!({
                    "title": "personal-knowledge-hub",
                    "summary": "React 19 • TypeScript • Vite",
                    "language": "TypeScript",
                    "framework": "React 19",
                    "files": [
                        { "path": "src/App.tsx", "content": "export default function App() { return <div>Knowledge Hub</div>; }" }
                    ]
                })).unwrap()),
            })
            .await;

        // Simulate tool call delta to create document artifact
        let _ = event_sender
            .send(StreamEvent::ToolCallDelta {
                index: 1,
                id: Some("call_art_doc".to_string()),
                name: Some("artifact_create_document".to_string()),
                arguments_delta: Some(serde_json::to_string(&serde_json::json!({
                    "title": "Personal Knowledge Hub - PRD.md",
                    "doc_type": "Product Requirements Document",
                    "markdown": "# Personal Knowledge Hub PRD\n\n## Vision\nUnified knowledge orchestration."
                })).unwrap()),
            })
            .await;

        // Simulate tool call delta to create image artifact
        let _ = event_sender
            .send(StreamEvent::ToolCallDelta {
                index: 2,
                id: Some("call_art_img".to_string()),
                name: Some("artifact_create_image".to_string()),
                arguments_delta: Some(serde_json::to_string(&serde_json::json!({
                    "title": "hero-illustration.png",
                    "prompt": "Vibrant artistic workspace with knowledge nodes glowing in twilight",
                    "width": 1920,
                    "height": 1080,
                    "model": "FLUX.1-pro"
                })).unwrap()),
            })
            .await;

        let _ = event_sender.send(StreamEvent::Done).await;
        Ok(())
    }
}

#[tokio::test]
async fn test_sqlite_persistence() {
    let storage = StorageEngine::new_in_memory().unwrap();

    let session = Session::new("Knowledge Hub Session", SessionMode::Code, "odon-auto");
    storage.save_session(&session).unwrap();

    let loaded = storage.get_session(&session.id).unwrap();
    assert_eq!(loaded.title, "Knowledge Hub Session");
    assert_eq!(loaded.mode, SessionMode::Code);

    let sessions = storage.list_sessions().unwrap();
    assert_eq!(sessions.len(), 1);
}

#[tokio::test]
async fn test_security_policy_and_risk_tiers() {
    let policy = SecurityPolicy::default();
    let approval = ApprovalEngine::new(policy);

    assert_eq!(approval.check_risk("fs_read").await, RiskTier::Tier0ReadOnly);
    assert_eq!(approval.check_risk("git_status").await, RiskTier::Tier0ReadOnly);
    assert_eq!(approval.check_risk("github_get_repo").await, RiskTier::Tier0ReadOnly);
    assert_eq!(approval.check_risk("fs_write").await, RiskTier::Tier1WorkspaceMutating);
    assert_eq!(approval.check_risk("artifact_create_code").await, RiskTier::Tier1WorkspaceMutating);
    assert_eq!(approval.check_risk("shell_exec").await, RiskTier::Tier2SystemMutating);

    // Auto-approve read only Tier 0
    let decision = approval
        .evaluate_and_request("test_session", "fs_read", &serde_json::json!({ "path": "test.txt" }), "Read file")
        .await
        .unwrap();
    matches!(decision, ApprovalDecision::Approved);
}

#[tokio::test]
async fn test_conversational_agent_turn_with_artifacts() {
    let storage = Arc::new(StorageEngine::new_in_memory().unwrap());
    let policy = SecurityPolicy::default();
    let approval = Arc::new(ApprovalEngine::new(policy));
    let audit = Arc::new(AuditLogger::new());

    let (event_tx, mut event_rx) = create_event_bus(128);

    let mut registry = ToolRegistry::new();
    registry.register(Box::new(FsReadTool));
    registry.register(Box::new(FsWriteTool));
    registry.register(Box::new(FsListTool));
    registry.register(Box::new(GitStatusTool));
    registry.register(Box::new(GitDiffTool));
    registry.register(Box::new(GitHubGetRepoTool::new()));
    registry.register(Box::new(McpListToolsTool));
    registry.register(Box::new(ArtifactCreateCodeTool));
    registry.register(Box::new(ArtifactCreateDocumentTool));
    registry.register(Box::new(ArtifactCreateImageTool));
    let tools = Arc::new(registry);

    let temp_dir = tempfile::tempdir().unwrap();
    let workspace = temp_dir.path().to_string_lossy().to_string();

    let runtime = AgentRuntime::new(
        storage.clone(),
        tools,
        approval,
        audit,
        event_tx,
        workspace,
    );

    let provider = Arc::new(MockConversationalProvider);
    runtime.register_provider("default", provider, true).await;

    let session = Session::new("Knowledge Hub Turn", SessionMode::General, "odon-auto");
    storage.save_session(&session).unwrap();

    let mut received_artifacts = 0;
    tokio::spawn(async move {
        while let Ok(evt) = event_rx.recv().await {
            if let AgentEvent::ArtifactCreated { .. } = evt {
                received_artifacts += 1;
            }
        }
    });

    let cancel = CancellationToken::new();
    let result = runtime
        .execute_turn(
            &session.id,
            "I want to build a small personal knowledge hub project.",
            None,
            cancel,
        )
        .await
        .unwrap();

    // Verify conversational response
    assert!(result.content.text.contains("Personal Knowledge Hub"));
    assert!(result.content.thought.is_some());
    assert_eq!(result.content.action_chips.len(), 4);

    // Verify generated artifacts in SQLite
    let artifacts = storage.list_artifacts(&session.id).unwrap();
    assert_eq!(artifacts.len(), 3);
    assert_eq!(artifacts[0].kind, ArtifactKind::Code);
    assert_eq!(artifacts[1].kind, ArtifactKind::Document);
    assert_eq!(artifacts[2].kind, ArtifactKind::Image);
}
