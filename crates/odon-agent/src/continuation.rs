//! Continuation semantics ported from Hermes' conversation loop.
//!
//! Two problems appear when a model stops mid-answer:
//!
//! 1. The runtime cannot tell "the answer finished" from "the output budget ran
//!    out", so a cut answer is stored as if it were complete.
//! 2. The model, when asked to continue, usually repeats the tail of what it
//!    already said, producing a duplicated seam in the transcript.
//!
//! This module supplies the pure building blocks: nudge text for the model, a
//! continuation-intent test for user messages, and an overlap measurement used
//! to de-duplicate the seam.

/// Shortest repeated run worth de-duplicating. Below this, matching is noise.
pub const MIN_CONTINUATION_OVERLAP: usize = 24;

/// Text appended to the transcript when the output budget cut the answer.
pub const OUTPUT_LIMIT_NUDGE: &str = "[System: Your previous message was cut off by the output \
limit, not by the end of the task. Continue exactly where you stopped. Do not repeat the text you \
already produced, do not restart the answer, and do not summarise what you did. If a tool call \
was cut off mid-argument, issue that tool call again with complete arguments.]";

/// Text appended when the transport dropped the stream mid-answer.
pub const NETWORK_CUT_NUDGE: &str = "[System: Your previous message was interrupted by a \
transport error before it finished. Continue from the interruption point. Do not repeat the text \
you already produced and do not restart the answer.]";

/// Text appended when the model announced a next step but never took it.
///
/// A step with no tool call ends the ReAct loop, so a reply that only says
/// "Next I'll inspect…" strands the task even though nothing failed. This
/// nudge turns that promise into the action it describes.
pub const PROMISED_ACTION_NUDGE: &str = "[System: Your previous message announced the next step \
but contained no tool call, so the turn ended with the task still unfinished. Act now: issue the \
tool call you announced. If the task is genuinely finished, say so plainly instead of promising \
more work.]";

/// Text appended when the model streamed only reasoning and no answer.
///
/// Reasoning models sometimes spend the whole output budget on their hidden
/// thoughts and close the step with neither prose nor a tool call. The ReAct
/// loop treats that as the end of the turn, so the operator gets
/// "[The model returned no reply…]". One bounded follow-up asks for the answer
/// that reasoning was preparing.
pub const EMPTY_REPLY_NUDGE: &str = "[System: Your previous step ended without a tool call and \
without a final answer; its output budget was consumed by reasoning. Resume the task from the work \
already done: if you still need information, issue the next tool call now; otherwise write the \
final response now. Do not repeat tool calls that already succeeded, and do not restart the task.]";

/// How many empty-reply continuations one turn may trigger.
pub const MAX_EMPTY_REPLY_CONTINUATIONS: usize = 2;

/// How many promised-action continuations one turn may trigger.
pub const MAX_PROMISED_ACTION_CONTINUATIONS: usize = 2;

/// True when a reply ends by announcing a concrete next action instead of
/// taking it.
///
/// Only the tail is inspected: a complete answer may explain "the next step"
/// earlier and still be finished. The phrases are deliberately concrete
/// ("next I'll inspect", "saya lanjut") so a closing courtesy such as
/// "let me know" is not mistaken for unfinished work.
pub fn promises_next_action(text: &str) -> bool {
    let trimmed = text.trim();
    if trimmed.is_empty() || trimmed.chars().count() > 2_000 {
        return false;
    }
    let lower = trimmed.to_lowercase();
    let mut tail: Vec<char> = lower.chars().collect();
    let keep_from = tail.len().saturating_sub(500);
    tail.drain(..keep_from);
    let tail: String = tail.into_iter().collect();

    const PROMISES: &[&str] = &[
        "next i'll",
        "next, i'll",
        "next i will",
        "next i am going to",
        "i'll now ",
        "i will now ",
        "let me now ",
        "let me check",
        "let me look",
        "let me inspect",
        "let me read",
        "let me run",
        "let me search",
        "let me examine",
        "let me verify",
        "let me open",
        "let me find",
        "i'm going to check",
        "i'm going to inspect",
        "i'm going to read",
        "i'm going to run",
        "i'm going to search",
        "saya lanjut",
        "saya akan lanjut",
        "selanjutnya saya",
        "mari saya cek",
        "mari saya periksa",
        "mari saya lihat",
        "mari saya baca",
        "saya cek ",
        "saya periksa ",
        "saya baca ",
        "saya jalankan ",
    ];
    const COMPLETIONS: &[&str] = &[
        "done",
        "selesai",
        "finished",
        "complete",
        "no further",
        "that's all",
        "itu saja",
        "sudah selesai",
    ];

    PROMISES.iter().any(|phrase| tail.contains(phrase))
        && !COMPLETIONS.iter().any(|phrase| tail.contains(phrase))
}

