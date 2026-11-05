//! Wire-format normalisation for the message array sent to a provider.
//!
//! A transcript is a protocol, not just a log. Strict providers (DeepSeek, and
//! every gateway that proxies it) validate the shape of the array before they
//! generate anything, and two shapes are rejected outright:
//!
//! * an `assistant` turn with neither `content` nor `tool_calls`
//!   -> `HTTP 400 Invalid assistant message: content or tool_calls must be set`
//! * a `tool` result that no earlier assistant turn declared
//!   -> `HTTP 400 ... tool message must follow a tool_calls message`
//!
//! Both are easy to produce here. A turn that is stopped, cancelled, or that
//! dies with a provider error before any text streams still persists its
//! assistant row, and an assistant step that only ran tools can persist an
//! empty `text`. Lenient providers (OpenAI, and most gateways) shrug those rows
//! off, so the transcript looks healthy until the operator switches model or
//! switches workspace inside the same session — then the very same history is
//! replayed to a strict provider and every single turn 400s from then on. That
//! is the reported "works on model A, fails on model B, same session" failure:
//! the session is poisoned, not the request.
//!
//! Two defences, deliberately layered:
//!
//! 1. [`sanitize_history`] repairs an already poisoned transcript on the way
//!    out, so no migration is needed and old rows heal by themselves.
//! 2. The runtime no longer writes empty assistant rows in the first place
//!    (`crates/odon-agent/src/runtime.rs`).

use crate::message::{Message, MessageRole};

/// Text recorded in place of a tool result that never made it into storage.
///
/// Declared tool calls must be answered: a provider that sees an assistant
/// turn declaring three calls and only two `tool` messages rejects the whole
/// request. Inventing silence would be worse than saying the truth, so the gap
/// is filled with an explicit marker the model can act on.
pub const UNANSWERED_TOOL_RESULT: &str =
    "[tool result missing: this call was interrupted before it returned anything. \
Re-run the tool if its output is still needed; do not assume what it would have said.]";

/// Text recorded in place of a tool result that succeeded but returned nothing.
pub const NO_TOOL_OUTPUT: &str = "[the tool returned no output]";

