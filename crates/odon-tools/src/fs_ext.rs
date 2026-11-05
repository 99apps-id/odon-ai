use async_trait::async_trait;
use odon_core::{OdonError, Result};
use odon_security::RiskTier;
use serde_json::{json, Value};
use std::fs;
use std::path::{Component, Path, PathBuf};

use crate::registry::{Tool, ToolContext};

/// Charges `delta` bytes against an extraction budget.
///
/// Returns `true` while the running total is still within `limit`. Callers must
/// feed this the number of bytes *actually* decompressed, never the size the
/// archive header declares: a forged header can under-report its payload and
/// would otherwise let a zip bomb pass the cheap pre-check.
fn charge_extraction_budget(total: &mut u64, delta: u64, limit: u64) -> bool {
    *total = total.saturating_add(delta);
    *total <= limit
}

fn add_path_to_zip(
    zip: &mut zip::ZipWriter<std::fs::File>,
    base: &Path,
    path: &Path,
    options: zip::write::SimpleFileOptions,
) -> std::result::Result<(), String> {
    // Do not follow symlinks: a workspace symlink to ~/.ssh would otherwise
    // be archived verbatim into an in-workspace zip.
    let type_ = fs::symlink_metadata(path)
        .map_err(|e| e.to_string())?
        .file_type();
    if type_.is_dir() {
        let mut entries: Vec<_> = fs::read_dir(path)
            .map_err(|e| e.to_string())?
            .collect::<std::result::Result<Vec<_>, _>>()
            .map_err(|e| e.to_string())?;
        entries.sort_by_key(|entry| entry.path());
        for entry in entries {
            add_path_to_zip(zip, base, &entry.path(), options)?;
        }
    } else if type_.is_file() {
        let name = path
            .strip_prefix(base)
            .unwrap_or(path)
            .to_string_lossy()
            .replace('\\', "/");
        zip.start_file(name, options).map_err(|e| e.to_string())?;
        let mut file = fs::File::open(path).map_err(|e| e.to_string())?;
        std::io::copy(&mut file, zip).map_err(|e| e.to_string())?;
    }
    Ok(())
}

fn resolve_path(root: &str, relative: &str) -> Result<PathBuf> {
    let canonical_root = fs::canonicalize(root).map_err(|e| OdonError::Tool {
        tool: "filesystem_ext".to_string(),
        message: format!("Failed to resolve workspace root: {e}"),
    })?;
    let p = Path::new(relative);
    // fs_tree/fs_find are routinely handed the workspace root as an absolute
    // path, because that is how the harness prints it. Accept the absolute form
    // and let the containment check below decide; a path outside the workspace
    // is still rejected.
    let candidate = if p.is_absolute() {
        p.to_path_buf()
    } else {
        if p.components().any(|component| {
            matches!(
                component,
                Component::ParentDir | Component::RootDir | Component::Prefix(_)
            )
        }) {
            return Err(OdonError::Tool {
                tool: "filesystem_ext".to_string(),
                message: format!("Path must stay inside the workspace: {relative}"),
            });
        }
        canonical_root.join(p)
    };

    let mut existing = candidate.as_path();
    while std::fs::symlink_metadata(existing).is_err() {
        existing = existing.parent().ok_or_else(|| OdonError::Tool {
            tool: "filesystem_ext".to_string(),
            message: format!("Invalid workspace path: {relative}"),
        })?;
    }
    let canonical_existing = fs::canonicalize(existing).map_err(|e| OdonError::Tool {
        tool: "filesystem_ext".to_string(),
        message: format!("Failed to resolve path {relative}: {e}"),
    })?;
    if !canonical_existing.starts_with(&canonical_root) {
        return Err(OdonError::Tool {
            tool: "filesystem_ext".to_string(),
            message: format!("Path escapes the workspace: {relative}"),
        });
    }

    Ok(candidate)
}

// 1. fs_stat
pub struct FsStatTool;
#[async_trait]
impl Tool for FsStatTool {
    fn name(&self) -> &str {
        "fs_stat"
    }
    fn description(&self) -> &str {
        "Get comprehensive metadata and statistics for a file or directory"
    }
    fn parameters_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "path": { "type": "string", "description": "Relative file or directory path" }
            },
            "required": ["path"]
        })
    }
    fn risk_tier(&self) -> RiskTier {
        RiskTier::Tier0ReadOnly
    }
    async fn execute(&self, ctx: &ToolContext, args: Value) -> Result<Value> {
        let rel = args
            .get("path")
            .and_then(|p| p.as_str())
            .ok_or_else(|| OdonError::Tool {
                tool: self.name().to_string(),
                message: "Missing 'path' argument".to_string(),
            })?;
        let full = resolve_path(&ctx.workspace_root, rel)?;
        let meta = fs::metadata(&full).map_err(|e| OdonError::Tool {
            tool: self.name().to_string(),
            message: format!("Failed to read metadata for {rel}: {e}"),
        })?;

        let modified = meta.modified().ok().map(|t| {
            let dt: chrono::DateTime<chrono::Utc> = t.into();
            dt.to_rfc3339()
        });
        let created = meta.created().ok().map(|t| {
            let dt: chrono::DateTime<chrono::Utc> = t.into();
            dt.to_rfc3339()
        });

        Ok(json!({
            "path": rel,
            "is_dir": meta.is_dir(),
            "is_file": meta.is_file(),
            "is_symlink": meta.is_symlink(),
            "size_bytes": meta.len(),
            "readonly": meta.permissions().readonly(),
            "modified_at": modified,
            "created_at": created,
        }))
    }
}

