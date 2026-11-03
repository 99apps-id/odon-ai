use async_trait::async_trait;
use odon_core::{OdonError, Result};
use odon_security::RiskTier;
use serde_json::{json, Value};
use std::path::{Component, Path, PathBuf};

use crate::registry::{Tool, ToolContext};

fn resolve_path(root: &str, relative: &str) -> Result<PathBuf> {
    let p = Path::new(relative);
    if p.is_absolute()
        || p.components().any(|component| {
            matches!(
                component,
                Component::ParentDir | Component::RootDir | Component::Prefix(_)
            )
        })
    {
        return Err(OdonError::Tool {
            tool: "filesystem".to_string(),
            message: format!("Path must stay inside the workspace: {relative}"),
        });
    }

    let canonical_root = std::fs::canonicalize(root).map_err(|e| OdonError::Tool {
        tool: "filesystem".to_string(),
        message: format!("Failed to resolve workspace root: {e}"),
    })?;
    let candidate = canonical_root.join(p);

    // Canonicalize the target when it exists. For a new path, canonicalize its
    // nearest existing ancestor so an in-workspace symlink cannot escape root.
    let mut existing = candidate.as_path();
    while std::fs::symlink_metadata(existing).is_err() {
        existing = existing.parent().ok_or_else(|| OdonError::Tool {
            tool: "filesystem".to_string(),
            message: format!("Invalid workspace path: {relative}"),
        })?;
    }
    let canonical_existing = std::fs::canonicalize(existing).map_err(|e| OdonError::Tool {
        tool: "filesystem".to_string(),
        message: format!("Failed to resolve path {relative}: {e}"),
    })?;
    if !canonical_existing.starts_with(&canonical_root) {
        return Err(OdonError::Tool {
            tool: "filesystem".to_string(),
            message: format!("Path escapes the workspace: {relative}"),
        });
    }

    Ok(candidate)
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
        let path_str =
            args.get("path")
                .and_then(|p| p.as_str())
                .ok_or_else(|| OdonError::Tool {
                    tool: self.name().to_string(),
                    message: "Missing 'path' parameter".to_string(),
                })?;

        let full_path = resolve_path(&ctx.workspace_root, path_str)?;
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
        let path_str =
            args.get("path")
                .and_then(|p| p.as_str())
                .ok_or_else(|| OdonError::Tool {
                    tool: self.name().to_string(),
                    message: "Missing 'path' parameter".to_string(),
                })?;
        let content = args
            .get("content")
            .and_then(|c| c.as_str())
            .ok_or_else(|| OdonError::Tool {
                tool: self.name().to_string(),
                message: "Missing 'content' parameter".to_string(),
            })?;

        let full_path = resolve_path(&ctx.workspace_root, path_str)?;
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
        let full_path = resolve_path(&ctx.workspace_root, dir_str)?;

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
        let path_str =
            args.get("path")
                .and_then(|p| p.as_str())
                .ok_or_else(|| OdonError::Tool {
                    tool: self.name().to_string(),
                    message: "Missing 'path' parameter".to_string(),
                })?;
        let target =
            args.get("target")
                .and_then(|t| t.as_str())
                .ok_or_else(|| OdonError::Tool {
                    tool: self.name().to_string(),
                    message: "Missing 'target' parameter".to_string(),
                })?;
        let replacement = args
            .get("replacement")
            .and_then(|r| r.as_str())
            .ok_or_else(|| OdonError::Tool {
                tool: self.name().to_string(),
                message: "Missing 'replacement' parameter".to_string(),
            })?;

        let full_path = resolve_path(&ctx.workspace_root, path_str)?;
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
        let query = args
            .get("query")
            .and_then(|q| q.as_str())
            .ok_or_else(|| OdonError::Tool {
                tool: self.name().to_string(),
                message: "Missing 'query' parameter".to_string(),
            })?;
        let subpath = args.get("path").and_then(|p| p.as_str()).unwrap_or(".");
        let limit = args.get("limit").and_then(|l| l.as_u64()).unwrap_or(30) as usize;

        let root_dir = resolve_path(&ctx.workspace_root, subpath)?;
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

                if file_name.starts_with('.')
                    || file_name == "target"
                    || file_name == "node_modules"
                    || file_name == "dist"
                {
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
                                    let rel_path = path
                                        .strip_prefix(&root_dir)
                                        .unwrap_or(&path)
                                        .to_string_lossy()
                                        .to_string();
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
        let path_str =
            args.get("path")
                .and_then(|p| p.as_str())
                .ok_or_else(|| OdonError::Tool {
                    tool: self.name().to_string(),
                    message: "Missing 'path' parameter".to_string(),
                })?;

        let full_path = resolve_path(&ctx.workspace_root, path_str)?;
        tokio::fs::remove_file(&full_path)
            .await
            .map_err(|e| OdonError::Tool {
                tool: self.name().to_string(),
                message: format!("Failed to delete file {}: {}", path_str, e),
            })?;

        Ok(json!({ "path": path_str, "status": "deleted" }))
    }
}

