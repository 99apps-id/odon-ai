use async_trait::async_trait;
use odon_core::{OdonError, Result};
use odon_security::RiskTier;
use serde_json::{json, Value};

use crate::registry::{Tool, ToolContext};

pub struct MemoryStoreTool;

#[async_trait]
impl Tool for MemoryStoreTool {
    fn name(&self) -> &str {
        "memory_store"
    }
    fn description(&self) -> &str {
        "Store important user preferences, architectural decisions, or facts in long-term memory"
    }
    fn parameters_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "key": { "type": "string", "description": "Unique key or topic identifier" },
                "value": { "type": "string", "description": "Information or memory text to retain" },
                "category": { "type": "string", "description": "Category (e.g. 'preference', 'project', 'decision', 'scratchpad')" }
            },
            "required": ["key", "value"]
        })
    }
    fn risk_tier(&self) -> RiskTier {
        RiskTier::Tier1WorkspaceMutating
    }
    async fn execute(&self, ctx: &ToolContext, args: Value) -> Result<Value> {
        let key = args
            .get("key")
            .and_then(|k| k.as_str())
            .ok_or_else(|| OdonError::Tool {
                tool: self.name().to_string(),
                message: "Missing 'key' parameter".to_string(),
            })?;
        let value = args
            .get("value")
            .and_then(|v| v.as_str())
            .ok_or_else(|| OdonError::Tool {
                tool: self.name().to_string(),
                message: "Missing 'value' parameter".to_string(),
            })?;
        let category = args
            .get("category")
            .and_then(|c| c.as_str())
            .unwrap_or("general");

        if is_reserved_memory_key(key) {
            return Err(OdonError::Tool {
                tool: self.name().to_string(),
                message: format!(
                    "'{key}' is a runtime-owned key and cannot be written from a turn"
                ),
            });
        }

        if let Some(storage) = &ctx.storage {
            storage
                .set_memory(key, value, category)
                .map_err(|e| OdonError::Tool {
                    tool: self.name().to_string(),
                    message: format!("Failed to store memory: {}", e),
                })?;

            Ok(json!({
                "status": "stored",
                "key": key,
                "category": category
            }))
        } else {
            Err(OdonError::Tool {
                tool: self.name().to_string(),
                message: "Storage engine unavailable in tool context".to_string(),
            })
        }
    }
}

/// Key prefixes the runtime owns.
///
/// A turn may write and delete ordinary memories freely, but these keys hold
/// the harness's own per-session state — plan mode above all. The flag is read
/// again before every tool call, so a `memory_forget` on one of these keys
/// lifts the session's own plan-mode restriction mid-turn, and a
/// `memory_store` switches another session's on. Both are model-reachable, so
/// the namespace is closed here instead of trusting the model to behave.
const RESERVED_KEY_PREFIXES: [&str; 1] = ["plan_mode:"];

/// Whether `key` belongs to the runtime rather than to the operator's memory.
pub fn is_reserved_memory_key(key: &str) -> bool {
    RESERVED_KEY_PREFIXES
        .iter()
        .any(|prefix| key.starts_with(prefix))
}

pub struct MemoryRecallTool;