// 2. fs_chmod
pub struct FsChmodTool;
#[async_trait]
impl Tool for FsChmodTool {
    fn name(&self) -> &str {
        "fs_chmod"
    }
    fn description(&self) -> &str {
        "Change file permissions or readonly state"
    }
    fn parameters_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "path": { "type": "string", "description": "Target file path" },
                "readonly": { "type": "boolean", "description": "Set readonly flag" }
            },
            "required": ["path"]
        })
    }
    fn risk_tier(&self) -> RiskTier {
        RiskTier::Tier1WorkspaceMutating
    }
    async fn execute(&self, ctx: &ToolContext, args: Value) -> Result<Value> {
        let rel = args.get("path").and_then(|p| p.as_str()).unwrap_or("");
        let readonly = args
            .get("readonly")
            .and_then(|r| r.as_bool())
            .unwrap_or(false);
        let full = resolve_path(&ctx.workspace_root, rel)?;

        let mut perms = fs::metadata(&full)
            .map_err(|e| OdonError::Tool {
                tool: self.name().to_string(),
                message: e.to_string(),
            })?
            .permissions();
        perms.set_readonly(readonly);
        fs::set_permissions(&full, perms).map_err(|e| OdonError::Tool {
            tool: self.name().to_string(),
            message: e.to_string(),
        })?;

        Ok(json!({ "path": rel, "readonly": readonly, "status": "updated" }))
    }
}

// 3. fs_owner_check (read-only; never claims to mutate ownership)
pub struct FsOwnerCheckTool;
#[async_trait]
impl Tool for FsOwnerCheckTool {
    fn name(&self) -> &str {
        "fs_owner_check"
    }
    fn description(&self) -> &str {
        "Inspect ownership and permission metadata for a workspace path without modifying it"
    }
    fn parameters_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "path": { "type": "string", "description": "File or folder path" }
            },
            "required": ["path"]
        })
    }
    fn risk_tier(&self) -> RiskTier {
        RiskTier::Tier0ReadOnly
    }
    async fn execute(&self, ctx: &ToolContext, args: Value) -> Result<Value> {
        let input = args.get("path").and_then(|p| p.as_str()).unwrap_or("");
        let path = crate::workspace_path::resolve_existing(&ctx.workspace_root, input)?;
        let metadata = std::fs::metadata(&path).map_err(|error| OdonError::Tool {
            tool: self.name().to_string(),
            message: format!("cannot read path metadata: {error}"),
        })?;
        let readonly = metadata.permissions().readonly();

        #[cfg(unix)]
        {
            use std::os::unix::fs::MetadataExt;
            use std::os::unix::fs::PermissionsExt;
            let mode = format!("{:o}", metadata.permissions().mode());
            return Ok(json!({
                "path": path,
                "uid": metadata.uid(),
                "gid": metadata.gid(),
                "mode": mode,
                "readonly": readonly,
                "is_file": metadata.is_file(),
                "is_dir": metadata.is_dir(),
                "status": "inspected"
            }));
        }

        #[cfg(not(unix))]
        {
            Ok(json!({
                "path": path,
                "readonly": readonly,
                "is_file": metadata.is_file(),
                "is_dir": metadata.is_dir(),
                "unix_ownership": "not applicable on this platform",
                "status": "inspected"
            }))
        }
    }
}

// 4. fs_symlink_create
pub struct FsSymlinkCreateTool;
#[async_trait]
impl Tool for FsSymlinkCreateTool {
    fn name(&self) -> &str {
        "fs_symlink_create"
    }
    fn description(&self) -> &str {
        "Create a symbolic link within the workspace"
    }
    fn parameters_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "target": { "type": "string", "description": "Target path the link points to" },
                "link_path": { "type": "string", "description": "Where to create the symlink" }
            },
            "required": ["target", "link_path"]
        })
    }
    fn risk_tier(&self) -> RiskTier {
        RiskTier::Tier1WorkspaceMutating
    }
    async fn execute(&self, ctx: &ToolContext, args: Value) -> Result<Value> {
        let target_rel = args.get("target").and_then(|t| t.as_str()).unwrap_or("");
        let link_rel = args.get("link_path").and_then(|l| l.as_str()).unwrap_or("");
        let target_full = resolve_path(&ctx.workspace_root, target_rel)?;
        let link_full = resolve_path(&ctx.workspace_root, link_rel)?;

        #[cfg(target_os = "windows")]
        {
            if target_full.is_dir() {
                std::os::windows::fs::symlink_dir(&target_full, &link_full).map_err(|e| {
                    OdonError::Tool {
                        tool: self.name().to_string(),
                        message: format!("Symlink failed: {e}"),
                    }
                })?;
            } else {
                std::os::windows::fs::symlink_file(&target_full, &link_full).map_err(|e| {
                    OdonError::Tool {
                        tool: self.name().to_string(),
                        message: format!("Symlink failed: {e}"),
                    }
                })?;
            }
        }
        #[cfg(not(target_os = "windows"))]
        {
            std::os::unix::fs::symlink(&target_full, &link_full).map_err(|e| OdonError::Tool {
                tool: self.name().to_string(),
                message: format!("Symlink failed: {e}"),
            })?;
        }

        Ok(json!({ "link_path": link_rel, "target": target_rel, "status": "created" }))
    }
}

// 5. fs_readlink
pub struct FsReadlinkTool;
#[async_trait]
impl Tool for FsReadlinkTool {
    fn name(&self) -> &str {
        "fs_readlink"
    }
    fn description(&self) -> &str {
        "Resolve target destination of a symbolic link"
    }
    fn parameters_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "link_path": { "type": "string", "description": "Symlink path" }
            },
            "required": ["link_path"]
        })
    }
    fn risk_tier(&self) -> RiskTier {
        RiskTier::Tier0ReadOnly
    }
    async fn execute(&self, ctx: &ToolContext, args: Value) -> Result<Value> {
        let rel = args.get("link_path").and_then(|l| l.as_str()).unwrap_or("");
        let full = resolve_path(&ctx.workspace_root, rel)?;
        let dest = fs::read_link(&full).map_err(|e| OdonError::Tool {
            tool: self.name().to_string(),
            message: format!("read_link failed: {e}"),
        })?;
        Ok(json!({ "link_path": rel, "destination": dest.to_string_lossy().to_string() }))
    }
}

