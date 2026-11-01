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

pub struct FsEditTool;

#[async_trait]
impl Tool for FsEditTool {
    fn name(&self) -> &str {
        "fs_edit"
    }
    fn description(&self) -> &str {
        "Surgically edit a file by finding a target string and replacing it with a replacement"
    }
    fn parameters_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "path": { "type": "string", "description": "Target file path" },
                "target": { "type": "string", "description": "Exact text sequence to be replaced" },
                "replacement": { "type": "string", "description": "Replacement text content" }
            },
            "required": ["path", "target", "replacement"]
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
        let target = args.get("target").and_then(|t| t.as_str()).ok_or_else(|| {
            OdonError::Tool {
                tool: self.name().to_string(),
                message: "Missing 'target' parameter".to_string(),
            }
        })?;
        let replacement = args.get("replacement").and_then(|r| r.as_str()).ok_or_else(|| {
            OdonError::Tool {
                tool: self.name().to_string(),
                message: "Missing 'replacement' parameter".to_string(),
            }
        })?;

        let full_path = resolve_path(&ctx.workspace_root, path_str);
        let content = tokio::fs::read_to_string(&full_path)
            .await
            .map_err(|e| OdonError::Tool {
                tool: self.name().to_string(),
                message: format!("Failed to read file {}: {}", path_str, e),
            })?;

        if !content.contains(target) {
            return Err(OdonError::Tool {
                tool: self.name().to_string(),
                message: format!("Target sequence not found in file {}", path_str),
            });
        }

        let occurrences = content.matches(target).count();
        let new_content = content.replace(target, replacement);

        tokio::fs::write(&full_path, new_content)
            .await
            .map_err(|e| OdonError::Tool {
                tool: self.name().to_string(),
                message: format!("Failed to write edited file {}: {}", path_str, e),
            })?;

        Ok(json!({
            "path": path_str,
            "replacements": occurrences,
            "status": "success"
        }))
    }
}

pub struct FsSearchTool;

#[async_trait]
impl Tool for FsSearchTool {
    fn name(&self) -> &str {
        "fs_search"
    }
    fn description(&self) -> &str {
        "Recursively search for text or patterns in workspace files"
    }
    fn parameters_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "query": { "type": "string", "description": "Text or regex pattern to search for" },
                "path": { "type": "string", "description": "Subdirectory to search (defaults to workspace root)" },
                "limit": { "type": "integer", "description": "Maximum number of results to return (default 30)" }
            },
            "required": ["query"]
        })
    }
    fn risk_tier(&self) -> RiskTier {
        RiskTier::Tier0ReadOnly
    }
    async fn execute(&self, ctx: &ToolContext, args: Value) -> Result<Value> {
        let query = args.get("query").and_then(|q| q.as_str()).ok_or_else(|| {
            OdonError::Tool {
                tool: self.name().to_string(),
                message: "Missing 'query' parameter".to_string(),
            }
        })?;
        let subpath = args.get("path").and_then(|p| p.as_str()).unwrap_or(".");
        let limit = args.get("limit").and_then(|l| l.as_u64()).unwrap_or(30) as usize;

        let root_dir = resolve_path(&ctx.workspace_root, subpath);
        let mut matches = Vec::new();

        let mut queue = vec![root_dir.clone()];
        while let Some(current_dir) = queue.pop() {
            if matches.len() >= limit {
                break;
            }
            let mut read_dir = match tokio::fs::read_dir(&current_dir).await {
                Ok(rd) => rd,
                Err(_) => continue,
            };

            while let Ok(Some(entry)) = read_dir.next_entry().await {
                if matches.len() >= limit {
                    break;
                }
                let path = entry.path();
                let file_name = entry.file_name().to_string_lossy().to_string();

                if file_name.starts_with('.') || file_name == "target" || file_name == "node_modules" || file_name == "dist" {
                    continue;
                }

                if let Ok(file_type) = entry.file_type().await {
                    if file_type.is_dir() {
                        queue.push(path);
                    } else if file_type.is_file() {
                        // Check if file is text and size is reasonable (< 2MB)
                        if let Ok(metadata) = entry.metadata().await {
                            if metadata.len() > 2_000_000 {
                                continue;
                            }
                        }

                        if let Ok(content) = tokio::fs::read_to_string(&path).await {
                            for (line_idx, line) in content.lines().enumerate() {
                                if line.contains(query) {
                                    let rel_path = path.strip_prefix(&root_dir).unwrap_or(&path).to_string_lossy().to_string();
                                    matches.push(json!({
                                        "file": rel_path,
                                        "line": line_idx + 1,
                                        "content": line.trim()
                                    }));
                                    if matches.len() >= limit {
                                        break;
                                    }
                                }
                            }
                        }
                    }
                }
            }
        }

        Ok(json!({
            "query": query,
            "results_count": matches.len(),
            "matches": matches
        }))
    }
}

pub struct FsDeleteTool;

#[async_trait]
impl Tool for FsDeleteTool {
    fn name(&self) -> &str {
        "fs_delete"
    }
    fn description(&self) -> &str {
        "Delete a file from the workspace (irreversible)"
    }
    fn parameters_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "path": { "type": "string", "description": "Path to file to delete" }
            },
            "required": ["path"]
        })
    }
    fn risk_tier(&self) -> RiskTier {
        RiskTier::Tier2SystemMutating
    }
    async fn execute(&self, ctx: &ToolContext, args: Value) -> Result<Value> {
        let path_str = args.get("path").and_then(|p| p.as_str()).ok_or_else(|| {
            OdonError::Tool {
                tool: self.name().to_string(),
                message: "Missing 'path' parameter".to_string(),
            }
        })?;

        let full_path = resolve_path(&ctx.workspace_root, path_str);
        tokio::fs::remove_file(&full_path)
            .await
            .map_err(|e| OdonError::Tool {
                tool: self.name().to_string(),
                message: format!("Failed to delete file {}: {}", path_str, e),
            })?;

        Ok(json!({ "path": path_str, "status": "deleted" }))
    }
}
