use async_trait::async_trait;
use odon_core::{Artifact, ArtifactFile, OdonError, Result};
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
        let title = args
            .get("title")
            .and_then(|t| t.as_str())
            .unwrap_or("Untitled Project");
        let summary = args
            .get("summary")
            .and_then(|s| s.as_str())
            .unwrap_or("Generated code");
        let language = args
            .get("language")
            .and_then(|l| l.as_str())
            .unwrap_or("TypeScript");
        let framework = args
            .get("framework")
            .and_then(|f| f.as_str())
            .map(|s| s.to_string());

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

        let artifact =
            Artifact::new_code(&ctx.session_id, title, summary, language, framework, files);

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
        let title = args
            .get("title")
            .and_then(|t| t.as_str())
            .unwrap_or("Untitled Document");
        let doc_type = args
            .get("doc_type")
            .and_then(|d| d.as_str())
            .unwrap_or("Document");
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

/// Real image generation. Calls an image model (OpenAI `gpt-image-1` or Google
/// Gemini image preview), writes the PNG into `<workspace>/generated/images/`
/// and returns an image artifact whose preview URL is served by the daemon.
/// If no image-capable key is configured it fails loudly instead of faking output.

pub struct ArtifactCreateChartTool;

#[async_trait]
impl Tool for ArtifactCreateChartTool {
    fn name(&self) -> &str {
        "artifact_create_chart"
    }
    fn description(&self) -> &str {
        "Create a bar or line chart artifact rendered as SVG in the chat and browser preview"
    }
    fn parameters_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "title": { "type": "string", "description": "Chart title" },
                "chart_type": { "type": "string", "enum": ["bar", "line"], "description": "Chart type" },
                "labels": {
                    "type": "array",
                    "items": { "type": "string" },
                    "description": "X-axis labels"
                },
                "values": {
                    "type": "array",
                    "items": { "type": "number" },
                    "description": "Values for a single series"
                },
                "series": {
                    "type": "array",
                    "description": "Optional multiple series. Use instead of values.",
                    "items": {
                        "type": "object",
                        "properties": {
                            "name": { "type": "string" },
                            "values": { "type": "array", "items": { "type": "number" } }
                        },
                        "required": ["values"]
                    }
                }
            },
            "required": ["title", "chart_type", "labels"]
        })
    }
    fn risk_tier(&self) -> RiskTier {
        RiskTier::Tier1WorkspaceMutating
    }
    async fn execute(&self, ctx: &ToolContext, args: Value) -> Result<Value> {
        let title = args
            .get("title")
            .and_then(|t| t.as_str())
            .unwrap_or("Chart");
        let chart_type = args
            .get("chart_type")
            .and_then(|t| t.as_str())
            .unwrap_or("bar");
        if chart_type != "bar" && chart_type != "line" {
            return Err(OdonError::Tool {
                tool: self.name().to_string(),
                message: "chart_type must be 'bar' or 'line'".to_string(),
            });
        }
        let labels: Vec<String> = args
            .get("labels")
            .and_then(|l| l.as_array())
            .map(|arr| {
                arr.iter()
                    .filter_map(|v| v.as_str().map(str::to_string))
                    .collect()
            })
            .unwrap_or_default();
        if labels.is_empty() {
            return Err(OdonError::Tool {
                tool: self.name().to_string(),
                message: "labels must contain at least one entry".to_string(),
            });
        }

        let mut spec = json!({
            "type": chart_type,
            "title": title,
            "labels": labels,
        });
        if let Some(series) = args.get("series").and_then(|s| s.as_array()) {
            if !series.is_empty() {
                spec["series"] = Value::Array(series.clone());
            }
        }
        if spec.get("series").is_none() {
            let values = args
                .get("values")
                .and_then(|v| v.as_array())
                .cloned()
                .unwrap_or_default();
            if values.len() != labels.len() {
                return Err(OdonError::Tool {
                    tool: self.name().to_string(),
                    message: format!(
                        "values ({}) must match labels ({})",
                        values.len(),
                        labels.len()
                    ),
                });
            }
            spec["values"] = Value::Array(values);
        }

        let markdown = format!("## {}\n\n```chart\n{}\n```\n", title, spec);
        let artifact = Artifact::new_document(&ctx.session_id, title, "Chart", &markdown);

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

struct ImageBackend {
    kind: &'static str, // "openai" | "google"
    key: String,
    base_url: String,
}

fn pick_backend(ctx: &ToolContext, want: Option<&str>) -> Option<ImageBackend> {
    let stored = |id: &str| {
        ctx.storage
            .as_ref()
            .and_then(|s| s.get_provider_config(id).ok().flatten())
    };
    let env = |n: &str| std::env::var(n).ok().filter(|v| !v.trim().is_empty());
    let openai = || {
        let (key, base) = match stored("openai") {
            Some(c) if !c.api_key.is_empty() => (c.api_key, c.base_url),
            _ => (env("OPENAI_API_KEY")?, env("OPENAI_BASE_URL")),
        };
        Some(ImageBackend {
            kind: "openai",
            key,
            base_url: base.unwrap_or_else(|| "https://api.openai.com/v1".into()),
        })
    };
    let google = || {
        let key = match stored("google") {
            Some(c) if !c.api_key.is_empty() => c.api_key,
            _ => env("GEMINI_API_KEY").or_else(|| env("GOOGLE_API_KEY"))?,
        };
        Some(ImageBackend {
            kind: "google",
            key,
            base_url: "https://generativelanguage.googleapis.com/v1beta".into(),
        })
    };
    match want {
        Some("google") | Some("gemini") => google(),
        Some("openai") => openai(),
        _ => openai().or_else(google),
    }
}

fn slugify(s: &str) -> String {
    let base: String = s
        .trim_end_matches(".png")
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() {
                c.to_ascii_lowercase()
            } else {
                '-'
            }
        })
        .collect();
    let base = base.trim_matches('-');
    let base = if base.is_empty() { "image" } else { base };
    base.chars().take(48).collect()
}

