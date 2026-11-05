//! Bounded rendering of long-term memory into the system harness.
//!
//! Memory is unbounded by nature: every `memory_store` call adds a row, every
//! rated fact is content-addressed so the same preference recorded twice becomes
//! two rows, and every provider failure appends a lesson. Rendered naively, a
//! few hundred rows of that turns the harness into a wall of stale text that
//! *displaces* the actual conversation — the classic way a model starts
//! "remembering" things that were never true in this session.
//!
//! So the harness shows a bounded, deduplicated, truncated window: the 40 most
//! recent non-plumbing facts, one line each, with the total block capped in
//! characters. Older rows stay in storage (recallable with `memory_recall`);
//! they just stop competing for the context budget.

use odon_storage::MemoryItem;

/// How many memories the harness shows at most.
pub const MAX_HARNESS_FACTS: usize = 40;

/// Characters kept per memory line before truncation.
pub const MAX_FACT_CHARS: usize = 240;

/// Characters the whole memory block may occupy in the harness.
pub const MAX_MEMORY_BLOCK_CHARS: usize = 6_000;

/// Categories that are per-session plumbing, never user memory.
pub const PLUMBING_CATEGORIES: &[&str] = &["plan_mode", "todos"];

/// Lessons are rendered in their own section; showing them here as well is what
/// produced double-printed "do not repeat these" blocks.
pub const LESSON_CATEGORY: &str = "lessons";

/// Number of lessons the harness shows.
pub const MAX_HARNESS_LESSONS: usize = 5;

/// Content rows kept across all categories in storage. Older rows are evicted
/// (they were already invisible to the harness), so a long-lived install stays
/// bounded instead of accumulating every fact it ever rated.
pub const MAX_MEMORY_ROWS: usize = 400;

/// Content rows kept per category, so one noisy category cannot fill the table.
pub const MAX_MEMORY_ROWS_PER_CATEGORY: usize = 80;

/// Plan items shown in the harness. A pasted 200-item plan is a context bomb;
/// the model can always re-read the full list with `todo_read`.
pub const MAX_PLAN_ITEMS: usize = 30;

/// Truncate on a character boundary, marking that the text was cut.
pub fn clip(text: &str, limit: usize) -> String {
    let length = text.chars().count();
    if length <= limit {
        return text.to_string();
    }
    let kept: String = text.chars().take(limit).collect();
    format!("{}… [{} more characters stored]", kept, length - limit)
}

/// Collapse whitespace so one memory cannot smuggle in many pseudo-lines.
fn one_line(text: &str) -> String {
    text.split_whitespace().collect::<Vec<_>>().join(" ")
}

/// Render the user-memory block: bounded, deduplicated by value, recent first.
///
/// Returns an empty string when there is nothing worth showing, so the harness
/// stays clean on a fresh install.
pub fn render_memory_block(memories: &[MemoryItem]) -> String {
    let mut seen: Vec<String> = Vec::new();
    let mut lines: Vec<String> = Vec::new();
    let mut used = 0usize;
    let mut shown = 0usize;

    for memory in memories {
        if shown >= MAX_HARNESS_FACTS {
            break;
        }
        let category = memory.category.trim();
        if PLUMBING_CATEGORIES.contains(&category) || category == LESSON_CATEGORY {
            continue;
        }
        let value = one_line(&memory.value);
        if value.is_empty() {
            continue;
        }
        // Content-addressed facts can repeat the same value under different
        // keys; the model only needs to read it once.
        if seen.iter().any(|existing| existing == &value) {
            continue;
        }
        let line = if category == "facts" {
            format!("- {}", clip(&value, MAX_FACT_CHARS))
        } else {
            format!(
                "- [{}] {}: {}",
                category,
                memory.key,
                clip(&value, MAX_FACT_CHARS)
            )
        };
        if used + line.chars().count() + 1 > MAX_MEMORY_BLOCK_CHARS {
            break;
        }
        used += line.chars().count() + 1;
        seen.push(value);
        lines.push(line);
        shown += 1;
    }

    if lines.is_empty() {
        return String::new();
    }
    format!(
        "\n### Known User Preferences & Project Memory:\n{}\n",
        lines.join("\n")
    )
}