pub struct FsMkdirTool;

#[async_trait]
impl Tool for FsMkdirTool {
    fn name(&self) -> &str {
        "create_directory"
    }
    fn description(&self) -> &str {
        "Create a directory (and any missing parents) inside workspace"
    }
    fn parameters_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "path": { "type": "string", "description": "Directory path to create" }
            },
            "required": ["path"]
        })
    }
    fn risk_tier(&self) -> RiskTier {
        RiskTier::Tier1WorkspaceMutating
    }
    async fn execute(&self, ctx: &ToolContext, args: Value) -> Result<Value> {
        let path_str =
            args.get("path")
                .and_then(|p| p.as_str())
                .ok_or_else(|| OdonError::Tool {
                    tool: self.name().to_string(),
                    message: "Missing 'path' parameter".to_string(),
                })?;

        let full_path = resolve_path(&ctx.workspace_root, path_str)?;
        tokio::fs::create_dir_all(&full_path)
            .await
            .map_err(|e| OdonError::Tool {
                tool: self.name().to_string(),
                message: format!("Failed to create directory {}: {}", path_str, e),
            })?;

        Ok(json!({ "path": path_str, "status": "created" }))
    }
}

pub struct FsMoveTool;

#[async_trait]
impl Tool for FsMoveTool {
    fn name(&self) -> &str {
        "move_file"
    }
    fn description(&self) -> &str {
        "Move or rename a file or directory within workspace"
    }
    fn parameters_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "from": { "type": "string", "description": "Source path relative to workspace" },
                "to": { "type": "string", "description": "Destination path relative to workspace" }
            },
            "required": ["from", "to"]
        })
    }
    fn risk_tier(&self) -> RiskTier {
        RiskTier::Tier1WorkspaceMutating
    }
    async fn execute(&self, ctx: &ToolContext, args: Value) -> Result<Value> {
        let from_str =
            args.get("from")
                .and_then(|p| p.as_str())
                .ok_or_else(|| OdonError::Tool {
                    tool: self.name().to_string(),
                    message: "Missing 'from' parameter".to_string(),
                })?;
        let to_str = args
            .get("to")
            .and_then(|p| p.as_str())
            .ok_or_else(|| OdonError::Tool {
                tool: self.name().to_string(),
                message: "Missing 'to' parameter".to_string(),
            })?;

        let from_path = resolve_path(&ctx.workspace_root, from_str)?;
        let to_path = resolve_path(&ctx.workspace_root, to_str)?;

        if let Some(parent) = to_path.parent() {
            let _ = tokio::fs::create_dir_all(parent).await;
        }

        tokio::fs::rename(&from_path, &to_path)
            .await
            .map_err(|e| OdonError::Tool {
                tool: self.name().to_string(),
                message: format!("Failed to move {} to {}: {}", from_str, to_str, e),
            })?;

        Ok(json!({ "from": from_str, "to": to_str, "status": "moved" }))
    }
}

pub struct FsCopyTool;

#[async_trait]
impl Tool for FsCopyTool {
    fn name(&self) -> &str {
        "copy_file"
    }
    fn description(&self) -> &str {
        "Copy a file inside the workspace"
    }
    fn parameters_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "from": { "type": "string", "description": "Source file path" },
                "to": { "type": "string", "description": "Destination file path" }
            },
            "required": ["from", "to"]
        })
    }
    fn risk_tier(&self) -> RiskTier {
        RiskTier::Tier1WorkspaceMutating
    }
    async fn execute(&self, ctx: &ToolContext, args: Value) -> Result<Value> {
        let from_str =
            args.get("from")
                .and_then(|p| p.as_str())
                .ok_or_else(|| OdonError::Tool {
                    tool: self.name().to_string(),
                    message: "Missing 'from' parameter".to_string(),
                })?;
        let to_str = args
            .get("to")
            .and_then(|p| p.as_str())
            .ok_or_else(|| OdonError::Tool {
                tool: self.name().to_string(),
                message: "Missing 'to' parameter".to_string(),
            })?;

        let from_path = resolve_path(&ctx.workspace_root, from_str)?;
        let to_path = resolve_path(&ctx.workspace_root, to_str)?;

        if let Some(parent) = to_path.parent() {
            let _ = tokio::fs::create_dir_all(parent).await;
        }

        let bytes = tokio::fs::copy(&from_path, &to_path)
            .await
            .map_err(|e| OdonError::Tool {
                tool: self.name().to_string(),
                message: format!("Failed to copy {} to {}: {}", from_str, to_str, e),
            })?;

        Ok(json!({ "from": from_str, "to": to_str, "bytes_copied": bytes, "status": "copied" }))
    }
}

