<p align="center">
  <img src="assets/icons/cat.svg" width="64" height="64" alt="ODON Logo" />
</p>

<h1 align="center">ODON (Open Digital Orchestration Network)</h1>

<p align="center">
  <strong>Autonomous AI Desktop Assistant, Systems Orchestrator & Multi-Provider Agent Runtime</strong><br>
  <em>ChatGPT Desktop + Codex Workspace experience powered by an ultra-fast, native Rust daemon runtime</em>
</p>

<p align="center">
  <img src="https://img.shields.io/badge/Rust-2021_Edition-orange?style=flat-square&logo=rust" alt="Rust" />
  <img src="https://img.shields.io/badge/React-19-blue?style=flat-square&logo=react" alt="React 19" />
  <img src="https://img.shields.io/badge/TypeScript-5.8-blue?style=flat-square&logo=typescript" alt="TypeScript" />
  <img src="https://img.shields.io/badge/Tailwind_CSS-v4-38B2AC?style=flat-square&logo=tailwind-css" alt="Tailwind CSS" />
  <img src="https://img.shields.io/badge/Tools_Registry-227_Registered-emerald?style=flat-square" alt="227 registered tools" />
  <img src="https://img.shields.io/badge/License-Apache--2.0-green?style=flat-square" alt="License" />
</p>

---

## 📸 Preview

<p align="center">
  <img src="odon-ai.png" alt="ODON AI Desktop Workspace" width="100%" style="border-radius: 12px; box-shadow: 0 8px 30px rgba(0,0,0,0.12);" />
</p>

---

## 🌟 Overview

**ODON (Open Digital Orchestration Network)** is an autonomous AI desktop assistant and systems agent embedded directly in your local development workspace. Designed with the fluid, conversational aesthetics of ChatGPT Desktop and Codex, ODON couples a high-performance **Rust backend daemon (`odond`)** with a **React 19 desktop interface**.

Unlike traditional rigid terminal loops, ODON delivers seamless multi-turn thought streaming, live tool execution, interactive code & document artifacts, a full **File Explorer**, and an **SSH Remote Terminal**.

---

## 🚀 Key Features

### 1. 🗂️ Integrated File Explorer
- **Workspace Tree View**: Browse project files and subdirectories with instant search filtering.
- **In-App File Editor**: View and edit code with line numbers, syntax highlighting, and quick save (`Ctrl+S`).
- **Context Actions**: Send any open file directly to the AI agent (*"Ask AI about this file"*), copy relative paths, or open directories in the terminal.
- **File Management**: Create files, create directories, and delete items safely.

### 2. 🌐 SSH Remote Terminal
- **Saved Host Profiles**: Configure remote servers with custom SSH ports, identities, and keys with secure persistent storage.
- **Interactive Remote Execution**: Run remote shell sessions directly through the backend daemon using system OpenSSH.
- **Session History**: Command history buffer with arrow key navigation, exit code tracking, and one-click output copy.

### 3. 🛡️ ChatGPT / Muse-Style Light Approval UI
- **Zero Black Backgrounds**: High-contrast, elegant light-card design matching modern frontier AI interfaces.
- **Smart Risk Tiers**:
  - **Tier 0**: Read-only operations (auto-allowed).
  - **Tier 1**: Workspace mutations (file edits, commits).
  - **Tier 2**: Out-of-workspace file access (flags `Outside Workspace` badge).
  - **Tier 3**: Dangerous system commands & shell executions (flags warning badge & command preview).
- **Three Precision Actions**:
  - **Approve Once**: Executes the single requested action.
  - **Always Allow**: Persists the tool + path scope to `.odon/approval-policies.json`.
  - **Deny**: Rejects execution and passes feedback back to the agent.

