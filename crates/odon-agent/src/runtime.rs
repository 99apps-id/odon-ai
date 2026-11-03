use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;
use tokio::sync::{broadcast, mpsc, RwLock};
use tokio::task::JoinSet;
use tokio_util::sync::CancellationToken;
use tracing::{error, info};

use odon_core::{
    AgentEvent, AgentState, Artifact, Message, MessageAttachment, MessageRole, OdonError, Result,
    Session, SessionMode, ToolCallInfo,
};
use odon_provider::{ChatRequest, ModelProvider, StreamEvent, ToolDefinition};
use odon_security::{ApprovalEngine, AuditLogger};
use odon_storage::StorageEngine;
use odon_tools::{ToolContext, ToolRegistry};

use crate::steer::SteerQueue;

#[derive(Default, Debug, Clone)]
struct AccumulatedToolCall {
    id: String,
    name: String,
    arguments: String,
}

/// Default per-turn step cap. Deliberately Codex-scale rather than a
/// small fixed number: a coding turn legitimately needs dozens of tool
/// steps, and the cap exists to stop a runaway loop, not normal work.
/// Override with `ODON_MAX_TURN_ITERATIONS` or `set_max_turn_iterations`.
pub const DEFAULT_MAX_TURN_ITERATIONS: usize = 100;

/// Default context budget in estimated tokens for one provider request.
/// History beyond it is compacted (see `compact_history`). Override with
/// `ODON_CONTEXT_BUDGET_TOKENS` or `set_context_budget_tokens`.
pub const DEFAULT_CONTEXT_BUDGET_TOKENS: usize = 120_000;

/// Most tool schemas one request may carry inline. Past this the runtime
/// sends a core set plus `find_tools` and loads the rest on demand; the
/// number is the documented function-calling limit of the strictest
/// provider family this runtime talks to.
pub const MAX_INLINE_TOOL_SCHEMAS: usize = 128;

/// How many independent tool calls from one step may run at once.
const MAX_PARALLEL_TOOLS: usize = 4;

/// Step cap for a delegated subagent run.
const SUBAGENT_MAX_ITERATIONS: usize = 25;

fn env_usize(name: &str, default: usize, min: usize, max: usize) -> usize {
    std::env::var(name)
        .ok()
        .and_then(|value| value.trim().parse::<usize>().ok())
        .filter(|value| *value >= min && *value <= max)
        .unwrap_or(default)
}

pub struct AgentRuntime {
    storage: Arc<StorageEngine>,
    tools: Arc<ToolRegistry>,
    approval: Arc<ApprovalEngine>,
    audit: Arc<AuditLogger>,
    providers: Arc<RwLock<HashMap<String, Arc<dyn ModelProvider>>>>,
    default_provider: Arc<RwLock<String>>,
    event_tx: broadcast::Sender<AgentEvent>,
    workspace_root: String,
    max_turn_iterations: AtomicUsize,
    context_budget_tokens: AtomicUsize,
}

impl AgentRuntime {
    pub fn new(
        storage: Arc<StorageEngine>,
        tools: Arc<ToolRegistry>,
        approval: Arc<ApprovalEngine>,
        audit: Arc<AuditLogger>,
        event_tx: broadcast::Sender<AgentEvent>,
        workspace_root: String,
    ) -> Self {
        Self {
            storage,
            tools,
            approval,
            audit,
            providers: Arc::new(RwLock::new(HashMap::new())),
            default_provider: Arc::new(RwLock::new("default".to_string())),
            event_tx,
            workspace_root,
            max_turn_iterations: AtomicUsize::new(env_usize(
                "ODON_MAX_TURN_ITERATIONS",
                DEFAULT_MAX_TURN_ITERATIONS,
                1,
                10_000,
            )),
            context_budget_tokens: AtomicUsize::new(env_usize(
                "ODON_CONTEXT_BUDGET_TOKENS",
                DEFAULT_CONTEXT_BUDGET_TOKENS,
                8_000,
                10_000_000,
            )),
        }
    }

    /// The per-turn step cap currently in force.
    pub fn max_turn_iterations(&self) -> usize {
        self.max_turn_iterations.load(Ordering::Relaxed)
    }

    /// Sets the per-turn step cap (clamped to 1..=10000).
    pub fn set_max_turn_iterations(&self, value: usize) {
        self.max_turn_iterations
            .store(value.clamp(1, 10_000), Ordering::Relaxed);
    }

    /// The context budget in estimated tokens currently in force.
    pub fn context_budget_tokens(&self) -> usize {
        self.context_budget_tokens.load(Ordering::Relaxed)
    }

    /// Sets the context budget in estimated tokens (clamped at 8000+).
    pub fn set_context_budget_tokens(&self, value: usize) {
        self.context_budget_tokens
            .store(value.max(8_000), Ordering::Relaxed);
    }

    pub fn storage(&self) -> Arc<StorageEngine> {
        self.storage.clone()
    }

    pub fn tools(&self) -> Arc<ToolRegistry> {
        self.tools.clone()
    }

    pub fn approval(&self) -> Arc<ApprovalEngine> {
        self.approval.clone()
    }

    pub fn event_sender(&self) -> broadcast::Sender<AgentEvent> {
        self.event_tx.clone()
    }

    pub async fn register_provider(
        &self,
        name: impl Into<String>,
        provider: Arc<dyn ModelProvider>,
        as_default: bool,
    ) {
        let name_str = name.into();
        let mut providers = self.providers.write().await;
        providers.insert(name_str.clone(), provider);
        if as_default {
            let mut def = self.default_provider.write().await;
            *def = name_str;
        }
    }

    pub async fn unregister_provider(&self, name: &str) {
        let mut providers = self.providers.write().await;
        providers.remove(name);
        let mut def = self.default_provider.write().await;
        if def.as_str() == name {
            *def = providers
                .keys()
                .find(|key| key.as_str() != "odon-simulated" && key.as_str() != "default")
                .cloned()
                .unwrap_or_else(|| "default".to_string());
        }
    }

    pub async fn get_provider(&self, name: Option<&str>) -> Option<Arc<dyn ModelProvider>> {
        let providers = self.providers.read().await;
        if let Some(n) = name {
            if let Some(p) = providers.get(n) {
                return Some(p.clone());
            }
            // Strict routing: do not silently fall back if a specific provider was requested
            return None;
        }
        let def = self.default_provider.read().await;
        providers.get(&*def).cloned().or_else(|| {
            // Pick first non-simulated provider if available
            providers
                .iter()
                .find(|(k, _)| k.as_str() != "odon-simulated")
                .map(|(_, v)| v.clone())
        })
    }

    /// The folder a session is pinned to, falling back to the daemon default.
    fn session_workspace(&self, session_id: &str) -> String {
        self.storage
            .get_session(session_id)
            .ok()
            .and_then(|session| session.workspace_root)
            .filter(|path| std::path::Path::new(path).is_dir())
            .unwrap_or_else(|| self.workspace_root.clone())
    }