pub struct FsGlobTool;

#[async_trait]
impl Tool for FsGlobTool {
    fn name(&self) -> &str {
        "glob"
    }
    fn description(&self) -> &str {
        "Find files matching glob pattern inside workspace (e.g. '**/*.rs', 'src/**/*.tsx')"
    }
    fn parameters_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "pattern": { "type": "string", "description": "Glob or extension pattern" },
                "path": { "type": "string", "description": "Subdirectory to scan (defaults to workspace root)" }
            },
            "required": ["pattern"]
        })
    }
    fn risk_tier(&self) -> RiskTier {
        RiskTier::Tier0ReadOnly
    }
    async fn execute(&self, ctx: &ToolContext, args: Value) -> Result<Value> {
        let pattern = args.get("pattern").and_then(|p| p.as_str()).unwrap_or("*");
        let subpath = args.get("path").and_then(|p| p.as_str()).unwrap_or(".");
        let root_dir = resolve_path(&ctx.workspace_root, subpath)?;

        let mut matches = Vec::new();
        let mut queue = vec![root_dir.clone()];

        let clean_pat = pattern.trim_start_matches("**/").trim_start_matches('*');

        while let Some(current_dir) = queue.pop() {
            if matches.len() >= 100 {
                break;
            }
            let mut read_dir = match tokio::fs::read_dir(&current_dir).await {
                Ok(rd) => rd,
                Err(_) => continue,
            };

            while let Ok(Some(entry)) = read_dir.next_entry().await {
                if matches.len() >= 100 {
                    break;
                }
                let path = entry.path();
                let file_name = entry.file_name().to_string_lossy().to_string();

                if file_name.starts_with('.')
                    || file_name == "target"
                    || file_name == "node_modules"
                {
                    continue;
                }

                if let Ok(file_type) = entry.file_type().await {
                    if file_type.is_dir() {
                        queue.push(path);
                    } else if file_type.is_file()
                        && (clean_pat.is_empty()
                            || file_name.ends_with(clean_pat)
                            || file_name.contains(clean_pat))
                    {
                        let rel = path
                            .strip_prefix(&root_dir)
                            .unwrap_or(&path)
                            .to_string_lossy()
                            .to_string();
                        matches.push(rel);
                    }
                }
            }
        }

        Ok(json!({ "pattern": pattern, "count": matches.len(), "matches": matches }))
    }
}

pub struct MultiEditTool;

#[async_trait]
impl Tool for MultiEditTool {
    fn name(&self) -> &str {
        "multi_edit"
    }
    fn description(&self) -> &str {
        "Apply several exact-string replacements to a single file atomically"
    }
    fn parameters_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "path": { "type": "string", "description": "File path to edit" },
                "edits": {
                    "type": "array",
                    "items": {
                        "type": "object",
                        "properties": {
                            "old_string": { "type": "string" },
                            "new_string": { "type": "string" }
                        },
                        "required": ["old_string", "new_string"]
                    }
                }
            },
            "required": ["path", "edits"]
        })
    }
    fn risk_tier(&self) -> RiskTier {
        RiskTier::Tier1WorkspaceMutating
    }
    async fn execute(&self, ctx: &ToolContext, args: Value) -> Result<Value> {
        let path_str =
            args.get("path")
                .and_then(|p| p.as_str())
                .ok_or_else(|| OdonError::Tool {
                    tool: self.name().to_string(),
                    message: "Missing 'path' parameter".to_string(),
                })?;
        let edits = args
            .get("edits")
            .and_then(|e| e.as_array())
            .ok_or_else(|| OdonError::Tool {
                tool: self.name().to_string(),
                message: "Missing 'edits' array parameter".to_string(),
            })?;

        let full_path = resolve_path(&ctx.workspace_root, path_str)?;
        let mut content =
            tokio::fs::read_to_string(&full_path)
                .await
                .map_err(|e| OdonError::Tool {
                    tool: self.name().to_string(),
                    message: format!("Failed to read file {}: {}", path_str, e),
                })?;

        let mut applied = 0;
        for edit in edits {
            let old_str = edit
                .get("old_string")
                .and_then(|s| s.as_str())
                .unwrap_or("");
            let new_str = edit
                .get("new_string")
                .and_then(|s| s.as_str())
                .unwrap_or("");
            if !old_str.is_empty() && content.contains(old_str) {
                content = content.replacen(old_str, new_str, 1);
                applied += 1;
            }
        }

        tokio::fs::write(&full_path, content)
            .await
            .map_err(|e| OdonError::Tool {
                tool: self.name().to_string(),
                message: format!("Failed to write edited file {}: {}", path_str, e),
            })?;

        Ok(json!({ "path": path_str, "applied_edits": applied, "status": "success" }))
    }
}