// 6. fs_diff
pub struct FsDiffTool;
#[async_trait]
impl Tool for FsDiffTool {
    fn name(&self) -> &str {
        "fs_diff"
    }
    fn description(&self) -> &str {
        "Compute line-by-line unified diff between two files"
    }
    fn parameters_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "file_a": { "type": "string", "description": "Original file path" },
                "file_b": { "type": "string", "description": "Modified file path" }
            },
            "required": ["file_a", "file_b"]
        })
    }
    fn risk_tier(&self) -> RiskTier {
        RiskTier::Tier0ReadOnly
    }
    async fn execute(&self, ctx: &ToolContext, args: Value) -> Result<Value> {
        let fa = args.get("file_a").and_then(|f| f.as_str()).unwrap_or("");
        let fb = args.get("file_b").and_then(|f| f.as_str()).unwrap_or("");
        let pa = resolve_path(&ctx.workspace_root, fa)?;
        let pb = resolve_path(&ctx.workspace_root, fb)?;

        let content_a = fs::read_to_string(&pa).unwrap_or_default();
        let content_b = fs::read_to_string(&pb).unwrap_or_default();

        let mut diff_lines = Vec::new();
        let lines_a: Vec<&str> = content_a.lines().collect();
        let lines_b: Vec<&str> = content_b.lines().collect();

        diff_lines.push(format!("--- {}", fa));
        diff_lines.push(format!("+++ {}", fb));

        let max_len = lines_a.len().max(lines_b.len());
        for i in 0..max_len {
            match (lines_a.get(i), lines_b.get(i)) {
                (Some(a), Some(b)) if a == b => diff_lines.push(format!(" {}", a)),
                (Some(a), Some(b)) => {
                    diff_lines.push(format!("-{}", a));
                    diff_lines.push(format!("+{}", b));
                }
                (Some(a), None) => diff_lines.push(format!("-{}", a)),
                (None, Some(b)) => diff_lines.push(format!("+{}", b)),
                (None, None) => {}
            }
        }

        Ok(json!({
            "file_a": fa,
            "file_b": fb,
            "diff": diff_lines.join("\n"),
            "has_changes": content_a != content_b
        }))
    }
}

// 7. fs_patch
pub struct FsPatchTool;
#[async_trait]
impl Tool for FsPatchTool {
    fn name(&self) -> &str {
        "fs_patch"
    }
    fn description(&self) -> &str {
        "Apply replacements or patch blocks to a target file"
    }
    fn parameters_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "path": { "type": "string", "description": "Target file to patch" },
                "search": { "type": "string", "description": "Text block to find" },
                "replace": { "type": "string", "description": "Replacement text" }
            },
            "required": ["path", "search", "replace"]
        })
    }
    fn risk_tier(&self) -> RiskTier {
        RiskTier::Tier1WorkspaceMutating
    }
    async fn execute(&self, ctx: &ToolContext, args: Value) -> Result<Value> {
        let rel = args.get("path").and_then(|p| p.as_str()).unwrap_or("");
        let search = args.get("search").and_then(|s| s.as_str()).unwrap_or("");
        let replace = args.get("replace").and_then(|r| r.as_str()).unwrap_or("");
        let full = resolve_path(&ctx.workspace_root, rel)?;

        let content = fs::read_to_string(&full).map_err(|e| OdonError::Tool {
            tool: self.name().to_string(),
            message: e.to_string(),
        })?;

        if !content.contains(search) {
            return Err(OdonError::Tool {
                tool: self.name().to_string(),
                message: format!("Target block not found in {rel}"),
            });
        }

        let updated = content.replace(search, replace);
        fs::write(&full, updated).map_err(|e| OdonError::Tool {
            tool: self.name().to_string(),
            message: e.to_string(),
        })?;

        Ok(json!({ "path": rel, "patched": true }))
    }
}

// 8. fs_tree
pub struct FsTreeTool;
#[async_trait]
impl Tool for FsTreeTool {
    fn name(&self) -> &str {
        "fs_tree"
    }
    fn description(&self) -> &str {
        "Generate a structured ASCII directory tree view of the workspace"
    }
    fn parameters_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "path": { "type": "string", "description": "Root path for tree (default: .)", "default": "." },
                "max_depth": { "type": "integer", "description": "Maximum directory depth", "default": 3 }
            }
        })
    }
    fn risk_tier(&self) -> RiskTier {
        RiskTier::Tier0ReadOnly
    }
    async fn execute(&self, ctx: &ToolContext, args: Value) -> Result<Value> {
        let rel = args.get("path").and_then(|p| p.as_str()).unwrap_or(".");
        let max_depth = args
            .get("max_depth")
            .and_then(|d| d.as_u64())
            .unwrap_or(3)
            .clamp(1, 16) as usize;
        let root = resolve_path(&ctx.workspace_root, rel)?;

        let mut lines = Vec::new();
        lines.push(format!("{}/", rel));

        fn build_tree(dir: &Path, prefix: &str, depth: usize, max: usize, lines: &mut Vec<String>) {
            if depth >= max {
                return;
            }
            if let Ok(entries) = fs::read_dir(dir) {
                let mut sorted: Vec<_> = entries.filter_map(std::result::Result::ok).collect();
                sorted.sort_by_key(|e| e.file_name());
                let total = sorted.len();
                for (i, entry) in sorted.into_iter().enumerate() {
                    let name = entry.file_name().to_string_lossy().to_string();
                    if name.starts_with('.') || name == "target" || name == "node_modules" {
                        continue;
                    }
                    // Do not follow symlinks: avoids infinite cycles and
                    // leaking the outside-workspace tree into the output.
                    let ty = match entry.file_type() {
                        Ok(t) => t,
                        Err(_) => continue,
                    };
                    if ty.is_symlink() {
                        continue;
                    }
                    let is_last = i == total - 1;
                    let connector = if is_last { "└── " } else { "├── " };
                    let sub_prefix = if is_last { "    " } else { "│   " };
                    let path = entry.path();
                    if ty.is_dir() {
                        lines.push(format!("{}{}{}/", prefix, connector, name));
                        build_tree(
                            &path,
                            &format!("{}{}", prefix, sub_prefix),
                            depth + 1,
                            max,
                            lines,
                        );
                    } else {
                        lines.push(format!("{}{}{}", prefix, connector, name));
                    }
                }
            }
        }

        build_tree(&root, "", 0, max_depth, &mut lines);
        Ok(json!({ "path": rel, "tree": lines.join("\n") }))
    }
}

