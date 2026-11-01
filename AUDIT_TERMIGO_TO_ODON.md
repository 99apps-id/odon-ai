# Audit & Architecture Migration: Termigo -> ODON
**ODON (Open Digital Orchestration Network)**

---

## 1. Executive Summary & Architectural Shift

| Dimension | **Termigo** | **ODON (Open Digital Orchestration Network)** |
| :--- | :--- | :--- |
| **Product Metaphor** | AI-native terminal emulator & developer workbench | **Conversational Desktop & Codex Workspace** (ChatGPT Desktop + Codex feel) |
| **Primary Focus** | Command-line execution, PTY sessions, terminal tabs | **Conversations, autonomous orchestration, & multimodal artifacts** |
| **Agent Execution** | Executed in **frontend TypeScript / webview** (coupled to browser/UI loop) | **Native Rust Agent Runtime** (headless daemon `odond` + CLI `odon` + desktop Tauri client) |
| **Extensibility** | Tauri IPC commands to frontend state | **Model Context Protocol (MCP)**, **GitHub Connectors**, **Unified Channel Adapters** |
| **Workspaces** | Terminal pane + Editor pane + Side-by-side terminal splits | **Universal Workspaces**: AI Chat, Code, Create, Documents, Files, Agents, Automations |
| **Multichannel** | Telegram bot coupled to frontend React state | **First-class headless relay daemon** (CLI, Telegram, WhatsApp-ready abstraction) |

---

## 2. Comprehensive Module Audit

### Category A: Directly Reusable / Adaptable Modules (from Termigo Rust backend)
These modules exist in Termigo's `src-tauri` and provide rock-solid OS primitives that can be adapted directly into ODON's Rust workspace:

1. **`fs` (Filesystem module)**:
   - *Status*: Adapt with security enforcement.
   - *Capability*: Ripgrep-backed searcher (`grep-searcher`, `grep-regex`), globbing (`globset`), line-based file reading, safe atomic file writing.
   - *Adaptation in ODON*: Encapsulate behind `odon-tools` with mandatory approval checks before any mutating call (`fs_write`, `fs_edit`, `fs_delete`).
2. **`git` (Git subsystem)**:
   - *Status*: Adapt into `odon-tools`.
   - *Capability*: Status check, diff calculation, checkpointing, branching, commit helpers.
3. **`mcp` (Model Context Protocol client transport)**:
   - *Status*: Adapt into `odon-tools`.
   - *Capability*: Stdio transport for external MCP servers, tool discovery, and tool invocation.
4. **`secrets` & OS Keychain integration**:
   - *Status*: Adapt into `odon-core` / `odon-security`.
   - *Capability*: Secure credential storage (API keys for OpenAI, Anthropic, OpenRouter, GitHub tokens, Telegram tokens).

---

### Category B: Modules Requiring a Rust Port (Critical Architectural Upgrade)
In Termigo, the following mission-critical systems were written in **TypeScript / React** (`src/modules/ai/lib/`, `src/modules/telegram/`). Because ODON requires headless, daemon-first execution without a GUI webview process, these must be ported to **native Rust**:

1. **Core Agent Loop & State Machine**:
   - *Termigo*: `src/modules/ai/lib/agent.ts` & `orchestrator.ts` (TypeScript async generator in webview).
   - *ODON*: Port to `odon-agent` (Rust async state machine with `tokio`, supporting `idle`, `thinking`, `tool_call`, `awaiting_approval`, `executing`, `streaming`, `completed`, `failed`).
2. **Model Provider Abstraction**:
   - *Termigo*: `src/modules/ai/lib/transport.ts` & fetch wrappers in JS.
   - *ODON*: Port to `odon-provider` in Rust. Unified `ModelProvider` trait supporting OpenAI-compatible streaming (`eventsource-stream`), Anthropic Messages API, OpenRouter, tool calling, schema validation, and cancellation tokens.
3. **Central Security Policy & Approval Engine**:
   - *Termigo*: `src/modules/ai/lib/approvalPolicy.ts` & `policyEngine.ts` in JS.
   - *ODON*: Port to `odon-security` in Rust. Enforces deterministic risk tiers:
     - Tier 0: Read-only (auto-approved, e.g., `fs_read`, `git_status`, `github_list_issues`).
     - Tier 1: Local mutating (requires approval or configurable rule, e.g., `fs_write`, `git_commit`).
     - Tier 2: High-risk system execution (destructive commands, shell execution, credential mutation).
4. **Session & History Persistence**:
   - *Termigo*: IndexedDB + local filesystem JSON files in frontend store.
   - *ODON*: Port to `odon-storage` in Rust using **SQLite** (`rusqlite` / `sqlx`). Durable, ACID-compliant message histories, artifact tracking, run logs, and audit trails.
5. **Channel Adapter Interface (Telegram Relay)**:
   - *Termigo*: `src/modules/telegram/bot.ts` (React hook + Telegraf-style polling).
   - *ODON*: Port to `odon-channel` in Rust. Common `ChannelAdapter` trait powering CLI, Desktop IPC, Telegram Bot long-polling, and future WhatsApp gateway.

---

### Category C: Modules That Should Be Rewritten / Redesigned
These components from Termigo are too terminal-specific and do not fit ODON's universal assistant + artifact-first identity:

1. **Terminal-Centric UI (Xterm & Multi-split webviews)**:
   - *Decision*: **REWRITE**.
   - *Rationale*: Termigo is an IDE/terminal shell. ODON is a **ChatGPT Desktop + Codex Workspace** app.
   - *New Design*: 3-column layout matching the ODON UI specifications:
     - Left: Global navigation (AI Chat, Code, Create, Documents, Files, Agents, Automations) + Recent Chats list.
     - Center: Clean conversational workspace with prompt cards, markdown renderer, and interactive output action chips (`Run project and preview`, `Create Git repository`, `Turn this into an automation`, `Refine PRD`).
     - Right: Dedicated **Artifacts & Relays Drawer** (preview images, markdown documents, git patch diffs, relay connection monitors, agent sub-tasks).
2. **Subagent Tree Orchestration**:
   - *Decision*: **REDESIGN**.
   - *Rationale*: Termigo's subagents focused heavily on terminal commands and code-review diffs. ODON subagents are multi-disciplinary:
     - `Research Agent` (web research & knowledge aggregation)
     - `Content / Creative Agent` (copywriting, document crafting)
     - `Developer Agent` (code generation & patch synthesizing)
     - `Orchestration Agent` (multi-step workflow pipeline)
3. **Multimodal Media Dispatcher**:
   - *Decision*: **NEW IMPLEMENTATION**.
   - *Rationale*: Termigo lacked image/media generation output cards. ODON incorporates prompt-to-image and media artifact generation interfaces (with hooks to Flux, Midjourney API, Fal.ai, or local endpoints) displayed as first-class visual cards in the Artifacts drawer.

---

## 3. Preservation of Apache-2.0 License & Attribution

ODON honors all terms of the Apache License, Version 2.0:
1. The full **Apache-2.0 LICENSE** is included in the root directory.
2. The **NOTICE** file explicitly credits `99apps-id`, `Crynta`, and the `Termigo` project as the source of foundational patterns and licensed components.
3. Source files incorporating adapted logic contain standard Apache-2.0 header notices.