### 4. 🧠 Explicit, Self-Diagnosing Tool Registry
ODON exposes 227 registered tools with complete parameter schemas, risk tiers, availability metadata, and runtime requirement checks. Run `odon tools doctor` to inspect readiness; unavailable capabilities are reported honestly and blocked before execution. A golden manifest is checked into the repository and validated in CI via `cargo test -p odon-tools tests::golden_manifest_matches_registry`. A turn sends at most 128 tool schemas inline; past that a core set rides with `find_tools` and the rest of the catalogue loads on demand, and `run_subagent` delegates a bounded side task to a child session. Independent tool calls in one step run with bounded parallelism (4). Long sessions are compacted to a context budget before each request (trimming oversized tool outputs first, then eliding the oldest middle turns), and a turn that hits its step cap pauses with everything persisted instead of failing; send "continue" to resume. Tune with `ODON_MAX_TURN_ITERATIONS` (default 100) and `ODON_CONTEXT_BUDGET_TOKENS` (default 120000).
- **Filesystem (17 tools)**: `read_file`, `write_file`, `edit`, `multi_edit`, `list_directory`, `create_directory`, `delete_file`, `move_file`, `copy_file`, `grep`, `glob`, etc.
- **Git (14 tools)**: `git_status`, `git_diff`, `git_commit`, `git_log`, `git_branch`, `git_add`, `git_checkout`, `git_show`, `git_stash`, `git_pull`, `git_push`, `git_reset`, etc.
- **Browser Control (4 tools)**: `browser_open`, `browser_navigate`, `browser_screenshot`, and `browser_click`. `browser_click` launches a temporary headless Chrome/Edge instance and clicks via CDP.
- **Persistent Browser Sessions (4 tools)**: `browser_session_create`, `browser_session_list`, `browser_session_resume`, and `browser_session_close`. Sessions keep Chromium cookies and localStorage under `.odon/browser-sessions/<session_id>`; credentials are never copied into the manifest. Use a session profile for sites that require repeated login.
- **Local Application Control (5 tools)**: `app_list`, `app_launch`, `app_focus`, `app_close`, and `app_input`. Windows support uses structured PowerShell/native process calls with approval-gated mutations; unsupported operating systems return an explicit unavailable error.
- **Outbound Media (3 tools)**: `send_document`, `send_image`, and `send_media`. These upload real files through the Telegram Bot API using explicit `chat_id` and `TELEGRAM_BOT_TOKEN`; files must be inside the workspace.
- **Code Intelligence**: `code_index`, `code_search`, `format_code`, `run_checks`, `test_file`. The persistent code index uses workspace-scoped chunking with BM25 lexical ranking, enabling fast semantic-style discovery without requiring an external language server.
- **Review & Change Analysis (5 tools)**: `review_diff`, `review_file`, `review_security`, `review_session_changes`, `change_summary`. These tools surface structured findings by severity and category, making pre-commit and post-change review actionable for both correctness and security.
- **Runtime & Process (3 tools)**: `process_wait`, `dev_server_status`, `browser_status`. These tools inspect and monitor running processes, dev server ports, and browser automation availability without requiring shell-level introspection.
- **Dev Server Lifecycle (3 tools)**: `dev_server_start`, `dev_server_stop`, `dev_server_logs`. These tools manage dev server processes in the background. Task state is maintained for the current session; restart the agent if you need to reattach to running servers.
- **Shell & Terminal (3 tools)**: `shell_exec`, `bash_run`, `terminal_exec`.
- **Cybersecurity & Network (7 tools)**: `web_search`, `web_fetch`, `probe_url`, `http_get`, `http_post`, `dns_lookup`, `port_scan`.
- **Autonomous Skills (3 tools)**: `skill_list`, `skill_create`, `skill_run`.
- **System & Environment (6 tools)**: `system_info`, `calc_eval`, `env_get`, `env_list`, `process_list`, `disk_usage`.
- **Harness & Planning (6 tools)**: `todo_write`, `todo_read`, `todo_update`, `think`, `plan_mode`, `remember`.
- **Model Context Protocol (2 tools)**: `mcp_list_tools`, `mcp_call`.
- **GitHub Connectors (4 tools)**: `github_get_repo`, `github_list_issues`, `github_create_issue`, `github_list_prs`.
- **Artifact Synthesis (4 tools)**: `artifact_create_code`, `artifact_create_document`, `artifact_create_image`, `artifact_update`.

### 5. ⚙️ Enriched 7-Tab Settings Center
- **Providers**: Official OAuth (GitHub Copilot, Google Antigravity, OpenAI Codex, Claude, xAI) + BYOK (DeepSeek, OpenAI, Anthropic, Gemini, Qwen, Mistral, Groq, OpenRouter, Ollama, Meta Muse via the pay-as-you-go Model API at `https://api.meta.ai/v1`).
- **Agents & Personas**: Switch between **Autonomous Coder**, **Security Auditor**, **Pentester & RE**, **Systems Architect**, and **Autonomous Sentinel**, with custom system prompt overrides and thinking effort levels.
- **Skills**: Inspect and author reusable automation routines stored in `.odon/skills/*.md`.
- **MCP Servers**: Configure `stdio` and `sse` Model Context Protocol endpoints.
- **Mode Harness & Safety**: Toggle **Autonomous** vs **Supervised** mode, enforce **Plan Mode**, and manage saved approval whitelists.
- **SSH Remote Profiles**: Add and edit remote SSH machine profiles.
- **System & Daemon**: Monitor the native daemon health, port configurations, and interface themes.

