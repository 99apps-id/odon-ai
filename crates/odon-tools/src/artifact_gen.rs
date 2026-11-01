use async_trait::async_trait;
use odon_core::{Artifact, ArtifactFile, Result};
use odon_security::RiskTier;
use serde_json::{json, Value};

use crate::registry::{Tool, ToolContext};

pub struct ArtifactCreateCodeTool;

#[async_trait]
impl Tool for ArtifactCreateCodeTool {
    fn name(&self) -> &str {
        "artifact_create_code"
    }
    fn description(&self) -> &str {
        "Synthesize a code project or module artifact (React 19, Rust, TypeScript, etc.)"
    }
    fn parameters_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "title": { "type": "string", "description": "Project or artifact title" },
                "summary": { "type": "string", "description": "Brief description of the generated code" },
                "language": { "type": "string", "description": "Programming language (e.g. TypeScript, Rust)" },
                "framework": { "type": "string", "description": "Framework (e.g. React 19, Vite, Axum)" },
                "files": {
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
            "required": ["title", "summary", "language", "files"]
        })
    }
    fn risk_tier(&self) -> RiskTier {
        RiskTier::Tier1WorkspaceMutating
    }
    async fn execute(&self, ctx: &ToolContext, args: Value) -> Result<Value> {
        let title = args.get("title").and_then(|t| t.as_str()).unwrap_or("Untitled Project");
        let summary = args.get("summary").and_then(|s| s.as_str()).unwrap_or("Generated code");
        let language = args.get("language").and_then(|l| l.as_str()).unwrap_or("TypeScript");
        let framework = args.get("framework").and_then(|f| f.as_str()).map(|s| s.to_string());

        let mut files = Vec::new();
        if let Some(arr) = args.get("files").and_then(|f| f.as_array()) {
            for item in arr {
                if let (Some(path), Some(content)) = (
                    item.get("path").and_then(|p| p.as_str()),
                    item.get("content").and_then(|c| c.as_str()),
                ) {
                    files.push(ArtifactFile {
                        path: path.to_string(),
                        content: content.to_string(),
                    });
                }
            }
        }

        let artifact = Artifact::new_code(
            &ctx.session_id,
            title,
            summary,
            language,
            framework,
            files,
        );

        Ok(json!({
            "artifact_id": artifact.id,
            "title": artifact.title,
            "kind": "code",
            "summary": artifact.summary,
            "artifact": artifact,
        }))
    }
}

pub struct ArtifactCreateDocumentTool;

#[async_trait]
impl Tool for ArtifactCreateDocumentTool {
    fn name(&self) -> &str {
        "artifact_create_document"
    }
    fn description(&self) -> &str {
        "Create a formatted Markdown document artifact (PRD, Technical Spec, Architecture, Report)"
    }
    fn parameters_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "title": { "type": "string", "description": "Document title" },
                "doc_type": { "type": "string", "description": "Type of document (e.g. PRD, Architecture, Report)" },
                "markdown": { "type": "string", "description": "Markdown body content" }
            },
            "required": ["title", "doc_type", "markdown"]
        })
    }
    fn risk_tier(&self) -> RiskTier {
        RiskTier::Tier1WorkspaceMutating
    }
    async fn execute(&self, ctx: &ToolContext, args: Value) -> Result<Value> {
        let title = args.get("title").and_then(|t| t.as_str()).unwrap_or("Untitled Document");
        let doc_type = args.get("doc_type").and_then(|d| d.as_str()).unwrap_or("Document");
        let markdown = args.get("markdown").and_then(|m| m.as_str()).unwrap_or("");

        let artifact = Artifact::new_document(&ctx.session_id, title, doc_type, markdown);

        Ok(json!({
            "artifact_id": artifact.id,
            "title": artifact.title,
            "kind": "document",
            "summary": artifact.summary,
            "artifact": artifact,
        }))
    }
}

pub struct ArtifactCreateImageTool;

#[async_trait]
impl Tool for ArtifactCreateImageTool {
    fn name(&self) -> &str {
        "artifact_create_image"
    }
    fn description(&self) -> &str {
        "Create an image generation artifact card with prompt, aspect ratio, and model metadata"
    }
    fn parameters_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "title": { "type": "string", "description": "Image title (e.g. hero-illustration.png)" },
                "prompt": { "type": "string", "description": "Image generation prompt" },
                "width": { "type": "integer", "default": 1920 },
                "height": { "type": "integer", "default": 1080 },
                "model": { "type": "string", "default": "FLUX.1-pro" }
            },
            "required": ["title", "prompt"]
        })
    }
    fn risk_tier(&self) -> RiskTier {
        RiskTier::Tier1WorkspaceMutating
    }
    async fn execute(&self, ctx: &ToolContext, args: Value) -> Result<Value> {
        let title = args.get("title").and_then(|t| t.as_str()).unwrap_or("image.png");
        let prompt = args.get("prompt").and_then(|p| p.as_str()).unwrap_or("");
        let width = args.get("width").and_then(|w| w.as_u64()).unwrap_or(1920) as u32;
        let height = args.get("height").and_then(|h| h.as_u64()).unwrap_or(1080) as u32;
        let model = args.get("model").and_then(|m| m.as_str()).unwrap_or("FLUX.1-pro");

        let artifact = Artifact::new_image(
            &ctx.session_id,
            title,
            prompt,
            width,
            height,
            model,
            None,
            Some("data:image/svg+xml;utf8,<svg xmlns='http://www.w3.org/2000/svg' width='1920' height='1080'><rect width='100%' height='100%' fill='%236366f1'/><text x='50%' y='50%' dominant-baseline='middle' text-anchor='middle' fill='white' font-family='sans-serif' font-size='48'>Hero Illustration Preview</text></svg>".to_string()),
            1_800_000,
        );

        Ok(json!({
            "artifact_id": artifact.id,
            "title": artifact.title,
            "kind": "image",
            "summary": artifact.summary,
            "artifact": artifact,
        }))
    }
}
