use odon_core::{OdonError, Result};
use std::path::{Path, PathBuf};

fn tool_error(message: String) -> OdonError {
    OdonError::Tool {
        tool: "workspace_path".to_string(),
        message,
    }
}

fn canonical_root(workspace_root: &str) -> Result<PathBuf> {
    Path::new(workspace_root)
        .canonicalize()
        .map_err(|error| tool_error(format!("cannot resolve workspace root: {error}")))
}

/// Resolve an existing file/directory and require it to stay inside the
/// workspace. Symlink escape is rejected because the final target is
/// canonicalized before containment is checked.
pub fn resolve_existing(workspace_root: &str, input: &str) -> Result<PathBuf> {
    let root = canonical_root(workspace_root)?;
    let candidate = Path::new(input);
    let joined = if candidate.is_absolute() {
        candidate.to_path_buf()
    } else {
        root.join(candidate)
    };
    let canonical = joined
        .canonicalize()
        .map_err(|error| tool_error(format!("cannot resolve workspace path: {error}")))?;
    if !canonical.starts_with(&root) {
        return Err(tool_error("path escapes the workspace".to_string()));
    }
    Ok(canonical)
}

/// Resolve a write target safely. The nearest existing ancestor is
/// canonicalized to prevent symlink escape, then the remaining relative tail is
/// appended without following non-existing components.
pub fn resolve_write(workspace_root: &str, input: &str) -> Result<PathBuf> {
    let root = canonical_root(workspace_root)?;
    let candidate = Path::new(input);
    let joined = if candidate.is_absolute() {
        candidate.to_path_buf()
    } else {
        root.join(candidate)
    };
    let mut ancestor = joined.as_path();
    let mut tail = Vec::new();
    while std::fs::symlink_metadata(ancestor).is_err() {
        let Some(name) = ancestor.file_name() else {
            return Err(tool_error("invalid write path".to_string()));
        };
        tail.push(name.to_os_string());
        let Some(parent) = ancestor.parent() else {
            return Err(tool_error("invalid write path".to_string()));
        };
        ancestor = parent;
    }
    let canonical_ancestor = ancestor
        .canonicalize()
        .map_err(|error| tool_error(format!("cannot resolve write path: {error}")))?;
    if !canonical_ancestor.starts_with(&root) {
        return Err(tool_error("write path escapes the workspace".to_string()));
    }
    let mut resolved = canonical_ancestor;
    for component in tail.into_iter().rev() {
        resolved.push(component);
    }
    if resolved.starts_with(&root) {
        Ok(resolved)
    } else {
        Err(tool_error("write path escapes the workspace".to_string()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn resolves_existing_paths_inside_workspace() {
        let root = std::env::temp_dir().join(format!("odon-path-{}", std::process::id()));
        let nested = root.join("nested");
        std::fs::create_dir_all(&nested).unwrap();
        std::fs::write(nested.join("file.txt"), "ok").unwrap();
        let root_str = root.to_string_lossy().to_string();

        assert!(resolve_existing(&root_str, "nested/file.txt").is_ok());
        assert!(resolve_existing(&root_str, "../escape").is_err());
        assert!(resolve_existing(&root_str, &std::env::temp_dir().to_string_lossy()).is_err());
        std::fs::remove_dir_all(root).ok();
    }

    #[test]
    fn resolves_non_existing_write_targets_inside_workspace() {
        let root = std::env::temp_dir().join(format!("odon-write-{}", std::process::id()));
        std::fs::create_dir_all(&root).unwrap();
        let root_str = root.to_string_lossy().to_string();
        let target = resolve_write(&root_str, "new-folder/file.txt").unwrap();
        assert_eq!(target.file_name().unwrap(), "file.txt");
        assert!(resolve_write(&root_str, "../outside.txt").is_err());
        let absolute_inside = format!(
            "{}{}nested{}file.txt",
            root_str,
            std::path::MAIN_SEPARATOR,
            std::path::MAIN_SEPARATOR
        );
        std::fs::create_dir_all(root.join("nested")).unwrap();
        assert!(resolve_write(&root_str, &absolute_inside).is_ok());
        assert!(resolve_write(&root_str, "/etc/passwd-odon-test").is_err());
        std::fs::remove_dir_all(root).ok();
    }

    #[test]
    fn rejects_parent_dir_chains_and_symlink_escape() {
        let base = std::env::temp_dir();
        let root = base.join(format!("odon-guard-root-{}", std::process::id()));
        let outside = base.join(format!("odon-guard-outside-{}", std::process::id()));
        std::fs::create_dir_all(root.join("nested")).unwrap();
        std::fs::create_dir_all(&outside).unwrap();
        std::fs::write(outside.join("secret.txt"), "secret").unwrap();
        let root_str = root.to_string_lossy().to_string();

        // Starts inside the workspace but resolves back out: the canonicalized
        // ancestor must still be rejected.
        assert!(resolve_write(&root_str, "nested/../../outside/evil.txt").is_err());
        assert!(resolve_existing(&root_str, "nested/../../outside/secret.txt").is_err());

        // A symlink planted inside the workspace must not become a trampoline.
        let link = root.join("escape");
        let linked = {
            #[cfg(unix)]
            {
                std::os::unix::fs::symlink(&outside, &link).is_ok()
            }
            #[cfg(windows)]
            {
                std::os::windows::fs::symlink_dir(&outside, &link).is_ok()
            }
        };
        if linked {
            assert!(resolve_existing(&root_str, "escape/secret.txt").is_err());
            assert!(resolve_write(&root_str, "escape/new.txt").is_err());
        }

        std::fs::remove_dir_all(root).ok();
        std::fs::remove_dir_all(outside).ok();
    }
}
