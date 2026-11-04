use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio::sync::{mpsc, RwLock};
use tokio::task::JoinSet;
use tokio_util::sync::CancellationToken;
use tracing::{error, info};

use odon_core::{
    AgentEvent, AgentState, Artifact, EventSender, Message, MessageAttachment, MessageRole,
    OdonError, Result, Session, SessionMode, ToolCallInfo, ACTIVE_SESSION_ID,
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
/// `0` means no cap at all (see [`UNLIMITED_TURN_ITERATIONS`]).
pub const DEFAULT_MAX_TURN_ITERATIONS: usize = 500;

/// How many times one turn may automatically resume after the provider reports
/// that the output budget cut the answer.
pub const MAX_TRUNCATION_CONTINUATIONS: usize = 4;

/// Per-turn step cap for a provider that bills neither per step nor per
/// token (OAuth / keyless: ChatGPT codex, Claude OAuth, Antigravity, xAI
/// OAuth, Copilot, Muse). Mirrors Termigo's `OAUTH_MAX_AGENT_STEPS` of
/// 10_000: for those providers the real bound is the provider's own usage
/// window, not a step count this runtime invents.
pub const OAUTH_MAX_TURN_ITERATIONS: usize = 10_000;

/// Largest subagent report handed back to the parent as a tool result.
/// The child's own session keeps the full transcript in storage, so
/// nothing is lost: the parent gets a bounded summary it can act on
/// instead of an unbounded blob that would crowd out its own context
/// (and stall the chat renderer).
pub const SUBAGENT_RESULT_MAX_CHARS: usize = 4_000;

/// Sentinel stored when the step cap is disabled. The loop compares
/// `current_iteration <= cap`, so this is "run until the model stops
/// asking for tools or the turn is cancelled".
pub const UNLIMITED_TURN_ITERATIONS: usize = usize::MAX;

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
const SUBAGENT_MAX_ITERATIONS: usize = 5_000;
const MAX_CONCURRENT_SUBAGENTS: usize = 4;

/// Minimum gap between two `SubagentProgress` updates mirrored to the parent.
const PROGRESS_THROTTLE: Duration = Duration::from_millis(400);

/// Aborts the progress-mirror task when the delegation ends, on every path.
struct MirrorGuard(Option<tokio::task::JoinHandle<()>>);

impl Drop for MirrorGuard {
    fn drop(&mut self) {
        if let Some(handle) = self.0.take() {
            handle.abort();
        }
    }
}

/// True when `provider_name` identifies a subscription / keyless provider
/// whose steps cost no metered tokens. Matched by substring because ODON
/// providers carry their name either as a fixed label or as the configured
/// provider id (for example `claude-oauth`, `github-copilot`).
pub fn is_unmetered_provider(provider_name: &str) -> bool {
    const MARKERS: [&str; 7] = [
        "antigravity",
        "openai-codex",
        "chatgpt",
        "claude-oauth",
        "xai-oauth",
        "github-copilot",
        "copilot",
    ];
    let name = provider_name.trim().to_ascii_lowercase();
    MARKERS.iter().any(|marker| name.contains(marker)) || name.contains("muse")
}

/// The step cap a turn gets when the operator set no explicit override.
/// Termigo's rule: one explicit number per model, never a hidden ladder.
pub fn default_step_cap_for(provider_name: &str) -> usize {
    if is_unmetered_provider(provider_name) {
        OAUTH_MAX_TURN_ITERATIONS
    } else {
        DEFAULT_MAX_TURN_ITERATIONS
    }
}

fn env_usize(name: &str, default: usize, min: usize, max: usize) -> usize {
    std::env::var(name)
        .ok()
        .and_then(|value| value.trim().parse::<usize>().ok())
        .filter(|value| *value >= min && *value <= max)
        .unwrap_or(default)
}

/// `0` disables the per-turn step cap, anything else is clamped to 1..=10_000.
pub fn normalize_step_cap(value: usize) -> usize {
    if value == 0 {
        UNLIMITED_TURN_ITERATIONS
    } else {
        value.clamp(1, 10_000)
    }
}

/// Which provider serves a bare model id, and how specific that provider is.
///
/// `catalogue` is the number of models the provider lists; the smallest wins so
/// a dedicated provider outranks a gateway that re-exports the same id.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ModelRoute {
    pub provider: String,
    pub catalogue: usize,
}

pub struct AgentRuntime {
    storage: Arc<StorageEngine>,
    tools: Arc<ToolRegistry>,
    approval: Arc<ApprovalEngine>,
    audit: Arc<AuditLogger>,
    providers: Arc<RwLock<HashMap<String, Arc<dyn ModelProvider>>>>,
    default_provider: Arc<RwLock<String>>,
    /// Model id → provider id. Lets a bare model name (an automation or a relay
    /// message) reach the provider that actually lists it, instead of whatever
    /// provider happens to hold the `default` slot.
    model_routes: Arc<RwLock<HashMap<String, ModelRoute>>>,
    event_tx: EventSender,
    workspace_root: String,
    max_turn_iterations: AtomicUsize,
    /// True once the operator pinned a cap (env var or
    /// [`AgentRuntime::set_max_turn_iterations`]). While false the cap is
    /// resolved per provider by [`default_step_cap_for`].
    step_cap_explicit: AtomicBool,
    context_budget_tokens: AtomicUsize,
}

