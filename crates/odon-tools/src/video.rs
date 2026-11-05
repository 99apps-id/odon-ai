//! Video inspection and editing backed by the host's `ffmpeg`/`ffprobe`.
//!
//! These are first-class tools instead of model-authored shell commands: the
//! arguments go to `Command` as an argv vector, never through a shell, so a
//! filename containing `;`, quotes or `$()` stays data. They deliberately cover
//! the common operations (probe, trim, concatenate); anything more elaborate
//! belongs in a script the operator can read and version.

use async_trait::async_trait;
use odon_core::{OdonError, Result};
use odon_security::RiskTier;
use serde_json::{json, Value};
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use crate::registry::{Tool, ToolContext};

const MAX_CAPTURED_STDERR: usize = 4_000;

fn tool_error(tool: &str, message: impl Into<String>) -> OdonError {
    OdonError::Tool {
        tool: tool.to_string(),
        message: message.into(),
    }
}

fn required_str<'a>(tool: &str, args: &'a Value, key: &str) -> Result<&'a str> {
    args.get(key)
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .ok_or_else(|| tool_error(tool, format!("Missing '{key}' parameter")))
}

/// Parse `SS(.ms)`, `MM:SS(.ms)` or `HH:MM:SS(.ms)` into seconds.
fn parse_timestamp(raw: &str) -> Option<f64> {
    let trimmed = raw.trim();
    if trimmed.is_empty() || trimmed.starts_with('-') {
        return None;
    }
    if let Ok(value) = trimmed.parse::<f64>() {
        return (value.is_finite() && value >= 0.0).then_some(value);
    }
    let parts: Vec<&str> = trimmed.split(':').collect();
    if parts.len() < 2 || parts.len() > 3 {
        return None;
    }
    let mut total = 0.0f64;
    for part in parts {
        let value: f64 = part.parse().ok()?;
        if !value.is_finite() || value < 0.0 {
            return None;
        }
        total = total * 60.0 + value;
    }
    Some(total)
}

/// One `file '…'` line for ffmpeg's concat demuxer.
fn concat_list_entry(path: &Path) -> String {
    let text = path.to_string_lossy().replace('\\', "/");
    format!("file '{}'\n", text.replace('\'', "'\\''"))
}

fn same_file(a: &Path, b: &Path) -> bool {
    #[cfg(windows)]
    {
        a.to_string_lossy()
            .eq_ignore_ascii_case(&b.to_string_lossy())
    }
    #[cfg(not(windows))]
    {
        a == b
    }
}

fn stderr_text(output: &std::process::Output) -> String {
    let text = String::from_utf8_lossy(&output.stderr);
    let text = text.trim();
    if text.is_empty() {
        "ffmpeg exited without writing an error message".to_string()
    } else {
        text.chars().take(MAX_CAPTURED_STDERR).collect()
    }
}

async fn run_binary(tool: &str, binary: &str, args: &[String]) -> Result<std::process::Output> {
    tokio::process::Command::new(binary)
        .args(args)
        .output()
        .await
        .map_err(|error| {
            if error.kind() == std::io::ErrorKind::NotFound {
                tool_error(
                    tool,
                    format!(
                        "{binary} is not installed or not on PATH. Install ffmpeg first \
                         (Windows: `winget install Gyan.FFmpeg`), then retry."
                    ),
                )
            } else {
                tool_error(tool, format!("could not start {binary}: {error}"))
            }
        })
}

fn ensure_output_parent(output: &Path, tool: &str) -> Result<()> {
    if let Some(parent) = output.parent() {
        std::fs::create_dir_all(parent)
            .map_err(|error| tool_error(tool, format!("cannot create output folder: {error}")))?;
    }
    Ok(())
}

// ─────────────────────────────────────────────────────────────
// video_probe
// ─────────────────────────────────────────────────────────────
pub struct VideoProbeTool;