/// Longest prefix of `continuation` that repeats the suffix of `previous`.
///
/// Uses the KMP prefix function, so the cost is linear in the input length
/// instead of the quadratic cost of testing every suffix.
pub fn continuation_overlap_length(previous: &str, continuation: &str) -> usize {
    if previous.len() < MIN_CONTINUATION_OVERLAP || continuation.len() < MIN_CONTINUATION_OVERLAP {
        return 0;
    }

    let continuation = continuation.as_bytes();
    let previous = previous.as_bytes();

    let mut prefix = vec![0usize; continuation.len()];
    let mut matched = 0usize;
    for index in 1..continuation.len() {
        while matched > 0 && continuation[index] != continuation[matched] {
            matched = prefix[matched - 1];
        }
        if continuation[index] == continuation[matched] {
            matched += 1;
            prefix[index] = matched;
        }
    }

    matched = 0;
    let last = previous.len() - 1;
    for (index, byte) in previous.iter().enumerate() {
        while matched > 0 && *byte != continuation[matched] {
            matched = prefix[matched - 1];
        }
        if *byte == continuation[matched] {
            matched += 1;
            if matched == continuation.len() {
                if index == last {
                    return matched;
                }
                matched = prefix[matched - 1];
            }
        }
    }

    if matched >= MIN_CONTINUATION_OVERLAP {
        matched
    } else {
        0
    }
}

/// Join `continuation` onto `previous`, dropping a repeated seam.
pub fn merge_continuation(previous: &str, continuation: &str) -> String {
    let overlap = continuation_overlap_length(previous, continuation);
    if overlap == 0 {
        return format!("{previous}{continuation}");
    }
    let remainder = continuation
        .get(overlap..)
        .map(str::to_string)
        .unwrap_or_default();
    format!("{previous}{remainder}")
}

/// True when the user message is nothing but a request to carry on.
///
/// Such a message should be turned into a continuation of the previous turn
/// rather than sent to the model as fresh instructions.
pub fn trailing_continue_intent(text: &str) -> bool {
    let trimmed = text.trim();
    if trimmed.is_empty() {
        return false;
    }
    let normalised = trimmed
        .trim_end_matches(|c: char| c.is_ascii_punctuation())
        .to_lowercase();
    matches!(
        normalised.as_str(),
        "continue"
            | "lanjut"
            | "lanjutkan"
            | "go on"
            | "keep going"
            | "carry on"
            | "resume"
            | "teruskan"
            | "next"
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn overlap_is_zero_when_continuation_does_not_repeat() {
        let previous = "The quick brown fox jumps over the lazy dog and keeps running.";
        let continuation = "A completely different sentence starts here without repetition.";
        assert_eq!(continuation_overlap_length(previous, continuation), 0);
        assert_eq!(
            merge_continuation(previous, continuation),
            format!("{previous}{continuation}")
        );
    }

    #[test]
    fn overlap_detects_the_repeated_seam() {
        let previous = "First paragraph. The build failed because the module was missing.";
        let continuation = "The build failed because the module was missing. So I fixed it.";
        let overlap = continuation_overlap_length(previous, continuation);
        assert!(
            overlap >= MIN_CONTINUATION_OVERLAP,
            "expected a repeated seam, got {overlap}"
        );
        let merged = merge_continuation(previous, continuation);
        assert_eq!(
            merged.matches("The build failed").count(),
            1,
            "the seam must appear once, got {merged:?}"
        );
        assert!(merged.ends_with("So I fixed it."));
    }

    #[test]
    fn overlap_ignores_tiny_repeats() {
        // Shorter than MIN_CONTINUATION_OVERLAP, so no de-duplication happens.
        assert_eq!(continuation_overlap_length("ok", "ok then, continue"), 0);
    }

    #[test]
    fn overlap_handles_multibyte_text() {
        let previous = "Ringkasan: proses build gagal karena modul belum terdaftar di workspace.";
        let continuation =
            "proses build gagal karena modul belum terdaftar di workspace. Lalu saya perbaiki.";
        let merged = merge_continuation(previous, continuation);
        assert_eq!(merged.matches("modul belum terdaftar").count(), 1);
        assert!(merged.ends_with("Lalu saya perbaiki."));
    }

    #[test]
    fn bare_continue_messages_are_recognised() {
        for text in [
            "continue",
            "Continue.",
            " lanjut ",
            "Lanjutkan!",
            "go on",
            "resume",
        ] {
            assert!(trailing_continue_intent(text), "should match {text:?}");
        }
    }

    #[test]
    fn promised_actions_are_detected_at_the_tail() {
        for text in [
            "Next I'll inspect the exact security-relevant modules.",
            "I'm switching to targeted reads now. Next I'll read approval.rs.",
            "Saya lanjut audit repo ini dengan fokus ke bug berpotensi.",
            "Setelah itu saya lanjut ke area lain seperti path traversal.",
            "Let me check the approval module now.",
            "Mari saya periksa bagian canonicalization.",
        ] {
            assert!(promises_next_action(text), "should match {text:?}");
        }
    }

    #[test]
    fn finished_answers_are_not_mistaken_for_promises() {
        for text in [
            "",
            "Done. The bug is in approval.rs:260 and the fix is applied.",
            "Laporan selesai: tidak ada masalah yang ditemukan.",
            "Let me know if you need anything else.",
            "The next step is documented in the README; the work is complete.",
        ] {
            assert!(!promises_next_action(text), "should not match {text:?}");
        }
    }

    #[test]
    fn ordinary_messages_are_not_continue_intent() {
        for text in [
            "",
            "continue the refactor of the storage layer",
            "lanjutkan migrasi database",
            "what did you mean by that?",
            "next steps please",
        ] {
            assert!(!trailing_continue_intent(text), "should not match {text:?}");
        }
    }
}
