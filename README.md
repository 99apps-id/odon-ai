# ODON (Open Digital Orchestration Network)

<p align="center">
  <strong>Universal AI Assistant & Autonomous Orchestration Network</strong><br>
  <em>ChatGPT Desktop + Codex Workspace experience powered by a native Rust agent runtime</em>
</p>

---

## 🌟 Overview

**ODON (Open Digital Orchestration Network)** is designed from the ground up as a conversational assistant and autonomous orchestration platform rather than a traditional IDE or terminal shell. 

While code editing and terminal executions are available as workspace extensions, the primary focus is on **natural dialogue, autonomous multi-modal execution, and rich interactive artifacts** (code applications, product requirement documents, high-fidelity visual cards, and git patches).

### Key Architectural Tenets:
1. **No Rigid Step-Turn Bureaucracy**: Unlike terminal loop systems that count turns and steps (`Turn 1/30`, `Step 3`), ODON responds fluidly like ChatGPT Desktop, Codex, and Muse.ai with real-time streaming, collapsible thought badges, and interactive action chips.
2. **Native Rust Runtime**: The core agent execution, state machines, SQLite persistence, and security approval engines are implemented completely in Rust. No webview or GUI process is required for headless daemon (`odond`) or CLI (`odon`) execution.
3. **Multi-Model & OAuth Support**:
   - **OAuth Providers**: Antigravity (Google Cloud Code), OpenAI Codex (ChatGPT backend with Responses API), Meta Muse Code (Device Flow & sanitized SSE).
   - **BYOK Providers**: OpenAI, Anthropic Claude, OpenRouter, and any OpenAI-compatible completions endpoint (Ollama, vLLM, LM Studio, Groq, DeepSeek).
4. **First-Class Artifacts**: Synthesize interactive Code artifacts (React 19, TypeScript, Rust), Markdown documents (PRDs, specs), and Image generation previews.
5. **Universal Relays**: Headless channels for Desktop IPC, CLI, Telegram bot relay, and extensible WhatsApp gateway.
6. **Model Context Protocol (MCP) & GitHub Connectors**: Connect to any stdio MCP server and GitHub repository.

---

## 🏛️ Cargo Workspace Architecture

```
odon-ai/
├── crates/
│   ├── odon-core/       # Domain models, Artifacts, Sessions, Event Bus
│   ├── odon-security/   # Central security policy, Risk Tiers (0/1/2), Approval engine, Audit logger
│   ├── odon-storage/    # Durable SQLite persistence for messages, artifacts, and sessions
│   ├── odon-provider/   # Async ModelProvider trait (OpenAI, Codex, Antigravity, Muse, OAuth PKCE)
│   ├── odon-tools/      # Tool registry: Filesystem, Git, Shell, GitHub connectors, MCP, Artifact synthesis
│   ├── odon-channel/    # Unified ChannelAdapter trait (CLI, Desktop IPC, Telegram Bot, WhatsApp stub)
│   ├── odon-agent/      # Headless conversational agent runtime & streaming orchestrator
│   ├── odon/            # Standalone CLI binary (`odon chat`, `odon run`, `odon status`)
│   └── odond/           # Standalone Headless Daemon (`odond serve`)
├── desktop/             # Sleek 3-column desktop UI (React 19 + Vite + Tailwind CSS)
├── AUDIT_TERMIGO_TO_ODON.md  # Detailed module migration audit from Termigo
├── LICENSE              # Apache-2.0 License
└── NOTICE               # Attribution to Termigo / 99apps-id / Crynta
```

---

## 🚀 Quick Start

### 1. Build and Test the Rust Runtime
```bash
# Check and compile all crates
cargo check

# Run the integration test suite
cargo test --test integration_test
```

### 2. Run the ODON CLI
```bash
# Check system status and relays
cargo run --bin odon -- status

# Run an autonomous prompt
cargo run --bin odon -- run "Create a personal knowledge hub spec"

# Start interactive chat
cargo run --bin odon -- chat
```

### 3. Run the Headless Daemon
```bash
# Start background daemon with optional Telegram relay
cargo run --bin odond -- serve --bind 127.0.0.1:4096 --telegram-token YOUR_BOT_TOKEN
```

### 4. Launch Desktop UI
```bash
cd desktop
pnpm install
pnpm dev
# Open http://localhost:3000 in your browser
```

---

## 🛡️ Security Policy & Tool Registry (18 Tools)

Every tool invocation in ODON is evaluated by the central `ApprovalEngine` before execution:
- **Tier 0 (Read-Only)**: `fs_read`, `fs_list`, `fs_search`, `git_status`, `git_diff`, `git_log`, `github_get_repo`, `github_list_issues`, `mcp_list_tools`, `web_search`, `web_fetch`, `system_info`, `calc_eval`, `memory_recall`. Auto-approved.
- **Tier 1 (Workspace Mutating)**: `fs_write`, `fs_edit`, `git_commit`, `git_branch`, `memory_store`, `artifact_create_code`, `artifact_create_document`, `artifact_create_image`. Auto-approved in Autonomous mode; requires approval in Supervised mode.
- **Tier 2 (System Mutating)**: `shell_exec`, `fs_delete`, destructive terminal commands. Gated by mandatory interactive user approval (modal / inline chip).

---

## 🌐 Full Agentic Harness & Daemon API (`odond`)

The `odond` daemon provides a high-performance REST + SSE server:
- `GET /api/status`: System health, runtime state, active relays.
- `GET /api/sessions`: List conversation histories from SQLite.
- `POST /api/sessions`: Create new session.
- `POST /api/sessions/{id}/messages`: Submit prompt and execute multi-step tool turns.
- `GET /api/events`: Real-time SSE streaming (text deltas, thought deltas, tool executions, artifact notifications).
- `GET /api/approvals` & `POST /api/approvals/{id}`: Interactive approval review for Tier 2 operations.
- `desktop/dist` auto-serving: Access the desktop web interface directly at `http://localhost:4096`.

---

## ⚖️ License & Attribution

ODON is licensed under the [Apache-2.0 License](LICENSE).
Adapted and evolved from foundational patterns created in [Termigo](https://github.com/99apps-id/termigo). Full attribution details are documented in [NOTICE](NOTICE) and [AUDIT_TERMIGO_TO_ODON.md](AUDIT_TERMIGO_TO_ODON.md).