    /// Build context harness with OS, time, workspace status, memories, and tools
    pub fn build_system_harness(&self, workspace_root: &str) -> String {
        let now = chrono::Utc::now().to_rfc3339();
        let os = std::env::consts::OS;
        let arch = std::env::consts::ARCH;

        let memories = self.storage.list_memories(None).unwrap_or_default();
        let mut memories_text = String::new();
        if !memories.is_empty() {
            memories_text.push_str("\n### Known User Preferences & Project Memory:\n");
            for m in memories.iter().take(10) {
                memories_text.push_str(&format!("- [{}] {}: {}\n", m.category, m.key, m.value));
            }
        }

        format!(
            "You are ODON (Open Digital Orchestration Network), an advanced universal AI assistant and autonomous orchestrator.\n\
             \n\
             ### Environment Harness:\n\
             - Current Timestamp: {}\n\
             - Host Platform: {} ({})\n\
             - Workspace Directory: {}\n\
             {}\n\
             ### Persona & Behavior Guidelines:\n\
             1. Natural, fluid conversational responses styled like modern ChatGPT, Muse AI, and Codex.\n\
             2. Do NOT output bureaucratic turn counters or terminal loop spam (e.g. 'Turn 1/30').\n\
             3. Formulate well-structured answers using clean Markdown with headers, bullet points, and code blocks.\n\
             4. When creating code, documents (PRDs, specs), or images, invoke the dedicated artifact tools (artifact_create_code, artifact_create_document, artifact_create_image) so they appear as first-class visual components.\n\
             5. When asked to inspect, read, search, or edit files, execute the corresponding filesystem or git tools directly.\\n\
             6. For any task with three or more distinct steps, plan before acting: write the steps with todo_write (exactly one in_progress at a time), start each step by marking it in_progress, and mark it completed as soon as it is done. Keep the plan current instead of narrating progress only in prose, and do not finish while plan items remain open unless you say why.",
            now, os, arch, workspace_root, memories_text
        )
    }

    /// Renders the session's stored todo plan for the system harness.
    /// The plan is the agent's own working memory for a large task: by
    /// showing it back at the start of every turn (and after compaction,
    /// since the harness is rebuilt per step), the model resumes the
    /// plan instead of relying on having read it earlier. Empty when no
    /// plan exists yet.
    pub fn session_plan_harness(&self, session_id: &str) -> String {
        let todos = self.storage.get_todos(session_id).unwrap_or_default();
        let Some(items) = todos.as_array().filter(|items| !items.is_empty()) else {
            return String::new();
        };
        let mut out = String::from(
            "\n### Current Session Plan (your todo list; keep it updated with todo_write):\n",
        );
        for item in items {
            let title = item
                .get("title")
                .and_then(|v| v.as_str())
                .unwrap_or_default();
            if title.trim().is_empty() {
                continue;
            }
            let marker = match item.get("status").and_then(|v| v.as_str()) {
                Some("completed") => "[x]",
                Some("in_progress") => "[>]",
                _ => "[ ]",
            };
            out.push_str(&format!("- {marker} {title}\n"));
        }
        out
    }

    /// Primary conversational execution entry point.
    /// Operates like ChatGPT / Codex / Muse:
    /// Natural streaming text, no rigid turn-step counter bureaucracy,
    /// seamless background tool execution, and rich artifact synthesis.
    pub async fn execute_turn(
        &self,
        session_id: &str,
        user_prompt: &str,
        model_override: Option<&str>,
        cancellation: CancellationToken,
    ) -> Result<Message> {
        self.execute_turn_with_effort(session_id, user_prompt, model_override, None, cancellation)
            .await
    }

    pub async fn execute_turn_with_effort(
        &self,
        session_id: &str,
        user_prompt: &str,
        model_override: Option<&str>,
        effort: Option<&str>,
        cancellation: CancellationToken,
    ) -> Result<Message> {
        self.execute_turn_with_attachments(
            session_id,
            user_prompt,
            Vec::new(),
            model_override,
            effort,
            cancellation,
        )
        .await
    }

    /// Execute a turn with binary attachments (images / PDFs) that providers
    /// render as native multimodal content.
    pub async fn execute_turn_with_attachments(
        &self,
        session_id: &str,
        user_prompt: &str,
        attachments: Vec<MessageAttachment>,
        model_override: Option<&str>,
        effort: Option<&str>,
        cancellation: CancellationToken,
    ) -> Result<Message> {
        self.execute_turn_controlled(
            session_id,
            user_prompt,
            attachments,
            model_override,
            effort,
            cancellation,
            None,
            None,
        )
        .await
    }

    /// Execute a turn with optional mid-turn steering. Steers are injected as
    /// user messages at the next agent-loop checkpoint; priority steers also
    /// interrupt the current provider stream so the new guidance is applied
    /// immediately.
    #[allow(clippy::too_many_arguments)]
    pub async fn execute_turn_controlled(
        &self,
        session_id: &str,
        user_prompt: &str,
        attachments: Vec<MessageAttachment>,
        model_override: Option<&str>,
        effort: Option<&str>,
        cancellation: CancellationToken,
        steer: Option<SteerQueue>,
        extra_system_prompt: Option<&str>,
    ) -> Result<Message> {
        self.execute_turn_inner(
            session_id,
            user_prompt,
            attachments,
            model_override,
            effort,
            cancellation,
            steer,
            extra_system_prompt,
            0,
            None,
        )
        .await
    }

