use crate::registry::{Tool, ToolContext};
use async_trait::async_trait;
use odon_core::error::{OdonError, Result};
use odon_security::RiskTier;
use serde_json::{json, Value};
use std::path::{Path, PathBuf};

pub struct ReviewDiffTool;

#[async_trait]
impl Tool for ReviewDiffTool {
    fn name(&self) -> &str {
        "review_diff"
    }
    fn description(&self) -> &str {
        "Review a git diff and return structured findings by severity and category"
    }
    fn parameters_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "base": { "type": "string", "description": "Base revision or commit (default: HEAD)" },
                "path": { "type": "string", "description": "Optional path filter" }
            }
        })
    }
    fn risk_tier(&self) -> RiskTier {
        RiskTier::Tier0ReadOnly
    }

    async fn execute(&self, ctx: &ToolContext, args: Value) -> Result<Value> {
        let base = args.get("base").and_then(Value::as_str).unwrap_or("HEAD");
        let path_filter = args
            .get("path")
            .and_then(Value::as_str)
            .map(|s| s.to_string());
        let diff = read_workspace_diff(&ctx.workspace_root, base, path_filter.as_deref()).await?;
        let findings = review_diff_content(&diff);
        Ok(json!({ "base": base, "findings": findings, "truncated": false }))
    }
}

pub struct ReviewFileTool;

#[async_trait]
impl Tool for ReviewFileTool {
    fn name(&self) -> &str {
        "review_file"
    }
    fn description(&self) -> &str {
        "Review a single source file and return structured findings"
    }
    fn parameters_schema(&self) -> Value {
        json!({
            "type": "object",
            "required": ["path"],
            "properties": {
                "path": { "type": "string", "minLength": 1 }
            }
        })
    }
    fn risk_tier(&self) -> RiskTier {
        RiskTier::Tier0ReadOnly
    }

    async fn execute(&self, ctx: &ToolContext, args: Value) -> Result<Value> {
        let path = args.get("path").and_then(Value::as_str).unwrap_or("");
        if path.is_empty() {
            return tool_error("review_file", "path is required");
        }
        let resolved = resolve_existing(&ctx.workspace_root, path, "review_file")?;
        if !resolved.is_file() {
            return tool_error(
                "review_file",
                format!("{} is not a file", resolved.display()),
            );
        }
        let content =
            std::fs::read_to_string(&resolved).map_err(|e| tool_message("review_file", e))?;
        let relative = path_for_output(
            resolved
                .strip_prefix(&ctx.workspace_root)
                .unwrap_or(&resolved),
        );
        let findings = review_file_content(&relative, &content);
        Ok(json!({ "path": relative, "findings": findings }))
    }
}

pub struct ReviewSecurityTool;

#[async_trait]
impl Tool for ReviewSecurityTool {
    fn name(&self) -> &str {
        "review_security"
    }
    fn description(&self) -> &str {
        "Run a security-focused review against a git diff or a single file"
    }
    fn parameters_schema(&self) -> Value {
        json!({
            "type": "object",
            "anyOf": [
                {
                    "required": ["path"],
                    "properties": { "path": { "type": "string", "minLength": 1 } }
                },
                {
                    "properties": { "base": { "type": "string" } }
                }
            ]
        })
    }
    fn risk_tier(&self) -> RiskTier {
        RiskTier::Tier0ReadOnly
    }

