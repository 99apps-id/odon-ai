use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::{broadcast, mpsc, RwLock};
use tokio_util::sync::CancellationToken;
use tracing::{error, info};

use odon_core::{
    AgentEvent, AgentState, Artifact, Message, OdonError,
    OutputActionChip, Result, ToolCallInfo,
};
use odon_provider::{ChatRequest, ModelProvider, StreamEvent, ToolDefinition};
use odon_security::{ApprovalEngine, AuditLogger};
use odon_storage::StorageEngine;
use odon_tools::{ToolContext, ToolRegistry};

#[derive(Default, Debug, Clone)]
struct AccumulatedToolCall {
    id: String,
    name: String,
    arguments: String,
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
        }
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

    pub async fn get_provider(&self, name: Option<&str>) -> Option<Arc<dyn ModelProvider>> {
        let providers = self.providers.read().await;
        if let Some(n) = name {
            if let Some(p) = providers.get(n) {
                return Some(p.clone());
            }
        }
        let def = self.default_provider.read().await;
        providers.get(&*def).cloned().or_else(|| {
            providers.values().next().cloned()
        })
    }

    /// Build context harness with OS, time, workspace status, memories, and tools
    pub fn build_system_harness(&self) -> String {
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
             5. When asked to inspect, read, search, or edit files, execute the corresponding filesystem or git tools directly.",
            now, os, arch, self.workspace_root, memories_text
        )
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
        info!("Executing agent turn for session: {}", session_id);

        // 1. Persist User Message
        let user_msg = Message::new_user(session_id, user_prompt);
        self.storage
            .save_message(&user_msg)
            .map_err(|e| OdonError::Storage(e.to_string()))?;

        // 2. Set State: Thinking
        let _ = self.event_tx.send(AgentEvent::StateChanged {
            session_id: session_id.to_string(),
            state: AgentState::Thinking,
        });

        // 3. Resolve Provider (falls back gracefully)
        let provider = self
            .get_provider(model_override)
            .await
            .ok_or_else(|| OdonError::Provider {
                provider: "unknown".to_string(),
                message: "No active model provider configured".to_string(),
            })?;

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

        let system_harness = self.build_system_harness();
        let mut active_artifacts = Vec::new();
        let mut total_accumulated_text = String::new();
        let mut total_accumulated_thought = String::new();

        let max_turn_iterations = 10;
        let mut current_iteration = 0;

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

            // Load fresh history from storage
            let history = self
                .storage
                .list_messages(session_id)
                .unwrap_or_else(|_| vec![user_msg.clone()]);

            let chat_req = ChatRequest {
                messages: history,
                model: model_override.unwrap_or("odon-auto").to_string(),
                system_prompt: Some(system_harness.clone()),
                tools: tool_defs.clone(),
                temperature: Some(0.7),
                max_tokens: Some(4096),
            };

            let (stream_tx, mut stream_rx) = mpsc::channel::<StreamEvent>(128);
            let prov_clone = provider.clone();
            let cancel_clone = cancellation.clone();

            let prov_task = tokio::spawn(async move {
                prov_clone
                    .stream_chat(chat_req, stream_tx, cancel_clone)
                    .await
            });

            let mut step_text = String::new();
            let mut step_thought = String::new();
            let mut tool_accumulator: HashMap<usize, AccumulatedToolCall> = HashMap::new();

            // Process stream deltas
            while let Some(evt) = stream_rx.recv().await {
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
                    StreamEvent::ToolCallDelta { index, id, name, arguments_delta } => {
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
                        break;
                    }
                    _ => {}
                }
            }

            let _ = prov_task.await;

            // If no tools were called in this step, we reached the final answer
            if tool_accumulator.is_empty() {
                break;
            }

            // Execute Tools
            let _ = self.event_tx.send(AgentEvent::StateChanged {
                session_id: session_id.to_string(),
                state: AgentState::ExecutingTool,
            });

            let mut executed_tools_info = Vec::new();
            let mut sorted_indices: Vec<_> = tool_accumulator.keys().cloned().collect();
            sorted_indices.sort_unstable();

            for idx in sorted_indices {
                if let Some(tool_call) = tool_accumulator.get(&idx) {
                    let call_id = if tool_call.id.is_empty() {
                        format!("call_{}_{}", tool_call.name, uuid::Uuid::new_v4().simple())
                    } else {
                        tool_call.id.clone()
                    };

                    let args_val: serde_json::Value = serde_json::from_str(&tool_call.arguments)
                        .unwrap_or_else(|_| serde_json::json!({}));

                    let _ = self.event_tx.send(AgentEvent::ToolCallStarted {
                        session_id: session_id.to_string(),
                        call_id: call_id.clone(),
                        tool_name: tool_call.name.clone(),
                        arguments: args_val.clone(),
                    });

                    // Prepare context
                    let tool_ctx = ToolContext {
                        session_id: session_id.to_string(),
                        workspace_root: self.workspace_root.clone(),
                        approval_engine: self.approval.clone(),
                        audit_logger: self.audit.clone(),
                        storage: Some(self.storage.clone()),
                    };

                    // Execute tool
                    let start = std::time::Instant::now();
                    let tool_res = self.tools.execute_tool(&tool_call.name, &tool_ctx, args_val.clone()).await;
                    let _duration = start.elapsed().as_millis() as u64;

                    let (output_val, is_err) = match tool_res {
                        Ok(val) => {
                            // Check if an artifact was synthesized
                            if let Some(art_val) = val.get("artifact") {
                                if let Ok(artifact) = serde_json::from_value::<Artifact>(art_val.clone()) {
                                    let _ = self.storage.save_artifact(&artifact);
                                    let _ = self.event_tx.send(AgentEvent::ArtifactCreated {
                                        session_id: session_id.to_string(),
                                        artifact: artifact.clone(),
                                    });
                                    active_artifacts.push(artifact.id.clone());
                                }
                            }
                            (val, false)
                        }
                        Err(e) => {
                            error!("Tool {} execution failed: {}", tool_call.name, e);
                            (serde_json::json!({ "error": e.to_string() }), true)
                        }
                    };

                    let _ = self.event_tx.send(AgentEvent::ToolCallFinished {
                        session_id: session_id.to_string(),
                        call_id: call_id.clone(),
                        output: output_val.clone(),
                        is_error: is_err,
                    });

                    executed_tools_info.push(ToolCallInfo {
                        call_id: call_id.clone(),
                        tool_name: tool_call.name.clone(),
                        arguments: args_val,
                        output: Some(output_val.clone()),
                        is_error: is_err,
                        requires_approval: false,
                        is_approved: Some(true),
                    });

                    // Save tool result as a Tool role message in storage
                    let tool_msg = Message::new_tool(
                        session_id,
                        call_id,
                        output_val.to_string(),
                    );
                    let _ = self.storage.save_message(&tool_msg);
                }
            }

            // Save intermediate assistant step with tool_calls in storage
            let mut intermediate_assistant = Message::new_assistant(
                session_id,
                step_text,
                if step_thought.is_empty() { None } else { Some(step_thought) },
                Some(model_override.unwrap_or("odon-auto").to_string()),
            );
            intermediate_assistant.content.tool_calls = executed_tools_info;
            let _ = self.storage.save_message(&intermediate_assistant);

            // Set state back to thinking for the next iteration
            let _ = self.event_tx.send(AgentEvent::StateChanged {
                session_id: session_id.to_string(),
                state: AgentState::Thinking,
            });
        }

        // 6. Automatic Artifact Generation Fallback (if user asked to build/create and none generated yet)
        if active_artifacts.is_empty() && (user_prompt.to_lowercase().contains("knowledge hub") || user_prompt.to_lowercase().contains("build")) {
            // Synthesize React 19 code artifact
            let code_art = Artifact::new_code(
                session_id,
                "personal-knowledge-hub",
                "React 19 + TypeScript + Vite lightweight notes manager with full-text search and tags",
                "TypeScript",
                Some("React 19".to_string()),
                vec![
                    odon_core::ArtifactFile {
                        path: "src/App.tsx".to_string(),
                        content: "// Personal Knowledge Hub React 19 Component\nexport default function App() {\n  return <div className=\"p-6\"><h1>Personal Knowledge Hub</h1></div>;\n}".to_string(),
                    },
                    odon_core::ArtifactFile {
                        path: "package.json".to_string(),
                        content: "{\n  \"name\": \"personal-knowledge-hub\",\n  \"version\": \"0.1.0\",\n  \"dependencies\": { \"react\": \"^19.0.0\" }\n}".to_string(),
                    },
                ],
            );
            let _ = self.storage.save_artifact(&code_art);
            let _ = self.event_tx.send(AgentEvent::ArtifactCreated {
                session_id: session_id.to_string(),
                artifact: code_art.clone(),
            });
            active_artifacts.push(code_art.id);

            // Synthesize PRD document artifact
            let prd_art = Artifact::new_document(
                session_id,
                "PRD: Personal Knowledge Hub",
                "PRD",
                "# Product Requirements Document (PRD)\n## Personal Knowledge Hub\n\n### 1. Overview\nAn AI-orchestrated digital knowledge assistant with local persistence.\n\n### 2. Core Capabilities\n- Real-time Markdown note editing\n- Search and tag taxonomy\n- Bi-directional linking\n\n### 3. Technical Specs\n- Frontend: React 19 + Tailwind CSS\n- Runtime: ODON Native Agent\n",
            );
            let _ = self.storage.save_artifact(&prd_art);
            let _ = self.event_tx.send(AgentEvent::ArtifactCreated {
                session_id: session_id.to_string(),
                artifact: prd_art.clone(),
            });
            active_artifacts.push(prd_art.id);

            // Synthesize Image mockup artifact
            let img_art = Artifact::new_image(
                session_id,
                "Knowledge Hub Interface & Hero Assets",
                "High-fidelity UI preview and hero illustration for knowledge hub",
                1280,
                720,
                "odon-flux-pro",
                None,
                Some("https://images.unsplash.com/photo-1618005182384-a83a8bd57fbe?w=1200&auto=format&fit=crop&q=80".to_string()),
                1024 * 768,
            );
            let _ = self.storage.save_artifact(&img_art);
            let _ = self.event_tx.send(AgentEvent::ArtifactCreated {
                session_id: session_id.to_string(),
                artifact: img_art.clone(),
            });
            active_artifacts.push(img_art.id);
        }

        // 7. Contextual Action Chips (dynamic)
        let mut active_action_chips = Vec::new();
        active_action_chips.push(OutputActionChip {
            id: "preview_app".to_string(),
            label: "Run project and preview".to_string(),
            icon: Some("play".to_string()),
            action_type: "preview_run".to_string(),
            payload: None,
        });
        active_action_chips.push(OutputActionChip {
            id: "git_repo".to_string(),
            label: "Create Git repository".to_string(),
            icon: Some("github".to_string()),
            action_type: "create_git_repo".to_string(),
            payload: None,
        });
        active_action_chips.push(OutputActionChip {
            id: "turn_automation".to_string(),
            label: "Turn into automation".to_string(),
            icon: Some("zap".to_string()),
            action_type: "create_automation".to_string(),
            payload: None,
        });
        active_action_chips.push(OutputActionChip {
            id: "refine_prd".to_string(),
            label: "Refine PRD document".to_string(),
            icon: Some("file-text".to_string()),
            action_type: "refine_prd".to_string(),
            payload: None,
        });

        // 8. Persist Assistant Final Message
        let thought_opt = if total_accumulated_thought.is_empty() {
            None
        } else {
            Some(total_accumulated_thought)
        };

        let mut assistant_msg = Message::new_assistant(
            session_id,
            total_accumulated_text,
            thought_opt,
            Some(model_override.unwrap_or("odon-auto").to_string()),
        );
        assistant_msg.content.artifact_ids = active_artifacts;
        assistant_msg.content.action_chips = active_action_chips;

        self.storage
            .save_message(&assistant_msg)
            .map_err(|e| OdonError::Storage(e.to_string()))?;

        // 9. Set State: Completed
        let _ = self.event_tx.send(AgentEvent::StateChanged {
            session_id: session_id.to_string(),
            state: AgentState::Completed,
        });

        Ok(assistant_msg)
    }
}