    /// The turn loop. `depth` counts subagent nesting (a delegated run may
    /// not delegate further), and `iteration_cap` overrides the configured
    /// step cap for bounded child runs.
    #[allow(clippy::too_many_arguments)]
    async fn execute_turn_inner(
        &self,
        session_id: &str,
        user_prompt: &str,
        attachments: Vec<MessageAttachment>,
        model_override: Option<&str>,
        effort: Option<&str>,
        cancellation: CancellationToken,
        steer: Option<SteerQueue>,
        extra_system_prompt: Option<&str>,
        depth: u32,
        iteration_cap: Option<usize>,
    ) -> Result<Message> {
        info!("Executing agent turn for session: {}", session_id);

        // 0. Ensure session exists in storage
        if self.storage.get_session(session_id).is_err() {
            let mut title: String = user_prompt.chars().take(30).collect();
            if user_prompt.chars().count() > 30 {
                title.push_str("...");
            }
            if title.is_empty() {
                title = "New Conversation".to_string();
            }
            let mut sess = Session::new(title, SessionMode::General, "odon-auto".to_string());
            sess.id = session_id.to_string();
            self.storage
                .save_session(&sess)
                .map_err(|e| OdonError::Storage(e.to_string()))?;
        }

        // 1. Persist User Message
        let user_msg = Message::new_user_with_attachments(session_id, user_prompt, attachments);
        self.storage
            .save_message(&user_msg)
            .map_err(|e| OdonError::Storage(e.to_string()))?;

        // 2. Set State: Thinking
        let _ = self.event_tx.send(AgentEvent::StateChanged {
            session_id: session_id.to_string(),
            state: AgentState::Thinking,
        });

        // 3. Resolve provider and wire model independently. Model selections use
        // `provider::model`, preventing a display/model id from being mistaken for
        // a provider registry key.
        let (provider_override, requested_model) = model_override
            .and_then(|value| value.split_once("::"))
            .map(|(provider, model)| (Some(provider), model))
            .unwrap_or((None, model_override.unwrap_or("odon-auto")));
        let provider = match self.get_provider(provider_override).await {
            Some(p) => {
                if p.provider_name() == "odon-simulated" && requested_model != "odon-simulated" {
                    return Err(OdonError::Provider {
                        provider: provider_override.unwrap_or("default").to_string(),
                        message: "Mock provider is disabled. Please configure your API key in Settings (BYOK) or sign in with GitHub Copilot / Antigravity to chat with real AI models.".to_string(),
                    });
                }
                p
            }
            None => {
                return Err(OdonError::Provider {
                    provider: provider_override.unwrap_or("default").to_string(),
                    message: format!(
                        "Provider '{}' is not configured or missing an API key. Please open Settings and configure your credentials to enable real AI responses.",
                        provider_override.unwrap_or("default")
                    ),
                });
            }
        };

        // 4. Build Tools list
        let tool_schemas = self.tools.schemas();
        let tool_defs: Vec<ToolDefinition> = tool_schemas
            .into_iter()
            .map(|s| ToolDefinition {
                name: s.name,
                description: s.description,
                parameters: s.parameters,
            })
            .collect();

        let workspace_root = self.session_workspace(session_id);
        let system_harness = self.build_system_harness(&workspace_root);
        let plan = self.session_plan_harness(session_id);
        let system_harness = if plan.is_empty() {
            system_harness
        } else {
            format!("{system_harness}\n{plan}")
        };
        let system_harness = match extra_system_prompt {
            Some(extra) if !extra.trim().is_empty() => format!(
                "{system_harness}\n\n## Operator Persona & Custom Instructions\n{}",
                extra.trim()
            ),
            _ => system_harness,
        };
        let mut active_artifacts = Vec::new();
        let mut total_accumulated_text = String::new();
        let mut total_accumulated_thought = String::new();

        // Tool schemas for this turn: the registry catalogue, capped at
        // MAX_INLINE_TOOL_SCHEMAS. Past the cap a core set goes inline
        // with `find_tools`, and the model loads the rest on demand; the
        // runtime's own tools (find_tools, run_subagent) ride along.
        let subagent_enabled = depth == 0;
        let mut active_tools = initial_active_tools(&tool_defs, subagent_enabled);

        let max_turn_iterations = iteration_cap
            .unwrap_or_else(|| self.max_turn_iterations())
            .max(1);
        let mut current_iteration = 0;
        let mut hit_step_cap = false;

        // 5. Multi-Step Agentic Loop (ReAct / Tool-Use Loop)
        while current_iteration < max_turn_iterations {
            current_iteration += 1;

            if cancellation.is_cancelled() {
                let _ = self.event_tx.send(AgentEvent::StateChanged {
                    session_id: session_id.to_string(),
                    state: AgentState::Completed,
                });
                break;
            }

            // Inject any steers that arrived since the previous checkpoint.
            if let Some(queue) = &steer {
                for message in queue.drain() {
                    let steer_message = Message::new_user_with_attachments(
                        session_id,
                        message.text.clone(),
                        message.attachments,
                    );
                    let _ = self.storage.save_message(&steer_message);
                    let _ = self.event_tx.send(AgentEvent::SteerApplied {
                        session_id: session_id.to_string(),
                        steer_id: message.id,
                        text: message.text,
                    });
                }
            }

            // Load fresh history from storage. The stored history stays
            // whole; only the request copy is compacted to the budget.
            let history = self
                .storage
                .list_messages(session_id)
                .unwrap_or_else(|_| vec![user_msg.clone()]);
            let history = compact_history(history, self.context_budget_tokens());

            let chat_req = ChatRequest {
                messages: history,
                model: requested_model.to_string(),
                system_prompt: Some(system_harness.clone()),
                tools: active_tools.clone(),
                temperature: Some(0.7),
                max_tokens: Some(4096),
                effort: effort
                    .filter(|e| matches!(*e, "low" | "medium" | "high" | "xhigh"))
                    .map(str::to_string),
            };

            let (stream_tx, mut stream_rx) = mpsc::channel::<StreamEvent>(128);
            let prov_clone = provider.clone();
            let iteration_cancel = cancellation.child_token();
            let cancel_clone = iteration_cancel.clone();

            let prov_task = tokio::spawn(async move {
                prov_clone
                    .stream_chat(chat_req, stream_tx, cancel_clone)
                    .await
            });

            let mut step_text = String::new();
            let mut step_thought = String::new();
            let mut tool_accumulator: HashMap<usize, AccumulatedToolCall> = HashMap::new();
            let mut stream_error = None;

            // Process stream deltas. A priority steer cancels only this
            // iteration's stream so the new guidance can be applied promptly.
            let mut steer_interrupted = false;
            loop {
                let evt = if let Some(queue) = &steer {
                    tokio::select! {
                        evt = stream_rx.recv() => evt,
                        _ = queue.wait_priority() => {
                            iteration_cancel.cancel();
                            steer_interrupted = true;
                            None
                        }
                    }
                } else {
                    stream_rx.recv().await
                };
                let Some(evt) = evt else {
                    break;
                };
                match evt {
                    StreamEvent::TextDelta(delta) => {
                        step_text.push_str(&delta);
                        total_accumulated_text.push_str(&delta);
                        let _ = self.event_tx.send(AgentEvent::TextDelta {
                            session_id: session_id.to_string(),
                            delta,
                        });
                    }
                    StreamEvent::ThoughtDelta(delta) => {
                        step_thought.push_str(&delta);
                        total_accumulated_thought.push_str(&delta);
                        let _ = self.event_tx.send(AgentEvent::ThoughtDelta {
                            session_id: session_id.to_string(),
                            delta,
                        });
                    }
                    StreamEvent::ToolCallDelta {
                        index,
                        id,
                        name,
                        arguments_delta,
                    } => {
                        let entry = tool_accumulator.entry(index).or_default();
                        if let Some(call_id) = id {
                            entry.id = call_id;
                        }
                        if let Some(tool_name) = name {
                            entry.name = tool_name;
                        }
                        if let Some(arg_chunk) = arguments_delta {
                            entry.arguments.push_str(&arg_chunk);
                        }
                    }
                    StreamEvent::Done => break,
                    StreamEvent::Error(err) => {
                        let _ = self.event_tx.send(AgentEvent::Error {
                            session_id: session_id.to_string(),
                            message: err.clone(),
                        });
                        stream_error = Some(err);
                        break;
                    }
                    _ => {}
                }
            }

            // Drop the receiver so a cancelled provider never blocks on a full
            // event channel while it is shutting down.
            drop(stream_rx);

            match prov_task.await {
                Ok(Ok(())) => {}
                Ok(Err(err)) => {
                    // Unwrap the provider's message so the caller does not see
                    // "Provider error (x): Provider error (x): ...".
                    stream_error = Some(match err {
                        OdonError::Provider { message, .. } => message,
                        other => other.to_string(),
                    });
                }
                Err(err) => stream_error = Some(format!("Provider task failed: {err}")),
            }

            if let Some(message) = stream_error {
                let _ = self.event_tx.send(AgentEvent::StateChanged {
                    session_id: session_id.to_string(),
                    state: AgentState::Failed,
                });
                return Err(OdonError::Provider {
                    provider: provider.provider_name().to_string(),
                    message,
                });
            }

            if steer_interrupted {
                // Preempted by a priority steer: skip this partial step and let
                // the next iteration drain and apply the guidance.
                continue;
            }

            // If no tools were called in this step, we reached the final answer.
            // If a steer is waiting, run another iteration so it is not lost.
            if tool_accumulator.is_empty() {
                let pending_steer = steer
                    .as_ref()
                    .map(|queue| queue.has_pending())
                    .unwrap_or(false);
                if pending_steer && current_iteration < max_turn_iterations {
                    continue;
                }
                break;
            }

            // Execute Tools. The runtime's own tools (find_tools,
            // run_subagent) run inline; registry tools run with bounded
            // parallelism, because the calls in one step are independent
            // by protocol. Every result is collected before anything is
            // persisted, so history order never depends on finish order.
            let _ = self.event_tx.send(AgentEvent::StateChanged {
                session_id: session_id.to_string(),
                state: AgentState::ExecutingTool,
            });

            #[derive(Clone)]
            struct PendingCall {
                index: usize,
                call_id: String,
                name: String,
                args: serde_json::Value,
            }
            struct ExecutedCall {
                index: usize,
                call_id: String,
                name: String,
                args: serde_json::Value,
                output: serde_json::Value,
                is_error: bool,
            }

            let mut sorted_indices: Vec<_> = tool_accumulator.keys().cloned().collect();
            sorted_indices.sort_unstable();
            let mut pending: Vec<PendingCall> = Vec::new();
            for idx in sorted_indices {
                if let Some(tool_call) = tool_accumulator.get(&idx) {
                    let call_id = if tool_call.id.is_empty() {
                        format!("call_{}_{}", tool_call.name, uuid::Uuid::new_v4().simple())
                    } else {
                        tool_call.id.clone()
                    };
                    let args_val: serde_json::Value = serde_json::from_str(&tool_call.arguments)
                        .unwrap_or_else(|_| serde_json::json!({}));
                    pending.push(PendingCall {
                        index: idx,
                        call_id,
                        name: tool_call.name.clone(),
                        args: args_val,
                    });
                }
            }
            for call in &pending {
                let _ = self.event_tx.send(AgentEvent::ToolCallStarted {
                    session_id: session_id.to_string(),
                    call_id: call.call_id.clone(),
                    tool_name: call.name.clone(),
                    arguments: call.args.clone(),
                });
            }

            let mut executed: Vec<ExecutedCall> = Vec::new();
            let mut registry_calls: Vec<PendingCall> = Vec::new();
            for call in pending {
                match call.name.as_str() {
                    "find_tools" => {
                        let output = self.handle_find_tools(&call.args, &mut active_tools);
                        executed.push(ExecutedCall {
                            index: call.index,
                            call_id: call.call_id,
                            name: call.name,
                            args: call.args,
                            output,
                            is_error: false,
                        });
                    }
                    "run_subagent" if subagent_enabled => {
                        let (output, is_error) = self
                            .execute_subagent(
                                session_id,
                                &call.args,
                                requested_model,
                                depth,
                                cancellation.clone(),
                            )
                            .await;
                        executed.push(ExecutedCall {
                            index: call.index,
                            call_id: call.call_id,
                            name: call.name,
                            args: call.args,
                            output,
                            is_error,
                        });
                    }
                    _ => registry_calls.push(call),
                }
            }

            if !registry_calls.is_empty() {
                let registry_meta = registry_calls.clone();
                if cancellation.is_cancelled() {
                    for call in registry_calls {
                        executed.push(ExecutedCall {
                            index: call.index,
                            call_id: call.call_id,
                            name: call.name,
                            args: call.args,
                            output: serde_json::json!({
                                "error": "Tool execution cancelled by user"
                            }),
                            is_error: true,
                        });
                    }
                } else {
                    let mut join_set = tokio::task::JoinSet::new();
                    let semaphore = Arc::new(tokio::sync::Semaphore::new(MAX_PARALLEL_TOOLS));
                    for call in registry_calls {
                        let tools = self.tools.clone();
                        let tool_ctx = ToolContext {
                            session_id: session_id.to_string(),
                            workspace_root: workspace_root.clone(),
                            approval_engine: self.approval.clone(),
                            audit_logger: self.audit.clone(),
                            storage: Some(self.storage.clone()),
                        };
                        let semaphore = semaphore.clone();
                        join_set.spawn(async move {
                            let _permit = semaphore.acquire_owned().await;
                            let (output, is_error) = match tools
                                .execute_tool(&call.name, &tool_ctx, call.args.clone())
                                .await
                            {
                                Ok(val) => (val, false),
                                Err(e) => {
                                    error!("Tool {} execution failed: {}", call.name, e);
                                    (serde_json::json!({ "error": e.to_string() }), true)
                                }
                            };
                            ExecutedCall {
                                index: call.index,
                                call_id: call.call_id,
                                name: call.name,
                                args: call.args,
                                output,
                                is_error,
                            }
                        });
                    }
                    loop {
                        tokio::select! {
                            _ = cancellation.cancelled() => {
                                join_set.abort_all();
                                break;
                            }
                            joined = join_set.join_next() => match joined {
                                Some(Ok(done)) => executed.push(done),
                                Some(Err(_)) => {}
                                None => break,
                            }
                        }
                    }
                    // Calls that never finished (a Stop aborted them)
                    // still owe the model a result, or the history the
                    // next step replays is invalid.
                    for meta in &registry_meta {
                        if !executed.iter().any(|done| done.index == meta.index) {
                            executed.push(ExecutedCall {
                                index: meta.index,
                                call_id: meta.call_id.clone(),
                                name: meta.name.clone(),
                                args: meta.args.clone(),
                                output: serde_json::json!({
                                    "error": "Tool execution cancelled by user"
                                }),
                                is_error: true,
                            });
                        }
                    }
                }
            }

            executed.sort_by_key(|call| call.index);
            let mut executed_tools_info = Vec::new();
            let mut tool_result_msgs: Vec<(String, String, String)> = Vec::new();
            for call in executed {
                if !call.is_error {
                    if let Some(art_val) = call.output.get("artifact") {
                        if let Ok(artifact) = serde_json::from_value::<Artifact>(art_val.clone()) {
                            let _ = self.storage.save_artifact(&artifact);
                            let _ = self.event_tx.send(AgentEvent::ArtifactCreated {
                                session_id: session_id.to_string(),
                                artifact: artifact.clone(),
                            });
                            active_artifacts.push(artifact.id.clone());
                        }
                    }
                }
                let _ = self.event_tx.send(AgentEvent::ToolCallFinished {
                    session_id: session_id.to_string(),
                    call_id: call.call_id.clone(),
                    output: call.output.clone(),
                    is_error: call.is_error,
                });
                executed_tools_info.push(ToolCallInfo {
                    call_id: call.call_id.clone(),
                    tool_name: call.name.clone(),
                    arguments: call.args,
                    output: Some(call.output.clone()),
                    is_error: call.is_error,
                    requires_approval: false,
                    is_approved: Some(true),
                });
                // Tool results are persisted AFTER the assistant message
                // that declares the calls (below): providers reject a
                // tool message that does not follow the matching
                // tool_calls message.
                tool_result_msgs.push((call.call_id, call.name, call.output.to_string()));
            }
            if current_iteration >= max_turn_iterations {
                hit_step_cap = true;
            }

            // Persist the assistant step with its tool_calls FIRST so history order
            // (assistant tool_calls, then tool results) is valid for every provider.
            let mut intermediate_assistant = Message::new_assistant(
                session_id,
                step_text,
                if step_thought.is_empty() {
                    None
                } else {
                    Some(step_thought)
                },
                Some(requested_model.to_string()),
            );
            intermediate_assistant.content.tool_calls = executed_tools_info;
            let _ = self.storage.save_message(&intermediate_assistant);

            for (call_id, tool_name, output) in tool_result_msgs {
                let mut tool_msg = Message::new_tool(session_id, call_id, output);
                if let Some(info) = tool_msg.content.tool_calls.first_mut() {
                    info.tool_name = tool_name;
                }
                let _ = self.storage.save_message(&tool_msg);
            }

            // Set state back to thinking for the next iteration
            let _ = self.event_tx.send(AgentEvent::StateChanged {
                session_id: session_id.to_string(),
                state: AgentState::Thinking,
            });
        }

        // 6. Persist Assistant Final Message. A turn that hit the step
        // cap with tool work still in flight pauses instead of
        // completing: everything is persisted, the reply says so, and a
        // follow-up message resumes the same session from that history.
        if hit_step_cap {
            if !total_accumulated_text.is_empty() {
                total_accumulated_text.push_str("\n\n");
            }
            total_accumulated_text.push_str(&format!(
                "[Turn paused after {max_turn_iterations} steps with work still in progress. Send \"continue\" to resume this session.]"
            ));
        }
        let thought_opt = if total_accumulated_thought.is_empty() {
            None
        } else {
            Some(total_accumulated_thought)
        };

        let mut assistant_msg = Message::new_assistant(
            session_id,
            total_accumulated_text,
            thought_opt,
            Some(requested_model.to_string()),
        );
        assistant_msg.content.artifact_ids = active_artifacts;
        assistant_msg.content.action_chips = Vec::new();

        self.storage
            .save_message(&assistant_msg)
            .map_err(|e| OdonError::Storage(e.to_string()))?;

        // 7. Set State: Completed, or Paused when the step cap cut the
        // turn short.
        let _ = self.event_tx.send(AgentEvent::StateChanged {
            session_id: session_id.to_string(),
            state: if hit_step_cap {
                AgentState::Paused
            } else {
                AgentState::Completed
            },
        });

        Ok(assistant_msg)
    }