impl AgentRuntime {
    pub fn new(
        storage: Arc<StorageEngine>,
        tools: Arc<ToolRegistry>,
        approval: Arc<ApprovalEngine>,
        audit: Arc<AuditLogger>,
        event_tx: EventSender,
        workspace_root: String,
    ) -> Self {
        Self {
            storage,
            tools,
            approval,
            audit,
            providers: Arc::new(RwLock::new(HashMap::new())),
            default_provider: Arc::new(RwLock::new("default".to_string())),
            model_routes: Arc::new(RwLock::new(HashMap::new())),
            event_tx,
            workspace_root,
            max_turn_iterations: AtomicUsize::new(normalize_step_cap(env_usize(
                "ODON_MAX_TURN_ITERATIONS",
                DEFAULT_MAX_TURN_ITERATIONS,
                0,
                10_000,
            ))),
            step_cap_explicit: AtomicBool::new(std::env::var("ODON_MAX_TURN_ITERATIONS").is_ok()),
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

    /// Sets the per-turn step cap. `0` removes the cap entirely; other
    /// values are clamped to 1..=10000.
    pub fn set_max_turn_iterations(&self, value: usize) {
        self.step_cap_explicit.store(true, Ordering::Relaxed);
        self.max_turn_iterations
            .store(normalize_step_cap(value), Ordering::Relaxed);
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

    pub fn event_sender(&self) -> EventSender {
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

    /// Replace the model → provider index. One provider may claim many models.
    pub async fn set_model_routes(&self, routes: HashMap<String, ModelRoute>) {
        *self.model_routes.write().await = routes;
    }

    /// Record that `provider` serves `model`.
    ///
    /// `catalogue` is how many models that provider offers. When two providers
    /// claim the same bare model id, the narrower catalogue wins: a real
    /// provider listing two dozen models is a better answer than a gateway that
    /// re-exports thousands of other vendors' ids. Without this, whichever
    /// provider registered last silently captured every shared id, which sent
    /// `deepseek-chat` to a gateway instead of to DeepSeek.
    pub async fn register_model_route(&self, model: &str, provider: &str, catalogue: usize) {
        let model = model.trim();
        if model.is_empty() || model == "odon-auto" {
            return;
        }
        let mut routes = self.model_routes.write().await;
        match routes.get(model) {
            Some(existing) if existing.catalogue <= catalogue => {}
            _ => {
                routes.insert(
                    model.to_string(),
                    ModelRoute {
                        provider: provider.to_string(),
                        catalogue,
                    },
                );
            }
        }
    }

    /// The provider that lists this exact model, if any.
    pub async fn provider_for_model(&self, model: &str) -> Option<String> {
        self.model_routes
            .read()
            .await
            .get(model.trim())
            .map(|route| route.provider.clone())
    }

    /// A model this provider actually lists, used to turn `odon-auto` into a
    /// concrete id. A provider that receives the literal `odon-auto` answers
    /// "no model selected", which is what automations used to hit.
    pub async fn first_model_for_provider(&self, provider: &str) -> Option<String> {
        let routes = self.model_routes.read().await;
        routes
            .iter()
            .filter(|(_, route)| route.provider == provider)
            .min_by(|a, b| a.0.cmp(b.0))
            .map(|(model, _)| model.clone())
    }

    /// Drop every model route that pointed at a removed provider.
    pub async fn remove_model_routes_for_provider(&self, provider: &str) {
        self.model_routes
            .write()
            .await
            .retain(|_, route| route.provider != provider);
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

    /// Extract the upstream status from a provider error such as
    /// `HTTP 400 from https://...`. Deterministic where free-text matching is not.
    fn http_status_of(lower: &str) -> Option<u16> {
        let idx = lower.find("http ")?;
        let digits: String = lower[idx + 5..]
            .chars()
            .take_while(|c| c.is_ascii_digit())
            .collect();
        digits.parse().ok()
    }

    /// Map a provider failure onto a stable lesson kind plus the action that
    /// avoids it. Deterministic so the same failure overwrites one memory row
    /// instead of filling the store with near-duplicates.
    fn lesson_kind(message: &str) -> (&'static str, &'static str) {
        let lower = message.to_ascii_lowercase();
        let status = Self::http_status_of(&lower);
        let mentions_model = lower.contains("model");

        if lower.contains("no model selected") {
            return (
                "model_selection",
                "pick provider and model together; a gateway has no default model, so the request must name a real id from its catalogue",
            );
        }
        if lower.contains("context") && lower.contains("length") {
            return (
                "context_limit",
                "trim the prompt or let older messages fall out of the window before resending",
            );
        }
        // Status first: a 400 listing the supported models says nothing about
        // credentials, so a generic keyword match must not claim "auth".
        match status {
            Some(401) | Some(403) => {
                return (
                    "auth",
                    "the credential was rejected; ask the operator to re-check Settings instead of retrying the same call",
                )
            }
            Some(429) => {
                return (
                    "rate_limit",
                    "back off and prefer another configured provider or a cheaper model before retrying",
                )
            }
            Some(400) | Some(404) if mentions_model => {
                return (
                    "model_not_found",
                    "the model id is unknown to this endpoint; choose one from the provider catalogue",
                )
            }
            _ => {}
        }
        if lower.contains("not configured") {
            return (
                "provider_config",
                "the provider is missing a key or base URL; configure it in Settings before sending",
            );
        }
        if lower.contains("timeout") || lower.contains("timed out") {
            return (
                "timeout",
                "retry once, then reduce the request size; identical repeated retries only burn budget",
            );
        }
        if lower.contains("rate limit") || lower.contains("quota") {
            return (
                "rate_limit",
                "back off and prefer another configured provider or a cheaper model before retrying",
            );
        }
        if lower.contains("unauthorized") || lower.contains("invalid api key") {
            return (
                "auth",
                "the credential was rejected; ask the operator to re-check Settings instead of retrying the same call",
            );
        }
        if lower.contains("not found") {
            return (
                "model_not_found",
                "the model id is unknown to this endpoint; choose one from the provider catalogue",
            );
        }
        (
            "provider_error",
            "re-read the error text before retrying; do not resend an identical request",
        )
    }

    /// Persist one lesson per (provider, kind) so a later turn starts from it.
    fn record_lesson(&self, provider: &str, model: &str, message: &str) {
        let (kind, hint) = Self::lesson_kind(message);
        let detail: String = message.chars().take(180).collect();
        let value = format!(
            "{} with provider `{}` on model `{}`: {}. Next time: {}.",
            kind, provider, model, detail, hint
        );
        let key = format!("lesson:{}:{}", provider, kind);
        let _ = self.storage.set_memory(&key, &value, "lessons");
    }

    /// Build context harness with OS, time, workspace status, memories, and tools
    pub fn build_system_harness(&self, workspace_root: &str) -> String {
        let now = chrono::Utc::now().to_rfc3339();
        let os = std::env::consts::OS;
        let arch = std::env::consts::ARCH;

        let memories = self.storage.list_user_memories(10).unwrap_or_default();
        let mut memories_text = String::new();
        if !memories.is_empty() {
            memories_text.push_str("\n### Known User Preferences & Project Memory:\n");
            for m in memories.iter() {
                // Facts are stored content-addressed, so the key is a hash and
                // only the value carries meaning for the model.
                if m.category == "facts" {
                    memories_text.push_str(&format!("- {}\n", m.value));
                } else {
                    memories_text.push_str(&format!("- [{}] {}: {}\n", m.category, m.key, m.value));
                }
            }
        }

        // Lessons are separate from memory: they describe a request shape that
        // already failed, so the model can avoid repeating it this turn.
        let lessons = self
            .storage
            .list_memories(Some("lessons"))
            .unwrap_or_default();
        if !lessons.is_empty() {
            memories_text.push_str("\n### Lessons From Earlier Turns (do not repeat these):\n");
            for lesson in lessons.iter().take(5) {
                memories_text.push_str(&format!("- {}\n", lesson.value));
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

    /// Same as [`execute_turn_controlled`] but lets the caller bound this turn's
    /// step budget. The composer's execution mode uses it: an Agent turn may run
    /// longer than a plain Chat turn.
    #[allow(clippy::too_many_arguments)]
    pub async fn execute_turn_with_cap(
        &self,
        session_id: &str,
        user_prompt: &str,
        attachments: Vec<MessageAttachment>,
        model_override: Option<&str>,
        effort: Option<&str>,
        cancellation: CancellationToken,
        steer: Option<SteerQueue>,
        extra_system_prompt: Option<&str>,
        iteration_cap: Option<usize>,
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
            iteration_cap,
        )
        .await
    }

    /// Enters a turn with `session_id` bound to the current task.
    ///
    /// Binding here, once, instead of at each event callsite is what stamps a
    /// session id onto every event the turn publishes: the bus reads the
    /// task-local value when it sends. A delegated child turn re-enters this
    /// scope with its own session id, so nesting stays correct.
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
        ACTIVE_SESSION_ID
            .scope(
                session_id.to_string(),
                self.execute_turn_body(
                    session_id,
                    user_prompt,
                    attachments,
                    model_override,
                    effort,
                    cancellation,
                    steer,
                    extra_system_prompt,
                    depth,
                    iteration_cap,
                ),
            )
            .await
    }

    /// The turn loop. `depth` counts subagent nesting (a delegated run may
    /// not delegate further), and `iteration_cap` overrides the configured
    /// step cap for bounded child runs. Always entered through
    /// [`AgentRuntime::execute_turn_inner`], which supplies the session scope.
    #[allow(clippy::too_many_arguments)]
    #[async_recursion::async_recursion]
    async fn execute_turn_body(
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
        let (explicit_provider, requested_model) = model_override
            .and_then(|value| value.split_once("::"))
            .map(|(provider, model)| (Some(provider.to_string()), model))
            .unwrap_or((None, model_override.unwrap_or("odon-auto")));
        // A bare model id resolves to the provider that lists it, so an
        // automation or relay message cannot land on an unrelated default.
        let routed_provider = match explicit_provider {
            Some(name) => Some(name),
            None => self.provider_for_model(requested_model).await,
        };
        // `odon-auto` is a sentinel, not a model id. Turning it into a concrete
        // id here keeps providers from answering "no model selected", which is
        // what an automation with Auto used to hit.
        let requested_model: String = if requested_model == "odon-auto" {
            let owner = match routed_provider.clone() {
                Some(name) => Some(name),
                None => self.get_provider(None).await.map(|p| p.provider_name().to_string()),
            };
            match owner {
                Some(name) => self
                    .first_model_for_provider(&name)
                    .await
                    .unwrap_or_else(|| requested_model.to_string()),
                None => requested_model.to_string(),
            }
        } else {
            requested_model.to_string()
        };
        let requested_model = requested_model.as_str();
        let routed_label = routed_provider.as_deref().unwrap_or("default").to_string();
        // Kept for the failure lesson, which must name the model that was refused.
        let model_for_lesson = requested_model.to_string();
        let provider = match self.get_provider(routed_provider.as_deref()).await {
            Some(p) => {
                if p.provider_name() == "odon-simulated" && requested_model != "odon-simulated" {
                    return Err(OdonError::Provider {
                        provider: routed_label,
                        message: "Mock provider is disabled. Please configure your API key in Settings (BYOK) or sign in with GitHub Copilot / Antigravity to chat with real AI models.".to_string(),
                    });
                }
                p
            }
            None => {
                return Err(OdonError::Provider {
                    provider: routed_label.clone(),
                    message: format!(
                        "Provider '{}' is not configured or missing an API key. Please open Settings and configure your credentials to enable real AI responses.",
                        routed_label
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

        // Explicit caps win: a caller (a subagent budget) or the operator
        // (env / setter). Otherwise the cap follows the provider, because a
        // subscription provider costs no metered tokens per step while a
        // BYOK one does.
        let max_turn_iterations = iteration_cap
            .unwrap_or_else(|| {
                if self.step_cap_explicit.load(Ordering::Relaxed) {
                    self.max_turn_iterations()
                } else {
                    default_step_cap_for(provider.provider_name())
                }
            })
            .max(1);
        let mut current_iteration = 0;
        let mut hit_step_cap = false;
        // One closing model call after the cap. The cap exists to bound
        // autonomous work, not to cut the operator off mid-sentence: without
        // this the turn ends on a truncated transcript with tool calls whose
        // results the model never got to read.
        let mut grace_used = false;
        // Bounded auto-continuation when the provider reports that the output
        // budget cut the answer. Hermes does the same and de-duplicates the
        // repeated seam; without a bound, a model that always truncates would
        // loop until the step cap.
        let mut truncation_continuations = 0usize;
        let mut truncation_seam: Option<String> = None;

        // A step cap bounds the count, never the clock: a model that keeps
        // retrying a tool it believes is failing looks perfectly healthy at
        // every step, so with the OAuth cap at 10_000 one wedged turn spins
        // for hours and the operator sees a hang. Bound the wall clock too.
        let turn_budget = std::env::var("ODON_TURN_TIMEOUT_SECS")
            .ok()
            .and_then(|value| value.trim().parse::<u64>().ok())
            .unwrap_or(1800);
        let turn_budget = (turn_budget > 0).then(|| Duration::from_secs(turn_budget));
        let turn_started = Instant::now();

        // Identical calls carry no new information: the model already saw that
        // answer. Left unbounded it retries the same search or read forever,
        // which is the usual shape of a "stuck" turn.
        const MAX_IDENTICAL_CALLS: u32 = 3;
        // A stubborn model can answer a blocked repeat with the same call
        // again. Past this many blocked repeats the turn is not progressing,
        // so end it with an explicit reason instead of burning the step cap.
        const MAX_BLOCKED_REPEATS: u32 = 6;
        let mut blocked_repeats: u32 = 0;
        let mut repeat_counts: std::collections::HashMap<String, u32> =
            std::collections::HashMap::new();

        // 5. Multi-Step Agentic Loop (ReAct / Tool-Use Loop)
        // The counter is bumped before the pass, so `<= cap` runs exactly
        // `cap + 1` passes: `cap` working steps plus the single closing call.
        // The closing call is the pass whose bumped counter is `cap + 1`, so it
        // cannot repeat. A cap of `usize::MAX` (0 = unlimited) makes the bound
        // unreachable, which is what "no cap" has to mean.
        while current_iteration <= max_turn_iterations {
            if let Some(budget) = turn_budget {
                if turn_started.elapsed() > budget {
                    let message = format!(
                        "turn exceeded its {}s budget after {} step(s); raise ODON_TURN_TIMEOUT_SECS for a longer run",
                        budget.as_secs(),
                        current_iteration
                    );
                    let _ = self.event_tx.send(AgentEvent::StateChanged {
                        session_id: session_id.to_string(),
                        state: AgentState::Failed,
                    });
                    return Err(OdonError::Internal(message));
                }
            }
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

            // At the cap the model gets exactly one closing call with no tools:
            // the work it already did stays persisted, and the operator gets a
            // real answer instead of a truncated step.
            let grace_call = current_iteration > max_turn_iterations;
            if grace_call {
                grace_used = true;
            }
            let chat_req = ChatRequest {
                messages: history,
                model: requested_model.to_string(),
                system_prompt: Some(system_harness.clone()),
                tools: if grace_call {
                    Vec::new()
                } else {
                    active_tools.clone()
                },
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
            let mut output_truncated = false;

            // Process stream deltas. Steer messages are deliberately observed
            // at iteration boundaries, never by cancelling this stream: the
            // active task must finish before the guidance is applied.

            // A provider that stops producing events without closing the stream
            // leaves recv() pending forever, because the sender still lives
            // inside prov_task. The turn then never finishes and the UI sits in
            // "running" with no error. Bound the wait so a stall surfaces.
            let stream_idle_timeout = std::time::Duration::from_secs(180);
            loop {
                let next = tokio::time::timeout(stream_idle_timeout, stream_rx.recv()).await;
                let evt = match next {
                    Ok(evt) => evt,
                    Err(_) => {
                        iteration_cancel.cancel();
                        stream_error = Some(format!(
                            "model stream sent nothing for {}s; aborted",
                            stream_idle_timeout.as_secs()
                        ));
                        None
                    }
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
                    StreamEvent::OutputTruncated => {
                        output_truncated = true;
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

            // Backstop: a provider that ignores cancellation must not be able
            // to hold the turn open indefinitely.
            match tokio::time::timeout(stream_idle_timeout, prov_task).await {
                Ok(Ok(Ok(()))) => {}
                Ok(Ok(Err(err))) => {
                    // Unwrap the provider's message so the caller does not see
                    // "Provider error (x): Provider error (x): ...".
                    stream_error = Some(match err {
                        OdonError::Provider { message, .. } => message,
                        other => other.to_string(),
                    });
                }
                Ok(Err(err)) => stream_error = Some(format!("Provider task failed: {err}")),
                Err(_) => {
                    stream_error.get_or_insert_with(|| {
                        "provider task did not exit after its stream stalled".to_string()
                    });
                }
            }

            // A step that resumed a truncated answer usually repeats the tail of
            // the previous one. Drop that repeated seam here, before any branch
            // reads the text, so both the persisted history and the accumulated
            // transcript stay free of the duplicate.
            if let Some(previous) = truncation_seam.take() {
                let overlap =
                    crate::continuation::continuation_overlap_length(&previous, &step_text);
                if overlap > 0 && overlap <= step_text.len() {
                    let keep = total_accumulated_text.len().saturating_sub(step_text.len());
                    total_accumulated_text.truncate(keep);
                    if let Some(remainder) = step_text.get(overlap..) {
                        total_accumulated_text.push_str(remainder);
                        step_text = remainder.to_string();
                    }
                }
            }

            if let Some(message) = stream_error {
                let _ = self.event_tx.send(AgentEvent::StateChanged {
                    session_id: session_id.to_string(),
                    state: AgentState::Failed,
                });
                // Learn from the failure so the next turn (in any session) starts
                // from it instead of repeating the same request shape.
                self.record_lesson(provider.provider_name(), &model_for_lesson, &message);
                return Err(OdonError::Provider {
                    provider: provider.provider_name().to_string(),
                    message,
                });
            }

            // If no tools were called in this step, we reached the final answer.
            // If a steer is waiting, run another iteration so it is not lost.
            if tool_accumulator.is_empty() {
                let pending_steer = steer
                    .as_ref()
                    .map(|queue| queue.has_pending())
                    .unwrap_or(false);
                // A steer arriving with the final answer still deserves one
                // follow-up iteration; the turn budget is a safety cap for
                // autonomous work, not permission to drop user guidance.
                if pending_steer && !grace_call {
                    continue;
                }
                // The output budget ran out mid-answer. Without this the cut
                // answer is persisted as if it were complete. Ask the model to
                // resume and let it run again, bounded so a model that always
                // truncates cannot spin. The seam is de-duplicated when the next
                // step's text is persisted.
                if output_truncated
                    && !step_text.trim().is_empty()
                    && !grace_call
                    && truncation_continuations < MAX_TRUNCATION_CONTINUATIONS
                {
                    truncation_continuations += 1;
                    truncation_seam = Some(step_text.clone());
                    let nudge =
                        Message::new_user(session_id, crate::continuation::OUTPUT_LIMIT_NUDGE);
                    let _ = self.storage.save_message(&nudge);
                    let _ = self.event_tx.send(AgentEvent::StateChanged {
                        session_id: session_id.to_string(),
                        state: AgentState::Thinking,
                    });
                    continue;
                }
                // The closing call is the last model turn: mark the cap so the
                // transcript stays honest and the session remains resumable.
                if grace_call {
                    hit_step_cap = true;
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
            let mut executed: Vec<ExecutedCall> = Vec::new();
            for idx in sorted_indices {
                if let Some(tool_call) = tool_accumulator.get(&idx) {
                    let call_id = if tool_call.id.is_empty() {
                        format!("call_{}_{}", tool_call.name, uuid::Uuid::new_v4().simple())
                    } else {
                        tool_call.id.clone()
                    };
                    // Do not silently substitute `{}` for malformed streamed
                    // arguments: surface the parse failure to the model as a
                    // tool error instead of executing with empty defaults.
                    match serde_json::from_str::<serde_json::Value>(&tool_call.arguments) {
                        Ok(args_val) => {
                            let signature =
                                format!("{}\u{0}{}", tool_call.name, tool_call.arguments);
                            let seen = repeat_counts
                                .entry(signature)
                                .and_modify(|count| *count += 1)
                                .or_insert(1);
                            if *seen > MAX_IDENTICAL_CALLS {
                                blocked_repeats += 1;
                                executed.push(ExecutedCall {
                                    index: idx,
                                    call_id,
                                    name: tool_call.name.clone(),
                                    args: args_val,
                                    output: serde_json::json!({
                                        "error": format!(
                                            "this exact call was already made {} times with an identical result; it will not be run again. Change the arguments, use a different tool, or answer with what is already known.",
                                            MAX_IDENTICAL_CALLS
                                        )
                                    }),
                                    is_error: true,
                                });
                            } else {
                                pending.push(PendingCall {
                                    index: idx,
                                    call_id,
                                    name: tool_call.name.clone(),
                                    args: args_val,
                                });
                            }
                        }
                        Err(e) => executed.push(ExecutedCall {
                            index: idx,
                            call_id,
                            name: tool_call.name.clone(),
                            args: serde_json::Value::Null,
                            output: serde_json::json!({
                                "error": format!("tool arguments were not valid JSON: {e}")
                            }),
                            is_error: true,
                        }),
                    }
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
                                routed_provider.as_deref(),
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
                            // Bound each tool call. Without a ceiling, any block
                            // inside a tool - a child process holding an inherited
                            // pipe, a wedged HTTP client - leaves the turn waiting
                            // with no error while the UI keeps spinning. Generous
                            // by default, overridable for long builds.
                            let tool_timeout = std::time::Duration::from_secs(
                                std::env::var("ODON_TOOL_TIMEOUT_SECS")
                                    .ok()
                                    .and_then(|value| value.trim().parse().ok())
                                    .unwrap_or(600),
                            );
                            let outcome = match tokio::time::timeout(
                                tool_timeout,
                                tools.execute_tool(&call.name, &tool_ctx, call.args.clone()),
                            )
                            .await
                            {
                                Ok(result) => result,
                                Err(_) => Err(OdonError::Tool {
                                    tool: call.name.clone(),
                                    message: format!(
                                        "returned nothing after {}s",
                                        tool_timeout.as_secs()
                                    ),
                                }),
                            };
                            let (output, is_error) = match outcome {
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

            // A model that keeps repeating a blocked call is not progressing.
            // End the turn with a reason instead of spinning to the step cap.
            if blocked_repeats >= MAX_BLOCKED_REPEATS {
                let message = format!(
                    "turn ended after {} repeated identical tool calls made no progress; change the query or tool instead of retrying the same call",
                    blocked_repeats
                );
                let _ = self.event_tx.send(AgentEvent::StateChanged {
                    session_id: session_id.to_string(),
                    state: AgentState::Failed,
                });
                return Err(OdonError::Internal(message));
            }
        }

        // 6. Persist Assistant Final Message. A turn that hit the step
        // cap with tool work still in flight pauses instead of
        // completing: everything is persisted, the reply says so, and a
        // follow-up message resumes the same session from that history.
        if hit_step_cap {
            if !total_accumulated_text.is_empty() {
                total_accumulated_text.push_str("\n\n");
            }
            if grace_used {
                total_accumulated_text.push_str(&format!(
                    "[Step cap of {max_turn_iterations} reached; the closing answer above was written without tools. Send \"continue\" to resume this session with tools.]"
                ));
            } else {
                total_accumulated_text.push_str(&format!(
                    "[Turn paused after {max_turn_iterations} steps with work still in progress. Send \"continue\" to resume this session.]"
                ));
            }
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

        // A successful turn is the agent's completion boundary. Models can
        // forget the final todo_update after delivering the answer, so close
        // any remaining plan items here. Paused turns must remain resumable.
        if !hit_step_cap {
            let todos = self
                .storage
                .get_todos(session_id)
                .map_err(|e| OdonError::Storage(e.to_string()))?;
            if let Some(items) = todos.as_array() {
                let mut finalized = items.clone();
                let mut changed = false;
                for item in &mut finalized {
                    if item.get("status").and_then(|status| status.as_str()) != Some("completed") {
                        item["status"] = serde_json::Value::String("completed".to_string());
                        changed = true;
                    }
                }
                if changed {
                    self.storage
                        .set_todos(session_id, &serde_json::Value::Array(finalized))
                        .map_err(|e| OdonError::Storage(e.to_string()))?;
                }
            }
        }

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
        parent_provider: Option<&str>,
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
        // A subagent must keep the provider its parent was actually using.
        // The parent's model arrives here already stripped of its
        // `provider::` prefix, so without this the child would re-resolve the
        // provider from the model id alone and could land on a different (for
        // example, usage-capped) provider.
        let provider_override = args
            .get("provider")
            .and_then(|v| v.as_str())
            .filter(|v| !v.trim().is_empty())
            .map(str::trim)
            .or(parent_provider);
        let requested_model = args
            .get("model")
            .and_then(|v| v.as_str())
            .filter(|v| !v.trim().is_empty())
            .map(str::trim);
        let parent_model_override = self.resolve_model_for_session(parent_session_id);

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
            if let Ok(parent) = self.storage.get_session(parent_session_id) {
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
        let subagent_slots = Arc::new(tokio::sync::Semaphore::new(MAX_CONCURRENT_SUBAGENTS));
        let storage = self.storage.clone();
        let approval = self.approval.clone();
        let audit = self.audit.clone();

        for (index, child_session_id) in child_ids.iter().cloned().enumerate() {
            let task = tasks[index].clone();
            let storage = storage.clone();
            let approval = approval.clone();
            let _audit = audit.clone();
            let cancellation_child = cancellation.child_token();
            let parent_session_id = parent_session_id.to_string();
            let fallback_workspace = self.workspace_root.clone();
            let child_runtime = self.child_runtime(self.workspace_root.clone());
            child_runtime.set_max_turn_iterations(SUBAGENT_MAX_ITERATIONS);
            let subagent_slot = Arc::clone(&subagent_slots);
            let provider_name = provider_override.map(|v| v.to_string());
            let model_override = requested_model
                .or(parent_model_override.as_deref())
                .map(|v| v.to_string())
                .unwrap_or_else(|| model.to_string());

            join_set.spawn(async move {
                AgentRuntime::publish_child_event(
                    &approval,
                    &child_session_id,
                    AgentState::Thinking,
                    &task,
                    &parent_session_id,
                );
                AgentRuntime::publish_subagent_progress(
                    &approval,
                    &parent_session_id,
                    &child_session_id,
                    "started",
                    truncate_chars(&task, 120),
                )
                .await;
                // Mirror the child's activity onto the parent for as long as it
                // runs. The guard aborts it on every exit path, including the
                // early return below.
                let _mirror = MirrorGuard(AgentRuntime::spawn_progress_mirror(
                    &approval,
                    &child_session_id,
                    &parent_session_id,
                ));

                let worktree = AgentRuntime::prepare_child_workspace(
                    &parent_session_id,
                    &child_session_id,
                    storage.clone(),
                    &fallback_workspace,
                )
                .await;

                let mut attachments = Vec::new();
                attachments.extend(
                    AgentRuntime::collect_parent_attachments(storage.clone(), &parent_session_id)
                        .await,
                );

                let result = match worktree {
                    Ok(worktree_path) => {
                        AgentRuntime::update_child_workspace(
                            &child_session_id,
                            Some(worktree_path.to_string_lossy().to_string()),
                            storage.clone(),
                        );
                        let workspace_root = worktree_path.to_string_lossy().to_string();
                        let _slot = match subagent_slot.acquire_owned().await {
                            Ok(slot) => slot,
                            Err(_) => {
                                return (
                                    index,
                                    child_session_id,
                                    Err("subagent slot unavailable".to_string()),
                                );
                            }
                        };
                        match Box::pin(child_runtime.execute_child_turn(
                            &child_session_id,
                            task.clone(),
                            attachments,
                            provider_name.as_deref(),
                            &model_override,
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

                let patch_note = AgentRuntime::capture_child_patch(
                    &approval,
                    &parent_session_id,
                    &child_session_id,
                    storage.clone(),
                    &task,
                )
                .await;

                AgentRuntime::publish_child_event(
                    &approval,
                    &child_session_id,
                    AgentState::Completed,
                    &task,
                    &parent_session_id,
                );

                let _ = AgentRuntime::finalize_child_workspace(&child_session_id, storage.clone())
                    .await;

                let result = match (result, patch_note) {
                    (Ok(report), Some(note)) => Ok(format!("{report}\n\n{note}")),
                    (Err(error), Some(note)) => Err(format!("{error}\n\n{note}")),
                    (result, None) => result,
                };

                let (phase, detail) = match &result {
                    Ok(report) => ("finished", truncate_chars(report, 120)),
                    Err(error) => ("failed", truncate_chars(error, 120)),
                };
                AgentRuntime::publish_subagent_progress(
                    &approval,
                    &parent_session_id,
                    &child_session_id,
                    phase,
                    detail,
                )
                .await;

                (index, child_session_id, result)
            });
        }

        let mut completed_reports = Vec::new();
        let mut completed_errors = Vec::new();

        while let Some(join) = join_set.join_next().await {
            match join {
                Ok((index, child_session_id, Ok(output))) => {
                    completed_reports.push((index, child_session_id, output, None::<String>));
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
                report["output"] = serde_json::json!(cap_subagent_report(output));
                report["truncated"] =
                    serde_json::json!(output.chars().count() > SUBAGENT_RESULT_MAX_CHARS);
            } else if let Some((_, _, error)) = completed_errors.iter().find(|item| item.0 == index)
            {
                report["error"] = serde_json::json!(error);
            }
            reports.push(report);
        }

        let overall_error = completed_errors.first().map(|item| item.2.clone());

        // The report must name the provider and the model separately; the
        // parent needs both to retry with an explicit override. Deriving the
        // provider from the model string's `provider::model` prefix keeps it
        // correct even when the child inherited the parent's selection.
        let effective_model = requested_model
            .or(parent_model_override.as_deref())
            .unwrap_or(model);
        let provider_display = provider_override
            .map(str::to_string)
            .or_else(|| {
                effective_model
                    .split_once("::")
                    .map(|(provider, _)| provider.to_string())
            })
            .unwrap_or_else(|| "default".to_string());
        let model_display = effective_model
            .split_once("::")
            .map(|(_, model)| model.to_string())
            .unwrap_or_else(|| effective_model.to_string());

        if child_count > 1 {
            (
                serde_json::json!({
                    "subagents": reports,
                    "child_count": child_count,
                    "provider": provider_display,
                    "model": model_display,
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
                    "truncated": report.get("truncated").cloned().unwrap_or_default(),
                    "provider": provider_display,
                    "model": model_display,
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

    fn resolve_model_for_session(&self, session_id: &str) -> Option<String> {
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

    /// Mirrors a child session's activity onto the parent session as throttled
    /// [`AgentEvent::SubagentProgress`] events.
    ///
    /// The child publishes on its own session id, which the parent's UI filters
    /// out, so without this the parent looks idle for the whole delegation.
    /// Updates are capped at roughly one per `PROGRESS_THROTTLE` to keep a
    /// per-token delta stream from flooding the parent's SSE connection.
    fn spawn_progress_mirror(
        approval: &Arc<ApprovalEngine>,
        child_session_id: &str,
        parent_session_id: &str,
    ) -> Option<tokio::task::JoinHandle<()>> {
        let sender = approval.event_sender()?.clone();
        let mut receiver = sender.subscribe();
        let child = child_session_id.to_string();
        let parent = parent_session_id.to_string();
        Some(tokio::spawn(async move {
            let mut last = Instant::now() - PROGRESS_THROTTLE;
            loop {
                let Ok(envelope) = receiver.recv().await else {
                    return;
                };
                if envelope.session_id.as_deref() != Some(child.as_str()) {
                    continue;
                }
                let detail = match &envelope.event {
                    AgentEvent::ToolCallStarted { tool_name, .. } => {
                        format!("running {tool_name}")
                    }
                    AgentEvent::TextDelta { delta, .. } => truncate_chars(delta, 120),
                    AgentEvent::StateChanged { state, .. } => format!("{state:?}"),
                    _ => continue,
                };
                if last.elapsed() < PROGRESS_THROTTLE {
                    continue;
                }
                last = Instant::now();
                let _ = ACTIVE_SESSION_ID
                    .scope(parent.clone(), async {
                        let _ = sender.send(AgentEvent::SubagentProgress {
                            session_id: parent.clone(),
                            subagent_session_id: child.clone(),
                            phase: "working".to_string(),
                            detail,
                        });
                    })
                    .await;
            }
        }))
    }

    /// Publishes the terminal progress for a delegation and stops mirroring.
    async fn publish_subagent_progress(
        approval: &Arc<ApprovalEngine>,
        parent_session_id: &str,
        child_session_id: &str,
        phase: &str,
        detail: String,
    ) {
        let Some(sender) = approval.event_sender() else {
            return;
        };
        ACTIVE_SESSION_ID
            .scope(parent_session_id.to_string(), async {
                let _ = sender.send(AgentEvent::SubagentProgress {
                    session_id: parent_session_id.to_string(),
                    subagent_session_id: child_session_id.to_string(),
                    phase: phase.to_string(),
                    detail,
                });
            })
            .await;
    }

    fn publish_child_event(
        approval: &Arc<ApprovalEngine>,
        child_session_id: &str,
        state: AgentState,
        task: &str,
        parent_session_id: &str,
    ) {
        if let Some(tx) = approval.event_sender().as_ref() {
            let _ = tx.send(AgentEvent::StateChanged {
                session_id: child_session_id.to_string(),
                state,
            });
            let _ = tx.send(AgentEvent::TextDelta {
                session_id: child_session_id.to_string(),
                delta: format!(
                    "Delegated from {}: {}",
                    parent_session_id,
                    truncate_chars(task, 120)
                ),
            });
        }
    }

    async fn collect_parent_attachments(
        storage: Arc<StorageEngine>,
        parent_session_id: &str,
    ) -> Vec<MessageAttachment> {
        let mut collected: Vec<MessageAttachment> = Vec::new();
        if let Ok(messages) = storage.list_messages(parent_session_id) {
            for message in messages.into_iter().rev() {
                for attachment in message.content.attachments {
                    if !collected
                        .iter()
                        .any(|a| a.data_base64 == attachment.data_base64)
                    {
                        collected.push(attachment);
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
        fallback_workspace: &str,
    ) -> std::result::Result<PathBuf, String> {
        let parent_session = storage
            .get_session(parent_session_id)
            .map_err(|_| "parent session not found".to_string())?;
        let workspace = parent_session
            .workspace_root
            .clone()
            .filter(|root| !root.trim().is_empty())
            .unwrap_or_else(|| fallback_workspace.to_string());
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

    fn update_child_workspace(
        child_session_id: &str,
        worktree: Option<String>,
        storage: Arc<StorageEngine>,
    ) {
        if let Some(path) = worktree {
            if let Ok(mut session) = storage.get_session(child_session_id) {
                session.workspace_root = Some(path);
                let _ = storage.save_session(&session);
            }
        }
    }

    /// Captures a delegated child's file changes as a patch artifact on the
    /// PARENT session. `finalize_child_workspace` runs `git worktree remove
    /// --force`, which deletes uncommitted edits, so without this the child's
    /// whole body of work is silently discarded when it finishes.
    async fn capture_child_patch(
        approval: &Arc<ApprovalEngine>,
        parent_session_id: &str,
        child_session_id: &str,
        storage: Arc<StorageEngine>,
        task: &str,
    ) -> Option<String> {
        let session = storage.get_session(child_session_id).ok()?;
        let worktree = session.workspace_root?;
        // Only delegated worktrees, never a directory the user pointed at.
        if std::path::Path::new(&worktree)
            .file_name()
            .and_then(|name| name.to_str())
            != Some(child_session_id)
        {
            return None;
        }
        // -N marks untracked files intent-to-add so they appear in the diff.
        let _ = tokio::process::Command::new("git")
            .args(["add", "-A", "-N"])
            .current_dir(&worktree)
            .output()
            .await;
        let diff_output = tokio::process::Command::new("git")
            .args(["diff", "--no-color", "HEAD"])
            .current_dir(&worktree)
            .output()
            .await
            .ok()?;
        let diff = String::from_utf8_lossy(&diff_output.stdout).to_string();
        if diff.trim().is_empty() {
            return None;
        }
        let files_changed = diff
            .lines()
            .filter(|line| line.starts_with("+++ "))
            .count()
            .max(1);
        let additions = diff
            .lines()
            .filter(|line| line.starts_with('+') && !line.starts_with("+++"))
            .count();
        let deletions = diff
            .lines()
            .filter(|line| line.starts_with('-') && !line.starts_with("---"))
            .count();
        let artifact = Artifact::new_patch(
            parent_session_id,
            format!("Subagent changes: {}", truncate_chars(task, 60)),
            child_session_id,
            diff,
            files_changed,
            additions,
            deletions,
        );
        let _ = storage.save_artifact(&artifact);
        if let Some(tx) = approval.event_sender().as_ref() {
            let _ = tx.send(AgentEvent::ArtifactCreated {
                session_id: parent_session_id.to_string(),
                artifact: artifact.clone(),
            });
        }
        Some(format!(
            "Preserved the delegated edits as patch artifact \"{}\" ({}); the worktree is removed after this turn, so apply this patch to keep them.",
            artifact.title, artifact.summary
        ))
    }

    async fn finalize_child_workspace(child_session_id: &str, storage: Arc<StorageEngine>) {
        // `git worktree remove` takes the worktree PATH and needs a CWD inside
        // the repository. This used to pass the session id and no CWD, so it
        // could never match a worktree and every delegated worktree leaked.
        let Ok(session) = storage.get_session(child_session_id) else {
            return;
        };
        let Some(worktree) = session.workspace_root else {
            return;
        };
        let path = std::path::PathBuf::from(&worktree);
        // Only remove worktrees this runtime created under .odon/worktrees;
        // never delete a directory the user pointed the session at.
        if path.file_name().and_then(|name| name.to_str()) != Some(child_session_id) {
            return;
        }
        let Some(repo_root) = path
            .parent()
            .and_then(std::path::Path::parent)
            .and_then(std::path::Path::parent)
        else {
            return;
        };
        let _ = tokio::process::Command::new("git")
            .args(["worktree", "remove", "--force", worktree.as_str()])
            .current_dir(repo_root)
            .output()
            .await;
    }

    /// Runs one delegated child turn on a runtime that shares the parent's
    /// storage, tools, providers and approval gates. The child session is
    /// pre-created with the delegated workspace so its tools run there.
    // Nine parameters is acceptable here: this is a private single-call-site
    // helper whose arguments are already flat scalars describing one turn.
    #[allow(clippy::too_many_arguments)]
    async fn execute_child_turn(
        &self,
        child_session_id: &str,
        task: String,
        attachments: Vec<MessageAttachment>,
        provider_name: Option<&str>,
        model: &str,
        cancellation: CancellationToken,
        depth: u32,
        workspace_root: Option<String>,
    ) -> Result<Message> {
        if self.storage.get_session(child_session_id).is_err() {
            let mut child = Session::new(
                truncate_chars(&task, 48),
                SessionMode::General,
                model.to_string(),
            );
            child.id = child_session_id.to_string();
            child.workspace_root = workspace_root.clone();
            child.metadata = Some(serde_json::json!({
                "delegated": true,
                "task": truncate_chars(&task, 200),
            }));
            let _ = self.storage.save_session(&child);
        }
        let model_override = provider_name
            .map(|provider| format!("{provider}::{model}"))
            .unwrap_or_else(|| model.to_string());
        self.execute_turn_inner(
            child_session_id,
            &task,
            attachments,
            Some(model_override.as_str()),
            None,
            cancellation,
            None,
            None,
            depth,
            Some(self.max_turn_iterations()),
        )
        .await
    }

    /// A runtime for one delegated child that shares every long-lived
    /// collaborator with the parent (storage, tools, providers, approval,
    /// audit, events) so child turns see the same world.
    fn child_runtime(&self, workspace_root: String) -> AgentRuntime {
        AgentRuntime {
            storage: self.storage.clone(),
            tools: self.tools.clone(),
            approval: self.approval.clone(),
            audit: self.audit.clone(),
            providers: self.providers.clone(),
            default_provider: self.default_provider.clone(),
            model_routes: self.model_routes.clone(),
            event_tx: self.event_tx.clone(),
            workspace_root,
            max_turn_iterations: AtomicUsize::new(self.max_turn_iterations()),
            step_cap_explicit: AtomicBool::new(self.step_cap_explicit.load(Ordering::Relaxed)),
            context_budget_tokens: AtomicUsize::new(
                self.context_budget_tokens.load(Ordering::Relaxed),
            ),
        }
    }
}

/// Truncates a string to at most `max` characters, on a char boundary.
fn truncate_chars(text: &str, max: usize) -> String {
    text.chars().take(max).collect()
}

/// Bound a subagent report for the parent. Appends an explicit note so the
/// parent knows it is reading a truncated report rather than a complete
/// one, and can delegate a narrower follow-up instead of assuming the work
/// was tiny.
fn cap_subagent_report(report: &str) -> String {
    let total = report.chars().count();
    if total <= SUBAGENT_RESULT_MAX_CHARS {
        return report.to_string();
    }
    let kept: String = report.chars().take(SUBAGENT_RESULT_MAX_CHARS).collect();
    format!(
        "{kept}\n\n[subagent report truncated: {total} characters total, first {SUBAGENT_RESULT_MAX_CHARS} shown; the full transcript is in the subagent session]"
    )
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

    let mut head_len = 4.min(messages.len());
    // Do not end the kept head on an assistant message that declares
    // tool_calls: its tool results would fall into the elided window,
    // which permanently breaks provider validation for the session.
    // Shift such messages into the elided region instead.
    while head_len > 0 && !messages[head_len - 1].content.tool_calls.is_empty() {
        head_len -= 1;
    }
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
    "web_search",
    "web_fetch",
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
                "task": { "type": "string", "description": "One self-contained instruction for the subagent." },
                "model": {
                    "type": "string",
                    "description": "Optional model override for this subagent. Defaults to the model the parent session is using. Accepts 'provider::model' or a bare model id."
                },
                "provider": {
                    "type": "string",
                    "description": "Optional provider override (for example 'deepseek', 'mistral'). Pairs with 'model'; omit to let the model id pick the provider."
                }
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
    use odon_core::events::create_event_bus;
    use odon_security::SecurityPolicy;

    #[test]
    fn subagent_reports_are_capped_for_the_parent() {
        assert_eq!(cap_subagent_report("done"), "done");
        assert_eq!(cap_subagent_report(""), "");

        let huge = "x".repeat(SUBAGENT_RESULT_MAX_CHARS + 250);
        let capped = cap_subagent_report(&huge);
        assert!(
            capped.chars().count() < huge.chars().count(),
            "capped report must be shorter than the original"
        );
        assert!(
            capped.starts_with(&"x".repeat(SUBAGENT_RESULT_MAX_CHARS)),
            "the head of the report must be preserved verbatim"
        );
        assert!(
            capped.contains("subagent report truncated"),
            "a truncated report must say so instead of looking complete"
        );
        assert!(
            capped.contains(&huge.chars().count().to_string()),
            "the note must report the original length"
        );

        // Exactly at the limit stays untouched: no marker on a complete report.
        let exact = "y".repeat(SUBAGENT_RESULT_MAX_CHARS);
        assert_eq!(cap_subagent_report(&exact), exact);
    }

    /// The child publishes on its own session id, which the parent's UI
    /// filters out. The mirror must re-publish it on the parent session,
    /// otherwise a long delegation looks like an idle parent.
    #[tokio::test]
    async fn subagent_progress_reaches_the_parent_session() {
        let (tx, mut rx) = create_event_bus(64);
        let emitter = tx.clone();
        let approval = Arc::new(ApprovalEngine::new(SecurityPolicy::default()).with_events(tx));

        let mirror = AgentRuntime::spawn_progress_mirror(&approval, "child-1", "parent-1");
        assert!(mirror.is_some(), "an event bus must yield a mirror task");
        let _guard = MirrorGuard(mirror);
        // Let the mirror subscribe before anything is emitted.
        tokio::task::yield_now().await;

        ACTIVE_SESSION_ID
            .scope("child-1".to_string(), async {
                let _ = emitter.send(AgentEvent::ToolCallStarted {
                    session_id: "child-1".to_string(),
                    call_id: "call-1".to_string(),
                    tool_name: "fs_read".to_string(),
                    arguments: serde_json::json!({}),
                });
            })
            .await;

        let envelope = tokio::time::timeout(Duration::from_secs(2), async {
            loop {
                let envelope = rx.recv().await.expect("bus must stay open");
                if matches!(envelope.event, AgentEvent::SubagentProgress { .. }) {
                    return envelope;
                }
            }
        })
        .await
        .expect("mirrored progress must arrive");

        assert_eq!(envelope.session_id.as_deref(), Some("parent-1"));
        match envelope.event {
            AgentEvent::SubagentProgress {
                subagent_session_id,
                phase,
                detail,
                ..
            } => {
                assert_eq!(subagent_session_id, "child-1");
                assert_eq!(phase, "working");
                assert!(detail.contains("fs_read"), "detail was {detail:?}");
            }
            other => panic!("expected SubagentProgress, got {other:?}"),
        }
    }

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
    fn lesson_kind_classifies_failures_into_stable_actions() {
        let cases = [
            ("No model selected. Add a model ID", "model_selection"),
            ("upstream returned 429 rate limit", "rate_limit"),
            ("401 Unauthorized: invalid api key", "auth"),
            (
                "Provider 'x' is not configured or missing an API key",
                "provider_config",
            ),
            ("maximum context length exceeded", "context_limit"),
            ("request timed out after 60s", "timeout"),
            ("model not found (404)", "model_not_found"),
            ("something unexpected", "provider_error"),
        ];
        for (message, expected) in cases {
            let (kind, hint) = AgentRuntime::lesson_kind(message);
            assert_eq!(kind, expected, "misclassified: {message}");
            assert!(!hint.trim().is_empty(), "a lesson must carry an action");
        }
    }

    #[test]
    fn lesson_kind_reads_status_before_keywords() {
        // The real deepseek 400 body lists supported model names. Keyword
        // matching called it "auth"; the status says else.
        let real = "HTTP 400 from https://api.deepseek.com/v1/chat/completions (model: x). Response: {\"error\":{\"message\":\"The supported API model names are ...\"}}";
        assert_eq!(AgentRuntime::lesson_kind(real).0, "model_not_found");
        assert_eq!(
            AgentRuntime::lesson_kind("HTTP 503 from https://x (model: m)").0,
            "provider_error"
        );
        assert_eq!(
            AgentRuntime::lesson_kind("HTTP 403 from https://x (model: m)").0,
            "auth"
        );
    }

    #[test]
    fn a_failed_turn_leaves_one_reusable_lesson_in_the_harness() {
        let storage = Arc::new(StorageEngine::new_in_memory().unwrap());
        let (event_tx, _rx) = odon_core::events::create_event_bus(16);
        let runtime = AgentRuntime::new(
            storage.clone(),
            Arc::new(ToolRegistry::new()),
            Arc::new(ApprovalEngine::new(odon_security::SecurityPolicy::default())),
            Arc::new(AuditLogger::new()),
            event_tx,
            "/tmp".to_string(),
        );

        assert!(
            !runtime
                .build_system_harness("/tmp")
                .contains("Lessons From Earlier Turns"),
            "a fresh store must not invent lessons"
        );

        // The same failure twice must collapse into one row, not pile up.
        runtime.record_lesson(
            "openai-compatible",
            "odon-auto",
            "No model selected. Add a model ID under Settings",
        );
        runtime.record_lesson(
            "openai-compatible",
            "odon-auto",
            "No model selected. Add a model ID under Settings",
        );

        let stored = storage.list_memories(Some("lessons")).unwrap();
        assert_eq!(stored.len(), 1, "identical failures must dedupe");
        assert_eq!(stored[0].key, "lesson:openai-compatible:model_selection");

        let harness = runtime.build_system_harness("/tmp");
        assert!(
            harness.contains("Lessons From Earlier Turns"),
            "the next turn must see what already failed"
        );
        assert!(
            harness.contains("model_selection"),
            "the lesson must name the failure kind"
        );
        assert!(
            !harness.contains("Lessons From Earlier Turns:") || !stored[0].value.is_empty(),
            "the lesson must carry its detail"
        );
    }

    #[test]
    fn harness_teaches_the_todo_workflow_and_surfaces_the_stored_plan() {
        let storage = Arc::new(StorageEngine::new_in_memory().unwrap());
        let (event_tx, _rx) = odon_core::events::create_event_bus(16);
        let runtime = AgentRuntime::new(
            storage.clone(),
            Arc::new(ToolRegistry::new()),
            Arc::new(ApprovalEngine::new(odon_security::SecurityPolicy::default())),
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

    #[test]
    fn web_tools_survive_the_inline_cap() {
        // A real catalogue is larger than the inline cap, so web access must
        // not depend on the model calling find_tools first.
        let mut defs: Vec<ToolDefinition> = (0..300)
            .map(|i| ToolDefinition {
                name: format!("tool_{i}"),
                description: "d".to_string(),
                parameters: serde_json::json!({"type": "object"}),
            })
            .collect();
        for name in ["web_search", "web_fetch"] {
            defs.push(ToolDefinition {
                name: name.to_string(),
                description: "web".to_string(),
                parameters: serde_json::json!({"type": "object"}),
            });
        }
        let active = initial_active_tools(&defs, true);
        for name in ["web_search", "web_fetch"] {
            assert!(
                active.iter().any(|tool| tool.name == name),
                "{name} missing from the inline set"
            );
        }
    }

    #[test]
    fn step_cap_defaults_to_500_and_zero_means_no_cap() {
        // 500 is the shipped default so a long turn is not cut off mid-work.
        assert_eq!(DEFAULT_MAX_TURN_ITERATIONS, 500);
        assert_eq!(normalize_step_cap(DEFAULT_MAX_TURN_ITERATIONS), 500);
        // A subagent may outlive a metered parent turn, but never the
        // subscription ceiling, and it always stays inside the clamp.
        // Both bounds are constants, so assert at compile time: a regression
        // now fails the build instead of only the test run.
        const {
            assert!(SUBAGENT_MAX_ITERATIONS > DEFAULT_MAX_TURN_ITERATIONS);
        }
        assert_eq!(
            normalize_step_cap(SUBAGENT_MAX_ITERATIONS),
            SUBAGENT_MAX_ITERATIONS
        );
        // 0 disables the cap; the loop then never trips its step guard.
        assert_eq!(normalize_step_cap(0), UNLIMITED_TURN_ITERATIONS);
        // The grace pass is derived as `cap + 1`, so the unlimited sentinel
        // must saturate instead of wrapping to 0 (which would end a no-cap
        // turn on its very first step).
        assert_eq!(
            UNLIMITED_TURN_ITERATIONS.saturating_add(1),
            UNLIMITED_TURN_ITERATIONS
        );
        // Anything else keeps the documented clamp.
        assert_eq!(normalize_step_cap(1), 1);
        assert_eq!(normalize_step_cap(20_000), 10_000);
    }

    #[test]
    fn subagent_definition_advertises_model_and_provider_overrides() {
        let def = subagent_definition();
        let props = def
            .parameters
            .get("properties")
            .and_then(|v| v.as_object())
            .expect("run_subagent must declare properties");
        // The executor has always honoured `model` and `provider`; without
        // advertising them the model could never use the override, so a
        // usage-capped provider could not be routed around.
        assert!(props.contains_key("model"), "model override must be advertised");
        assert!(
            props.contains_key("provider"),
            "provider override must be advertised"
        );
        assert!(props.contains_key("task"));
    }

    #[test]
    fn subscription_providers_get_the_deeper_cap() {
        // Termigo parity: OAuth / keyless providers are bounded by their
        // own usage window, so they get 10_000 steps; a metered API-key
        // provider keeps 500.
        for name in [
            "openai-codex",
            "chatgpt",
            "claude-oauth",
            "antigravity",
            "xai-oauth",
            "github-copilot",
            "copilot",
            "muse-something",
        ] {
            assert!(is_unmetered_provider(name), "{name} should be unmetered");
            assert_eq!(default_step_cap_for(name), OAUTH_MAX_TURN_ITERATIONS);
        }
        for name in ["default", "mistral", "openai", "openrouter", "odon-auto"] {
            assert!(!is_unmetered_provider(name), "{name} is metered");
            assert_eq!(default_step_cap_for(name), DEFAULT_MAX_TURN_ITERATIONS);
        }
        // The deeper cap still obeys the documented clamp.
        assert_eq!(
            normalize_step_cap(OAUTH_MAX_TURN_ITERATIONS),
            OAUTH_MAX_TURN_ITERATIONS
        );
    }
}
