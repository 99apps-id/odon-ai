//! Scaffold a runnable browser-game project into the workspace.
//!
//! The templates are embedded with `include_str!` so the tool works offline and
//! always writes the same known-good layout. The project is plain HTML/JS with
//! no build step and a tiny CORS-aware static server, which is what makes it
//! previewable through the ODON Browser panel without an npm install.

use async_trait::async_trait;
use odon_core::{Artifact, ArtifactFile, OdonError, Result};
use odon_security::RiskTier;
use serde_json::{json, Value};

use crate::registry::{Tool, ToolContext};

const COMMON: &[(&str, &str)] = &[
    (
        "serve.mjs",
        include_str!("../assets/games/common/serve.mjs"),
    ),
    (
        "README.md",
        include_str!("../assets/games/common/README.md"),
    ),
];

const PLATFORMER: &[(&str, &str)] = &[
    (
        "index.html",
        include_str!("../assets/games/platformer-2d/index.html"),
    ),
    (
        "src/style.css",
        include_str!("../assets/games/platformer-2d/src/style.css"),
    ),
    (
        "src/main.js",
        include_str!("../assets/games/platformer-2d/src/main.js"),
    ),
    (
        "src/game.js",
        include_str!("../assets/games/platformer-2d/src/game.js"),
    ),
];

const ADVENTURE: &[(&str, &str)] = &[
    (
        "index.html",
        include_str!("../assets/games/adventure-3d/index.html"),
    ),
    (
        "src/style.css",
        include_str!("../assets/games/adventure-3d/src/style.css"),
    ),
    (
        "src/main.js",
        include_str!("../assets/games/adventure-3d/src/main.js"),
    ),
];

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Template {
    Platformer,
    Adventure,
}

impl Template {
    fn parse(raw: &str) -> Option<Self> {
        match raw.trim().to_ascii_lowercase().as_str() {
            "platformer-2d" | "platformer" | "2d" => Some(Self::Platformer),
            "adventure-3d" | "adventure" | "3d" => Some(Self::Adventure),
            _ => None,
        }
    }

    fn files(self) -> &'static [(&'static str, &'static str)] {
        match self {
            Self::Platformer => PLATFORMER,
            Self::Adventure => ADVENTURE,
        }
    }

    fn label(self) -> &'static str {
        match self {
            Self::Platformer => "platformer-2d",
            Self::Adventure => "adventure-3d",
        }
    }

    fn framework(self) -> &'static str {
        match self {
            Self::Platformer => "canvas2d",
            Self::Adventure => "three.js",
        }
    }
}

fn slugify(value: &str) -> String {
    let mut out = String::new();
    let mut dash = false;
    for ch in value.trim().to_ascii_lowercase().chars() {
        if ch.is_ascii_alphanumeric() {
            if dash && !out.is_empty() {
                out.push('-');
            }
            out.push(ch);
            dash = false;
        } else if !out.is_empty() {
            dash = true;
        }
    }
    if out.is_empty() {
        "game".to_string()
    } else {
        out
    }
}

fn normalize_directory(raw: &str) -> std::result::Result<String, OdonError> {
    let trimmed = raw.trim().replace('\\', "/");
    let trimmed = trimmed.trim_matches('/');
    if trimmed.is_empty() || trimmed == "." {
        return Ok("games/game".to_string());
    }
    if trimmed
        .split('/')
        .any(|part| part.is_empty() || part == "." || part == "..")
    {
        return Err(OdonError::Tool {
            tool: "game_scaffold".to_string(),
            message:
                "directory must be a clean workspace-relative folder (no '..', no empty segments)"
                    .to_string(),
        });
    }
    Ok(trimmed.to_string())
}

fn render(template: &str, title: &str, port: u16, directory: &str) -> String {
    template
        .replace("{{TITLE}}", title)
        .replace("{{PORT}}", &port.to_string())
        .replace("{{DIR}}", directory)
}

pub struct GameScaffoldTool;