// 9. fs_du
pub struct FsDuTool;
#[async_trait]
impl Tool for FsDuTool {
    fn name(&self) -> &str {
        "fs_du"
    }
    fn description(&self) -> &str {
        "Calculate disk usage and size breakdown of directories and files"
    }
    fn parameters_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "path": { "type": "string", "description": "Target directory or file path", "default": "." }
            }
        })
    }
    fn risk_tier(&self) -> RiskTier {
        RiskTier::Tier0ReadOnly
    }
    async fn execute(&self, ctx: &ToolContext, args: Value) -> Result<Value> {
        let rel = args.get("path").and_then(|p| p.as_str()).unwrap_or(".");
        let root = resolve_path(&ctx.workspace_root, rel)?;

        fn dir_size(p: &Path) -> (u64, usize) {
            let mut bytes = 0u64;
            let mut files = 0usize;
            if let Ok(entries) = fs::read_dir(p) {
                for entry in entries.filter_map(std::result::Result::ok) {
                    let ty = match entry.file_type() {
                        Ok(t) => t,
                        Err(_) => continue,
                    };
                    // Skip symlinks entirely: prevents cycles and escaping
                    // the workspace into outside directories.
                    if ty.is_symlink() {
                        continue;
                    }
                    let ep = entry.path();
                    if ty.is_file() {
                        bytes += entry.metadata().map(|m| m.len()).unwrap_or(0);
                        files += 1;
                    } else if ty.is_dir() {
                        let name = entry.file_name().to_string_lossy().to_string();
                        if name != "target" && name != "node_modules" && !name.starts_with('.') {
                            let (sb, sf) = dir_size(&ep);
                            bytes += sb;
                            files += sf;
                        }
                    }
                }
            }
            (bytes, files)
        }

        let (total_bytes, total_files) = if root.is_file() {
            (root.metadata().map(|m| m.len()).unwrap_or(0), 1)
        } else {
            dir_size(&root)
        };

        Ok(json!({
            "path": rel,
            "total_bytes": total_bytes,
            "total_files": total_files,
            "human_size": format!("{:.2} MB", (total_bytes as f64) / 1_048_576.0)
        }))
    }
}

// 10. fs_copy_recursive
pub struct FsCopyRecursiveTool;
#[async_trait]
impl Tool for FsCopyRecursiveTool {
    fn name(&self) -> &str {
        "fs_copy_recursive"
    }
    fn description(&self) -> &str {
        "Recursively copy a directory and its contents safely within workspace"
    }
    fn parameters_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "source": { "type": "string", "description": "Source path" },
                "destination": { "type": "string", "description": "Destination path" }
            },
            "required": ["source", "destination"]
        })
    }
    fn risk_tier(&self) -> RiskTier {
        RiskTier::Tier1WorkspaceMutating
    }
    async fn execute(&self, ctx: &ToolContext, args: Value) -> Result<Value> {
        let src_rel = args.get("source").and_then(|s| s.as_str()).unwrap_or("");
        let dst_rel = args
            .get("destination")
            .and_then(|d| d.as_str())
            .unwrap_or("");
        let src = resolve_path(&ctx.workspace_root, src_rel)?;
        let dst = resolve_path(&ctx.workspace_root, dst_rel)?;

        fn copy_dir_all(src: &Path, dst: &Path) -> std::io::Result<()> {
            fs::create_dir_all(dst)?;
            for entry in fs::read_dir(src)? {
                let entry = entry?;
                let ty = entry.file_type()?;
                if ty.is_symlink() {
                    // Skip symlinks instead of copying their targets.
                    continue;
                }
                if ty.is_dir() {
                    copy_dir_all(&entry.path(), &dst.join(entry.file_name()))?;
                } else {
                    fs::copy(entry.path(), dst.join(entry.file_name()))?;
                }
            }
            Ok(())
        }

        copy_dir_all(&src, &dst).map_err(|e| OdonError::Tool {
            tool: self.name().to_string(),
            message: format!("Recursive copy failed: {e}"),
        })?;

        Ok(json!({ "source": src_rel, "destination": dst_rel, "status": "copied" }))
    }
}

