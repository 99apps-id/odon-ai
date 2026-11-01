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
        let key = args.get("key").and_then(|k| k.as_str()).ok_or_else(|| {
            OdonError::Tool {
                tool: self.name().to_string(),
                message: "Missing 'key' parameter".to_string(),
            }
        })?;
        let value = args.get("value").and_then(|v| v.as_str()).ok_or_else(|| {
            OdonError::Tool {
                tool: self.name().to_string(),
                message: "Missing 'value' parameter".to_string(),
            }
        })?;
        let category = args.get("category").and_then(|c| c.as_str()).unwrap_or("general");

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
                let items = storage.list_memories(cat_opt).map_err(|e| OdonError::Tool {
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