### 6. 📎 Documents & Images Attachment
- Single dedicated **Paperclip** attach button.
- Supports images (`png`, `jpg`, `webp`, `gif`) and code/data documents (`pdf`, `txt`, `md`, `json`, `csv`, `py`, `rs`, `ts`, `html`, `yaml`).
- Interactive file preview chips with thumbnail and remove (`X`) buttons.

### 7. 🔌 Dynamic OpenAI-Compatible Custom Model Manager
- Fully customizable model list for local gateways (vLLM, Ollama, LiteLLM, LM Studio).
- Manual input for Model ID, Display Name, and Context Window with instant `+ Tambah Baris Model` button.

### 8. 📡 Messaging Relays (Telegram + WhatsApp)
- **Telegram Bot**: configure the bot token and allowed user IDs directly in **Settings → Relays** (runtime connect/disconnect, no daemon restart). Long-polling relay with a shared agent dispatcher; text, images, PDFs, and text documents are forwarded to the agent, and the final answer is sent back to the same chat. Commands: `/start`, `/help`, `/new`, `/status`.
- **WhatsApp Web**: **Settings → Relays → Generate QR / Connect** starts a local Baileys bridge. Pair by scanning the QR code (`WhatsApp → Linked devices`) or by entering a phone number to receive a **pairing code**. Sessions persist in `~/.odon/whatsapp-auth`, and inbound/outbound messages use the same agent runtime.
- **WhatsApp Cloud API** remains available as an optional transport via Meta webhooks (`GET/POST /api/relays/whatsapp`) and Graph API replies.
- Per-chat sessions are stable (`tg_<chat_id>` / `wa_<phone>`) and both relays run through the existing agent runtime, tools, and approval engine. Relay tokens are stored in `~/.odon/relays.json`.

### 9. 🗃️ Multi-Workspace Composer
- Add folders from the chat composer (`FolderPlus` button). Every folder is validated, canonicalized, and persisted in `~/.odon/workspaces.json` before the agent can use it.
- Each chat keeps its own workspace; switching folders updates the session, and all filesystem/shell/terminal tools plus the system prompt are scoped to that folder.
- System directories (drive roots, `C:\Windows`, `/etc`, `/bin`, `/proc`, …) are refused. File Explorer and file-serving APIs only read inside allowed workspaces.

---

## 🔐 Security Hardening

- **Command policy**: catastrophic commands (`rm -rf /`, fork bombs, disk wipes, download-and-execute, reverse shells, credential theft, …) are denied in the registry, the shell tools, and the HTTP terminal endpoint, independently of approval mode. Matching is normalized against quoting, `$IFS`, and line-continuation tricks.
- **SSRF guard**: `web_fetch`, `web_scrape`, `url_preview`, `http_*`, `probe_url`, `browser_*` only allow public `http(s)` URLs by default. Private/loopback/link-local/CGNAT/metadata addresses and cross-host redirects are blocked. Opt in with `ODON_ALLOW_PRIVATE_NETWORK=1`, or restrict with `ODON_ALLOWED_HOSTS` / `ODON_BLOCKED_HOSTS`.
- **Daemon auth & CORS**: binding to a non-loopback address requires `ODON_API_TOKEN`. CORS only accepts localhost/127.0.0.1/Tauri origins plus `ODON_ALLOWED_ORIGINS`. The desktop app has an API token field under Settings → System.
- **Relay trust**: Telegram/WhatsApp deny all inbound messages when their allowlist is empty unless `TELEGRAM_ALLOW_ANYONE=1` / `WHATSAPP_ALLOW_ANYONE=1` is set. WhatsApp also verifies `X-Hub-Signature-256` when `WHATSAPP_APP_SECRET` is provided.
- **SSH**: `-i` key paths are restricted to `~/.ssh`, `~/.odon/ssh-keys`, and `ODON_SSH_KEY_DIRS`; host/user values are validated; private key material is never stored inside `ssh-profiles.json`; host keys use `accept-new`.
- **Filesystem**: API paths must resolve inside an allowed workspace, with canonical symlink checks. Skill names and SSH certificate filenames are slugified.
- **Frontend**: Markdown output is HTML-escaped before formatting (no raw model HTML/XSS), links allow only http(s)/mailto, the browser preview iframe is sandboxed without `allow-same-origin`, and API keys are no longer stored in `localStorage`.
- **Audit**: tool executions are appended to `~/.odon/audit.jsonl` (and approval decisions to `~/.odon/approval-audit.jsonl`) so history survives restarts.