// 11. fs_move_recursive
pub struct FsMoveRecursiveTool;
#[async_trait]
impl Tool for FsMoveRecursiveTool {
    fn name(&self) -> &str {
        "fs_move_recursive"
    }
    fn description(&self) -> &str {
        "Recursively move/rename a directory within workspace"
    }
    fn parameters_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "source": { "type": "string", "description": "Source path" },
                "destination": { "type": "string", "description": "Destination path" }
            },
            "required": ["source", "destination"]
        })
    }
    fn risk_tier(&self) -> RiskTier {
        RiskTier::Tier1WorkspaceMutating
    }
    async fn execute(&self, ctx: &ToolContext, args: Value) -> Result<Value> {
        let src_rel = args.get("source").and_then(|s| s.as_str()).unwrap_or("");
        let dst_rel = args
            .get("destination")
            .and_then(|d| d.as_str())
            .unwrap_or("");
        let src = resolve_path(&ctx.workspace_root, src_rel)?;
        let dst = resolve_path(&ctx.workspace_root, dst_rel)?;

        if let Some(parent) = dst.parent() {
            let _ = fs::create_dir_all(parent);
        }
        fs::rename(&src, &dst).map_err(|e| OdonError::Tool {
            tool: self.name().to_string(),
            message: format!("Move recursive failed: {e}"),
        })?;

        Ok(json!({ "source": src_rel, "destination": dst_rel, "status": "moved" }))
    }
}

// 12. fs_find
pub struct FsFindTool;
#[async_trait]
impl Tool for FsFindTool {
    fn name(&self) -> &str {
        "fs_find"
    }
    fn description(&self) -> &str {
        "Find files by name substring, extension, and size criteria"
    }
    fn parameters_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "name": { "type": "string", "description": "Filename pattern to match" },
                "extension": { "type": "string", "description": "File extension (e.g. ts, rs)" },
                "path": { "type": "string", "description": "Search directory root", "default": "." }
            }
        })
    }
    fn risk_tier(&self) -> RiskTier {
        RiskTier::Tier0ReadOnly
    }
    async fn execute(&self, ctx: &ToolContext, args: Value) -> Result<Value> {
        let name_filter = args
            .get("name")
            .and_then(|n| n.as_str())
            .map(|n| n.to_lowercase());
        let ext_filter = args
            .get("extension")
            .and_then(|e| e.as_str())
            .map(|e| e.trim_start_matches('.').to_lowercase());
        let search_root_rel = args.get("path").and_then(|p| p.as_str()).unwrap_or(".");
        let search_root = resolve_path(&ctx.workspace_root, search_root_rel)?;

        let mut matches = Vec::new();
        fn scan(
            dir: &Path,
            root_len: usize,
            name_f: &Option<String>,
            ext_f: &Option<String>,
            matches: &mut Vec<Value>,
        ) {
            if matches.len() >= 100 {
                return;
            }
            if let Ok(entries) = fs::read_dir(dir) {
                for entry in entries.filter_map(std::result::Result::ok) {
                    let p = entry.path();
                    let file_name = entry.file_name().to_string_lossy().to_string();
                    if file_name.starts_with('.')
                        || file_name == "target"
                        || file_name == "node_modules"
                    {
                        continue;
                    }
                    let ty = match entry.file_type() {
                        Ok(t) => t,
                        Err(_) => continue,
                    };
                    // Do not follow symlinks (avoids cycles and escaping
                    // the search root).
                    if ty.is_symlink() {
                        continue;
                    }

                    if ty.is_dir() {
                        scan(&p, root_len, name_f, ext_f, matches);
                    } else if ty.is_file() {
                        let matches_name = name_f
                            .as_ref()
                            .is_none_or(|n| file_name.to_lowercase().contains(n));
                        let matches_ext = ext_f.as_ref().is_none_or(|e| {
                            p.extension()
                                .and_then(|ext| ext.to_str())
                                .is_some_and(|ext| ext.eq_ignore_ascii_case(e))
                        });

                        if matches_name && matches_ext {
                            let rel = p.to_string_lossy()[root_len..]
                                .replace('\\', "/")
                                .trim_start_matches('/')
                                .to_string();
                            let size = entry.metadata().map(|m| m.len()).unwrap_or(0);
                            matches.push(json!({ "path": rel, "size_bytes": size }));
                        }
                    }
                }
            }
        }

        let root_str = search_root.to_string_lossy().to_string();
        scan(
            &search_root,
            root_str.len(),
            &name_filter,
            &ext_filter,
            &mut matches,
        );

        Ok(json!({ "total_found": matches.len(), "files": matches }))
    }
}

// 13. fs_zip & 14. fs_unzip
pub struct FsZipTool;
#[async_trait]
impl Tool for FsZipTool {
    fn name(&self) -> &str {
        "fs_zip"
    }
    fn description(&self) -> &str {
        "Create a compressed archive bundle of files or directory"
    }
    fn parameters_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "source": { "type": "string", "description": "Source path to archive" },
                "archive_path": { "type": "string", "description": "Destination .zip file path" }
            },
            "required": ["source", "archive_path"]
        })
    }
    fn risk_tier(&self) -> RiskTier {
        RiskTier::Tier1WorkspaceMutating
    }
    async fn execute(&self, ctx: &ToolContext, args: Value) -> Result<Value> {
        let src = args.get("source").and_then(|s| s.as_str()).unwrap_or("");
        let arc = args
            .get("archive_path")
            .and_then(|a| a.as_str())
            .unwrap_or("");
        let src_full = resolve_path(&ctx.workspace_root, src)?;
        let arc_full = resolve_path(&ctx.workspace_root, arc)?;
        if !src_full.exists() {
            return Err(OdonError::Tool {
                tool: self.name().to_string(),
                message: format!("Source path does not exist: {src}"),
            });
        }
        if !arc_full.to_string_lossy().to_lowercase().ends_with(".zip") {
            return Err(OdonError::Tool {
                tool: self.name().to_string(),
                message: "archive_path must end with .zip".to_string(),
            });
        }
        if let Some(parent) = arc_full.parent() {
            fs::create_dir_all(parent).map_err(|e| OdonError::Tool {
                tool: self.name().to_string(),
                message: e.to_string(),
            })?;
        }

        let file = fs::File::create(&arc_full).map_err(|e| OdonError::Tool {
            tool: self.name().to_string(),
            message: e.to_string(),
        })?;
        let mut zip = zip::ZipWriter::new(file);
        let options = zip::write::SimpleFileOptions::default()
            .compression_method(zip::CompressionMethod::Deflated);
        let base = if src_full.is_dir() {
            src_full.clone()
        } else {
            src_full.parent().unwrap_or(&src_full).to_path_buf()
        };
        add_path_to_zip(&mut zip, &base, &src_full, options).map_err(|e| OdonError::Tool {
            tool: self.name().to_string(),
            message: e,
        })?;
        zip.finish().map_err(|e| OdonError::Tool {
            tool: self.name().to_string(),
            message: e.to_string(),
        })?;

        Ok(json!({ "source": src, "archive_path": arc, "status": "created" }))
    }
}