#[async_trait]
impl Tool for VideoProbeTool {
    fn name(&self) -> &str {
        "video_probe"
    }
    fn description(&self) -> &str {
        "Inspect a video or audio file with ffprobe: duration, container, codecs, resolution, and stream layout. Use this before trimming or merging to learn the stream order."
    }
    fn parameters_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "path": { "type": "string", "description": "Workspace-relative path of the media file" }
            },
            "required": ["path"]
        })
    }
    fn risk_tier(&self) -> RiskTier {
        RiskTier::Tier0ReadOnly
    }
    async fn execute(&self, ctx: &ToolContext, args: Value) -> Result<Value> {
        let raw = required_str(self.name(), &args, "path")?;
        let path = crate::workspace_path::resolve_existing(&ctx.workspace_root, raw)?;
        if !path.is_file() {
            return Err(tool_error(self.name(), format!("not a file: {raw}")));
        }

        let args_out = vec![
            "-v".to_string(),
            "error".to_string(),
            "-print_format".to_string(),
            "json".to_string(),
            "-show_format".to_string(),
            "-show_streams".to_string(),
            path.to_string_lossy().to_string(),
        ];
        let output = run_binary(self.name(), "ffprobe", &args_out).await?;
        if !output.status.success() {
            return Err(tool_error(
                self.name(),
                format!("ffprobe failed: {}", stderr_text(&output)),
            ));
        }
        if output.stdout.len() > 200_000 {
            return Err(tool_error(
                self.name(),
                "ffprobe output is unusually large; probe a shorter file or one stream at a time",
            ));
        }
        let probe: Value = serde_json::from_slice(&output.stdout)
            .map_err(|error| tool_error(self.name(), format!("invalid ffprobe JSON: {error}")))?;

        Ok(json!({
            "path": raw,
            "probe": probe,
        }))
    }
}

// ─────────────────────────────────────────────────────────────
// video_trim
// ─────────────────────────────────────────────────────────────
pub struct VideoTrimTool;

#[async_trait]
impl Tool for VideoTrimTool {
    fn name(&self) -> &str {
        "video_trim"
    }
    fn description(&self) -> &str {
        "Trim a video/audio file between a start and end timestamp. Stream-copies by default (fast, no quality loss, cuts near keyframes); set reencode=true for an exact cut or when the container rejects a copy."
    }
    fn parameters_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "input": { "type": "string", "description": "Workspace-relative source media path" },
                "output": { "type": "string", "description": "Workspace-relative destination path (parent folders are created)" },
                "start": { "type": "string", "description": "Start timestamp: SS, MM:SS, or HH:MM:SS(.ms)" },
                "end": { "type": "string", "description": "End timestamp on the source timeline: SS, MM:SS, or HH:MM:SS(.ms)" },
                "reencode": { "type": "boolean", "description": "Re-encode (H.264/AAC) instead of stream copy", "default": false }
            },
            "required": ["input", "output", "start", "end"]
        })
    }
    fn risk_tier(&self) -> RiskTier {
        RiskTier::Tier1WorkspaceMutating
    }
    async fn execute(&self, ctx: &ToolContext, args: Value) -> Result<Value> {
        let input_raw = required_str(self.name(), &args, "input")?;
        let output_raw = required_str(self.name(), &args, "output")?;
        let start_raw = required_str(self.name(), &args, "start")?;
        let end_raw = required_str(self.name(), &args, "end")?;
        let reencode = args
            .get("reencode")
            .and_then(Value::as_bool)
            .unwrap_or(false);

        let start = parse_timestamp(start_raw).ok_or_else(|| {
            tool_error(
                self.name(),
                format!("invalid start timestamp: {start_raw:?} (use SS, MM:SS, or HH:MM:SS)"),
            )
        })?;
        let end = parse_timestamp(end_raw).ok_or_else(|| {
            tool_error(
                self.name(),
                format!("invalid end timestamp: {end_raw:?} (use SS, MM:SS, or HH:MM:SS)"),
            )
        })?;
        if end <= start {
            return Err(tool_error(
                self.name(),
                "end must be later than start on the source timeline",
            ));
        }
        let duration = end - start;

        let input = crate::workspace_path::resolve_existing(&ctx.workspace_root, input_raw)?;
        if !input.is_file() {
            return Err(tool_error(self.name(), format!("not a file: {input_raw}")));
        }
        let output = crate::workspace_path::resolve_write(&ctx.workspace_root, output_raw)?;
        if same_file(&input, &output) {
            return Err(tool_error(
                self.name(),
                "input and output must be different files",
            ));
        }
        ensure_output_parent(&output, self.name())?;

        let mut ffmpeg_args = vec![
            "-hide_banner".to_string(),
            "-loglevel".to_string(),
            "error".to_string(),
            "-y".to_string(),
            "-i".to_string(),
            input.to_string_lossy().to_string(),
            "-ss".to_string(),
            start_raw.to_string(),
            "-t".to_string(),
            format!("{duration:.3}"),
        ];
        if reencode {
            ffmpeg_args.extend(
                [
                    "-c:v", "libx264", "-preset", "veryfast", "-crf", "20", "-c:a", "aac", "-b:a",
                    "192k",
                ]
                .into_iter()
                .map(str::to_string),
            );
        } else {
            ffmpeg_args.push("-c".to_string());
            ffmpeg_args.push("copy".to_string());
        }
        ffmpeg_args.push(output.to_string_lossy().to_string());

        let result = run_binary(self.name(), "ffmpeg", &ffmpeg_args).await?;
        if !result.status.success() {
            return Err(tool_error(
                self.name(),
                format!("ffmpeg trim failed: {}", stderr_text(&result)),
            ));
        }
        let bytes = std::fs::metadata(&output)
            .map(|meta| meta.len())
            .unwrap_or(0);

        Ok(json!({
            "input": input_raw,
            "output": output_raw,
            "start_seconds": start,
            "end_seconds": end,
            "duration_seconds": duration,
            "reencoded": reencode,
            "bytes": bytes,
        }))
    }
}

