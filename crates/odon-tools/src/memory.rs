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

            let mut scored_matches: Vec<(u32, odon_storage::MemoryItem)> = Vec::new();
            for item in items {
                let key_lower = item.key.to_lowercase();
                let val_lower = item.value.to_lowercase();
                let mut score = 0u32;

                if key_lower == q {
                    score += 100;
                } else if key_lower.contains(&q) {
                    score += 50;
                }

                if val_lower.contains(&q) {
                    score += 25;
                }

                if score > 0 {
                    scored_matches.push((score, item));
                }
            }

            scored_matches.sort_by_key(|item| std::cmp::Reverse(item.0));

            let results: Vec<Value> = scored_matches
                .into_iter()
                .map(|(score, item)| {
                    json!({
                        "key": item.key,
                        "value": item.value,
                        "category": item.category,
                        "score": score,
                        "updated_at": item.updated_at.to_rfc3339()
                    })
                })
                .collect();

            return Ok(json!({
                "query": q,
                "category": cat,
                "count": results.len(),
                "matches": results
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