pub struct FsUnzipTool;
#[async_trait]
impl Tool for FsUnzipTool {
    fn name(&self) -> &str {
        "fs_unzip"
    }
    fn description(&self) -> &str {
        "Extract an archive file to a destination directory"
    }
    fn parameters_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "archive_path": { "type": "string", "description": "Path to archive file" },
                "destination": { "type": "string", "description": "Extract destination directory" }
            },
            "required": ["archive_path", "destination"]
        })
    }
    fn risk_tier(&self) -> RiskTier {
        RiskTier::Tier1WorkspaceMutating
    }
    async fn execute(&self, ctx: &ToolContext, args: Value) -> Result<Value> {
        let arc = args
            .get("archive_path")
            .and_then(|a| a.as_str())
            .unwrap_or("");
        let dst = args
            .get("destination")
            .and_then(|d| d.as_str())
            .unwrap_or("");
        let arc_full = resolve_path(&ctx.workspace_root, arc)?;
        let dst_full = resolve_path(&ctx.workspace_root, dst)?;
        let file = fs::File::open(&arc_full).map_err(|e| OdonError::Tool {
            tool: self.name().to_string(),
            message: format!("Failed to open archive: {e}"),
        })?;
        let mut archive = zip::ZipArchive::new(file).map_err(|e| OdonError::Tool {
            tool: self.name().to_string(),
            message: format!("Invalid zip archive: {e}"),
        })?;
        fs::create_dir_all(&dst_full).map_err(|e| OdonError::Tool {
            tool: self.name().to_string(),
            message: e.to_string(),
        })?;

        // Zip-bomb guard: cap entry count and cumulative expanded bytes.
        const MAX_ENTRIES: usize = 10_000;
        const MAX_TOTAL_BYTES: u64 = 512 * 1024 * 1024; // 512 MiB
        let mut extracted = 0usize;
        let mut total_bytes = 0u64;
        for index in 0..archive.len() {
            if index >= MAX_ENTRIES {
                return Err(OdonError::Tool {
                    tool: self.name().to_string(),
                    message: format!("Archive exceeds {MAX_ENTRIES} entries"),
                });
            }
            let mut entry = archive.by_index(index).map_err(|e| OdonError::Tool {
                tool: self.name().to_string(),
                message: e.to_string(),
            })?;
            // `entry.size()` is the size the central directory *declares*; it is a
            // cheap early rejection, not the authoritative accounting. The real
            // budget is enforced against bytes actually decompressed below.
            if !charge_extraction_budget(&mut total_bytes, entry.size(), MAX_TOTAL_BYTES) {
                return Err(OdonError::Tool {
                    tool: self.name().to_string(),
                    message: format!(
                        "Archive exceeds the {MAX_TOTAL_BYTES}-byte extraction budget"
                    ),
                });
            }
            // `enclosed_name` rejects absolute paths and `..` traversal.
            let Some(relative) = entry.enclosed_name().map(|p| p.to_path_buf()) else {
                continue;
            };
            let out_path = dst_full.join(relative);
            if entry.is_dir() {
                fs::create_dir_all(&out_path).map_err(|e| OdonError::Tool {
                    tool: self.name().to_string(),
                    message: e.to_string(),
                })?;
            } else {
                if let Some(parent) = out_path.parent() {
                    fs::create_dir_all(parent).map_err(|e| OdonError::Tool {
                        tool: self.name().to_string(),
                        message: e.to_string(),
                    })?;
                }
                let mut out_file = fs::File::create(&out_path).map_err(|e| OdonError::Tool {
                    tool: self.name().to_string(),
                    message: e.to_string(),
                })?;
                let copied =
                    std::io::copy(&mut entry, &mut out_file).map_err(|e| OdonError::Tool {
                        tool: self.name().to_string(),
                        message: e.to_string(),
                    })?;
                // Charge the bytes that were actually decompressed, not the size the
                // archive header declares. A forged header can under-report and slip
                // past the pre-check above, which only reads the declared size.
                let within_budget =
                    charge_extraction_budget(&mut total_bytes, copied, MAX_TOTAL_BYTES);
                if !within_budget {
                    drop(out_file);
                    let _ = fs::remove_file(&out_path);
                    return Err(OdonError::Tool {
                        tool: self.name().to_string(),
                        message: format!(
                            "Archive exceeds the {MAX_TOTAL_BYTES}-byte extraction budget"
                        ),
                    });
                }
                extracted += 1;
            }
        }

        Ok(json!({
            "archive_path": arc,
            "destination": dst,
            "status": "extracted",
            "files": extracted
        }))
    }
}

