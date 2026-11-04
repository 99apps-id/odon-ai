use async_trait::async_trait;
use odon_agent::{AgentRuntime, SteerMessage, SteerQueue};
use odon_core::events::create_event_bus;
use odon_core::session::{Session, SessionMode};
use odon_core::{AgentEvent, ArtifactKind, ArtifactPayload, Result};
use odon_core::events::AgentState;
use odon_provider::{ChatRequest, ModelProvider, StreamEvent};
use odon_security::{ApprovalDecision, ApprovalEngine, AuditLogger, RiskTier, SecurityPolicy};
use odon_storage::StorageEngine;
use odon_tools::artifact_gen::{
    ArtifactCreateCodeTool, ArtifactCreateDocumentTool, ArtifactCreateImageTool,
};
use odon_tools::fs::{FsListTool, FsReadTool, FsWriteTool};
use odon_tools::git::{GitDiffTool, GitStatusTool};
use odon_tools::github::GitHubGetRepoTool;
use odon_tools::mcp::McpListToolsTool;
use odon_tools::ToolRegistry;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::mpsc::Sender;
use tokio_util::sync::CancellationToken;

/// The registry now asks the approval engine before every mutating tool. Tests
/// have no UI, so approve each request as it is published on the event bus.
fn auto_approve(policy: SecurityPolicy) -> Arc<ApprovalEngine> {
    let (tx, mut rx) = create_event_bus(64);
    let engine = Arc::new(ApprovalEngine::new(policy).with_events(tx));
    let approver = engine.clone();
    tokio::spawn(async move {
        while let Ok(event) = rx.recv().await {
            if let AgentEvent::ApprovalRequested { approval_id, .. } = event.event {
                let _ = approver
                    .submit_decision(&approval_id, ApprovalDecision::Approved)
                    .await;
            }
        }
    });
    engine
}

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
        request: ChatRequest,
        event_sender: Sender<StreamEvent>,
        _cancellation: CancellationToken,
    ) -> Result<()> {
        let has_tools = request
            .messages
            .iter()
            .any(|m| m.role == odon_core::MessageRole::Tool);
        if has_tools {
            let _ = event_sender
                .send(StreamEvent::TextDelta(
                    "All requested artifacts (Code and PRD) for Personal Knowledge Hub have been created successfully."
                        .to_string(),
                ))
                .await;
            let _ = event_sender.send(StreamEvent::Done).await;
            return Ok(());
        }

        let _ = event_sender
            .send(StreamEvent::ThoughtDelta(
                "Analyzing user request for Personal Knowledge Hub and planning components..."
                    .to_string(),
            ))
            .await;

        let _ = event_sender
            .send(StreamEvent::TextDelta(
                "Got it! I'll help you build the 'Personal Knowledge Hub' end-to-end \
                 — including the code (React 19) and a product requirements document (PRD)."
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

        // Image generation requires a real external provider key, so it is
        // covered by its own failure-path expectation rather than this offline
        // conversational turn.

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

    assert_eq!(
        approval.check_risk("fs_read").await,
        RiskTier::Tier0ReadOnly
    );
    assert_eq!(
        approval.check_risk("git_status").await,
        RiskTier::Tier0ReadOnly
    );
    assert_eq!(
        approval.check_risk("github_get_repo").await,
        RiskTier::Tier0ReadOnly
    );
    assert_eq!(
        approval.check_risk("fs_write").await,
        RiskTier::Tier1WorkspaceMutating
    );
    assert_eq!(
        approval.check_risk("artifact_create_code").await,
        RiskTier::Tier1WorkspaceMutating
    );
    assert_eq!(
        approval.check_risk("shell_exec").await,
        RiskTier::Tier2SystemMutating
    );

    // Auto-approve read only Tier 0
    let decision = approval
        .evaluate_and_request(
            "test_session",
            "fs_read",
            &serde_json::json!({ "path": "test.txt" }),
            "Read file",
        )
        .await
        .unwrap();
    matches!(decision, ApprovalDecision::Approved);
}

#[tokio::test]
async fn test_conversational_agent_turn_with_artifacts() {
    let storage = Arc::new(StorageEngine::new_in_memory().unwrap());
    let approval = auto_approve(SecurityPolicy::default());
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

    let runtime = AgentRuntime::new(storage.clone(), tools, approval, audit, event_tx, workspace);

    let provider = Arc::new(MockConversationalProvider);
    runtime.register_provider("default", provider, true).await;

    let session = Session::new("Knowledge Hub Turn", SessionMode::General, "odon-auto");
    storage.save_session(&session).unwrap();

    let received_artifacts = std::sync::Arc::new(std::sync::atomic::AtomicUsize::new(0));
    let received_clone = received_artifacts.clone();
    tokio::spawn(async move {
        while let Ok(evt) = event_rx.recv().await {
            if let AgentEvent::ArtifactCreated { .. } = evt.event {
                received_clone.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
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
    // No action chip is advertised until the desktop has a wired handler for it.
    assert!(result.content.action_chips.is_empty());

    // Verify generated artifacts in SQLite
    let artifacts = storage.list_artifacts(&session.id).unwrap();
    assert_eq!(artifacts.len(), 2);
    assert_eq!(artifacts[0].kind, ArtifactKind::Code);
    assert_eq!(artifacts[1].kind, ArtifactKind::Document);
}

#[tokio::test]
async fn test_extended_tools_execution() {
    let storage = Arc::new(StorageEngine::new_in_memory().unwrap());
    let approval = auto_approve(SecurityPolicy::default());
    let audit = Arc::new(AuditLogger::new());

    let registry = odon_tools::build_full_tool_registry();
    let temp_dir = tempfile::tempdir().unwrap();
    let workspace = temp_dir.path().to_string_lossy().to_string();

    let ctx = odon_tools::ToolContext {
        session_id: "test_tools".to_string(),
        workspace_root: workspace.clone(),
        approval_engine: approval.clone(),
        audit_logger: audit.clone(),
        storage: Some(storage.clone()),
    };

    // 1. fs_write
    let write_res = registry
        .execute_tool(
            "fs_write",
            &ctx,
            serde_json::json!({
                "path": "hello.txt",
                "content": "Hello world from ODON agent runtime!"
            }),
        )
        .await
        .unwrap();
    assert_eq!(
        write_res.get("path").and_then(|p| p.as_str()),
        Some("hello.txt")
    );

    // 2. fs_edit
    let edit_res = registry
        .execute_tool(
            "fs_edit",
            &ctx,
            serde_json::json!({
                "path": "hello.txt",
                "target": "ODON agent runtime",
                "replacement": "ODON full agentic harness"
            }),
        )
        .await
        .unwrap();
    assert_eq!(
        edit_res.get("status").and_then(|s| s.as_str()),
        Some("success")
    );

    // 3. fs_search
    let search_res = registry
        .execute_tool(
            "fs_search",
            &ctx,
            serde_json::json!({
                "query": "harness"
            }),
        )
        .await
        .unwrap();
    assert!(
        search_res
            .get("results_count")
            .and_then(|c| c.as_u64())
            .unwrap_or(0)
            >= 1
    );

    // 4. system_info
    let sys_res = registry
        .execute_tool("system_info", &ctx, serde_json::json!({}))
        .await
        .unwrap();
    assert!(sys_res.get("os").is_some());
    assert!(sys_res.get("architecture").is_some());

    // 5. calc_eval
    let calc_res = registry
        .execute_tool(
            "calc_eval",
            &ctx,
            serde_json::json!({
                "expression": "(100 * 2) + 50"
            }),
        )
        .await
        .unwrap();
    assert_eq!(calc_res.get("result").and_then(|r| r.as_f64()), Some(250.0));

    // 6. memory_store & recall
    let store_res = registry
        .execute_tool(
            "memory_store",
            &ctx,
            serde_json::json!({
                "key": "user_ui_preference",
                "value": "Clean ChatGPT style, no AI slop",
                "category": "preferences"
            }),
        )
        .await
        .unwrap();
    assert_eq!(
        store_res.get("status").and_then(|s| s.as_str()),
        Some("stored")
    );

    let recall_res = registry
        .execute_tool(
            "memory_recall",
            &ctx,
            serde_json::json!({
                "key": "user_ui_preference"
            }),
        )
        .await
        .unwrap();
    assert!(recall_res.get("memory").is_some());
}

#[tokio::test]
async fn test_simulated_provider_turn() {
    let storage = std::sync::Arc::new(StorageEngine::new_in_memory().unwrap());
    let approval = auto_approve(SecurityPolicy::default());
    let audit = std::sync::Arc::new(AuditLogger::new());
    let (event_tx, _event_rx) = create_event_bus(64);
    let registry = odon_tools::build_full_tool_registry();
    let temp_dir = tempfile::tempdir().unwrap();
    let workspace = temp_dir.path().to_string_lossy().to_string();

    let runtime = AgentRuntime::new(
        storage.clone(),
        std::sync::Arc::new(registry),
        approval,
        audit,
        event_tx,
        workspace,
    );

    let provider = std::sync::Arc::new(odon_provider::SimulatedProvider::default());
    runtime.register_provider("default", provider, true).await;

    let session = Session::new("Simulated Turn", SessionMode::General, "odon-auto");
    storage.save_session(&session).unwrap();

    let cancel = CancellationToken::new();
    let result = runtime
        .execute_turn(
            &session.id,
            "tampilkan info ringkas sistem",
            Some("odon-simulated"),
            cancel,
        )
        .await
        .unwrap();

    println!("RESULT_TEXT: {}", result.content.text);
    let msgs = storage.list_messages(&session.id).unwrap();
    println!("MSGS COUNT: {}", msgs.len());
    for m in &msgs {
        println!("MSG: role={:?} id={}", m.role, m.id);
    }
}

#[tokio::test]
async fn test_tool_history_orders_assistant_calls_before_tool_results() {
    let storage = Arc::new(StorageEngine::new_in_memory().unwrap());
    let approval = auto_approve(SecurityPolicy::default());
    let audit = Arc::new(AuditLogger::new());
    let (event_tx, _rx) = create_event_bus(64);
    let temp_dir = tempfile::tempdir().unwrap();
    let runtime = AgentRuntime::new(
        storage.clone(),
        Arc::new(odon_tools::build_full_tool_registry()),
        approval,
        audit,
        event_tx,
        temp_dir.path().to_string_lossy().to_string(),
    );
    runtime
        .register_provider("default", Arc::new(MockConversationalProvider), true)
        .await;

    let session = Session::new("Order", SessionMode::General, "odon-auto");
    storage.save_session(&session).unwrap();
    runtime
        .execute_turn(
            &session.id,
            "please build it",
            None,
            CancellationToken::new(),
        )
        .await
        .unwrap();

    let msgs = storage.list_messages(&session.id).unwrap();
    let first_calls = msgs
        .iter()
        .position(|m| {
            m.role == odon_core::MessageRole::Assistant && !m.content.tool_calls.is_empty()
        })
        .expect("an assistant message declaring tool_calls must be stored");
    let first_tool = msgs
        .iter()
        .position(|m| m.role == odon_core::MessageRole::Tool)
        .expect("tool results must be stored");
    assert!(
        first_calls < first_tool,
        "assistant tool_calls (#{first_calls}) must precede tool results (#{first_tool})"
    );

    // Every tool result must answer a call declared by the preceding assistant message.
    let declared: Vec<&str> = msgs[first_calls]
        .content
        .tool_calls
        .iter()
        .map(|c| c.call_id.as_str())
        .collect();
    for tool_msg in msgs
        .iter()
        .filter(|m| m.role == odon_core::MessageRole::Tool)
    {
        let id = tool_msg.content.tool_calls[0].call_id.as_str();
        assert!(
            declared.contains(&id),
            "tool result {id} has no declaring call"
        );
        assert!(!tool_msg.content.tool_calls[0].tool_name.is_empty());
    }
}

/// Provider that streams slowly so a steer must wait for the active task.
struct SteerableProvider {
    calls: Arc<AtomicUsize>,
}

#[async_trait]
impl ModelProvider for SteerableProvider {
    fn provider_name(&self) -> &str {
        "steerable"
    }

    async fn stream_chat(
        &self,
        request: ChatRequest,
        event_sender: Sender<StreamEvent>,
        cancellation: CancellationToken,
    ) -> Result<()> {
        self.calls.fetch_add(1, Ordering::SeqCst);
        let last_user = request
            .messages
            .iter()
            .rev()
            .find(|message| message.role == odon_core::MessageRole::User)
            .map(|message| message.content.text.clone())
            .unwrap_or_default();
        let ack = last_user.contains("STEER");

        for _ in 0..20 {
            if cancellation.is_cancelled() {
                let _ = event_sender.send(StreamEvent::Done).await;
                return Ok(());
            }
            let delta = if ack { "steer-ack " } else { "working " };
            let _ = event_sender
                .send(StreamEvent::TextDelta(delta.to_string()))
                .await;
            tokio::time::sleep(Duration::from_millis(25)).await;
        }
        let _ = event_sender.send(StreamEvent::Done).await;
        Ok(())
    }
}

#[tokio::test]
async fn test_steer_waits_for_running_turn_then_is_applied() {
    let storage = Arc::new(StorageEngine::new_in_memory().unwrap());
    let approval = auto_approve(SecurityPolicy::default());
    let audit = Arc::new(AuditLogger::new());
    let (event_tx, mut event_rx) = create_event_bus(128);
    let calls = Arc::new(AtomicUsize::new(0));
    let provider = Arc::new(SteerableProvider {
        calls: calls.clone(),
    });

    let runtime = Arc::new(AgentRuntime::new(
        storage.clone(),
        Arc::new(ToolRegistry::new()),
        approval,
        audit,
        event_tx,
        std::env::temp_dir().to_string_lossy().to_string(),
    ));
    runtime.register_provider("default", provider, true).await;

    let session = Session::new("Steer Test", SessionMode::General, "steerable");
    storage.save_session(&session).unwrap();

    let queue = SteerQueue::new();
    let cancel = CancellationToken::new();
    let rt = runtime.clone();
    let sid = session.id.clone();
    let steer_queue = queue.clone();
    let task = tokio::spawn(async move {
        rt.execute_turn_controlled(
            &sid,
            "Please do the long task",
            Vec::new(),
            None,
            None,
            cancel,
            Some(steer_queue),
            None,
        )
        .await
    });

    tokio::time::sleep(Duration::from_millis(90)).await;
    queue.push(SteerMessage {
        id: "steer-1".to_string(),
        text: "STEER: stop and use approach B".to_string(),
        attachments: Vec::new(),
        priority: true,
    });

    // Sending steer must not cancel or preempt the active provider stream.
    tokio::time::sleep(Duration::from_millis(100)).await;
    assert_eq!(calls.load(Ordering::SeqCst), 1);

    let result = tokio::time::timeout(Duration::from_secs(5), task)
        .await
        .expect("turn timed out")
        .expect("task join failed")
        .expect("turn failed");
    assert!(
        result.content.text.contains("steer-ack"),
        "final text did not include the steered answer: {}",
        result.content.text
    );
    assert!(
        calls.load(Ordering::SeqCst) >= 2,
        "the provider should be called again after the steer"
    );

    let messages = storage.list_messages(&session.id).unwrap();
    assert!(
        messages
            .iter()
            .any(|message| message.role == odon_core::MessageRole::User
                && message.content.text.contains("STEER")),
        "the steer must be persisted as a user message"
    );

    let mut saw_applied = false;
    let mut applied_session = None;
    while let Ok(event) = event_rx.try_recv() {
        if let AgentEvent::SteerApplied { steer_id, .. } = event.event {
            if steer_id == "steer-1" {
                saw_applied = true;
                applied_session = event.session_id;
            }
        }
    }
    assert!(saw_applied, "SteerApplied event must be emitted");
    // The turn's session id is stamped on the envelope, so an SSE subscriber
    // can route this event without parsing the payload.
    assert_eq!(
        applied_session.as_deref(),
        Some(session.id.as_str()),
        "the event must carry the session id bound to the running turn"
    );
}

/// Provider that delegates with run_subagent whenever the user asks, runs
/// the child task itself, and reports the tool output afterwards. It pins
/// the whole delegation seam: the nested turn, the child session, and the
/// tool result coming back into the parent step.
struct DelegatingProvider;

#[async_trait]
impl ModelProvider for DelegatingProvider {
    fn provider_name(&self) -> &str {
        "delegating"
    }

    async fn stream_chat(
        &self,
        request: ChatRequest,
        event_sender: Sender<StreamEvent>,
        _cancellation: CancellationToken,
    ) -> Result<()> {
        let last_user = request
            .messages
            .iter()
            .rev()
            .find(|m| m.role == odon_core::MessageRole::User)
            .map(|m| m.content.text.clone())
            .unwrap_or_default();
        let tool_output = request
            .messages
            .iter()
            .rev()
            .find(|m| m.role == odon_core::MessageRole::Tool)
            .map(|m| m.content.text.clone());
        if let Some(output) = tool_output {
            let _ = event_sender
                .send(StreamEvent::TextDelta(format!("Parent received: {output}")))
                .await;
        } else if last_user.contains("please delegate") {
            let _ = event_sender
                .send(StreamEvent::ToolCallDelta {
                    index: 0,
                    id: Some("call_delegate_1".to_string()),
                    name: Some("run_subagent".to_string()),
                    arguments_delta: Some("{\"task\":\"child task\"}".to_string()),
                })
                .await;
        } else if last_user == "child task" {
            let _ = event_sender
                .send(StreamEvent::TextDelta("child report".to_string()))
                .await;
        } else {
            let _ = event_sender
                .send(StreamEvent::TextDelta("nothing to do".to_string()))
                .await;
        }
        let _ = event_sender.send(StreamEvent::Done).await;
        Ok(())
    }
}

#[tokio::test]
async fn test_run_subagent_delegates_and_returns_the_child_report() {
    let storage = Arc::new(StorageEngine::new_in_memory().unwrap());
    let approval = auto_approve(SecurityPolicy::default());
    let audit = Arc::new(AuditLogger::new());
    let (event_tx, _event_rx) = create_event_bus(64);
    let registry = odon_tools::build_full_tool_registry();
    let temp_dir = tempfile::tempdir().unwrap();
    let runtime = AgentRuntime::new(
        storage.clone(),
        Arc::new(registry),
        approval,
        audit,
        event_tx,
        temp_dir.path().to_string_lossy().to_string(),
    );
    runtime
        .register_provider("default", Arc::new(DelegatingProvider), true)
        .await;

    let session = Session::new("Delegation", SessionMode::General, "delegating-model");
    storage.save_session(&session).unwrap();

    let result = runtime
        .execute_turn(
            &session.id,
            "please delegate this work",
            Some("delegating-model"),
            CancellationToken::new(),
        )
        .await
        .unwrap();
    assert!(
        result.content.text.contains("child report"),
        "parent must receive the child report, got: {}",
        result.content.text
    );
    // The child ran on its own sub- session with the task persisted.
    let sessions = storage.list_sessions().unwrap();
    let child = sessions
        .iter()
        .find(|s| s.id.starts_with("sub-"))
        .expect("a child session must exist");
    let child_messages = storage.list_messages(&child.id).unwrap();
    assert!(
        child_messages
            .iter()
            .any(|m| m.content.text.contains("child report")),
        "the child session must hold the child turn"
    );
}

/// Provider that delegates with run_subagent and the child writes a file.
/// Pins the regression where `git worktree remove --force` discarded the
/// child's uncommitted work: the changes must survive as a patch artifact
/// on the parent session and be announced in the tool result.
struct DelegatingWriterProvider;

#[async_trait]
impl ModelProvider for DelegatingWriterProvider {
    fn provider_name(&self) -> &str {
        "delegating-writer"
    }

    async fn stream_chat(
        &self,
        request: ChatRequest,
        event_sender: Sender<StreamEvent>,
        _cancellation: CancellationToken,
    ) -> Result<()> {
        let last_user = request
            .messages
            .iter()
            .rev()
            .find(|m| m.role == odon_core::MessageRole::User)
            .map(|m| m.content.text.clone())
            .unwrap_or_default();
        let tool_output = request
            .messages
            .iter()
            .rev()
            .find(|m| m.role == odon_core::MessageRole::Tool)
            .map(|m| m.content.text.clone());
        if let Some(output) = tool_output {
            if last_user == "write a child file" {
                let _ = event_sender
                    .send(StreamEvent::TextDelta("child wrote the file".to_string()))
                    .await;
            } else {
                let _ = event_sender
                    .send(StreamEvent::TextDelta(format!("Parent received: {output}")))
                    .await;
            }
        } else if last_user.contains("please delegate") {
            let _ = event_sender
                .send(StreamEvent::ToolCallDelta {
                    index: 0,
                    id: Some("call_delegate_write".to_string()),
                    name: Some("run_subagent".to_string()),
                    arguments_delta: Some("{\"task\":\"write a child file\"}".to_string()),
                })
                .await;
        } else if last_user == "write a child file" {
            let _ = event_sender
                .send(StreamEvent::ToolCallDelta {
                    index: 0,
                    id: Some("call_child_write".to_string()),
                    name: Some("fs_write".to_string()),
                    arguments_delta: Some(
                        "{\"path\":\"child-note.txt\",\"content\":\"from child\\n\"}".to_string(),
                    ),
                })
                .await;
        } else {
            let _ = event_sender
                .send(StreamEvent::TextDelta("nothing to do".to_string()))
                .await;
        }
        let _ = event_sender.send(StreamEvent::Done).await;
        Ok(())
    }
}

fn git_in(dir: &std::path::Path, args: &[&str]) {
    let status = std::process::Command::new("git")
        .args(args)
        .current_dir(dir)
        .output()
        .unwrap_or_else(|e| panic!("git {args:?} failed to spawn: {e}"));
    assert!(
        status.status.success(),
        "git {args:?} failed: {}",
        String::from_utf8_lossy(&status.stderr)
    );
}

#[tokio::test]
async fn test_subagent_edits_survive_worktree_removal_as_a_patch_artifact() {
    let storage = Arc::new(StorageEngine::new_in_memory().unwrap());
    let approval = auto_approve(SecurityPolicy::default());
    let audit = Arc::new(AuditLogger::new());
    let (event_tx, _event_rx) = create_event_bus(64);
    let registry = odon_tools::build_full_tool_registry();

    // A real git repo: delegation only materialises a worktree inside git.
    let repo = tempfile::tempdir().unwrap();
    let root = repo.path();
    std::fs::write(root.join("seed.txt"), "seed\n").unwrap();
    git_in(root, &["init"]);
    git_in(root, &["config", "user.email", "test@example.com"]);
    git_in(root, &["config", "user.name", "Test"]);
    git_in(root, &["add", "-A"]);
    git_in(root, &["commit", "-m", "seed"]);

    let runtime = AgentRuntime::new(
        storage.clone(),
        Arc::new(registry),
        approval,
        audit,
        event_tx,
        root.to_string_lossy().to_string(),
    );
    runtime
        .register_provider("default", Arc::new(DelegatingWriterProvider), true)
        .await;

    let session = Session::new("Delegation", SessionMode::General, "delegating-model");
    storage.save_session(&session).unwrap();

    let result = runtime
        .execute_turn(
            &session.id,
            "please delegate this work",
            Some("delegating-model"),
            CancellationToken::new(),
        )
        .await
        .unwrap();

    let artifacts = storage.list_artifacts(&session.id).unwrap();
    let patch = artifacts
        .iter()
        .find(|a| a.kind == ArtifactKind::Patch)
        .unwrap_or_else(|| {
            panic!("the child's edits must be captured as a patch artifact; got {artifacts:?}")
        });
    match &patch.payload {
        ArtifactPayload::Patch {
            diff,
            files_changed,
            additions,
            ..
        } => {
            assert!(
                diff.contains("child-note.txt") && diff.contains("+from child"),
                "the patch must carry the child's file content, got: {diff}"
            );
            assert_eq!(*files_changed, 1, "one file changed, got: {diff}");
            assert_eq!(*additions, 1, "one added line, got: {diff}");
        }
        other => panic!("expected a Patch payload, got: {other:?}"),
    }
    // The parent turn is told where the work went, so it can act on it.
    assert!(
        result.content.text.contains("Subagent changes:"),
        "the parent must be told the child's changes were captured, got: {}",
        result.content.text
    );

    // The worktree really was removed; the artifact is the only trace.
    let sessions = storage.list_sessions().unwrap();
    let child = sessions
        .iter()
        .find(|s| s.id.starts_with("sub-"))
        .expect("a child session must exist");
    if let Some(path) = child.workspace_root.as_ref() {
        assert!(
            !std::path::Path::new(path).exists(),
            "the delegated worktree must be cleaned up, {path} still exists"
        );
    }
}

/// Provider that calls two independent registry tools in one step. The
/// runtime must execute both (bounded-parallel path) and persist two
/// tool results before the final answer.
struct ParallelToolsProvider;

#[async_trait]
impl ModelProvider for ParallelToolsProvider {
    fn provider_name(&self) -> &str {
        "parallel-tools"
    }

    async fn stream_chat(
        &self,
        request: ChatRequest,
        event_sender: Sender<StreamEvent>,
        _cancellation: CancellationToken,
    ) -> Result<()> {
        let tool_results = request
            .messages
            .iter()
            .filter(|m| m.role == odon_core::MessageRole::Tool)
            .count();
        if tool_results >= 2 {
            let _ = event_sender
                .send(StreamEvent::TextDelta("both tools done".to_string()))
                .await;
        } else if tool_results == 0 {
            for (index, name, arguments) in
                [(0, "fs_list", "{\"path\":\".\"}"), (1, "system_info", "{}")]
            {
                let _ = event_sender
                    .send(StreamEvent::ToolCallDelta {
                        index,
                        id: Some(format!("call_parallel_{index}")),
                        name: Some(name.to_string()),
                        arguments_delta: Some(arguments.to_string()),
                    })
                    .await;
            }
        }
        let _ = event_sender.send(StreamEvent::Done).await;
        Ok(())
    }
}

#[tokio::test]
async fn test_parallel_tool_calls_in_one_step_all_execute() {
    let storage = Arc::new(StorageEngine::new_in_memory().unwrap());
    let approval = auto_approve(SecurityPolicy::default());
    let audit = Arc::new(AuditLogger::new());
    let (event_tx, _event_rx) = create_event_bus(64);
    let registry = odon_tools::build_full_tool_registry();
    let temp_dir = tempfile::tempdir().unwrap();
    let runtime = AgentRuntime::new(
        storage.clone(),
        Arc::new(registry),
        approval,
        audit,
        event_tx,
        temp_dir.path().to_string_lossy().to_string(),
    );
    runtime
        .register_provider("default", Arc::new(ParallelToolsProvider), true)
        .await;

    let session = Session::new("Parallel", SessionMode::General, "parallel-model");
    storage.save_session(&session).unwrap();

    let result = runtime
        .execute_turn(
            &session.id,
            "list and inspect",
            Some("parallel-model"),
            CancellationToken::new(),
        )
        .await
        .unwrap();
    assert!(
        result.content.text.contains("both tools done"),
        "got: {}",
        result.content.text
    );
    let tool_messages = storage
        .list_messages(&session.id)
        .unwrap()
        .into_iter()
        .filter(|m| m.role == odon_core::MessageRole::Tool)
        .count();
    assert_eq!(
        tool_messages, 2,
        "both calls in the step must persist results"
    );
}

/// A provider that never stops asking for tools. It records the size of the
/// tool list it was offered, which is how the closing call is detected: the
/// step after the cap must be offered zero tools.
struct NeverConcludesProvider {
    tool_counts: Arc<std::sync::Mutex<Vec<usize>>>,
}

#[async_trait]
impl ModelProvider for NeverConcludesProvider {
    fn provider_name(&self) -> &str {
        "never-concludes"
    }

    async fn stream_chat(
        &self,
        request: ChatRequest,
        event_sender: Sender<StreamEvent>,
        _cancellation: CancellationToken,
    ) -> Result<()> {
        self.tool_counts.lock().unwrap().push(request.tools.len());
        if request.tools.is_empty() {
            // The closing call: answer in prose, as the real contract allows.
            let _ = event_sender
                .send(StreamEvent::TextDelta("closing answer".to_string()))
                .await;
        } else {
            let _ = event_sender
                .send(StreamEvent::ToolCallDelta {
                    index: 0,
                    id: Some("call_never".to_string()),
                    name: Some("system_info".to_string()),
                    arguments_delta: Some("{}".to_string()),
                })
                .await;
        }
        let _ = event_sender.send(StreamEvent::Done).await;
        Ok(())
    }
}

#[tokio::test]
async fn test_step_cap_grants_one_closing_call_without_tools() {
    let storage = Arc::new(StorageEngine::new_in_memory().unwrap());
    let approval = auto_approve(SecurityPolicy::default());
    let audit = Arc::new(AuditLogger::new());
    let (event_tx, _event_rx) = create_event_bus(64);
    let registry = odon_tools::build_full_tool_registry();
    let temp_dir = tempfile::tempdir().unwrap();
    let runtime = AgentRuntime::new(
        storage.clone(),
        Arc::new(registry),
        approval,
        audit,
        event_tx,
        temp_dir.path().to_string_lossy().to_string(),
    );
    let tool_counts = Arc::new(std::sync::Mutex::new(Vec::new()));
    runtime
        .register_provider(
            "default",
            Arc::new(NeverConcludesProvider {
                tool_counts: tool_counts.clone(),
            }),
            true,
        )
        .await;

    let session = Session::new("Cap", SessionMode::General, "cap-model");
    storage.save_session(&session).unwrap();

    let result = runtime
        .execute_turn_with_cap(
            &session.id,
            "keep going forever",
            Vec::new(),
            Some("cap-model"),
            None,
            CancellationToken::new(),
            None,
            None,
            Some(2),
        )
        .await
        .unwrap();

    let counts = tool_counts.lock().unwrap().clone();
    assert_eq!(
        counts.len(),
        3,
        "expected two working steps then one closing call, got {:?}",
        counts
    );
    assert!(
        counts[0] > 0 && counts[1] > 0,
        "working steps must be offered tools, got {:?}",
        counts
    );
    assert_eq!(
        counts[2], 0,
        "the closing call must be offered no tools, got {:?}",
        counts
    );
    assert!(
        result.content.text.contains("closing answer"),
        "the closing answer must reach the operator, got: {}",
        result.content.text
    );
    assert!(
        result.content.text.contains("Step cap of 2 reached"),
        "the transcript must stay honest about the cap, got: {}",
        result.content.text
    );
}

/// A capped turn must stay resumable: the session is marked Paused (not
/// Completed), remaining plan items are not force-closed, and the operator is
/// told that a follow-up message continues the work.
#[tokio::test]
async fn test_step_cap_pauses_the_session_and_asks_to_continue() {
    let storage = Arc::new(StorageEngine::new_in_memory().unwrap());
    let approval = auto_approve(SecurityPolicy::default());
    let audit = Arc::new(AuditLogger::new());
    let (event_tx, mut event_rx) = create_event_bus(256);
    let registry = odon_tools::build_full_tool_registry();
    let temp_dir = tempfile::tempdir().unwrap();
    let runtime = AgentRuntime::new(
        storage.clone(),
        Arc::new(registry),
        approval,
        audit,
        event_tx,
        temp_dir.path().to_string_lossy().to_string(),
    );
    let tool_counts = Arc::new(std::sync::Mutex::new(Vec::new()));
    runtime
        .register_provider(
            "default",
            Arc::new(NeverConcludesProvider {
                tool_counts: tool_counts.clone(),
            }),
            true,
        )
        .await;

    let session = Session::new("Pause", SessionMode::General, "cap-model");
    storage.save_session(&session).unwrap();

    let result = runtime
        .execute_turn_with_cap(
            &session.id,
            "keep going forever",
            Vec::new(),
            Some("cap-model"),
            None,
            CancellationToken::new(),
            None,
            None,
            Some(1),
        )
        .await
        .unwrap();

    assert!(
        result
            .content
            .text
            .contains("Send \"continue\" to resume this session with tools"),
        "the operator must be told how to resume, got: {}",
        result.content.text
    );

    let mut final_state = None;
    while let Ok(event) = event_rx.try_recv() {
        if let AgentEvent::StateChanged { state, .. } = event.event {
            final_state = Some(state);
        }
    }
    assert_eq!(
        final_state,
        Some(AgentState::Paused),
        "a capped turn must pause, not report completion; saw {:?}",
        final_state
    );
}