/// Repair a stored transcript until it satisfies the provider wire format.
///
/// The transformer is intentionally lossless for everything that carries
/// meaning: real user turns, assistant prose, tool declarations and tool
/// results are preserved verbatim (only their identifiers are repaired when
/// empty). What it removes or fills is exactly what a provider would reject.
pub fn sanitize_history(messages: Vec<Message>) -> Vec<Message> {
    let session_id = messages
        .iter()
        .rev()
        .find(|message| !message.session_id.trim().is_empty())
        .map(|message| message.session_id.clone())
        .unwrap_or_default();
    let had_input = !messages.is_empty();

    let mut out: Vec<Message> = Vec::with_capacity(messages.len());
    // Tool calls declared by the last assistant turn and not yet answered.
    let mut pending: Vec<String> = Vec::new();
    let mut generated_ids = 0usize;

    for mut message in messages {
        match message.role {
            MessageRole::Assistant => {
                // A new assistant turn closes the previous tool exchange; an
                // unanswered declaration can never be retro-filled here.
                flush_unanswered(&mut out, &mut pending, &session_id);

                // The poison row: no prose and no calls. Reason-only steps and
                // cancelled turns leave this behind, and a provider that
                // validates `content || tool_calls` refuses the entire array.
                if message.content.text.trim().is_empty() && message.content.tool_calls.is_empty() {
                    continue;
                }

                for call in &mut message.content.tool_calls {
                    if call.call_id.trim().is_empty() {
                        generated_ids += 1;
                        call.call_id = format!("call_auto_{generated_ids}");
                    }
                }
                pending = message
                    .content
                    .tool_calls
                    .iter()
                    .map(|call| call.call_id.clone())
                    .collect();
                out.push(message);
            }
            MessageRole::Tool => {
                let mut call_id = message
                    .content
                    .tool_calls
                    .first()
                    .map(|call| call.call_id.trim().to_string())
                    .unwrap_or_default();
                // A streamed result that lost its id still belongs to the
                // oldest unanswered declaration of the turn.
                if call_id.is_empty() {
                    if let Some(first) = pending.first().cloned() {
                        call_id = first;
                    }
                }
                // Orphan result: nothing declared it, so no provider accepts it
                // and the model would read a result out of context.
                if call_id.is_empty() || !pending.iter().any(|id| id == &call_id) {
                    continue;
                }
                pending.retain(|id| id != &call_id);
                if let Some(call) = message.content.tool_calls.first_mut() {
                    call.call_id = call_id;
                    if call.tool_name.trim().is_empty() {
                        call.tool_name = "tool".to_string();
                    }
                }
                // A tool that succeeded silently (or wrote to a file only)
                // produces an empty body, which strict endpoints reject as
                // "content must not be empty". Say so explicitly.
                if message.content.text.trim().is_empty() {
                    message.content.text = NO_TOOL_OUTPUT.to_string();
                }
                out.push(message);
            }
            _ => {
                flush_unanswered(&mut out, &mut pending, &session_id);
                // Empty user/system turns carry nothing and some gateways
                // reject them as "content must not be empty". Attachments are
                // content on their own, so only truly blank turns go.
                if message.content.text.trim().is_empty() && message.content.attachments.is_empty()
                {
                    continue;
                }
                out.push(message);
            }
        }
    }
    flush_unanswered(&mut out, &mut pending, &session_id);

    if out.is_empty() && had_input {
        // Never hand a provider an empty array: that turns a recoverable empty
        // turn into an opaque 400 with no hint about what to change.
        let mut placeholder = Message::new_user(session_id, "(empty turn)");
        placeholder.content.text = "(empty turn)".to_string();
        out.push(placeholder);
    }
    out
}