/// Render the lessons section: deduplicated, bounded, most recent first.
pub fn render_lesson_block(lessons: &[MemoryItem]) -> String {
    let mut seen: Vec<String> = Vec::new();
    let mut lines: Vec<String> = Vec::new();
    let mut used = 0usize;

    for lesson in lessons {
        if lines.len() >= MAX_HARNESS_LESSONS {
            break;
        }
        let value = one_line(&lesson.value);
        if value.is_empty() || seen.contains(&value) {
            continue;
        }
        let line = format!("- {}", clip(&value, MAX_FACT_CHARS));
        if used + line.chars().count() + 1 > MAX_MEMORY_BLOCK_CHARS / 2 {
            break;
        }
        used += line.chars().count() + 1;
        seen.push(value);
        lines.push(line);
    }

    if lines.is_empty() {
        return String::new();
    }
    format!(
        "\n### Lessons From Earlier Turns (do not repeat these):\n{}\n",
        lines.join("\n")
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::Utc;

    fn item(key: &str, value: &str, category: &str) -> MemoryItem {
        MemoryItem {
            key: key.to_string(),
            value: value.to_string(),
            category: category.to_string(),
            updated_at: Utc::now(),
        }
    }

    #[test]
    fn caps_at_forty_facts() {
        let memories: Vec<MemoryItem> = (0..120)
            .map(|i| item(&format!("k{i}"), &format!("fact number {i}"), "facts"))
            .collect();
        let block = render_memory_block(&memories);
        assert_eq!(block.matches("\n- ").count(), MAX_HARNESS_FACTS);
    }

    #[test]
    fn duplicate_values_render_once() {
        let memories = vec![
            item("memory:facts:a", "prefers tabs", "facts"),
            item("memory:facts:b", "prefers tabs", "facts"),
            item("memory:facts:c", "prefers tabs.", "facts"),
        ];
        let block = render_memory_block(&memories);
        assert_eq!(block.matches("prefers tabs").count(), 2);
    }

    #[test]
    fn lessons_do_not_leak_into_the_fact_block() {
        let memories = vec![
            item(
                "lesson:deepseek:auth",
                "auth with provider deepseek",
                "lessons",
            ),
            item("ponytail", "clay accent", "project"),
        ];
        let block = render_memory_block(&memories);
        assert!(!block.contains("auth with provider deepseek"));
        assert!(block.contains("clay accent"));
    }

    #[test]
    fn plumbing_categories_are_hidden() {
        let memories = vec![
            item("plan_mode:s1", "false", "plan_mode"),
            item("todos:s1", "[{\"status\":\"pending\"}]", "todos"),
        ];
        assert!(render_memory_block(&memories).is_empty());
    }

    #[test]
    fn long_values_are_clipped_with_a_marker() {
        let memories = vec![item("k", &"x".repeat(MAX_FACT_CHARS * 3), "facts")];
        let block = render_memory_block(&memories);
        assert!(block.contains("more characters stored"));
        assert!(block.chars().count() < MAX_FACT_CHARS + 200);
    }

    #[test]
    fn multiline_values_collapse_to_one_line() {
        let memories = vec![item("k", "line one\nline two\n### Fake Section", "facts")];
        let block = render_memory_block(&memories);
        assert_eq!(block.matches("\n- ").count(), 1);
        assert!(!block.contains("\n### Fake Section"));
    }

    #[test]
    fn block_respects_the_character_budget() {
        let memories: Vec<MemoryItem> = (0..40)
            .map(|i| item(&format!("k{i}"), &"y".repeat(MAX_FACT_CHARS), "facts"))
            .collect();
        let block = render_memory_block(&memories);
        assert!(block.chars().count() <= MAX_MEMORY_BLOCK_CHARS + 120);
    }

    #[test]
    fn lesson_block_dedupes_and_bounds() {
        let mut lessons: Vec<MemoryItem> = (0..40)
            .map(|i| {
                item(
                    &format!("lesson:p{i}:auth"),
                    &format!("auth case {i}"),
                    "lessons",
                )
            })
            .collect();
        lessons.push(item("lesson:dup", "auth case 0", "lessons"));
        let block = render_lesson_block(&lessons);
        assert_eq!(block.matches("\n- ").count(), MAX_HARNESS_LESSONS);
    }

    #[test]
    fn empty_memory_renders_nothing() {
        assert!(render_memory_block(&[]).is_empty());
        assert!(render_lesson_block(&[]).is_empty());
    }
}
