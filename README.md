<p align="center">
  <img src="assets/icons/cat.svg" width="72" alt="ODON Logo" />
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
  <img src="https://img.shields.io/badge/Tools_Registry-255_Registered-emerald?style=flat-square" alt="255 registered tools" />
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
ODON exposes 255 registered tools (240 model-visible, 15 compatibility-only; 248 stable, 2 experimental, 5 honestly marked unavailable) with complete parameter schemas, risk tiers, availability metadata, and runtime requirement checks. Run `odon tools doctor` to inspect readiness; unavailable capabilities are reported honestly and blocked before execution. A golden manifest is checked into the repository and validated in CI via `cargo test -p odon-tools tests::golden_manifest_matches_registry`. A turn sends at most 128 tool schemas inline; past that a core set rides with `find_tools` and the rest of the catalogue loads on demand, and `run_subagent` delegates a bounded side task to a child session — name a persisted agent with `"agent": "<id|name>"` to hand the task to that teammate's persona, model, and office desk. Independent tool calls in one step run with bounded parallelism (4). Long sessions are compacted to a context budget before each request (trimming oversized tool outputs first, then eliding the oldest middle turns), and a turn that hits its step cap pauses with everything persisted instead of failing; send "continue" to resume, and a reply cut by the output budget is resumed automatically up to 4 times (the resume prompt carries the cut text so the model continues instead of restarting). Tune with `ODON_MAX_TURN_ITERATIONS` (default 500 for metered providers, 10000 for OAuth/keyless subscription providers, `0` for no cap) and `ODON_CONTEXT_BUDGET_TOKENS` (default 120000).
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
- **Autonomous Skills (3 tools)**: `skill_list`, `skill_create`, `skill_run`. Three skills ship inside the binary — `game-project`, `procedural-3d-gltf`, and `agents-office-app` — and a user skill with the same name overrides one.
- **Video & Audio (3 tools)**: `video_probe`, `video_trim`, `video_merge`. These call the host's `ffmpeg`/`ffprobe` with an argv vector (never a shell) and keep every path inside the workspace.
- **Games, 3D & Agents Office (3 tools)**: `game_scaffold` writes a runnable canvas platformer or Three.js adventure project with a zero-dependency CORS preview server; `model3d_create` builds low-poly glTF assets from primitives; `agents_office_create` scaffolds a standalone isometric office for real ODON sessions and subagents.
- **System & Environment (6 tools)**: `system_info`, `calc_eval`, `env_get`, `env_list`, `process_list`, `disk_usage`.
- **Harness & Planning (6 tools)**: `todo_write`, `todo_read`, `todo_update`, `think`, `plan_mode`, `remember`.
- **Model Context Protocol (2 tools)**: `mcp_list_tools`, `mcp_call`.
- **GitHub Connectors (4 tools)**: `github_get_repo`, `github_list_issues`, `github_create_issue`, `github_list_prs`.
- **Artifact Synthesis (4 tools)**: `artifact_create_code`, `artifact_create_document`, `artifact_create_image`, `artifact_update`.