#[async_trait]
impl Tool for MemoryRecallTool {
    fn name(&self) -> &str {
        "memory_recall"
    }
    fn description(&self) -> &str {
        "Recall stored memories, user preferences, or knowledge topics from long-term memory"
    }
    fn parameters_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "key": { "type": "string", "description": "Specific key to look up (optional)" },
                "category": { "type": "string", "description": "Category to filter memories by (optional)" }
            }
        })
    }
    fn risk_tier(&self) -> RiskTier {
        RiskTier::Tier0ReadOnly
    }
    async fn execute(&self, ctx: &ToolContext, args: Value) -> Result<Value> {
        if let Some(storage) = &ctx.storage {
            let key_opt = args.get("key").and_then(|k| k.as_str());
            let cat_opt = args.get("category").and_then(|c| c.as_str());

            if let Some(k) = key_opt {
                let item = storage.get_memory(k).map_err(|e| OdonError::Tool {
                    tool: self.name().to_string(),
                    message: format!("Failed to get memory: {}", e),
                })?;
                Ok(json!({
                    "key": k,
                    "memory": item
                }))
            } else {
                let items = storage
                    .list_memories(cat_opt)
                    .map_err(|e| OdonError::Tool {
                        tool: self.name().to_string(),
                        message: format!("Failed to list memories: {}", e),
                    })?;
                Ok(json!({
                    "category": cat_opt,
                    "count": items.len(),
                    "memories": items
                }))
            }
        } else {
            Err(OdonError::Tool {
                tool: self.name().to_string(),
                message: "Storage engine unavailable in tool context".to_string(),
            })
        }
    }
}

pub struct MemorySearchTool;

#[async_trait]
impl Tool for MemorySearchTool {
    fn name(&self) -> &str {
        "memory_search"
    }
    fn description(&self) -> &str {
        "Search through persistent long-term memories by keyword or concept with relevance scoring"
    }
    fn parameters_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "query": { "type": "string", "description": "Search keyword or phrase" },
                "category": { "type": "string", "description": "Optional category filter" }
            },
            "required": ["query"]
        })
    }
    fn risk_tier(&self) -> RiskTier {
        RiskTier::Tier0ReadOnly
    }
    async fn execute(&self, ctx: &ToolContext, args: Value) -> Result<Value> {
        let q = args
            .get("query")
            .and_then(|q| q.as_str())
            .unwrap_or("")
            .to_lowercase();
        let cat = args.get("category").and_then(|c| c.as_str());

        if let Some(storage) = &ctx.storage {
            let items = storage.list_memories(cat).map_err(|e| OdonError::Tool {
                tool: self.name().to_string(),
                message: format!("Failed to read memories: {e}"),
            })?;

            // Retrieval is term based, not substring based: a two word query
            // ("postgres connection") must still find a fact stored as
            // "connection: postgres", and a single shared token should not rank
            // as highly as a fact that covers every term. Scoring per term keeps
            // recall useful without pretending to be a semantic index.
            let terms = search_terms(&q);
            let mut scored_matches: Vec<(f32, usize, odon_storage::MemoryItem)> = Vec::new();
            for item in items {
                let key_lower = item.key.to_lowercase();
                let val_lower = item.value.to_lowercase();
                let mut score = 0.0f32;

                // Whole-query hits outrank scattered term hits.
                if key_lower == q {
                    score += 100.0;
                } else if !q.is_empty() && key_lower.contains(&q) {
                    score += 45.0;
                }
                if !q.is_empty() && val_lower.contains(&q) {
                    score += 25.0;
                }

                let mut matched = 0usize;
                for term in &terms {
                    if key_lower.contains(term.as_str()) {
                        matched += 1;
                        score += 12.0;
                    } else if val_lower.contains(term.as_str()) {
                        matched += 1;
                        score += 6.0;
                    }
                }

                if matched > 0 || score > 0.0 {
                    scored_matches.push((score, matched, item));
                }
            }

            // Coverage first, then weight: a fact that answers more of the query
            // is more useful than one with a single lucky token.
            scored_matches.sort_by(|a, b| {
                b.1.cmp(&a.1)
                    .then(b.0.partial_cmp(&a.0).unwrap_or(std::cmp::Ordering::Equal))
            });
            scored_matches.truncate(MAX_MEMORY_MATCHES);

            let result_count = scored_matches.len();
            let results: Vec<Value> = scored_matches
                .into_iter()
                .map(|(score, matched, item)| {
                    json!({
                        "key": item.key,
                        "value": item.value,
                        "category": item.category,
                        "score": score.round() as i64,
                        "terms_matched": matched,
                        "updated_at": item.updated_at.to_rfc3339()
                    })
                })
                .collect();

            return Ok(json!({
                "query": q,
                "category": cat,
                "count": result_count,
                "matches": results,
                // Zero matches is an answer, not an error: say plainly that the
                // fact is not stored instead of letting the model fill the gap.
                "note": if result_count == 0 {
                    Some(format!(
                        "no stored memory matched {terms:?}; the fact may never have been stored. \
            Say you do not have it, or ask the operator, instead of inventing a value."
                    ))
                } else {
                    None
                }
            }));
        }

        Err(OdonError::Tool {
            tool: self.name().to_string(),
            message: "Storage engine unavailable in tool context".to_string(),
        })
    }
}