// ─────────────────────────────────────────────────────────────
// video_merge
// ─────────────────────────────────────────────────────────────
pub struct VideoMergeTool;

#[async_trait]
impl Tool for VideoMergeTool {
    fn name(&self) -> &str {
        "video_merge"
    }
    fn description(&self) -> &str {
        "Concatenate two or more video/audio files in order. Stream-copies by default (fast; inputs must share codecs and parameters); set reencode=true to use the concat filter when the clips differ."
    }
    fn parameters_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "inputs": {
                    "type": "array",
                    "items": { "type": "string" },
                    "minItems": 2,
                    "description": "Workspace-relative media paths in the order they should play"
                },
                "output": { "type": "string", "description": "Workspace-relative destination path (parent folders are created)" },
                "reencode": { "type": "boolean", "description": "Re-encode with the concat filter instead of stream copy", "default": false }
            },
            "required": ["inputs", "output"]
        })
    }
    fn risk_tier(&self) -> RiskTier {
        RiskTier::Tier1WorkspaceMutating
    }
    async fn execute(&self, ctx: &ToolContext, args: Value) -> Result<Value> {
        let raw_inputs = args
            .get("inputs")
            .and_then(Value::as_array)
            .ok_or_else(|| tool_error(self.name(), "Missing 'inputs' array"))?;
        if raw_inputs.len() < 2 {
            return Err(tool_error(
                self.name(),
                "'inputs' needs at least two media paths",
            ));
        }
        let output_raw = required_str(self.name(), &args, "output")?;
        let reencode = args
            .get("reencode")
            .and_then(Value::as_bool)
            .unwrap_or(false);

        let mut inputs: Vec<PathBuf> = Vec::with_capacity(raw_inputs.len());
        for raw in raw_inputs {
            let value = raw
                .as_str()
                .map(str::trim)
                .filter(|value| !value.is_empty())
                .ok_or_else(|| tool_error(self.name(), "'inputs' entries must be paths"))?;
            let path = crate::workspace_path::resolve_existing(&ctx.workspace_root, value)?;
            if !path.is_file() {
                return Err(tool_error(self.name(), format!("not a file: {value}")));
            }
            inputs.push(path);
        }
        let output = crate::workspace_path::resolve_write(&ctx.workspace_root, output_raw)?;
        if inputs.iter().any(|input| same_file(input, &output)) {
            return Err(tool_error(
                self.name(),
                "the output must not overwrite one of the inputs",
            ));
        }
        ensure_output_parent(&output, self.name())?;

        let mut ffmpeg_args = vec![
            "-hide_banner".to_string(),
            "-loglevel".to_string(),
            "error".to_string(),
            "-y".to_string(),
        ];

        if reencode {
            for input in &inputs {
                ffmpeg_args.push("-i".to_string());
                ffmpeg_args.push(input.to_string_lossy().to_string());
            }
            let streams: String = (0..inputs.len())
                .map(|index| format!("[{index}:v][{index}:a]"))
                .collect();
            ffmpeg_args.extend([
                "-filter_complex".to_string(),
                format!("{streams}concat=n={}:v=1:a=1[v][a]", inputs.len()),
                "-map".to_string(),
                "[v]".to_string(),
                "-map".to_string(),
                "[a]".to_string(),
                "-c:v".to_string(),
                "libx264".to_string(),
                "-preset".to_string(),
                "veryfast".to_string(),
                "-crf".to_string(),
                "20".to_string(),
                "-c:a".to_string(),
                "aac".to_string(),
                "-b:a".to_string(),
                "192k".to_string(),
            ]);
            ffmpeg_args.push(output.to_string_lossy().to_string());

            let result = run_binary(self.name(), "ffmpeg", &ffmpeg_args).await?;
            if !result.status.success() {
                return Err(tool_error(
                    self.name(),
                    format!("ffmpeg merge failed: {}", stderr_text(&result)),
                ));
            }
        } else {
            let list_path = std::env::temp_dir().join(format!(
                "odon-concat-{}-{}.txt",
                std::process::id(),
                SystemTime::now()
                    .duration_since(UNIX_EPOCH)
                    .map(|elapsed| elapsed.as_nanos())
                    .unwrap_or_default()
            ));
            let mut list = String::new();
            for input in &inputs {
                list.push_str(&concat_list_entry(input));
            }
            std::fs::write(&list_path, list).map_err(|error| {
                tool_error(self.name(), format!("cannot write concat list: {error}"))
            })?;

            ffmpeg_args.extend([
                "-f".to_string(),
                "concat".to_string(),
                "-safe".to_string(),
                "0".to_string(),
                "-i".to_string(),
                list_path.to_string_lossy().to_string(),
                "-c".to_string(),
                "copy".to_string(),
                output.to_string_lossy().to_string(),
            ]);
            let result = run_binary(self.name(), "ffmpeg", &ffmpeg_args).await;
            let _ = std::fs::remove_file(&list_path);
            let result = result?;
            if !result.status.success() {
                return Err(tool_error(
                    self.name(),
                    format!(
                        "ffmpeg merge failed (inputs may use different codecs; retry with reencode=true): {}",
                        stderr_text(&result)
                    ),
                ));
            }
        }

        let bytes = std::fs::metadata(&output)
            .map(|meta| meta.len())
            .unwrap_or(0);
        Ok(json!({
            "output": output_raw,
            "inputs": inputs.len(),
            "reencoded": reencode,
            "bytes": bytes,
        }))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_the_timestamp_forms_a_model_uses() {
        assert_eq!(parse_timestamp("12"), Some(12.0));
        assert_eq!(parse_timestamp("01:30"), Some(90.0));
        assert_eq!(parse_timestamp("01:02:03"), Some(3723.0));
        assert_eq!(parse_timestamp("00:00:01.250"), Some(1.25));
        assert_eq!(parse_timestamp("bad"), None);
        assert_eq!(parse_timestamp("-00:01"), None);
        assert_eq!(parse_timestamp(""), None);
    }

    #[test]
    fn concat_list_quotes_ffmpeg_special_characters() {
        let entry = concat_list_entry(Path::new("/tmp/clip's one.mp4"));
        assert_eq!(entry, "file '/tmp/clip'\\''s one.mp4'\n");
        // Windows separators are normalised so the demuxer sees one spelling.
        let windows = concat_list_entry(Path::new(r"C:\work\a b.mp4"));
        assert!(windows.contains("C:/work/a b.mp4"));
    }

    #[test]
    fn schemas_are_object_shaped_with_required_paths() {
        for tool in [
            Box::new(VideoProbeTool) as Box<dyn Tool>,
            Box::new(VideoTrimTool),
            Box::new(VideoMergeTool),
        ] {
            let schema = tool.parameters_schema();
            assert_eq!(schema["type"], "object", "{} schema", tool.name());
            assert!(
                schema["required"]
                    .as_array()
                    .is_some_and(|items| !items.is_empty()),
                "{} must declare required parameters",
                tool.name()
            );
        }
    }

    fn binary_available(binary: &str) -> bool {
        std::process::Command::new(binary)
            .arg("-version")
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .status()
            .map(|status| status.success())
            .unwrap_or(false)
    }

    fn test_context(root: &Path) -> ToolContext {
        use odon_security::{ApprovalEngine, AuditLogger, SecurityPolicy};
        use std::sync::Arc;
        ToolContext {
            session_id: "video-test".to_string(),
            workspace_root: root.to_string_lossy().to_string(),
            approval_engine: Arc::new(ApprovalEngine::new(SecurityPolicy::default())),
            audit_logger: Arc::new(AuditLogger::new()),
            storage: None,
        }
    }

    fn make_clip(path: &Path, color: &str) {
        let source = format!("color=c={color}:s=160x120:d=1:r=30");
        let status = std::process::Command::new("ffmpeg")
            .args([
                "-hide_banner",
                "-loglevel",
                "error",
                "-y",
                "-f",
                "lavfi",
                "-i",
                source.as_str(),
                "-c:v",
                "libx264",
                "-pix_fmt",
                "yuv420p",
                "-g",
                "15",
                "-keyint_min",
                "15",
                "-sc_threshold",
                "0",
                path.to_string_lossy().as_ref(),
            ])
            .status()
            .expect("ffmpeg should run");
        assert!(status.success(), "ffmpeg failed to create the test clip");
    }

    /// End-to-end smoke test against the host's real ffmpeg. Skipped when the
    /// binary is absent, so CI without ffmpeg still passes.
    #[tokio::test]
    async fn real_ffmpeg_probe_trim_and_merge() {
        if !binary_available("ffmpeg") || !binary_available("ffprobe") {
            return;
        }
        let root = std::env::temp_dir().join(format!("odon-video-test-{}", std::process::id()));
        std::fs::create_dir_all(&root).expect("create temp workspace");
        make_clip(&root.join("red.mp4"), "red");
        make_clip(&root.join("blue.mp4"), "blue");
        let ctx = test_context(&root);

        let probe = VideoProbeTool
            .execute(&ctx, json!({ "path": "red.mp4" }))
            .await
            .expect("probe should succeed");
        assert!(
            probe["probe"]["streams"]
                .as_array()
                .is_some_and(|streams| !streams.is_empty()),
            "ffprobe must report at least one stream"
        );

        let trim = VideoTrimTool
            .execute(
                &ctx,
                json!({
                    "input": "red.mp4",
                    "output": "trimmed.mp4",
                    "start": "0",
                    "end": "0.5"
                }),
            )
            .await
            .expect("trim should succeed");
        assert!(trim["bytes"].as_u64().unwrap_or(0) > 0);
        assert!(root.join("trimmed.mp4").is_file());

        let merge = VideoMergeTool
            .execute(
                &ctx,
                json!({
                    "inputs": ["red.mp4", "blue.mp4"],
                    "output": "merged.mp4"
                }),
            )
            .await
            .expect("merge should succeed");
        assert!(merge["bytes"].as_u64().unwrap_or(0) > 0);
        assert!(root.join("merged.mp4").is_file());

        std::fs::remove_dir_all(root).ok();
    }
}