    /// Executes one `find_tools` call: searches the registry catalogue
    /// for the query, registers the best matches in the turn's active
    /// tool set so later steps can call them natively, and returns their
    /// schemas to the model.
    fn handle_find_tools(
        &self,
        args: &serde_json::Value,
        active: &mut Vec<ToolDefinition>,
    ) -> serde_json::Value {
        let query = args
            .get("query")
            .and_then(|v| v.as_str())
            .unwrap_or_default()
            .to_lowercase();
        let terms: Vec<&str> = query.split_whitespace().collect();
        let mut matches: Vec<ToolDefinition> = Vec::new();
        for schema in self.tools.schemas() {
            if active.iter().any(|tool| tool.name == schema.name) {
                continue;
            }
            let haystack = format!("{} {}", schema.name, schema.description).to_lowercase();
            let score = terms
                .iter()
                .filter(|term| haystack.contains(**term))
                .count();
            if !terms.is_empty() && score == 0 {
                continue;
            }
            matches.push(ToolDefinition {
                name: schema.name,
                description: schema.description,
                parameters: schema.parameters,
            });
            if matches.len() >= 8 {
                break;
            }
        }
        let loaded: Vec<serde_json::Value> = matches
            .iter()
            .map(|tool| {
                serde_json::json!({
                    "name": tool.name,
                    "description": tool.description,
                    "parameters": tool.parameters,
                })
            })
            .collect();
        active.extend(matches);
        serde_json::json!({
            "tools": loaded,
            "note": "These tools are now registered for this turn; call them by name in your next step. If none match, reformulate the query.",
        })
    }