    async fn execute(&self, ctx: &ToolContext, args: Value) -> Result<Value> {
        if let Some(path) = args.get("path").and_then(Value::as_str) {
            if path.is_empty() {
                return tool_error("review_security", "path is required");
            }
            let resolved = resolve_existing(&ctx.workspace_root, path, "review_security")?;
            if !resolved.is_file() {
                return tool_error(
                    "review_security",
                    format!("{} is not a file", resolved.display()),
                );
            }
            let content = std::fs::read_to_string(&resolved)
                .map_err(|e| tool_message("review_security", e))?;
            let relative = path_for_output(
                resolved
                    .strip_prefix(&ctx.workspace_root)
                    .unwrap_or(&resolved),
            );
            let findings = security_review_file(&relative, &content);
            return Ok(json!({ "path": relative, "findings": findings }));
        }
        let base = args.get("base").and_then(Value::as_str).unwrap_or("HEAD");
        let diff = read_workspace_diff(&ctx.workspace_root, base, None).await?;
        let findings = security_review_diff(&diff);
        Ok(json!({ "base": base, "findings": findings }))
    }
}

pub struct ReviewSessionChangesTool;

#[async_trait]
impl Tool for ReviewSessionChangesTool {
    fn name(&self) -> &str {
        "review_session_changes"
    }
    fn description(&self) -> &str {
        "Review changes from the current session's working tree against HEAD"
    }
    fn parameters_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "path": { "type": "string", "description": "Optional path filter" }
            }
        })
    }
    fn risk_tier(&self) -> RiskTier {
        RiskTier::Tier0ReadOnly
    }

    async fn execute(&self, ctx: &ToolContext, args: Value) -> Result<Value> {
        let path_filter = args
            .get("path")
            .and_then(Value::as_str)
            .map(|s| s.to_string());
        let diff = read_workspace_diff(&ctx.workspace_root, "HEAD", path_filter.as_deref()).await?;
        let findings = review_diff_content(&diff);
        Ok(json!({ "base": "HEAD", "findings": findings, "truncated": false }))
    }
}

pub struct ChangeSummaryTool;

#[async_trait]
impl Tool for ChangeSummaryTool {
    fn name(&self) -> &str {
        "change_summary"
    }
    fn description(&self) -> &str {
        "Summarize current working tree changes since HEAD in a compact structured form"
    }
    fn parameters_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "path": { "type": "string", "description": "Optional path filter" }
            }
        })
    }
    fn risk_tier(&self) -> RiskTier {
        RiskTier::Tier0ReadOnly
    }

    async fn execute(&self, ctx: &ToolContext, args: Value) -> Result<Value> {
        let path_filter = args
            .get("path")
            .and_then(Value::as_str)
            .map(|s| s.to_string());
        let diff = read_workspace_diff(&ctx.workspace_root, "HEAD", path_filter.as_deref()).await?;
        let summary = summarize_diff(&diff);
        Ok(json!({ "base": "HEAD", "summary": summary }))
    }
}

#[derive(Debug, serde::Serialize)]
struct Finding {
    severity: &'static str,
    category: &'static str,
    path: String,
    line: Option<usize>,
    finding: String,
    suggested_fix: Option<String>,
}

fn review_diff_content(diff: &str) -> Vec<Finding> {
    let mut findings = Vec::new();
    for (relative_path, line, current) in parse_diff_lines(diff) {
        if let Some(finding) = review_line(&relative_path, line, current) {
            findings.push(finding);
        }
    }
    findings
}

fn review_file_content(relative_path: &str, content: &str) -> Vec<Finding> {
    let mut findings = Vec::new();
    for (index, line) in content.lines().enumerate() {
        if let Some(finding) = review_line(relative_path, index + 1, line) {
            findings.push(finding);
        }
    }
    findings
}