### 5. ⚙️ Enriched 9-Tab Settings Center
- **Providers**: Official OAuth (GitHub Copilot, Google Antigravity, OpenAI Codex, Claude, xAI) + BYOK (DeepSeek, OpenAI, Anthropic, Gemini, Qwen, Mistral, Groq, OpenRouter, Ollama, Meta Muse via the pay-as-you-go Model API at `https://api.meta.ai/v1`). A gateway provider can discover its model list from its own endpoint, and each account is probed so the model menu only offers models that actually resolve to a provider.
- **Endpoints**: Base URLs, API keys, and per-provider model catalogues for openai-compatible gateways.
- **Agents & Personas**: Switch between **Autonomous Coder**, **Security Auditor**, **Pentester & RE**, **Systems Architect**, and **Autonomous Sentinel**, with custom system prompt overrides and thinking effort levels.
- **Skills**: Inspect and author reusable automation routines stored in `.odon/skills/*.md`.
- **MCP Servers**: Configure `stdio` and `sse` Model Context Protocol endpoints.
- **Mode Harness & Safety**: Toggle **Autonomous** vs **Supervised** mode, enforce **Plan Mode**, and manage saved approval whitelists.
- **Messaging Relays**: Telegram and WhatsApp bridges, including WhatsApp QR pairing.
- **SSH Remote Profiles**: Add and edit remote SSH machine profiles.
- **System & Daemon**: Monitor the native daemon health, port configurations, loopback token status, and interface themes.

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
- **Per-chat sessions are stable (`tg_<chat_id>` / `wa_<phone>`) and both relays run through the existing agent runtime, tools, and approval engine. Relay tokens are stored in `~/.odon/relays.json`.
- **Per-agent bots & webhook triggers**: each persisted agent can carry its own Telegram bot token (allowed-user allowlist included) and an inbound webhook at `POST /hooks/agents/<token>` (`{"text": "..."}` → reply in the same request). Both are managed in the Agents Office editor without touching the operator's main relay.
- **Off-shift message queue**: messages arriving for an agent outside its working hours are parked in the `agent_inbox` table and the sender gets an away notice with the queue position; a daemon scheduler releases them through the normal relay path when the shift starts. Slash commands still answer immediately. Disable entirely with `ODON_OFF_SHIFT_QUEUE=0`. Webhook callers (synchronous) instead receive `{"ok": false, "off_shift": true}` with the shift window.

### 9. 🗃️ Multi-Workspace Composer
- Add folders from the chat composer (`FolderPlus` button). Every folder is validated, canonicalized, and persisted in `~/.odon/workspaces.json` before the agent can use it.
- Each chat keeps its own workspace; switching folders updates the session, and all filesystem/shell/terminal tools plus the system prompt are scoped to that folder.
- System directories (drive roots, `C:\Windows`, `/etc`, `/bin`, `/proc`, …) are refused. File Explorer and file-serving APIs only read inside allowed workspaces.

### 10. 🏢 Agents Office
A live isometric 3D office rendered over real sessions — desks are conversations and persisted agents, the floor holds agent desks, delegated subagents appear in the **Subagent Wing**, and the back-office director's desk is you.

- **Persisted agents**: give each teammate a name, role, desk color, system prompt, model (`provider::model`), optional workspace, and skills picked from the catalogue as chips. Agents persist in SQLite (`agents`) and their desks update live from the daemon event stream.
- **Working hours with timezones**: each agent has a shift (start/end + weekdays + IANA timezone such as `Asia/Jakarta`; DST-aware). Outside the shift the desk shows *off shift*, idle agents fall *asleep*, and (see relays below) inbound messages are parked instead of waking the model.
- **Desk actions**: open a chat or start a new one, focus the camera, and **approve or reject pending tool approvals right from the desk** (tooltip or side panel) without leaving the office.
- **Live telemetry**: state badges (thinking / running tool / needs approval / completed / failed), a per-desk **activity trail** (tool calls, approvals, artifacts, delegation phases, with timestamps), and a **×N workload badge** counting active delegations.
- **Extras**: PNG **snapshot** export of the office, day/night mode, and desk filtering by name, room, state, or role. Per-agent **Telegram connector** and **webhook trigger** are configured in the same editor (see Relays).

---

## 🔐 Security Hardening