#[async_trait]
impl Tool for GameScaffoldTool {
    fn name(&self) -> &str {
        "game_scaffold"
    }
    fn description(&self) -> &str {
        "Create a runnable browser-game project in the workspace: a 2D canvas platformer or a Three.js 3D adventure, with a zero-dependency CORS preview server and a sample glTF model. Follow up with the built-in game-project skill."
    }
    fn parameters_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "template": {
                    "type": "string",
                    "enum": ["platformer-2d", "adventure-3d"],
                    "description": "2D canvas platformer or Three.js third-person adventure"
                },
                "directory": {
                    "type": "string",
                    "description": "Workspace-relative project folder (default games/<slug> from the title)"
                },
                "title": { "type": "string", "description": "Game title shown in the page and artifact" },
                "port": { "type": "integer", "minimum": 1024, "maximum": 65535, "description": "Preview server port (default 5173)" }
            },
            "required": ["template"]
        })
    }
    fn risk_tier(&self) -> RiskTier {
        RiskTier::Tier1WorkspaceMutating
    }
    async fn execute(&self, ctx: &ToolContext, args: Value) -> Result<Value> {
        let template_raw = args
            .get("template")
            .and_then(Value::as_str)
            .ok_or_else(|| OdonError::Tool {
                tool: self.name().to_string(),
                message: "Missing 'template' parameter".to_string(),
            })?;
        let template = Template::parse(template_raw).ok_or_else(|| OdonError::Tool {
            tool: self.name().to_string(),
            message: format!("unknown template '{template_raw}' (platformer-2d or adventure-3d)"),
        })?;
        let title = args
            .get("title")
            .and_then(Value::as_str)
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .unwrap_or("ODON Game")
            .to_string();
        let directory = match args.get("directory").and_then(Value::as_str) {
            Some(raw) => normalize_directory(raw)?,
            None => format!("games/{}", slugify(&title)),
        };
        let port = args
            .get("port")
            .and_then(Value::as_u64)
            .unwrap_or(5173)
            .clamp(1024, 65535) as u16;

        // index.html is written first and listed first: Artifact::new_code uses
        // the first file as the entry point, and the UI previews HTML entries.
        let mut files: Vec<(String, String)> = Vec::new();
        for (relative, body) in template.files() {
            files.push((
                format!("{directory}/{relative}"),
                render(body, &title, port, &directory),
            ));
        }
        let sample =
            crate::model3d::sample_model_gltf("sample").ok_or_else(|| OdonError::Tool {
                tool: self.name().to_string(),
                message: "failed to build the sample glTF model".to_string(),
            })?;
        files.push((format!("{directory}/assets/models/sample.gltf"), sample));
        for (relative, body) in COMMON {
            files.push((
                format!("{directory}/{relative}"),
                render(body, &title, port, &directory),
            ));
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
            format!("{title} ({})", template.label()),
            format!(
                "Playable {} project in `{}`. Start it with `node serve.mjs` and open http://127.0.0.1:{port}.",
                template.label(),
                directory
            ),
            "javascript",
            Some(template.framework().to_string()),
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
            "template": template.label(),
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
            session_id: "game-test".to_string(),
            workspace_root: root.to_string_lossy().to_string(),
            approval_engine: Arc::new(ApprovalEngine::new(SecurityPolicy::default())),
            audit_logger: Arc::new(AuditLogger::new()),
            storage: None,
        }
    }

    fn temp_root(prefix: &str) -> std::path::PathBuf {
        let root = std::env::temp_dir().join(format!(
            "odon-{prefix}-{}-{}",
            std::process::id(),
            uuid::Uuid::new_v4().simple()
        ));
        std::fs::create_dir_all(&root).unwrap();
        root
    }

    #[tokio::test]
    async fn platformer_project_is_runnable_and_artifact_ready() {
        let root = temp_root("game-2d");
        let ctx = test_context(&root);
        let out = GameScaffoldTool
            .execute(
                &ctx,
                json!({ "template": "platformer-2d", "title": "Coin Run", "directory": "games/coin-run" }),
            )
            .await
            .expect("scaffold should succeed");

        for relative in [
            "index.html",
            "src/main.js",
            "src/game.js",
            "src/style.css",
            "serve.mjs",
            "README.md",
            "assets/models/sample.gltf",
        ] {
            assert!(
                root.join("games/coin-run").join(relative).is_file(),
                "missing {relative}"
            );
        }
        let html = std::fs::read_to_string(root.join("games/coin-run/index.html")).unwrap();
        assert!(html.contains("src/main.js"));
        assert!(html.contains("Coin Run"));
        let server = std::fs::read_to_string(root.join("games/coin-run/serve.mjs")).unwrap();
        assert!(server.contains("Access-Control-Allow-Origin"));
        let model =
            std::fs::read_to_string(root.join("games/coin-run/assets/models/sample.gltf")).unwrap();
        serde_json::from_str::<Value>(&model).expect("sample glTF must parse");

        assert_eq!(out["project_dir"], "games/coin-run");
        assert_eq!(
            out["artifact"]["payload"]["data"]["entry_file"],
            "games/coin-run/index.html"
        );
        assert_eq!(out["preview"]["url"], "http://127.0.0.1:5173");
        std::fs::remove_dir_all(root).ok();
    }

    #[tokio::test]
    async fn adventure_project_uses_three_and_gltf() {
        let root = temp_root("game-3d");
        let ctx = test_context(&root);
        GameScaffoldTool
            .execute(
                &ctx,
                json!({ "template": "adventure-3d", "title": "Ring World", "directory": "games/ring-world" }),
            )
            .await
            .expect("scaffold should succeed");
        let html = std::fs::read_to_string(root.join("games/ring-world/index.html")).unwrap();
        assert!(html.contains("three@"));
        assert!(html.contains("importmap"));
        let main = std::fs::read_to_string(root.join("games/ring-world/src/main.js")).unwrap();
        assert!(main.contains("GLTFLoader"));
        assert!(main.contains("assets/models/sample.gltf"));
        std::fs::remove_dir_all(root).ok();
    }

    #[tokio::test]
    async fn traversal_and_unknown_templates_are_rejected() {
        let root = temp_root("game-escape");
        let ctx = test_context(&root);
        let err = GameScaffoldTool
            .execute(
                &ctx,
                json!({ "template": "platformer-2d", "directory": "../escape" }),
            )
            .await
            .expect_err("traversal must fail");
        assert!(format!("{err}").to_lowercase().contains("directory"));

        let err = GameScaffoldTool
            .execute(&ctx, json!({ "template": "racing" }))
            .await
            .expect_err("unknown template must fail");
        assert!(format!("{err}").contains("unknown template"));
        std::fs::remove_dir_all(root).ok();
    }
}