fn review_line(relative_path: &str, line: usize, text: &str) -> Option<Finding> {
    let normalized = text.trim();
    if normalized.is_empty() {
        return None;
    }
    let lowered = normalized.to_ascii_lowercase();
    if lowered.starts_with("unsafe ") {
        return Some(Finding {
            severity: "high",
            category: "safety",
            path: relative_path.to_string(),
            line: Some(line),
            finding: normalized.to_string(),
            suggested_fix: Some("Keep unsafe blocks minimal, document the invariant, and isolate behind a safe API.".to_string()),
        });
    }
    if lowered.contains("unwrap()") {
        return Some(Finding {
            severity: "medium",
            category: "robustness",
            path: relative_path.to_string(),
            line: Some(line),
            finding: normalized.to_string(),
            suggested_fix: Some("Prefer explicit error handling or context-aware unwrap_or_else with a useful message.".to_string()),
        });
    }
    if lowered.contains("expect(\"") && !lowered.contains("expect(\"\"") {
        return Some(Finding {
            severity: "low",
            category: "clarity",
            path: relative_path.to_string(),
            line: Some(line),
            finding: normalized.to_string(),
            suggested_fix: Some(
                "Use a more specific message or replace with context-aware error handling."
                    .to_string(),
            ),
        });
    }
    if lowered.contains("todo!") || lowered.contains("unimplemented!") {
        return Some(Finding {
            severity: "medium",
            category: "correctness",
            path: relative_path.to_string(),
            line: Some(line),
            finding: normalized.to_string(),
            suggested_fix: Some(
                "Implement the branch or gate it behind an explicit feature flag.".to_string(),
            ),
        });
    }
    None
}

fn security_review_diff(diff: &str) -> Vec<Finding> {
    let mut findings = Vec::new();
    for (relative_path, line, current) in parse_diff_lines(diff) {
        if let Some(finding) = security_review_line(&relative_path, line, current) {
            findings.push(finding);
        }
    }
    findings
}

fn security_review_file(relative_path: &str, content: &str) -> Vec<Finding> {
    let mut findings = Vec::new();
    for (index, line) in content.lines().enumerate() {
        if let Some(finding) = security_review_line(relative_path, index + 1, line) {
            findings.push(finding);
        }
    }
    findings
}

fn security_review_line(relative_path: &str, line: usize, text: &str) -> Option<Finding> {
    let normalized = text.trim();
    if normalized.is_empty() {
        return None;
    }
    let lowered = normalized.to_ascii_lowercase();
    if lowered.contains("secret")
        && (lowered.contains("print") || lowered.contains("log") || lowered.contains("eprint"))
    {
        return Some(Finding {
            severity: "critical",
            category: "secrets",
            path: relative_path.to_string(),
            line: Some(line),
            finding: normalized.to_string(),
            suggested_fix: Some("Remove secret material from logs; use a redaction helper or structured logging with allowlists.".to_string()),
        });
    }
    if lowered.contains("password") && lowered.contains("=") {
        return Some(Finding {
            severity: "high",
            category: "secrets",
            path: relative_path.to_string(),
            line: Some(line),
            finding: normalized.to_string(),
            suggested_fix: Some(
                "Use a secrets manager or environment-backed config instead of inline credentials."
                    .to_string(),
            ),
        });
    }
    if lowered.contains("eval(") || lowered.contains("exec(") || lowered.contains("execsync(") {
        return Some(Finding {
            severity: "high",
            category: "injection",
            path: relative_path.to_string(),
            line: Some(line),
            finding: normalized.to_string(),
            suggested_fix: Some("Avoid dynamic evaluation on untrusted input; use a safe parser or command allowlist.".to_string()),
        });
    }
    if lowered.contains("http://") && !lowered.contains("localhost") {
        return Some(Finding {
            severity: "medium",
            category: "transport",
            path: relative_path.to_string(),
            line: Some(line),
            finding: normalized.to_string(),
            suggested_fix: Some(
                "Prefer HTTPS or internal TLS; validate certificate pinning if applicable."
                    .to_string(),
            ),
        });
    }
    None
}

fn summarize_diff(diff: &str) -> Value {
    let mut files = Vec::new();
    let mut additions = 0usize;
    let mut deletions = 0usize;
    for line in diff.lines() {
        if line.starts_with("diff --git ") {
            if let Some(path) = parse_diff_path(line) {
                files.push(json!({ "path": path }));
            }
        } else if line.starts_with('+') && !line.starts_with("+++") {
            additions += 1;
        } else if line.starts_with('-') && !line.starts_with("---") {
            deletions += 1;
        }
    }
    json!({
        "files": files.len(),
        "additions": additions,
        "deletions": deletions,
        "file_paths": files
    })
}

