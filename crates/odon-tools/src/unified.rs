//! Minimal, allocation-friendly unified diff used to render edit boxes in chat.
//!
//! The generator trims the common prefix and suffix of the two texts, then
//! renders the changed middle as a single hunk with three lines of context.
//! That keeps the cost linear in the file size (no O(n*m) LCS table), which
//! matters because every edit an agent makes is diffed on the request path.

const CONTEXT: usize = 3;

pub struct UnifiedDiff {
    pub text: String,
    pub additions: usize,
    pub deletions: usize,
}

/// Build a unified diff for `before` → `after`, or `None` when nothing changed.
pub fn unified_diff(path: &str, before: &str, after: &str) -> Option<UnifiedDiff> {
    if before == after {
        return None;
    }

    let old: Vec<&str> = split_lines(before);
    let new: Vec<&str> = split_lines(after);

    let mut prefix = 0;
    while prefix < old.len() && prefix < new.len() && old[prefix] == new[prefix] {
        prefix += 1;
    }

    let mut suffix = 0;
    while suffix < old.len() - prefix
        && suffix < new.len() - prefix
        && old[old.len() - 1 - suffix] == new[new.len() - 1 - suffix]
    {
        suffix += 1;
    }

    let old_mid = &old[prefix..old.len() - suffix];
    let new_mid = &new[prefix..new.len() - suffix];

    let ctx_start = prefix.saturating_sub(CONTEXT);
    let old_tail = (suffix).min(CONTEXT);
    let new_tail = (suffix).min(CONTEXT);

    let old_span = prefix - ctx_start + old_mid.len() + old_tail;
    let new_span = prefix - ctx_start + new_mid.len() + new_tail;

    // Unified diff hunk numbering is 1-based; an empty span at the start of a
    // new file is written as "0,0" by convention.
    let old_first = if old_span == 0 { 0 } else { ctx_start + 1 };
    let new_first = if new_span == 0 { 0 } else { ctx_start + 1 };

    let mut text = String::with_capacity(64 + (old_span + new_span) * 32);
    text.push_str(&format!("--- a/{path}\n"));
    text.push_str(&format!("+++ b/{path}\n"));
    text.push_str(&format!(
        "@@ -{old_first},{old_span} +{new_first},{new_span} @@\n"
    ));

    for line in &old[ctx_start..prefix] {
        push_line(&mut text, ' ', line);
    }
    for line in old_mid {
        push_line(&mut text, '-', line);
    }
    for line in new_mid {
        push_line(&mut text, '+', line);
    }
    for line in &old[old.len() - suffix..old.len() - suffix + old_tail] {
        push_line(&mut text, ' ', line);
    }

    Some(UnifiedDiff {
        text,
        additions: if new_mid.is_empty() { 0 } else { new_mid.len() },
        deletions: if old_mid.is_empty() { 0 } else { old_mid.len() },
    })
}

fn push_line(out: &mut String, marker: char, line: &str) {
    out.push(marker);
    out.push_str(line);
    out.push('\n');
}

/// Split into lines without a trailing empty entry, so a file ending in a
/// newline does not report a spurious changed last line.
fn split_lines(text: &str) -> Vec<&str> {
    if text.is_empty() {
        return Vec::new();
    }
    let trimmed = text.strip_suffix('\n').unwrap_or(text);
    trimmed.split('\n').collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unchanged_text_produces_no_diff() {
        assert!(unified_diff("src/a.rs", "same\n", "same\n").is_none());
    }

    #[test]
    fn counts_additions_and_deletions() {
        let d = unified_diff("src/a.rs", "a\nb\nc\n", "a\nB\nc\n").expect("diff");
        assert_eq!(d.additions, 1);
        assert_eq!(d.deletions, 1);
        assert!(d.text.contains("-b"));
        assert!(d.text.contains("+B"));
        assert!(d.text.contains("--- a/src/a.rs"));
        assert!(d.text.contains("+++ b/src/a.rs"));
    }

    #[test]
    fn pure_addition_has_no_deletions() {
        let d = unified_diff("f", "a\n", "a\nb\n").expect("diff");
        assert_eq!(d.additions, 1);
        assert_eq!(d.deletions, 0);
    }

    #[test]
    fn pure_deletion_has_no_additions() {
        let d = unified_diff("f", "a\nb\n", "a\n").expect("diff");
        assert_eq!(d.additions, 0);
        assert_eq!(d.deletions, 1);
    }

    #[test]
    fn creates_file_from_empty() {
        let d = unified_diff("new.txt", "", "hello\n").expect("diff");
        assert_eq!(d.additions, 1);
        assert_eq!(d.deletions, 0);
        assert!(d.text.contains("+hello"));
    }

    #[test]
    fn context_lines_are_included() {
        let before = "1\n2\n3\n4\n5\n6\n7\n8\n9\n";
        let after = "1\n2\n3\nX\n5\n6\n7\n8\n9\n";
        let d = unified_diff("f", before, after).expect("diff");
        // three context lines above and below the change; line "4" is the
        // changed line itself, so it renders as "-4" rather than context.
        assert!(d.text.contains(" 2\n"));
        assert!(d.text.contains(" 3\n"));
        assert!(d.text.contains("-4\n"));
        assert!(d.text.contains(" 5\n"));
        assert!(d.text.contains(" 6\n"));
        assert!(d.text.contains("+X"));
    }

    #[test]
    fn output_stays_linear_for_large_files() {
        let before: String = (0..20_000).map(|i| format!("line {i}\n")).collect();
        let mut after = before.clone();
        after.push_str("appended\n");
        let d = unified_diff("big.txt", &before, &after).expect("diff");
        assert_eq!(d.additions, 1);
        assert_eq!(d.deletions, 0);
        assert!(
            d.text.len() < 4_000,
            "diff should stay small: {}",
            d.text.len()
        );
    }
}