fn png_dimensions(bytes: &[u8]) -> Option<(u32, u32)> {
    if bytes.len() > 24 && &bytes[0..8] == b"\x89PNG\r\n\x1a\n" {
        let w = u32::from_be_bytes(bytes[16..20].try_into().ok()?);
        let h = u32::from_be_bytes(bytes[20..24].try_into().ok()?);
        return Some((w, h));
    }
    None
}

#[async_trait]
impl Tool for ArtifactCreateImageTool {
    fn name(&self) -> &str {
        "artifact_create_image"
    }
    fn description(&self) -> &str {
        "Generate a real image from a text prompt with an image model and save it as a PNG file in the workspace (generated/images/). Returns an image artifact the user can view and download."
    }
    fn parameters_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "title": { "type": "string", "description": "Image title / file name (e.g. hero-illustration)" },
                "prompt": { "type": "string", "description": "Detailed image generation prompt" },
                "size": { "type": "string", "description": "OpenAI size: 1024x1024, 1536x1024 or 1024x1536 (default 1024x1024)" },
                "provider": { "type": "string", "description": "Optional: 'openai' or 'google'. Defaults to whichever is configured." }
            },
            "required": ["title", "prompt"]
        })
    }
    fn risk_tier(&self) -> RiskTier {
        RiskTier::Tier1WorkspaceMutating
    }
    async fn execute(&self, ctx: &ToolContext, args: Value) -> Result<Value> {
        use base64::Engine;
        let tool_err = |m: String| OdonError::Tool {
            tool: "artifact_create_image".into(),
            message: m,
        };

        let title = args
            .get("title")
            .and_then(|t| t.as_str())
            .unwrap_or("image");
        let prompt = args
            .get("prompt")
            .and_then(|p| p.as_str())
            .filter(|p| !p.trim().is_empty())
            .ok_or_else(|| tool_err("'prompt' is required".into()))?;
        let size = args
            .get("size")
            .and_then(|s| s.as_str())
            .unwrap_or("1024x1024");
        let want = args.get("provider").and_then(|p| p.as_str());

        let backend = pick_backend(ctx, want);

        let (bytes, model): (Vec<u8>, String) = match backend {
            Some(backend) => {
                let client = reqwest::Client::builder()
                    .timeout(std::time::Duration::from_secs(180))
                    .build()
                    .map_err(|e| tool_err(e.to_string()))?;

                if backend.kind == "openai" {
                    let model = "gpt-image-1";
                    let resp = client
                        .post(format!(
                            "{}/images/generations",
                            backend.base_url.trim_end_matches('/')
                        ))
                        .bearer_auth(&backend.key)
                        .json(&json!({ "model": model, "prompt": prompt, "size": size, "n": 1 }))
                        .send()
                        .await
                        .map_err(|e| tool_err(format!("Image request failed: {e}")))?;
                    let status = resp.status();
                    let v: Value = resp.json().await.map_err(|e| tool_err(e.to_string()))?;
                    if !status.is_success() {
                        return Err(tool_err(format!("Image API error {status}: {v}")));
                    }
                    let item = v
                        .pointer("/data/0")
                        .ok_or_else(|| tool_err("Image API returned no data".into()))?;
                    if let Some(b64) = item.get("b64_json").and_then(|b| b.as_str()) {
                        (
                            base64::engine::general_purpose::STANDARD
                                .decode(b64)
                                .map_err(|e| tool_err(e.to_string()))?,
                            model.into(),
                        )
                    } else if let Some(url) = item.get("url").and_then(|u| u.as_str()) {
                        let b = client
                            .get(url)
                            .send()
                            .await
                            .map_err(|e| tool_err(e.to_string()))?
                            .bytes()
                            .await
                            .map_err(|e| tool_err(e.to_string()))?;
                        (b.to_vec(), model.into())
                    } else {
                        return Err(tool_err(
                            "Image API response had neither b64_json nor url".into(),
                        ));
                    }
                } else {
                    let model = "gemini-2.5-flash-image";
                    let resp = client
                        .post(format!(
                            "{}/models/{}:generateContent",
                            backend.base_url, model
                        ))
                        .header("x-goog-api-key", &backend.key)
                        .json(&json!({
                            "contents": [{ "parts": [{ "text": prompt }] }],
                            "generationConfig": { "responseModalities": ["IMAGE", "TEXT"] }
                        }))
                        .send()
                        .await
                        .map_err(|e| tool_err(format!("Image request failed: {e}")))?;
                    let status = resp.status();
                    let v: Value = resp.json().await.map_err(|e| tool_err(e.to_string()))?;
                    if !status.is_success() {
                        return Err(tool_err(format!("Gemini image API error {status}: {v}")));
                    }
                    let b64 = v
                        .pointer("/candidates/0/content/parts")
                        .and_then(|p| p.as_array())
                        .and_then(|parts| {
                            parts.iter().find_map(|p| {
                                p.get("inlineData")
                                    .or_else(|| p.get("inline_data"))
                                    .and_then(|d| d.get("data"))
                                    .and_then(|d| d.as_str())
                            })
                        })
                        .ok_or_else(|| tool_err(format!("Gemini returned no image: {v}")))?;
                    (
                        base64::engine::general_purpose::STANDARD
                            .decode(b64)
                            .map_err(|e| tool_err(e.to_string()))?,
                        model.into(),
                    )
                }
            }
            None => {
                return Err(tool_err(
                    "No image generation provider is configured. Add an OpenAI or Google Gemini API key in Settings, or set OPENAI_API_KEY / GEMINI_API_KEY.".into(),
                ));
            }
        };

        let (width, height) = png_dimensions(&bytes).unwrap_or((1024, 1024));
        let rel = format!(
            "generated/images/{}-{}.png",
            slugify(title),
            &uuid::Uuid::new_v4().simple().to_string()[..8]
        );
        let abs = crate::workspace_path::resolve_write(&ctx.workspace_root, &rel)?;
        if let Some(dir) = abs.parent() {
            std::fs::create_dir_all(dir)
                .map_err(|e| tool_err(format!("Cannot create {}: {e}", dir.display())))?;
        }
        std::fs::write(&abs, &bytes).map_err(|e| tool_err(format!("Cannot save image: {e}")))?;

        let artifact = Artifact::new_image(
            &ctx.session_id,
            title,
            prompt,
            width,
            height,
            model,
            Some(abs.to_string_lossy().to_string()),
            Some(format!("/api/files/{rel}")),
            bytes.len(),
        );

        Ok(json!({
            "artifact_id": artifact.id,
            "title": artifact.title,
            "kind": "image",
            "saved_to": abs.to_string_lossy(),
            "relative_path": rel,
            "summary": artifact.summary,
            "artifact": artifact,
        }))
    }
}

