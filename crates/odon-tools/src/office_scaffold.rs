//! Scaffold a standalone Agents Office app into the workspace.
//!
//! The generated page talks to `/api/*` on its own `serve.mjs`, which proxies
//! to the daemon with the loopback token. That keeps the sandboxed preview
//! working without putting a token in the browser or widening the daemon's
//! origin policy.

use async_trait::async_trait;
use odon_core::{Artifact, ArtifactFile, OdonError, Result};
use odon_security::RiskTier;
use serde_json::{json, Value};

use crate::registry::{Tool, ToolContext};

const FILES: &[(&str, &str)] = &[
    ("index.html", include_str!("../assets/office/index.html")),
    ("src/main.js", include_str!("../assets/office/src/main.js")),
    (
        "src/office.js",
        include_str!("../assets/office/src/office.js"),
    ),
    ("src/data.js", include_str!("../assets/office/src/data.js")),
    (
        "src/style.css",
        include_str!("../assets/office/src/style.css"),
    ),
    ("serve.mjs", include_str!("../assets/office/serve.mjs")),
    ("README.md", include_str!("../assets/office/README.md")),
];

fn normalize_directory(raw: &str) -> std::result::Result<String, OdonError> {
    let trimmed = raw.trim().replace('\\', "/");
    let trimmed = trimmed.trim_matches('/');
    if trimmed.is_empty() || trimmed == "." {
        return Ok("agents-office".to_string());
    }
    if trimmed
        .split('/')
        .any(|part| part.is_empty() || part == "." || part == "..")
    {
        return Err(OdonError::Tool {
            tool: "agents_office_create".to_string(),
            message:
                "directory must be a clean workspace-relative folder (no '..', no empty segments)"
                    .to_string(),
        });
    }
    Ok(trimmed.to_string())
}

pub struct AgentsOfficeCreateTool;

#[async_trait]
impl Tool for AgentsOfficeCreateTool {
    fn name(&self) -> &str {
        "agents_office_create"
    }
    fn description(&self) -> &str {
        "Create a standalone Three.js Agents Office app in the workspace: an isometric office that maps real ODON sessions and subagents to desks with live state, tools, approvals, and artifacts. It includes a local proxy for the daemon token; follow up with the built-in agents-office-app skill."
    }
    fn parameters_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "directory": {
                    "type": "string",
                    "description": "Workspace-relative folder (default agents-office)"
                },
                "title": { "type": "string", "description": "App title (default ODON Agents Office)" },
                "port": { "type": "integer", "minimum": 1024, "maximum": 65535, "description": "Preview server port (default 5173)" }
            },
            "required": []
        })
    }
    fn risk_tier(&self) -> RiskTier {
        RiskTier::Tier1WorkspaceMutating
    }
    async fn execute(&self, ctx: &ToolContext, args: Value) -> Result<Value> {
        let directory = match args.get("directory").and_then(Value::as_str) {
            Some(raw) => normalize_directory(raw)?,
            None => "agents-office".to_string(),
        };
        let title = args
            .get("title")
            .and_then(Value::as_str)
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .unwrap_or("ODON Agents Office");
        let port = args
            .get("port")
            .and_then(Value::as_u64)
            .unwrap_or(5173)
            .clamp(1024, 65535) as u16;

        let mut files: Vec<(String, String)> = Vec::new();
        for (relative, body) in FILES {
            let rendered = body
                .replace("{{TITLE}}", title)
                .replace("{{PORT}}", &port.to_string())
                .replace("{{DIR}}", &directory);
            files.push((format!("{directory}/{relative}"), rendered));
        }

        for (relative, body) in &files {
            let path = crate::workspace_path::resolve_write(&ctx.workspace_root, relative)?;
            if let Some(parent) = path.parent() {
                std::fs::create_dir_all(parent).map_err(|error| OdonError::Tool {
                    tool: self.name().to_string(),
                    message: format!("cannot create project folder: {error}"),
                })?;
            }
            std::fs::write(&path, body.as_bytes()).map_err(|error| OdonError::Tool {
                tool: self.name().to_string(),
                message: format!("cannot write {relative}: {error}"),
            })?;
        }

        let artifact = Artifact::new_code(
            ctx.session_id.clone(),
            format!("{title} (agents office)"),
            format!(
                "Live isometric office for ODON sessions in `{directory}`. Run `cd {directory}; node serve.mjs` and open http://127.0.0.1:{port}."
            ),
            "javascript",
            Some("three.js".to_string()),
            files
                .iter()
                .map(|(path, content)| ArtifactFile {
                    path: path.clone(),
                    content: content.clone(),
                })
                .collect(),
        );

        Ok(json!({
            "project_dir": directory,
            "port": port,
            "files": files.iter().map(|(path, _)| path.clone()).collect::<Vec<_>>(),
            "preview": {
                "command": format!("cd {directory}; node serve.mjs"),
                "url": format!("http://127.0.0.1:{port}")
            },
            "artifact": artifact
        }))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use odon_security::{ApprovalEngine, AuditLogger, SecurityPolicy};
    use std::sync::Arc;

    fn test_context(root: &std::path::Path) -> ToolContext {
        ToolContext {
            session_id: "office-test".to_string(),
            workspace_root: root.to_string_lossy().to_string(),
            approval_engine: Arc::new(ApprovalEngine::new(SecurityPolicy::default())),
            audit_logger: Arc::new(AuditLogger::new()),
            storage: None,
        }
    }

    fn temp_root() -> std::path::PathBuf {
        let root = std::env::temp_dir().join(format!(
            "odon-office-{}-{}",
            std::process::id(),
            uuid::Uuid::new_v4().simple()
        ));
        std::fs::create_dir_all(&root).unwrap();
        root
    }

    #[tokio::test]
    async fn writes_the_office_app_with_its_proxy() {
        let root = temp_root();
        let ctx = test_context(&root);
        let out = AgentsOfficeCreateTool
            .execute(
                &ctx,
                json!({ "title": "Studio Office", "directory": "apps/office", "port": 5180 }),
            )
            .await
            .expect("office scaffold should succeed");

        for relative in [
            "index.html",
            "src/main.js",
            "src/office.js",
            "src/data.js",
            "src/style.css",
            "serve.mjs",
            "README.md",
        ] {
            assert!(
                root.join("apps/office").join(relative).is_file(),
                "missing {relative}"
            );
        }

        let html = std::fs::read_to_string(root.join("apps/office/index.html")).unwrap();
        assert!(html.contains("Studio Office"));
        assert!(html.contains("importmap"));
        let data = std::fs::read_to_string(root.join("apps/office/src/data.js")).unwrap();
        assert!(data.contains("/api/sessions?include_subagents=true"));
        assert!(data.contains("/api/events"));
        let server = std::fs::read_to_string(root.join("apps/office/serve.mjs")).unwrap();
        assert!(server.contains("/api/"));
        assert!(server.contains("api-token"));
        assert!(server.contains("5180"));

        assert_eq!(out["project_dir"], "apps/office");
        assert_eq!(
            out["artifact"]["payload"]["data"]["entry_file"],
            "apps/office/index.html"
        );
        std::fs::remove_dir_all(root).ok();
    }

    #[tokio::test]
    async fn traversal_is_rejected() {
        let root = temp_root();
        let ctx = test_context(&root);
        let error = AgentsOfficeCreateTool
            .execute(&ctx, json!({ "directory": "../escape" }))
            .await
            .expect_err("traversal must fail");
        assert!(format!("{error}").to_lowercase().contains("directory"));
        std::fs::remove_dir_all(root).ok();
    }
}