    /// Executes one `run_subagent` call: a bounded nested turn on a
    /// child session with the same providers, tools and workspace. The
    /// child's steps go through the same approval gates; only its final
    /// message comes back as the tool result.
    async fn execute_subagent(
        &self,
        parent_session_id: &str,
        args: &serde_json::Value,
        model: &str,
        depth: u32,
        cancellation: CancellationToken,
    ) -> (serde_json::Value, bool) {
        if depth >= 2 {
            return (
                serde_json::json!({ "error": "run_subagent cannot be nested" }),
                true,
            );
        }
        let tasks = match Self::parse_subagent_tasks(args) {
            Some(tasks) => tasks,
            None => {
                return (
                    serde_json::json!({ "error": "run_subagent needs 'task' or 'tasks[]'" }),
                    true,
                );
            }
        };
        let provider_override = args
            .get("provider")
            .and_then(|v| v.as_str())
            .filter(|v| !v.trim().is_empty())
            .map(str::trim);
        let requested_model = args
            .get("model")
            .and_then(|v| v.as_str())
            .filter(|v| !v.trim().is_empty())
            .map(str::trim);
        let parent_model_override = self.resolve_model_for_session(parent_session_id, model);

        let child_ids: Vec<String> = (0..tasks.len())
            .map(|index| format!("sub-{}-{}", uuid::Uuid::new_v4().simple(), index))
            .collect();
        let child_count = child_ids.len();

        for (index, child_session_id) in child_ids.iter().cloned().enumerate() {
            let mut session = Session::new(
                format!("Subagent: {}", truncate_chars(&tasks[index], 60)),
                SessionMode::Agents,
                model.to_string(),
            );
            session.id = child_session_id.clone();
            session.workspace_root = Some(self.session_workspace(parent_session_id));
            if let Ok(Some(parent)) = self.storage.get_session(parent_session_id) {
                if let Some(meta) = parent.metadata.clone() {
                    let mut child_meta = meta;
                    if let Some(obj) = child_meta.as_object_mut() {
                        obj.insert(
                            "parent_session".to_string(),
                            serde_json::Value::String(parent_session_id.to_string()),
                        );
                        obj.insert(
                            "parent_model".to_string(),
                            serde_json::Value::String(model.to_string()),
                        );
                        obj.insert(
                            "child_index".to_string(),
                            serde_json::Value::Number(serde_json::Number::from(index)),
                        );
                        obj.insert(
                            "child_count".to_string(),
                            serde_json::Value::Number(serde_json::Number::from(child_count)),
                        );
                    }
                    session.metadata = Some(child_meta);
                } else {
                    session.metadata = Some(serde_json::json!({
                        "parent_session": parent_session_id,
                        "parent_model": model,
                        "child_index": index,
                        "child_count": child_count,
                    }));
                }
            }
            let _ = self.storage.save_session(&session);
        }

        let mut join_set = JoinSet::new();
        let storage = self.storage.clone();
        let approval = self.approval.clone();
        let audit = self.audit.clone();

        for (index, child_session_id) in child_ids.iter().cloned().enumerate() {
            let task = tasks[index].clone();
            let storage = storage.clone();
            let approval = approval.clone();
            let audit = audit.clone();
            let cancellation_child = cancellation.child();
            let provider_name = provider_override.map(|v| v.to_string());
            let model_override = requested_model
                .or(parent_model_override.as_deref())
                .map(|v| v.to_string())
                .unwrap_or_else(|| model.to_string());

            join_set.spawn(async move {
                let _ = AgentRuntime::publish_child_event(
                    &approval,
                    &child_session_id,
                    AgentState::Thinking,
                    &task,
                    parent_session_id,
                );

                let worktree = AgentRuntime::prepare_child_workspace(
                    parent_session_id,
                    &child_session_id,
                    storage.clone(),
                )
                .await;

                let mut attachments = Vec::new();
                attachments.extend(
                    AgentRuntime::collect_parent_attachments(storage.clone(), parent_session_id)
                        .await,
                );

                let result = match worktree {
                    Ok(worktree_path) => {
                        let _ = AgentRuntime::update_child_workspace(
                            &child_session_id,
                            Some(worktree_path.to_string_lossy().to_string()),
                            storage.clone(),
                        );
                        let workspace_root = worktree_path.to_string_lossy().to_string();
                        match Box::pin(AgentRuntime::execute_child_turn(
                            &child_session_id,
                            task,
                            attachments,
                            provider_name.as_deref(),
                            &model_override,
                            storage.clone(),
                            approval,
                            audit,
                            cancellation_child,
                            depth + 1,
                            Some(workspace_root),
                        ))
                        .await
                        {
                            Ok(message) => Ok(message.content.text.clone()),
                            Err(error) => Err(error.to_string()),
                        }
                    }
                    Err(error) => Err(error),
                };

                let _ = AgentRuntime::publish_child_event(
                    &approval,
                    &child_session_id,
                    AgentState::Completed,
                    &task,
                    parent_session_id,
                );

                let _ = AgentRuntime::finalize_child_workspace(&child_session_id, storage.clone())
                    .await;

                (index, child_session_id, result)
            });
        }

        let mut completed_reports = Vec::new();
        let mut completed_errors = Vec::new();

        while let Some(join) = join_set.join_next().await {
            match join {
                Ok((index, child_session_id, Ok(output))) => {
                    completed_reports.push((index, child_session_id, output, None));
                }
                Ok((index, child_session_id, Err(error))) => {
                    completed_errors.push((index, child_session_id, error));
                }
                Err(join_error) => {
                    completed_errors.push((0, format!("join-error-{}", 0), join_error.to_string()));
                }
            }
        }

        completed_reports.sort_by_key(|item| item.0);
        completed_errors.sort_by_key(|item| item.0);

        let mut reports = Vec::with_capacity(child_count);
        for index in 0..child_count {
            let mut report = serde_json::json!({
                "session": child_ids.get(index).cloned().unwrap_or_default(),
                "status": "failed",
                "output": "",
            });
            if let Some((_, _, output, _)) = completed_reports.iter().find(|item| item.0 == index) {
                report["status"] = serde_json::json!("completed");
                report["output"] = serde_json::json!(output);
            } else if let Some((_, _, error)) = completed_errors.iter().find(|item| item.0 == index) {
                report["error"] = serde_json::json!(error);
            }
            reports.push(report);
        }

        let overall_error = completed_errors.first().map(|item| item.2.clone());

        if child_count > 1 {
            (
                serde_json::json!({
                    "subagents": reports,
                    "child_count": child_count,
                    "provider": provider_override.unwrap_or_else(|| parent_model_override.unwrap_or_else(|| model.to_string())),
                    "model": requested_model.unwrap_or_else(|| parent_model_override.unwrap_or_else(|| model.to_string())),
                }),
                overall_error.is_some(),
            )
        } else {
            let report = reports.into_iter().next().unwrap_or_default();
            (
                serde_json::json!({
                    "subagent_session": report.get("session").cloned().unwrap_or_default(),
                    "output": report.get("output").cloned().unwrap_or_default(),
                    "status": report.get("status").cloned().unwrap_or_default(),
                    "error": report.get("error").cloned().unwrap_or_default(),
                    "provider": provider_override.unwrap_or_else(|| parent_model_override.unwrap_or_else(|| model.to_string())),
                    "model": requested_model.unwrap_or_else(|| parent_model_override.unwrap_or_else(|| model.to_string())),
                }),
                overall_error.is_some(),
            )
        }
    }