---

## 🏛️ Cargo Workspace Architecture

```text
odon-ai/
├── crates/
│   ├── odon-core/       # Domain models, Artifacts, Sessions, Event Bus
│   ├── odon-security/   # Central security policy, Risk Tiers (0-3), Approval engine, Persistence
│   ├── odon-storage/    # Durable SQLite persistence for messages, artifacts, and sessions
│   ├── odon-provider/   # ModelProvider trait & OAuth PKCE / Device Flow implementations
│   ├── odon-tools/      # Registry of 149 real executable tools (Filesystem, Git, Web, Shell, SSH, MCP, Skills)
│   ├── odon-channel/    # Unified ChannelAdapter trait (CLI, Desktop IPC, Telegram Bot, WhatsApp relay)
│   ├── odon-agent/      # Headless conversational agent runtime & streaming orchestrator
│   ├── odon/            # Standalone CLI binary (`odon chat`, `odon run`, `odon status`)
│   └── odond/           # Standalone Daemon REST + SSE server (`odond serve` on port 4096)
├── desktop/             # Sleek React 19 + Vite + Tailwind CSS Desktop UI
├── .odon/skills/        # Reusable autonomous skills recipes
├── odon-ai.png          # Visual workspace screenshot
├── LICENSE              # Apache-2.0 License
└── NOTICE               # Attribution to Termigo / 99apps-id / Crynta
```

---

### Tool readiness

```powershell
# Human-readable readiness report
odon tools doctor

# Machine-readable report for automation
odon tools doctor --json

# Fail CI if a selected tool is not ready
odon tools doctor --filter browser --strict
```

`stable` tools are ready for normal use, `experimental` tools are operational but may change, and `unavailable` tools are visible for discovery but rejected before approval or execution with a concrete reason.

## 🏁 Getting Started

### Prerequisites
- **Rust Toolchain**: `rustc` and `cargo` (1.80+ recommended)
- **Node.js**: v20+ or v22+ with `pnpm`
- **OpenSSH Client**: standard on Windows 10/11, macOS, and Linux

### 1. Clone & Build Backend
```bash
git clone https://github.com/99apps-id/odon-ai.git
cd odon-ai

# Build all workspace crates
cargo build --workspace

# Run tool registry and core tests
cargo test -p odon-tools -p odon-security --lib
```

### 2. Start the Daemon (`odond`)
```bash
# Start background daemon on port 4096
cargo run --bin odond -- serve
```

### 3. Launch Desktop UI
```bash
cd desktop
pnpm install
pnpm dev

# Open http://localhost:3000 in your browser
```

### 4. Enable Telegram / WhatsApp Relays
Open the desktop app and go to **Settings → Relays**:

- **Telegram**: paste the bot token from @BotFather and the allowed user IDs/usernames, then click **Save & Connect**.
- **WhatsApp Web**: install the bridge dependencies once, then click **Generate QR / Connect**:
  ```bash
  cd scripts/whatsapp-bridge
  npm install
  ```
  Scan the QR with `WhatsApp → Linked devices`, or enter a phone number to use a pairing code. The bridge runs locally on port 4097 (configurable with `WA_BRIDGE_PORT`).

Environment variables still work as a fallback (`TELEGRAM_BOT_TOKEN`, `TELEGRAM_ALLOWED_USERS`, `WHATSAPP_*`). For the optional WhatsApp Cloud API transport, expose the daemon through HTTPS and set the Meta webhook URL to `https://<your-domain>/api/relays/whatsapp`, subscribing to the `messages` field. Telegram and WhatsApp Web require no public URL.

---

## 📄 License & Attribution

This project is licensed under the [Apache-2.0 License](LICENSE).  
Attribution and inspirations from Termigo are documented in [NOTICE](NOTICE). Developed by [99apps-id](https://github.com/99apps-id).