pub struct ArtifactUpdateTool;

#[async_trait]
impl Tool for ArtifactUpdateTool {
    fn name(&self) -> &str {
        "artifact_update"
    }
    fn description(&self) -> &str {
        "Update the content or metadata of an existing artifact"
    }
    fn parameters_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "artifact_id": { "type": "string", "description": "ID of artifact to update" },
                "content": { "type": "string", "description": "New artifact content" }
            },
            "required": ["artifact_id", "content"]
        })
    }
    fn risk_tier(&self) -> RiskTier {
        RiskTier::Tier1WorkspaceMutating
    }
    async fn execute(&self, ctx: &ToolContext, args: Value) -> Result<Value> {
        let artifact_id = args
            .get("artifact_id")
            .and_then(|a| a.as_str())
            .unwrap_or("");
        let content = args.get("content").and_then(|c| c.as_str()).unwrap_or("");

        if let Some(storage) = &ctx.storage {
            if let Ok(mut art) = storage.get_artifact(artifact_id) {
                art.summary = format!("Updated: {}", content.chars().take(60).collect::<String>());
                match &mut art.payload {
                    odon_core::ArtifactPayload::Document {
                        markdown,
                        size_bytes,
                        ..
                    } => {
                        *markdown = content.to_string();
                        *size_bytes = content.len();
                    }
                    odon_core::ArtifactPayload::Code { files, .. } => {
                        if let Some(first) = files.first_mut() {
                            first.content = content.to_string();
                        }
                    }
                    odon_core::ArtifactPayload::Patch { diff, .. } => {
                        *diff = content.to_string();
                    }
                    _ => {}
                }
                let _ = storage.save_artifact(&art);
                return Ok(
                    json!({ "updated": true, "artifact_id": artifact_id, "size_bytes": content.len() }),
                );
            }
        }

        Ok(json!({ "updated": false, "artifact_id": artifact_id }))
    }
}