async fn read_workspace_diff(
    workspace: &str,
    base: &str,
    path_filter: Option<&str>,
) -> Result<String> {
    let mut args = vec!["diff", "--no-color", base];
    if let Some(path) = path_filter {
        args.push("--");
        args.push(path);
    }
    let output = tokio::process::Command::new("git")
        .args(args)
        .current_dir(Path::new(workspace))
        .output()
        .await
        .map_err(|e| tool_message("review_diff", e))?;
    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        return tool_error("review_diff", format!("git diff failed: {stderr}"));
    }
    Ok(String::from_utf8_lossy(&output.stdout).into_owned())
}

fn parse_diff_lines(diff: &str) -> Vec<(String, usize, &str)> {
    let mut out = Vec::new();
    let mut current = String::new();
    for raw in diff.lines() {
        if raw.starts_with("diff --git ") {
            if let Some(path) = parse_diff_path(raw) {
                current = path;
            }
        }
        if (raw.starts_with('+') && !raw.starts_with("+++"))
            || (raw.starts_with('-') && !raw.starts_with("---"))
        {
            out.push((current.clone(), 0, &raw[1..]));
        }
    }
    out
}

fn parse_diff_path(line: &str) -> Option<String> {
    let rest = line.strip_prefix("diff --git ")?;
    let mut parts = rest.split_whitespace();
    let _a = parts.next()?;
    let b = parts.next()?;
    let path = b.strip_prefix("b/").unwrap_or(b);
    Some(path.to_string())
}

fn resolve_existing(workspace: &str, input: &str, tool: &str) -> Result<PathBuf> {
    let workspace = Path::new(workspace);
    let candidate = Path::new(input);
    let joined = if candidate.is_absolute() {
        candidate.to_path_buf()
    } else {
        workspace.join(candidate)
    };
    let resolved = joined.canonicalize().map_err(|e| tool_message(tool, e))?;
    if !resolved.starts_with(workspace) {
        return tool_error(tool, "path escapes the workspace boundary");
    }
    Ok(resolved)
}

fn path_for_output(path: &Path) -> String {
    path.to_string_lossy().replace('\\', "/")
}

fn tool_error<T>(tool: &str, message: impl std::fmt::Display) -> Result<T> {
    Err(tool_message(tool, message))
}

fn tool_message(tool: &str, message: impl std::fmt::Display) -> OdonError {
    OdonError::Tool {
        tool: tool.to_string(),
        message: message.to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn detects_unsafe_unwrap_todo_and_secret_patterns() {
        let cases = [
            ("unsafe { ptr::read(src) }", Some(("high", "safety"))),
            ("let x = option.unwrap()", Some(("medium", "robustness"))),
            ("todo!(\"later\")", Some(("medium", "correctness"))),
            ("println!(\"{secret}\")", Some(("critical", "secrets"))),
            ("password = \"hunter2\"", Some(("high", "secrets"))),
        ];
        for (text, expected) in cases {
            let finding = review_line("src/app.rs", 1, text)
                .or_else(|| security_review_line("src/app.rs", 1, text))
                .unwrap();
            assert_eq!(finding.severity, expected.unwrap().0);
            assert_eq!(finding.category, expected.unwrap().1);
        }
    }

    #[test]
    fn summarizes_additions_and_deletions() {
        let diff = "\
diff --git a/src/lib.rs b/src/lib.rs
--- a/src/lib.rs
+++ b/src/lib.rs
@@ -1 +1,2 @@
 fn main() {
+    hello();
 }
";
        let summary = summarize_diff(diff);
        assert_eq!(summary["files"], 1);
        assert_eq!(summary["additions"], 1);
        assert_eq!(summary["deletions"], 0);
    }
}