    fn parse_subagent_tasks(args: &serde_json::Value) -> Option<Vec<String>> {
        let tasks = args
            .get("tasks")
            .and_then(|v| v.as_array())
            .map(|arr| {
                arr.iter()
                    .filter_map(|v| v.as_str().map(str::trim).filter(|s| !s.is_empty()))
                    .collect::<Vec<_>>()
            })
            .filter(|arr| !arr.is_empty());
        if tasks.is_some() {
            return tasks.map(|v| v.into_iter().map(|s| s.to_string()).collect());
        }
        args.get("task")
            .and_then(|v| v.as_str())
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .map(|s| vec![s.to_string()])
    }

    fn resolve_model_for_session(&self, session_id: &str, fallback: &str) -> Option<String> {
        self.storage
            .list_sessions()
            .ok()
            .and_then(|sessions| sessions.into_iter().find(|s| s.id == session_id))
            .and_then(|session| {
                if session.active_model.is_empty() || session.active_model == "odon-auto" {
                    None
                } else {
                    Some(session.active_model)
                }
            })
    }

    async fn publish_child_event(
        approval: &Arc<ApprovalEngine>,
        child_session_id: &str,
        state: AgentState,
        task: &str,
        parent_session_id: &str,
    ) {
        let _ = approval
            .event_tx
            .send(AgentEvent::StateChanged {
                session_id: child_session_id.to_string(),
                state,
            });
        let _ = approval.event_tx.send(AgentEvent::TextDelta {
            session_id: child_session_id.to_string(),
            delta: format!(
                "Delegated from {}: {}",
                parent_session_id,
                truncate_chars(task, 120)
            ),
        });
    }

    async fn collect_parent_attachments(
        storage: Arc<StorageEngine>,
        parent_session_id: &str,
    ) -> Vec<MessageAttachment> {
        let mut collected = Vec::new();
        if let Ok(messages) = storage.list_messages(parent_session_id) {
            for message in messages.into_iter().rev() {
                if let Some(attachments) = message.content.attachments {
                    for attachment in attachments {
                        if !collected.iter().any(|a| a.data_base64 == attachment.data_base64) {
                            collected.push(attachment);
                        }
                    }
                }
                if collected.len() >= 12 {
                    break;
                }
            }
        }
        collected
    }