// 15. Termigo Paste & Buffer Tools
pub struct TermigoMultiPasteTool;
#[async_trait]
impl Tool for TermigoMultiPasteTool {
    fn name(&self) -> &str {
        "termigo_multi_paste"
    }
    fn description(&self) -> &str {
        "Batch write or paste multiple code snippets to files"
    }
    fn parameters_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "snippets": {
                    "type": "array",
                    "items": {
                        "type": "object",
                        "properties": {
                            "path": { "type": "string" },
                            "content": { "type": "string" }
                        },
                        "required": ["path", "content"]
                    }
                }
            },
            "required": ["snippets"]
        })
    }
    fn risk_tier(&self) -> RiskTier {
        RiskTier::Tier1WorkspaceMutating
    }
    async fn execute(&self, ctx: &ToolContext, args: Value) -> Result<Value> {
        let snippets = args
            .get("snippets")
            .and_then(|s| s.as_array())
            .ok_or_else(|| OdonError::Tool {
                tool: self.name().to_string(),
                message: "Missing 'snippets' array".to_string(),
            })?;

        let mut written = Vec::new();
        for item in snippets {
            if let (Some(rel), Some(content)) = (
                item.get("path").and_then(|p| p.as_str()),
                item.get("content").and_then(|c| c.as_str()),
            ) {
                let full = resolve_path(&ctx.workspace_root, rel)?;
                if let Some(parent) = full.parent() {
                    let _ = fs::create_dir_all(parent);
                }
                fs::write(&full, content).map_err(|e| OdonError::Tool {
                    tool: self.name().to_string(),
                    message: e.to_string(),
                })?;
                written.push(rel.to_string());
            }
        }
        Ok(json!({ "written_count": written.len(), "files": written }))
    }
}

pub struct TermigoCopySelectionTool;
#[async_trait]
impl Tool for TermigoCopySelectionTool {
    fn name(&self) -> &str {
        "termigo_copy_selection"
    }
    fn description(&self) -> &str {
        "Copy a slice of file lines into the clipboard buffer"
    }
    fn parameters_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "path": { "type": "string", "description": "Source file path" },
                "start_line": { "type": "integer", "description": "1-based start line" },
                "end_line": { "type": "integer", "description": "1-based end line" }
            },
            "required": ["path"]
        })
    }
    fn risk_tier(&self) -> RiskTier {
        RiskTier::Tier1WorkspaceMutating
    }
    async fn execute(&self, ctx: &ToolContext, args: Value) -> Result<Value> {
        let rel = args.get("path").and_then(|p| p.as_str()).unwrap_or("");
        let start = args.get("start_line").and_then(|s| s.as_u64()).unwrap_or(1) as usize;
        let end = args
            .get("end_line")
            .and_then(|e| e.as_u64())
            .unwrap_or(usize::MAX as u64) as usize;

        let full = resolve_path(&ctx.workspace_root, rel)?;
        let content = fs::read_to_string(&full).map_err(|e| OdonError::Tool {
            tool: self.name().to_string(),
            message: e.to_string(),
        })?;
        let lines: Vec<&str> = content.lines().collect();

        let s_idx = start.saturating_sub(1).min(lines.len());
        let e_idx = end.min(lines.len()).max(s_idx);
        let selected = lines[s_idx..e_idx].join("\n");
        crate::shell_ext::write_clipboard_text(selected.clone(), self.name())
            .await
            .map_err(|message| OdonError::Tool {
                tool: self.name().to_string(),
                message,
            })?;

        Ok(json!({
            "path": rel,
            "lines_copied": e_idx.saturating_sub(s_idx),
            "selection": selected
        }))
    }
}

pub struct TermigoPasteBufferHistoryTool;
#[async_trait]
impl Tool for TermigoPasteBufferHistoryTool {
    fn name(&self) -> &str {
        "termigo_paste_buffer_history"
    }
    fn description(&self) -> &str {
        "Get recent clipboard and paste buffer history"
    }
    fn parameters_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "limit": {
                    "type": "integer",
                    "minimum": 1,
                    "maximum": 50,
                    "default": 20,
                    "description": "Maximum number of recent clipboard entries to return"
                }
            }
        })
    }
    fn risk_tier(&self) -> RiskTier {
        RiskTier::Tier1WorkspaceMutating
    }
    async fn execute(&self, _ctx: &ToolContext, args: Value) -> Result<Value> {
        let limit = args
            .get("limit")
            .and_then(Value::as_u64)
            .unwrap_or(20)
            .clamp(1, 50) as usize;
        let current_error = crate::shell_ext::read_clipboard_text(self.name())
            .await
            .err();
        let entries = crate::shell_ext::clipboard_history_snapshot(limit);
        Ok(json!({
            "count": entries.len(),
            "entries": entries,
            "current_error": current_error,
        }))
    }
}

// 16. Editor Integration Tools
pub struct EditorOpenTool;
#[async_trait]
impl Tool for EditorOpenTool {
    fn name(&self) -> &str {
        "editor_open"
    }
    fn description(&self) -> &str {
        "Open and focus a file in the desktop code editor at line and column"
    }
    fn parameters_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "path": { "type": "string", "description": "File to open" },
                "line": { "type": "integer", "description": "Target line number", "default": 1 },
                "column": { "type": "integer", "description": "Target column number", "default": 1 }
            },
            "required": ["path"]
        })
    }
    fn risk_tier(&self) -> RiskTier {
        RiskTier::Tier0ReadOnly
    }
    fn availability(&self) -> crate::registry::ToolAvailability {
        crate::registry::ToolAvailability::Unavailable {
            reason: "Desktop editor open integration is not connected yet".to_string(),
        }
    }
    async fn execute(&self, ctx: &ToolContext, args: Value) -> Result<Value> {
        let rel = args.get("path").and_then(|p| p.as_str()).unwrap_or("");
        let line = args.get("line").and_then(|l| l.as_u64()).unwrap_or(1);
        let col = args.get("column").and_then(|c| c.as_u64()).unwrap_or(1);
        let _ = resolve_path(&ctx.workspace_root, rel)?;
        Err(OdonError::Tool {
            tool: self.name().to_string(),
            message: format!(
                "Desktop editor open integration is not connected yet ({rel}:{line}:{col})"
            ),
        })
    }
}