- **Command policy**: catastrophic commands (`rm -rf /`, fork bombs, disk wipes, download-and-execute, reverse shells, credential theft, …) are denied in the registry, the shell tools, and the HTTP terminal endpoint, independently of approval mode. Matching is normalized against quoting, `$IFS`, and line-continuation tricks.
- **SSRF guard**: `web_fetch`, `web_scrape`, `url_preview`, `http_*`, `probe_url`, `browser_*` only allow public `http(s)` URLs by default. Private/loopback/link-local/CGNAT/metadata addresses and cross-host redirects are blocked — including IPv4 smuggled through IPv4-mapped, IPv4-compatible, and NAT64 (`64:ff9b::/96`) IPv6 forms, and bracketed IPv6 literals. Opt in with `ODON_ALLOW_PRIVATE_NETWORK=1`, or restrict with `ODON_ALLOWED_HOSTS` / `ODON_BLOCKED_HOSTS`.
- **Daemon auth & CORS**: binding to a non-loopback address requires `ODON_API_TOKEN`. CORS only accepts localhost/127.0.0.1/Tauri origins plus `ODON_ALLOWED_ORIGINS`. The desktop app has an API token field under Settings → System.
- **Relay trust**: Telegram/WhatsApp deny all inbound messages when their allowlist is empty unless `TELEGRAM_ALLOW_ANYONE=1` / `WHATSAPP_ALLOW_ANYONE=1` is set. WhatsApp also verifies `X-Hub-Signature-256` when `WHATSAPP_APP_SECRET` is provided.
- **SSH**: `-i` key paths are restricted to `~/.ssh`, `~/.odon/ssh-keys`, and `ODON_SSH_KEY_DIRS` (re-validated at connect time, so hand-edited or pre-hardening profiles cannot point `-i` elsewhere); host/user values are validated; private key material is never stored inside `ssh-profiles.json`; host keys use `accept-new` and the known-hosts store is written atomically. SFTP mutations (write, mkdir, rename, delete, create) pass through the same approval engine as workspace tools, and profile edits drop the cached authenticated session.
- **Filesystem**: API paths must resolve inside an allowed workspace, with canonical symlink checks. Skill names and SSH certificate filenames are slugified.
- **Frontend**: Markdown output is HTML-escaped before formatting (no raw model HTML/XSS), links allow only http(s)/mailto, the browser preview iframe is sandboxed without `allow-same-origin`, and API keys are no longer stored in `localStorage`.
- **Audit**: tool executions are appended to `~/.odon/audit.jsonl` (and approval decisions to `~/.odon/approval-audit.jsonl`) so history survives restarts.

---

## 🏛️ Cargo Workspace Architecture

```text
odon-ai/
├── crates/
│   ├── odon-core/       # Domain models, Artifacts, Sessions, Agents & schedules, Event Bus
│   ├── odon-security/   # Central security policy, Risk Tiers (0-3), Approval engine, Persistence
│   ├── odon-storage/    # Durable SQLite persistence: messages, artifacts, sessions, agents, off-shift inbox
│   ├── odon-provider/   # ModelProvider trait & OAuth PKCE / Device Flow implementations
│   ├── odon-tools/      # Registry of 255 real executable tools (Filesystem, Git, Web, Shell, SSH, MCP, Skills, Video, Games & 3D, Office)
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

### Install the Windows app (recommended)
Download `ODON_0.1.0_x64-setup.exe` from the releases page and run it. The installer bundles both `odon-desktop.exe` and the `odond` sidecar, so no Rust or Node toolchain is needed. On first launch the desktop shell starts the daemon on `127.0.0.1:4096`, writes a loopback token to `%USERPROFILE%\.odon\api-token`, hands that token to the webview, and stops the daemon when you close the app.

Build the installer yourself with:
```powershell
powershell -ExecutionPolicy Bypass -File scripts\build-installer.ps1
```
The signed-out path is `desktop\src-tauri\target\release\bundle\nsis\ODON_0.1.0_x64-setup.exe`. To ship OAuth client credentials inside the packaged daemon, put them in `.env.local` (git-ignored) before building; `scripts/stage-sidecar.mjs` passes them to the cargo build so they are stamped into the binary and the packaged app can log in without a `.env.local` at runtime.

### Prerequisites (for building from source)
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
