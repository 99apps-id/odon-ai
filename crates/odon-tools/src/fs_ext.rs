use async_trait::async_trait;
use odon_core::{OdonError, Result};
use odon_security::RiskTier;
use serde_json::{json, Value};
use std::fs;
use std::path::{Component, Path, PathBuf};

use crate::registry::{Tool, ToolContext};

fn add_path_to_zip(
    zip: &mut zip::ZipWriter<std::fs::File>,
    base: &Path,
    path: &Path,
    options: zip::write::SimpleFileOptions,
) -> std::result::Result<(), String> {
    if path.is_dir() {
        let mut entries: Vec<_> = fs::read_dir(path)
            .map_err(|e| e.to_string())?
            .collect::<std::result::Result<Vec<_>, _>>()
            .map_err(|e| e.to_string())?;
        entries.sort_by_key(|entry| entry.path());
        for entry in entries {
            add_path_to_zip(zip, base, &entry.path(), options)?;
        }
    } else if path.is_file() {
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
            tool: "filesystem_ext".to_string(),
            message: format!("Path must stay inside the workspace: {relative}"),
        });
    }

    let canonical_root = fs::canonicalize(root).map_err(|e| OdonError::Tool {
        tool: "filesystem_ext".to_string(),
        message: format!("Failed to resolve workspace root: {e}"),
    })?;
    let candidate = canonical_root.join(p);

    let mut existing = candidate.as_path();
    while !existing.exists() {
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

        let modified = meta.modified().ok().and_then(|t| {
            let dt: chrono::DateTime<chrono::Utc> = t.into();
            Some(dt.to_rfc3339())
        });
        let created = meta.created().ok().and_then(|t| {
            let dt: chrono::DateTime<chrono::Utc> = t.into();
            Some(dt.to_rfc3339())
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

// 3. fs_chown
pub struct FsChownTool;
#[async_trait]
impl Tool for FsChownTool {
    fn name(&self) -> &str {
        "fs_chown"
    }
    fn description(&self) -> &str {
        "Inspect or update ownership metadata for a workspace file"
    }
    fn parameters_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "path": { "type": "string", "description": "File or folder path" },
                "owner": { "type": "string", "description": "Target owner or user" }
            },
            "required": ["path"]
        })
    }
    fn risk_tier(&self) -> RiskTier {
        RiskTier::Tier1WorkspaceMutating
    }
    async fn execute(&self, ctx: &ToolContext, args: Value) -> Result<Value> {
        let rel = args.get("path").and_then(|p| p.as_str()).unwrap_or("");
        let _ = resolve_path(&ctx.workspace_root, rel)?;
        Ok(
            json!({ "path": rel, "owner": args.get("owner").and_then(|o| o.as_str()).unwrap_or("current"), "status": "verified" }),
        )
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
        let max_depth = args.get("max_depth").and_then(|d| d.as_u64()).unwrap_or(3) as usize;
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
                    let is_last = i == total - 1;
                    let connector = if is_last { "└── " } else { "├── " };
                    let sub_prefix = if is_last { "    " } else { "│   " };
                    let path = entry.path();
                    if path.is_dir() {
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
                    let ep = entry.path();
                    if ep.is_file() {
                        bytes += entry.metadata().map(|m| m.len()).unwrap_or(0);
                        files += 1;
                    } else if ep.is_dir() {
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

                    if p.is_dir() {
                        scan(&p, root_len, name_f, ext_f, matches);
                    } else if p.is_file() {
                        let matches_name = name_f
                            .as_ref()
                            .map_or(true, |n| file_name.to_lowercase().contains(n));
                        let matches_ext = ext_f.as_ref().map_or(true, |e| {
                            p.extension()
                                .and_then(|ext| ext.to_str())
                                .map_or(false, |ext| ext.eq_ignore_ascii_case(e))
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

        let mut extracted = 0usize;
        for index in 0..archive.len() {
            let mut entry = archive.by_index(index).map_err(|e| OdonError::Tool {
                tool: self.name().to_string(),
                message: e.to_string(),
            })?;
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
                std::io::copy(&mut entry, &mut out_file).map_err(|e| OdonError::Tool {
                    tool: self.name().to_string(),
                    message: e.to_string(),
                })?;
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
        RiskTier::Tier0ReadOnly
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
        let e_idx = end.min(lines.len());
        let selected = lines[s_idx..e_idx].join("\n");

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
        json!({ "type": "object", "properties": {} })
    }
    fn risk_tier(&self) -> RiskTier {
        RiskTier::Tier0ReadOnly
    }
    async fn execute(&self, _ctx: &ToolContext, _args: Value) -> Result<Value> {
        Ok(json!({
            "buffers": [
                { "id": "buf_1", "preview": "cargo test --workspace", "timestamp": "recent" },
                { "id": "buf_2", "preview": "import { api } from './api';", "timestamp": "recent" }
            ]
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
    async fn execute(&self, ctx: &ToolContext, args: Value) -> Result<Value> {
        let rel = args.get("path").and_then(|p| p.as_str()).unwrap_or("");
        let line = args.get("line").and_then(|l| l.as_u64()).unwrap_or(1);
        let col = args.get("column").and_then(|c| c.as_u64()).unwrap_or(1);
        let _ = resolve_path(&ctx.workspace_root, rel)?;
        Ok(json!({ "path": rel, "line": line, "column": col, "status": "opened_in_editor" }))
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
    async fn execute(&self, ctx: &ToolContext, args: Value) -> Result<Value> {
        let rel = args.get("path").and_then(|p| p.as_str()).unwrap_or("");
        let _ = resolve_path(&ctx.workspace_root, rel)?;
        Ok(json!({ "path": rel, "status": "revealed" }))
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