// Aliases matching Termigo conventions
pub struct TermigoReadFileTool;
#[async_trait]
impl Tool for TermigoReadFileTool {
    fn name(&self) -> &str {
        "read_file"
    }
    fn description(&self) -> &str {
        "Read content of a file at the specified path"
    }
    fn parameters_schema(&self) -> Value {
        FsReadTool.parameters_schema()
    }
    fn risk_tier(&self) -> RiskTier {
        RiskTier::Tier0ReadOnly
    }
    async fn execute(&self, ctx: &ToolContext, args: Value) -> Result<Value> {
        FsReadTool.execute(ctx, args).await
    }
}

pub struct TermigoWriteFileTool;
#[async_trait]
impl Tool for TermigoWriteFileTool {
    fn name(&self) -> &str {
        "write_file"
    }
    fn description(&self) -> &str {
        "Create or overwrite a file with the given content"
    }
    fn parameters_schema(&self) -> Value {
        FsWriteTool.parameters_schema()
    }
    fn risk_tier(&self) -> RiskTier {
        RiskTier::Tier1WorkspaceMutating
    }
    async fn execute(&self, ctx: &ToolContext, args: Value) -> Result<Value> {
        FsWriteTool.execute(ctx, args).await
    }
}

pub struct TermigoEditTool;
#[async_trait]
impl Tool for TermigoEditTool {
    fn name(&self) -> &str {
        "edit"
    }
    fn description(&self) -> &str {
        "Replace target text in a file with replacement text"
    }
    fn parameters_schema(&self) -> Value {
        FsEditTool.parameters_schema()
    }
    fn risk_tier(&self) -> RiskTier {
        RiskTier::Tier1WorkspaceMutating
    }
    async fn execute(&self, ctx: &ToolContext, args: Value) -> Result<Value> {
        FsEditTool.execute(ctx, args).await
    }
}

pub struct TermigoListDirTool;
#[async_trait]
impl Tool for TermigoListDirTool {
    fn name(&self) -> &str {
        "list_directory"
    }
    fn description(&self) -> &str {
        "List immediate entries in a directory"
    }
    fn parameters_schema(&self) -> Value {
        FsListTool.parameters_schema()
    }
    fn risk_tier(&self) -> RiskTier {
        RiskTier::Tier0ReadOnly
    }
    async fn execute(&self, ctx: &ToolContext, args: Value) -> Result<Value> {
        FsListTool.execute(ctx, args).await
    }
}

pub struct TermigoDeleteFileTool;
#[async_trait]
impl Tool for TermigoDeleteFileTool {
    fn name(&self) -> &str {
        "delete_file"
    }
    fn description(&self) -> &str {
        "Delete a file or directory"
    }
    fn parameters_schema(&self) -> Value {
        FsDeleteTool.parameters_schema()
    }
    fn risk_tier(&self) -> RiskTier {
        RiskTier::Tier2SystemMutating
    }
    async fn execute(&self, ctx: &ToolContext, args: Value) -> Result<Value> {
        FsDeleteTool.execute(ctx, args).await
    }
}

pub struct TermigoGrepTool;
#[async_trait]
impl Tool for TermigoGrepTool {
    fn name(&self) -> &str {
        "grep"
    }
    fn description(&self) -> &str {
        "Search file contents in the workspace using text query"
    }
    fn parameters_schema(&self) -> Value {
        FsSearchTool.parameters_schema()
    }
    fn risk_tier(&self) -> RiskTier {
        RiskTier::Tier0ReadOnly
    }
    async fn execute(&self, ctx: &ToolContext, args: Value) -> Result<Value> {
        FsSearchTool.execute(ctx, args).await
    }
}

#[cfg(test)]
mod tests {
    use super::resolve_path;

    #[test]
    fn rejects_absolute_and_parent_paths() {
        let root = std::env::temp_dir().join(format!("odon-fs-test-{}", std::process::id()));
        std::fs::create_dir_all(&root).unwrap();
        let root_str = root.to_string_lossy();

        assert!(resolve_path(&root_str, "../outside.txt").is_err());
        assert!(resolve_path(&root_str, &root.join("absolute.txt").to_string_lossy()).is_err());
        assert!(resolve_path(&root_str, "nested/file.txt").is_ok());

        std::fs::remove_dir_all(root).unwrap();
    }
}
