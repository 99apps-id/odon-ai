use async_trait::async_trait;
use odon_core::{OdonError, Result};
use odon_security::RiskTier;
use serde_json::{json, Value};
use std::path::{Path, PathBuf};

use crate::registry::{Tool, ToolContext};

fn resolve_path(root: &str, relative: &str) -> PathBuf {
    let p = Path::new(relative);
    if p.is_absolute() {
        p.to_path_buf()
    } else {
        Path::new(root).join(relative)
    }
}

pub struct FsReadTool;

#[async_trait]
impl Tool for FsReadTool {
    fn name(&self) -> &str {
        "fs_read"
    }
    fn description(&self) -> &str {
        "Read content of a file at the specified path"
    }
    fn parameters_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "path": { "type": "string", "description": "File path to read" }
            },
            "required": ["path"]
        })
    }
    fn risk_tier(&self) -> RiskTier {
        RiskTier::Tier0ReadOnly
    }
    async fn execute(&self, ctx: &ToolContext, args: Value) -> Result<Value> {
        let path_str = args.get("path").and_then(|p| p.as_str()).ok_or_else(|| {
            OdonError::Tool {
                tool: self.name().to_string(),
                message: "Missing 'path' parameter".to_string(),
            }
        })?;

        let full_path = resolve_path(&ctx.workspace_root, path_str);
        let content = tokio::fs::read_to_string(&full_path)
            .await
            .map_err(|e| OdonError::Tool {
                tool: self.name().to_string(),
                message: format!("Failed to read file {}: {}", path_str, e),
            })?;

        Ok(json!({ "path": path_str, "content": content }))
    }
}

pub struct FsWriteTool;

#[async_trait]
impl Tool for FsWriteTool {
    fn name(&self) -> &str {
        "fs_write"
    }
    fn description(&self) -> &str {
        "Write content to a file, creating parent directories if needed"
    }
    fn parameters_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "path": { "type": "string", "description": "Target file path" },
                "content": { "type": "string", "description": "Text content to write" }
            },
            "required": ["path", "content"]
        })
    }
    fn risk_tier(&self) -> RiskTier {
        RiskTier::Tier1WorkspaceMutating
    }
    async fn execute(&self, ctx: &ToolContext, args: Value) -> Result<Value> {
        let path_str = args.get("path").and_then(|p| p.as_str()).ok_or_else(|| {
            OdonError::Tool {
                tool: self.name().to_string(),
                message: "Missing 'path' parameter".to_string(),
            }
        })?;
        let content = args
            .get("content")
            .and_then(|c| c.as_str())
            .ok_or_else(|| OdonError::Tool {
                tool: self.name().to_string(),
                message: "Missing 'content' parameter".to_string(),
            })?;

        let full_path = resolve_path(&ctx.workspace_root, path_str);
        if let Some(parent) = full_path.parent() {
            let _ = tokio::fs::create_dir_all(parent).await;
        }

        tokio::fs::write(&full_path, content)
            .await
            .map_err(|e| OdonError::Tool {
                tool: self.name().to_string(),
                message: format!("Failed to write file {}: {}", path_str, e),
            })?;

        Ok(json!({ "path": path_str, "bytes_written": content.len() }))
    }
}

pub struct FsListTool;

#[async_trait]
impl Tool for FsListTool {
    fn name(&self) -> &str {
        "fs_list"
    }
    fn description(&self) -> &str {
        "List files and directories in a directory"
    }
    fn parameters_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "path": { "type": "string", "description": "Directory path (defaults to workspace root)" }
            }
        })
    }
    fn risk_tier(&self) -> RiskTier {
        RiskTier::Tier0ReadOnly
    }
    async fn execute(&self, ctx: &ToolContext, args: Value) -> Result<Value> {
        let dir_str = args.get("path").and_then(|p| p.as_str()).unwrap_or(".");
        let full_path = resolve_path(&ctx.workspace_root, dir_str);

        let mut read_dir = tokio::fs::read_dir(&full_path)
            .await
            .map_err(|e| OdonError::Tool {
                tool: self.name().to_string(),
                message: format!("Failed to list directory {}: {}", dir_str, e),
            })?;

        let mut entries = Vec::new();
        while let Ok(Some(entry)) = read_dir.next_entry().await {
            let name = entry.file_name().to_string_lossy().to_string();
            let file_type = entry.file_type().await.ok();
            let is_dir = file_type.map(|t| t.is_dir()).unwrap_or(false);
            entries.push(json!({
                "name": name,
                "is_dir": is_dir,
            }));
        }

        Ok(json!({ "path": dir_str, "entries": entries }))
    }
}