pub struct EditorRevealTool;
#[async_trait]
impl Tool for EditorRevealTool {
    fn name(&self) -> &str {
        "editor_reveal"
    }
    fn description(&self) -> &str {
        "Reveal and highlight a file or directory in the workspace file tree"
    }
    fn parameters_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "path": { "type": "string", "description": "Path to reveal" }
            },
            "required": ["path"]
        })
    }
    fn risk_tier(&self) -> RiskTier {
        RiskTier::Tier0ReadOnly
    }
    fn availability(&self) -> crate::registry::ToolAvailability {
        crate::registry::ToolAvailability::Unavailable {
            reason: "Workspace file-tree reveal integration is not connected yet".to_string(),
        }
    }
    async fn execute(&self, ctx: &ToolContext, args: Value) -> Result<Value> {
        let rel = args.get("path").and_then(|p| p.as_str()).unwrap_or("");
        let _ = resolve_path(&ctx.workspace_root, rel)?;
        Err(OdonError::Tool {
            tool: self.name().to_string(),
            message: format!("Workspace file-tree reveal integration is not connected yet ({rel})"),
        })
    }
}

pub struct EditorSaveTool;
#[async_trait]
impl Tool for EditorSaveTool {
    fn name(&self) -> &str {
        "editor_save"
    }
    fn description(&self) -> &str {
        "Save current active editor buffer to workspace file"
    }
    fn parameters_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "path": { "type": "string", "description": "File path" },
                "content": { "type": "string", "description": "Buffer contents to save" }
            },
            "required": ["path", "content"]
        })
    }
    fn risk_tier(&self) -> RiskTier {
        RiskTier::Tier1WorkspaceMutating
    }
    async fn execute(&self, ctx: &ToolContext, args: Value) -> Result<Value> {
        let rel = args.get("path").and_then(|p| p.as_str()).unwrap_or("");
        let content = args.get("content").and_then(|c| c.as_str()).unwrap_or("");
        let full = resolve_path(&ctx.workspace_root, rel)?;
        fs::write(&full, content).map_err(|e| OdonError::Tool {
            tool: self.name().to_string(),
            message: e.to_string(),
        })?;
        Ok(json!({ "path": rel, "status": "saved" }))
    }
}

pub struct EditorReloadTool;
#[async_trait]
impl Tool for EditorReloadTool {
    fn name(&self) -> &str {
        "editor_reload"
    }
    fn description(&self) -> &str {
        "Reload file contents from disk into the editor"
    }
    fn parameters_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "path": { "type": "string", "description": "File path to reload" }
            },
            "required": ["path"]
        })
    }
    fn risk_tier(&self) -> RiskTier {
        RiskTier::Tier0ReadOnly
    }
    async fn execute(&self, ctx: &ToolContext, args: Value) -> Result<Value> {
        let rel = args.get("path").and_then(|p| p.as_str()).unwrap_or("");
        let full = resolve_path(&ctx.workspace_root, rel)?;
        let content = fs::read_to_string(&full).map_err(|e| OdonError::Tool {
            tool: self.name().to_string(),
            message: e.to_string(),
        })?;
        Ok(json!({ "path": rel, "content": content, "status": "reloaded" }))
    }
}

#[cfg(test)]
mod budget_tests {
    use super::charge_extraction_budget;

    #[test]
    fn accepts_a_payload_within_the_budget() {
        let mut total = 0u64;
        assert!(charge_extraction_budget(&mut total, 100, 1_000));
        assert_eq!(total, 100);
    }

    #[test]
    fn rejects_the_entry_that_crosses_the_budget() {
        let mut total = 900u64;
        assert!(!charge_extraction_budget(&mut total, 200, 1_000));
        assert_eq!(total, 1_100);
    }

    #[test]
    fn saturates_instead_of_overflowing_on_absurd_deltas() {
        let mut total = u64::MAX - 1;
        assert!(!charge_extraction_budget(&mut total, u64::MAX, 1_000));
        assert_eq!(total, u64::MAX);
    }

    /// Regression for the zip-bomb guard: a forged central directory that
    /// declares one byte must not smuggle a hugely expanded payload past the
    /// budget. The declared size clears the cheap pre-check; the byte count
    /// reported by the decompressor is what actually trips it.
    #[test]
    fn forged_small_declared_size_still_trips_the_budget() {
        const LIMIT: u64 = 512 * 1024 * 1024;
        let mut total = 0u64;
        assert!(charge_extraction_budget(&mut total, 1, LIMIT));
        assert!(!charge_extraction_budget(&mut total, LIMIT + 1, LIMIT));
    }
}

#[cfg(test)]
mod path_tests {
    use super::resolve_path;

    /// `fs_tree` and `fs_find` are routinely given the workspace root exactly
    /// as the harness prints it — an absolute path. Rejecting that made the
    /// agent unable to inspect its own workspace.
    #[test]
    fn accepts_absolute_paths_inside_the_workspace_only() {
        let root = std::env::temp_dir().join(format!("odon-ext-path-{}", std::process::id()));
        let outside = std::env::temp_dir().join(format!("odon-ext-outside-{}", std::process::id()));
        std::fs::create_dir_all(root.join("nested")).unwrap();
        std::fs::create_dir_all(&outside).unwrap();
        let root_str = root.to_string_lossy().to_string();

        assert!(resolve_path(&root_str, &root_str).is_ok());
        assert!(resolve_path(&root_str, &root.join("nested").to_string_lossy()).is_ok());
        assert!(resolve_path(&root_str, &outside.to_string_lossy()).is_err());
        assert!(resolve_path(&root_str, "../outside").is_err());
        assert!(resolve_path(&root_str, "nested").is_ok());

        std::fs::remove_dir_all(root).ok();
        std::fs::remove_dir_all(outside).ok();
    }
}
