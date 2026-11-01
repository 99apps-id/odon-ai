use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::{broadcast, mpsc, RwLock};
use tokio_util::sync::CancellationToken;
use tracing::{error, info};

use odon_core::{
    AgentEvent, AgentState, Artifact, Message, OdonError, OutputActionChip, Result,
};
use odon_provider::{ChatRequest, ModelProvider, StreamEvent, ToolDefinition};
use odon_security::{ApprovalEngine, AuditLogger};
use odon_storage::StorageEngine;
use odon_tools::{ToolContext, ToolRegistry};

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
            providers.get(n).cloned()
        } else {
            let def = self.default_provider.read().await;
            providers.get(&*def).cloned()
        }
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

        // 3. Load Session Conversation History
        let history = self
            .storage
            .list_messages(session_id)
            .unwrap_or_else(|_| vec![user_msg.clone()]);

        // 4. Resolve Provider
        let provider = self
            .get_provider(model_override)
            .await
            .ok_or_else(|| OdonError::Provider {
                provider: "unknown".to_string(),
                message: "No active model provider configured".to_string(),
            })?;

        // 5. Build Tools list
        let tool_schemas = self.tools.schemas();
        let tool_defs: Vec<ToolDefinition> = tool_schemas
            .into_iter()
            .map(|s| ToolDefinition {
                name: s.name,
                description: s.description,
                parameters: s.parameters,
            })
            .collect();

        // 6. Spawn Stream Channel
        let (stream_tx, mut stream_rx) = mpsc::channel::<StreamEvent>(128);

        let chat_req = ChatRequest {
            messages: history,
            model: model_override.unwrap_or("odon-auto").to_string(),
            system_prompt: Some(
                "You are ODON (Open Digital Orchestration Network), an advanced universal AI assistant and autonomous orchestrator. \
                 You provide clean, thoughtful, conversational responses accompanied by high-quality code, documents, and media artifacts. \
                 Never output rigid terminal loop counters. Present your work conversationally with clear structure."
                    .to_string(),
            ),
            tools: tool_defs,
            temperature: Some(0.7),
            max_tokens: Some(4096),
        };

        let prov_clone = provider.clone();
        let cancel_clone = cancellation.clone();
        let prov_task = tokio::spawn(async move {
            prov_clone
                .stream_chat(chat_req, stream_tx, cancel_clone)
                .await
        });

        let mut accumulated_text = String::new();
        let mut accumulated_thought = String::new();
        let mut active_artifacts = Vec::new();
        let mut active_action_chips = Vec::new();

        // 7. Process Stream Events
        while let Some(evt) = stream_rx.recv().await {
            match evt {
                StreamEvent::TextDelta(delta) => {
                    accumulated_text.push_str(&delta);
                    let _ = self.event_tx.send(AgentEvent::TextDelta {
                        session_id: session_id.to_string(),
                        delta,
                    });
                }
                StreamEvent::ThoughtDelta(delta) => {
                    accumulated_thought.push_str(&delta);
                    let _ = self.event_tx.send(AgentEvent::ThoughtDelta {
                        session_id: session_id.to_string(),
                        delta,
                    });
                }
                StreamEvent::ToolCallDelta { name, arguments_delta, .. } => {
                    // Check if a tool needs execution
                    if let (Some(tname), Some(args_str)) = (name, arguments_delta) {
                        let _ = self.event_tx.send(AgentEvent::StateChanged {
                            session_id: session_id.to_string(),
                            state: AgentState::ExecutingTool,
                        });

                        if let Ok(args_val) = serde_json::from_str::<serde_json::Value>(&args_str) {
                            let tool_ctx = ToolContext {
                                session_id: session_id.to_string(),
                                workspace_root: self.workspace_root.clone(),
                                approval_engine: self.approval.clone(),
                                audit_logger: self.audit.clone(),
                            };

                            let tool_res = self.tools.execute_tool(&tname, &tool_ctx, args_val).await;
                            match tool_res {
                                Ok(val) => {
                                    // Check if this tool produced an artifact
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
                                }
                                Err(e) => {
                                    error!("Tool {} execution failed: {}", tname, e);
                                }
                            }
                        }
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

        // 8. Attach Contextual Action Chips (as seen in the mockup)
        active_action_chips.push(OutputActionChip {
            id: "preview_app".to_string(),
            label: "Run the project and preview it".to_string(),
            icon: Some("play".to_string()),
            action_type: "preview_run".to_string(),
            payload: None,
        });
        active_action_chips.push(OutputActionChip {
            id: "git_repo".to_string(),
            label: "Create a Git repository".to_string(),
            icon: Some("github".to_string()),
            action_type: "create_git_repo".to_string(),
            payload: None,
        });
        active_action_chips.push(OutputActionChip {
            id: "turn_automation".to_string(),
            label: "Turn this into an automation".to_string(),
            icon: Some("zap".to_string()),
            action_type: "create_automation".to_string(),
            payload: None,
        });
        active_action_chips.push(OutputActionChip {
            id: "refine_prd".to_string(),
            label: "Refine the PRD".to_string(),
            icon: Some("file-text".to_string()),
            action_type: "refine_prd".to_string(),
            payload: None,
        });

        // 9. Persist Assistant Message
        let thought_opt = if accumulated_thought.is_empty() {
            None
        } else {
            Some(accumulated_thought)
        };

        let mut assistant_msg = Message::new_assistant(
            session_id,
            accumulated_text,
            thought_opt,
            Some(model_override.unwrap_or("odon-auto").to_string()),
        );
        assistant_msg.content.artifact_ids = active_artifacts;
        assistant_msg.content.action_chips = active_action_chips;

        self.storage
            .save_message(&assistant_msg)
            .map_err(|e| OdonError::Storage(e.to_string()))?;

        // 10. Set State: Completed
        let _ = self.event_tx.send(AgentEvent::StateChanged {
            session_id: session_id.to_string(),
            state: AgentState::Completed,
        });

        Ok(assistant_msg)
    }
}