/// Answer every still-pending tool call with the explicit missing-result marker.
fn flush_unanswered(out: &mut Vec<Message>, pending: &mut Vec<String>, session_id: &str) {
    for call_id in pending.drain(..) {
        out.push(Message::new_tool(
            session_id.to_string(),
            call_id,
            UNANSWERED_TOOL_RESULT,
        ));
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::message::ToolCallInfo;

    fn call(id: &str, name: &str) -> ToolCallInfo {
        ToolCallInfo {
            call_id: id.to_string(),
            tool_name: name.to_string(),
            arguments: serde_json::json!({}),
            output: None,
            is_error: false,
            requires_approval: false,
            is_approved: None,
        }
    }

    fn assistant_with_calls(ids: &[&str]) -> Message {
        let mut message =
            Message::new_assistant("s1", "", None, Some("deepseek-flash".to_string()));
        message.content.tool_calls = ids.iter().map(|id| call(id, "fs_read")).collect();
        message
    }

    fn roles(messages: &[Message]) -> Vec<&'static str> {
        messages
            .iter()
            .map(|message| match message.role {
                MessageRole::User => "user",
                MessageRole::Assistant => "assistant",
                MessageRole::System => "system",
                MessageRole::Tool => "tool",
            })
            .collect()
    }

    #[test]
    fn empty_assistant_turn_is_dropped() {
        let history = vec![
            Message::new_user("s1", "hello"),
            Message::new_assistant("s1", "", None, None),
            Message::new_assistant("s1", "hi back", None, None),
        ];
        let cleaned = sanitize_history(history);
        assert_eq!(roles(&cleaned), vec!["user", "assistant"]);
        assert_eq!(cleaned[1].content.text, "hi back");
    }

    #[test]
    fn whitespace_only_assistant_turn_is_dropped() {
        let history = vec![
            Message::new_user("s1", "hello"),
            Message::new_assistant("s1", "  \n\t ", Some("thinking".to_string()), None),
        ];
        let cleaned = sanitize_history(history);
        assert_eq!(roles(&cleaned), vec!["user"]);
    }

    #[test]
    fn assistant_tool_declaration_survives_with_empty_text() {
        let history = vec![
            Message::new_user("s1", "read the file"),
            assistant_with_calls(&["c1"]),
            Message::new_tool("s1", "c1", "{\"ok\":true}"),
        ];
        let cleaned = sanitize_history(history);
        assert_eq!(roles(&cleaned), vec!["user", "assistant", "tool"]);
        assert_eq!(cleaned[1].content.tool_calls.len(), 1);
    }

    #[test]
    fn orphan_tool_result_is_dropped() {
        let history = vec![
            Message::new_user("s1", "hello"),
            Message::new_tool("s1", "ghost", "{\"ok\":true}"),
            Message::new_assistant("s1", "answer", None, None),
        ];
        let cleaned = sanitize_history(history);
        assert_eq!(roles(&cleaned), vec!["user", "assistant"]);
    }

    #[test]
    fn duplicated_tool_result_is_dropped() {
        let history = vec![
            Message::new_user("s1", "read the file"),
            assistant_with_calls(&["c1"]),
            Message::new_tool("s1", "c1", "first"),
            Message::new_tool("s1", "c1", "second"),
        ];
        let cleaned = sanitize_history(history);
        assert_eq!(roles(&cleaned), vec!["user", "assistant", "tool"]);
        assert_eq!(cleaned[2].content.text, "first");
    }

    #[test]
    fn unanswered_declaration_gets_an_explicit_marker() {
        let history = vec![
            Message::new_user("s1", "read two files"),
            assistant_with_calls(&["c1", "c2"]),
            Message::new_tool("s1", "c1", "{\"ok\":true}"),
        ];
        let cleaned = sanitize_history(history);
        assert_eq!(roles(&cleaned), vec!["user", "assistant", "tool", "tool"]);
        assert_eq!(cleaned[3].content.tool_calls[0].call_id, "c2");
        assert_eq!(cleaned[3].content.text, UNANSWERED_TOOL_RESULT);
    }

    #[test]
    fn tool_result_without_id_is_paired_with_oldest_declaration() {
        let history = vec![
            Message::new_user("s1", "read the file"),
            assistant_with_calls(&["c1"]),
            Message::new_tool("s1", "", "{\"ok\":true}"),
        ];
        let cleaned = sanitize_history(history);
        assert_eq!(roles(&cleaned), vec!["user", "assistant", "tool"]);
        assert_eq!(cleaned[2].content.tool_calls[0].call_id, "c1");
        assert!(!cleaned[2].content.text.is_empty());
    }

    #[test]
    fn empty_call_ids_are_repaired_before_sending() {
        let history = vec![
            Message::new_user("s1", "read the file"),
            assistant_with_calls(&[""]),
        ];
        let cleaned = sanitize_history(history);
        assert_eq!(cleaned[1].content.tool_calls[0].call_id, "call_auto_1");
    }

    #[test]
    fn healthy_conversation_is_untouched() {
        let history = vec![
            Message::new_user("s1", "read the file"),
            assistant_with_calls(&["c1"]),
            Message::new_tool("s1", "c1", "{\"ok\":true}"),
            Message::new_assistant("s1", "done", None, None),
        ];
        let expected_len = history.len();
        let cleaned = sanitize_history(history);
        assert_eq!(
            roles(&cleaned),
            vec!["user", "assistant", "tool", "assistant"]
        );
        assert_eq!(cleaned.len(), expected_len);
    }

    #[test]
    fn an_all_empty_transcript_still_yields_a_message() {
        let history = vec![Message::new_assistant("s1", "", None, None)];
        let cleaned = sanitize_history(history);
        assert_eq!(roles(&cleaned), vec!["user"]);
    }
}