    async fn prepare_child_workspace(
        parent_session_id: &str,
        child_session_id: &str,
        storage: Arc<StorageEngine>,
    ) -> Result<PathBuf, String> {
        let parent_session = storage
            .list_sessions()
            .ok()
            .and_then(|sessions| sessions.into_iter().find(|s| s.id == parent_session_id))
            .ok_or("parent session not found")?;
        let workspace = parent_session
            .workspace_root
            .clone()
            .unwrap_or_default();
        if workspace.is_empty() {
            return Err("workspace is not configured".to_string());
        }
        let root = PathBuf::from(&workspace);
        if !root.join(".git").exists() {
            return Ok(root);
        }
        let worktree = root.join(".odon").join("worktrees").join(child_session_id);
        tokio::fs::create_dir_all(&worktree)
            .await
            .map_err(|e| format!("failed to create worktree dir: {e}"))?;
        let output = tokio::process::Command::new("git")
            .args([
                "worktree",
                "add",
                "--detach",
                worktree.to_string_lossy().as_ref(),
                "HEAD",
            ])
            .current_dir(&root)
            .output()
            .await
            .map_err(|e| format!("failed to spawn git: {e}"))?;
        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            return Err(format!("git worktree add failed: {stderr}"));
        }
        Ok(worktree)
    }

    async fn update_child_workspace(
        child_session_id: &str,
        worktree: Option<String>,
        storage: Arc<StorageEngine>,
    ) {
        if let Some(path) = worktree {
            if let Ok(Some(mut session)) = storage.get_session(child_session_id) {
                session.workspace_root = Some(path);
                let _ = storage.save_session(&session);
            }
        }
    }

    async fn finalize_child_workspace(
        child_session_id: &str,
        storage: Arc<StorageEngine>,
    ) {
        let _ = storage.delete_session(child_session_id);
        let _ = tokio::process::Command::new("git")
            .args([
                "worktree",
                "remove",
                "--force",
                child_session_id,
            ])
            .output()
            .await;
    }

    async fn execute_child_turn(
        child_session_id: &str,
        task: Vec<String>,
        attachments: Vec<MessageAttachment>,
        provider_name: Option<&str>,
        model: &str,
        storage: Arc<StorageEngine>,
        approval: Arc<ApprovalEngine>,
        audit: Arc<AuditLogger>,
        cancellation: CancellationToken,
        depth: u32,
        workspace_root: Option<String>,
    ) -> Result<Message> {
        Err(OdonError::Provider {
            provider: "subagent".to_string(),
            description: "child turn execution is not wired yet".to_string(),
        })
    }
}

/// Truncates a string to at most `max` characters, on a char boundary.
fn truncate_chars(text: &str, max: usize) -> String {
    text.chars().take(max).collect()
}

/// Rough token estimate (one token per four characters), good enough for
/// a budget check that only decides when to compact.
fn estimate_tokens(text: &str) -> usize {
    text.chars().count().div_ceil(4)
}

fn message_estimated_tokens(message: &Message) -> usize {
    let mut total = estimate_tokens(&message.content.text);
    if let Some(thought) = &message.content.thought {
        total += estimate_tokens(thought);
    }
    for call in &message.content.tool_calls {
        total += estimate_tokens(&call.arguments.to_string());
        if let Some(output) = &call.output {
            total += estimate_tokens(&output.to_string());
        }
    }
    total
}

/// Largest tool output kept whole through compaction, in characters.
const COMPACT_TOOL_OUTPUT_CHARS: usize = 4_000;

fn trim_long_text(text: &str, max_chars: usize) -> String {
    if text.chars().count() <= max_chars {
        return text.to_string();
    }
    let head: String = text.chars().take(max_chars * 3 / 4).collect();
    let tail: String = {
        let skipped = text.chars().count().saturating_sub(max_chars / 4);
        text.chars().skip(skipped).collect()
    };
    format!(
        "{head}\n... [{} characters trimmed by context compaction] ...\n{tail}",
        text.chars().count()
    )
}

/// Compacts a request copy of the history to `budget_tokens` estimated
/// tokens. The stored history is never touched. Oversized tool outputs
/// are trimmed first (they are usually the bulk and the least load
/// bearing); if that is not enough, the oldest middle messages are
/// elided between a kept head and a kept tail, with a marker so the
/// model knows context was dropped. The kept tail never starts with a
/// tool result, which providers reject without its declaring message.
fn compact_history(messages: Vec<Message>, budget_tokens: usize) -> Vec<Message> {
    let total: usize = messages.iter().map(message_estimated_tokens).sum();
    if total <= budget_tokens || messages.len() <= 2 {
        return messages;
    }

    let mut messages: Vec<Message> = messages
        .into_iter()
        .map(|mut message| {
            if message.role == MessageRole::Tool
                && message.content.text.chars().count() > COMPACT_TOOL_OUTPUT_CHARS
            {
                message.content.text =
                    trim_long_text(&message.content.text, COMPACT_TOOL_OUTPUT_CHARS);
            }
            for call in &mut message.content.tool_calls {
                if let Some(serde_json::Value::String(output)) = &mut call.output {
                    if output.chars().count() > COMPACT_TOOL_OUTPUT_CHARS {
                        *output = trim_long_text(output, COMPACT_TOOL_OUTPUT_CHARS);
                    }
                }
            }
            message
        })
        .collect();

    let total: usize = messages.iter().map(message_estimated_tokens).sum();
    if total <= budget_tokens {
        return messages;
    }

    let head_len = 4.min(messages.len());
    let tail_budget = budget_tokens / 2;
    let mut tail_start = messages.len();
    let mut tail_tokens = 0usize;
    while tail_start > head_len {
        let next = message_estimated_tokens(&messages[tail_start - 1]);
        if tail_tokens + next > tail_budget && tail_start < messages.len() {
            break;
        }
        tail_tokens += next;
        tail_start -= 1;
    }
    while tail_start < messages.len() && messages[tail_start].role == MessageRole::Tool {
        tail_start += 1;
    }
    if tail_start >= messages.len() && messages.len() > head_len {
        // Keep at least the final exchange even if it alone is large.
        tail_start = messages.len().saturating_sub(2).max(head_len);
        while tail_start < messages.len() && messages[tail_start].role == MessageRole::Tool {
            tail_start += 1;
        }
    }

    let elided = tail_start.saturating_sub(head_len);
    if elided == 0 {
        return messages;
    }
    let session_id = messages
        .first()
        .map(|m| m.session_id.clone())
        .unwrap_or_default();
    let mut compacted: Vec<Message> = messages.drain(..head_len).collect();
    compacted.push(Message::new_user(
        &session_id,
        format!(
            "[Context compacted: {elided} earlier message(s) were elided to fit the context budget. Ask for anything you need re-read.]"
        ),
    ));
    compacted.extend(messages.into_iter().skip(tail_start - head_len));
    compacted
}

/// Tools that stay inline when the catalogue exceeds the inline cap:
/// the everyday coding set. Names that do not exist in a given registry
/// are simply skipped.
const CORE_TOOL_NAMES: &[&str] = &[
    "fs_read",
    "fs_write",
    "fs_edit",
    "fs_patch",
    "fs_list",
    "fs_search",
    "fs_find",
    "bash_run",
    "shell_exec",
    "git_status",
    "git_diff",
    "git_log",
    "todo_write",
    "todo_read",
    "think",
];

/// The `find_tools` definition offered when the catalogue is capped.
fn find_tools_definition() -> ToolDefinition {
    ToolDefinition {
        name: "find_tools".to_string(),
        description:
            "Search the full tool catalogue by keyword and register matching tools for this turn. Use this when the tool you need is not in the current tool list."
                .to_string(),
        parameters: serde_json::json!({
            "type": "object",
            "properties": {
                "query": { "type": "string", "description": "Keywords describing the capability, e.g. 'screenshot browser'." }
            },
            "required": ["query"]
        }),
    }
}