pub struct ArtifactImportTool;

#[async_trait]
impl Tool for ArtifactImportTool {
    fn name(&self) -> &str {
        "artifact_import"
    }
    fn description(&self) -> &str {
        "Import an existing file from workspace/disk into persistent artifacts storage"
    }
    fn parameters_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "path": { "type": "string", "description": "Workspace-relative path of file to import" },
                "title": { "type": "string", "description": "Optional custom artifact title" },
                "summary": { "type": "string", "description": "Optional brief summary" }
            },
            "required": ["path"]
        })
    }
    fn risk_tier(&self) -> RiskTier {
        RiskTier::Tier0ReadOnly
    }
    async fn execute(&self, ctx: &ToolContext, args: Value) -> Result<Value> {
        let path_str = args.get("path").and_then(|p| p.as_str()).unwrap_or("");
        if path_str.is_empty() {
            return Err(OdonError::Tool {
                tool: self.name().to_string(),
                message: "Path is required".into(),
            });
        }

        let abs = crate::workspace_path::resolve_existing(&ctx.workspace_root, path_str)?;

        if !abs.exists() {
            return Err(OdonError::Tool {
                tool: self.name().to_string(),
                message: format!("File not found: {}", abs.display()),
            });
        }

        let bytes = std::fs::read(&abs).map_err(|e| OdonError::Tool {
            tool: self.name().to_string(),
            message: format!("Failed to read file: {e}"),
        })?;

        let file_name = abs
            .file_name()
            .unwrap_or_default()
            .to_string_lossy()
            .to_string();
        let ext = abs
            .extension()
            .map(|e| e.to_string_lossy().to_lowercase())
            .unwrap_or_default();
        let title = args
            .get("title")
            .and_then(|t| t.as_str())
            .map(String::from)
            .unwrap_or_else(|| file_name.clone());
        let summary = args
            .get("summary")
            .and_then(|s| s.as_str())
            .map(String::from)
            .unwrap_or_else(|| format!("Imported from {}", path_str));

        let artifact = match ext.as_str() {
            "rs" | "ts" | "tsx" | "js" | "jsx" | "py" | "go" | "html" | "css" | "json" | "sh"
            | "yaml" | "yml" | "toml" => {
                let code_text = String::from_utf8_lossy(&bytes).to_string();
                let lang = match ext.as_str() {
                    "rs" => "rust",
                    "ts" | "tsx" => "typescript",
                    "js" | "jsx" => "javascript",
                    "py" => "python",
                    "go" => "go",
                    "html" => "html",
                    "css" => "css",
                    "json" => "json",
                    "sh" => "bash",
                    _ => "plaintext",
                };
                Artifact::new_code(
                    &ctx.session_id,
                    &title,
                    &summary,
                    lang,
                    None,
                    vec![odon_core::ArtifactFile {
                        path: file_name.clone(),
                        content: code_text,
                    }],
                )
            }
            "png" | "jpg" | "jpeg" | "webp" | "gif" => Artifact::new_image(
                &ctx.session_id,
                &title,
                &summary,
                1024,
                1024,
                "imported",
                Some(abs.to_string_lossy().to_string()),
                Some(format!("/api/files/{}", path_str.replace('\\', "/"))),
                bytes.len(),
            ),
            _ => {
                let doc_text = String::from_utf8_lossy(&bytes).to_string();
                let doc_type = if ext == "md" { "markdown" } else { "text" };
                Artifact::new_document(&ctx.session_id, &title, doc_type, &doc_text)
            }
        };

        if let Some(storage) = &ctx.storage {
            storage
                .save_artifact(&artifact)
                .map_err(|e| OdonError::Tool {
                    tool: self.name().to_string(),
                    message: format!("Failed to save artifact to storage: {e}"),
                })?;
        }

        Ok(json!({
            "imported": true,
            "artifact_id": artifact.id,
            "title": artifact.title,
            "kind": artifact.kind,
            "size_bytes": bytes.len(),
            "path": path_str
        }))
    }
}