pub struct MemoryForgetTool;

#[async_trait]
impl Tool for MemoryForgetTool {
    fn name(&self) -> &str {
        "memory_forget"
    }
    fn description(&self) -> &str {
        "Delete or forget a key from persistent long-term memory"
    }
    fn parameters_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "key": { "type": "string", "description": "Memory key to delete" }
            },
            "required": ["key"]
        })
    }
    fn risk_tier(&self) -> RiskTier {
        RiskTier::Tier1WorkspaceMutating
    }
    async fn execute(&self, ctx: &ToolContext, args: Value) -> Result<Value> {
        let key = args.get("key").and_then(|k| k.as_str()).unwrap_or("");
        if is_reserved_memory_key(key) {
            return Err(OdonError::Tool {
                tool: self.name().to_string(),
                message: format!(
                    "'{key}' is a runtime-owned key and cannot be deleted from a turn"
                ),
            });
        }
        let Some(storage) = &ctx.storage else {
            return Err(OdonError::Tool {
                tool: self.name().to_string(),
                message: "Storage engine unavailable in tool context".to_string(),
            });
        };
        let deleted = storage.delete_memory(key).map_err(|e| OdonError::Tool {
            tool: self.name().to_string(),
            message: format!("Failed to delete memory: {e}"),
        })?;
        Ok(json!({ "key": key, "forgotten": deleted }))
    }
}

/// How many memory rows a search returns at most.
///
/// The model pays for every row it reads, and a keyword that matches forty
/// facts is a keyword that needs narrowing, not a longer answer.
const MAX_MEMORY_MATCHES: usize = 25;

/// Split a query into the terms retrieval actually scores.
///
/// Lowercased, punctuation treated as a separator (so `postgres.connection` and
/// `postgres connection` behave the same), tokens shorter than two characters
/// dropped because single letters match nearly everything, and duplicates
/// removed so a repeated word cannot outweigh the rest of the query.
fn search_terms(query: &str) -> Vec<String> {
    let mut terms: Vec<String> = query
        .split(|c: char| !c.is_alphanumeric() && c != '_' && c != '-' && c != '/')
        .map(|term| term.trim_matches('-').to_lowercase())
        .filter(|term| term.chars().count() >= 2)
        .collect();
    terms.sort();
    terms.dedup();
    terms
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn terms_drop_punctuation_short_words_and_duplicates() {
        assert_eq!(
            search_terms("Postgres.connection, the of postgres"),
            vec![
                "connection".to_string(),
                "of".to_string(),
                "postgres".to_string(),
                "the".to_string()
            ]
        );
        assert!(search_terms("a b").is_empty());
    }

    #[test]
    fn multi_word_query_survives_word_order() {
        // The stored fact reads "connection: postgres"; the query is reversed.
        // Substring matching used to score zero here, which is exactly the case
        // where the model then claimed it had no memory of the setup.
        let terms = search_terms("postgres connection");
        let haystack = "connection: postgres on port 5432".to_lowercase();
        let matched = terms
            .iter()
            .filter(|term| haystack.contains(term.as_str()))
            .count();
        assert_eq!(matched, 2);
    }
}