/// The `run_subagent` definition offered on top-level turns.
fn subagent_definition() -> ToolDefinition {
    ToolDefinition {
        name: "run_subagent".to_string(),
        description:
            "Delegate one self-contained task to a subagent with its own session and a bounded step budget. It works in the same workspace and returns its final report. Use for independent side tasks; never for work that needs the current conversation's full context."
                .to_string(),
        parameters: serde_json::json!({
            "type": "object",
            "properties": {
                "task": { "type": "string", "description": "One self-contained instruction for the subagent." }
            },
            "required": ["task"]
        }),
    }
}

/// Builds the tool set a turn starts with: the whole catalogue when it
/// fits the inline cap, otherwise a core set plus `find_tools` (the rest
/// loads on demand), plus `run_subagent` on top-level turns.
fn initial_active_tools(
    registry_defs: &[ToolDefinition],
    subagent_enabled: bool,
) -> Vec<ToolDefinition> {
    let mut active: Vec<ToolDefinition> = Vec::new();
    if subagent_enabled {
        active.push(subagent_definition());
    }
    let needs_search = registry_defs.len() + active.len() > MAX_INLINE_TOOL_SCHEMAS;
    let registry_budget =
        MAX_INLINE_TOOL_SCHEMAS.saturating_sub(active.len() + usize::from(needs_search));
    if registry_defs.len() <= registry_budget {
        active.extend(registry_defs.iter().cloned());
    } else {
        let mut picked: Vec<ToolDefinition> = Vec::new();
        for name in CORE_TOOL_NAMES {
            if let Some(def) = registry_defs.iter().find(|def| &def.name == name) {
                if !picked.iter().any(|tool| &tool.name == name) {
                    picked.push(def.clone());
                }
            }
        }
        for def in registry_defs {
            if picked.len() >= registry_budget {
                break;
            }
            if !picked.iter().any(|tool| tool.name == def.name) {
                picked.push(def.clone());
            }
        }
        picked.truncate(registry_budget);
        active.extend(picked);
    }
    if needs_search {
        active.push(find_tools_definition());
    }
    active
}

#[cfg(test)]
mod tests {
    use super::*;

    fn message(role: MessageRole, text: &str) -> Message {
        let session = "s1";
        match role {
            MessageRole::User => Message::new_user(session, text.to_string()),
            MessageRole::Assistant => Message::new_assistant(session, text.to_string(), None, None),
            MessageRole::System => {
                let mut msg = Message::new_user(session, text.to_string());
                msg.role = MessageRole::System;
                msg
            }
            MessageRole::Tool => Message::new_tool(session, "call_1".to_string(), text.to_string()),
        }
    }

    #[test]
    fn compaction_under_budget_is_unchanged() {
        let history = vec![
            message(MessageRole::User, "hello"),
            message(MessageRole::Assistant, "hi"),
        ];
        let out = compact_history(history.clone(), 120_000);
        assert_eq!(out.len(), history.len());
    }

    #[test]
    fn compaction_trims_a_huge_tool_output_first() {
        let big = "x".repeat(40_000);
        let history = vec![
            message(MessageRole::User, "read it"),
            message(MessageRole::Tool, &big),
            message(MessageRole::Assistant, "done"),
        ];
        let out = compact_history(history, 4_000);
        let total: usize = out.iter().map(message_estimated_tokens).sum();
        assert!(total <= 4_500, "total {total}");
        assert!(out[1]
            .content
            .text
            .contains("trimmed by context compaction"));
    }

    #[test]
    fn compaction_drops_the_middle_and_never_starts_the_tail_with_a_tool() {
        let mut history = vec![message(MessageRole::System, "sys")];
        for i in 0..40 {
            history.push(message(
                MessageRole::User,
                &format!("question {i} {}", "q".repeat(200)),
            ));
            history.push(message(
                MessageRole::Assistant,
                &format!("answer {i} {}", "a".repeat(200)),
            ));
            history.push(message(
                MessageRole::Tool,
                &format!("output {i} {}", "o".repeat(200)),
            ));
        }
        let out = compact_history(history, 2_000);
        let total: usize = out.iter().map(message_estimated_tokens).sum();
        assert!(total <= 2_600, "total {total}");
        assert!(
            out.iter()
                .any(|m| m.content.text.contains("Context compacted")),
            "the elision must be marked"
        );
        // After the kept head and the marker, no message may be a tool
        // result whose declaring assistant message was elided.
        let marker = out
            .iter()
            .position(|m| m.content.text.contains("Context compacted"))
            .expect("marker present");
        if let Some(first_tail) = out.get(marker + 1) {
            assert_ne!(first_tail.role, MessageRole::Tool);
        }
    }

    #[test]
    fn harness_teaches_the_todo_workflow_and_surfaces_the_stored_plan() {
        let storage = Arc::new(StorageEngine::new_in_memory().unwrap());
        let (event_tx, _rx) = odon_core::events::create_event_bus(16);
        let runtime = AgentRuntime::new(
            storage.clone(),
            Arc::new(ToolRegistry::new()),
            Arc::new(ApprovalEngine::new(
                odon_security::SecurityPolicy::default(),
            )),
            Arc::new(AuditLogger::new()),
            event_tx,
            "/tmp".to_string(),
        );

        let harness = runtime.build_system_harness("/tmp");
        assert!(
            harness.contains("todo_write"),
            "the harness must teach the todo workflow for large tasks"
        );
        assert!(
            runtime.session_plan_harness("missing").is_empty(),
            "no stored plan, no plan section"
        );

        let mut session = Session::new("t", SessionMode::General, "m");
        session.id = "s-plan".to_string();
        storage.save_session(&session).unwrap();
        storage
            .set_todos(
                "s-plan",
                &serde_json::json!([
                    {"title": "Inspect the repo", "status": "completed"},
                    {"title": "Implement the fix", "status": "in_progress"},
                    {"title": "Run the tests", "status": "pending"},
                ]),
            )
            .unwrap();
        let plan = runtime.session_plan_harness("s-plan");
        assert!(plan.contains("[x] Inspect the repo"), "{plan}");
        assert!(plan.contains("[>] Implement the fix"), "{plan}");
        assert!(plan.contains("[ ] Run the tests"), "{plan}");
    }

    #[test]
    fn tool_selection_caps_the_inline_schemas_and_offers_search() {
        let defs: Vec<ToolDefinition> = (0..300)
            .map(|i| ToolDefinition {
                name: format!("tool_{i}"),
                description: "d".to_string(),
                parameters: serde_json::json!({"type": "object"}),
            })
            .collect();
        let active = initial_active_tools(&defs, true);
        assert_eq!(active.len(), MAX_INLINE_TOOL_SCHEMAS);
        assert!(active.iter().any(|tool| tool.name == "find_tools"));
        assert!(active.iter().any(|tool| tool.name == "run_subagent"));
    }

    #[test]
    fn tool_selection_sends_everything_when_it_fits() {
        let defs: Vec<ToolDefinition> = (0..20)
            .map(|i| ToolDefinition {
                name: format!("tool_{i}"),
                description: "d".to_string(),
                parameters: serde_json::json!({"type": "object"}),
            })
            .collect();
        let active = initial_active_tools(&defs, false);
        assert_eq!(active.len(), 20);
        assert!(!active.iter().any(|tool| tool.name == "find_tools"));
    }
}
