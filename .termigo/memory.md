# Termigo agent memory

Written by the Termigo agent as it works. Safe to edit or delete by hand;
removing a line makes the agent forget it. Your own notes belong in
TERMIGO.md, which the agent never rewrites.
- 2026-11-01 ODON (Open Digital Orchestration Network) repo initialized at C:/Project/odon-ai with remote https://github.com/99apps-id/odon-ai.git: modular Rust workspace (9 crates: core, security, storage, provider, tools, channel, agent, odon CLI, odond daemon) + 3-column React 19 desktop UI matching mockups.
- 2026-11-02 [GOTCHA] ODON `cargo clippy --all-targets -- -D warnings` currently fails on baseline warnings in `odon-core/src/artifact.rs` (`too_many_arguments`) and `odon-core/src/session.rs` (`derivable_impls`); distinguish these from feature-patch regressions.
- 2026-11-02 ODON tool registry currently has 164 registered tools: 162 ready and two explicitly unavailable (`browser_click`, `session_switch`); readiness is inspectable with `odon tools doctor`.
- 2026-11-02 ODON now has Windows-first local app tools (`app_list`, `app_launch`, `app_focus`, `app_close`, `app_input`) and Telegram Bot API media tools (`send_document`, `send_image`, `send_media`) requiring explicit chat_id, workspace-contained file, and TELEGRAM_BOT_TOKEN.
- 2026-11-02 ODON clipboard_read, clipboard_write, termigo_copy_selection, and termigo_paste_buffer_history now use native arboard clipboard access; history is process-local, deduplicated by SHA-256, capped at 50 entries, and each stored text payload is capped at 64 KiB.
