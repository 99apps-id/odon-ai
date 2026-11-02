use crate::registry::{Tool, ToolContext};
use async_trait::async_trait;
use odon_core::error::{OdonError, Result};
use odon_security::RiskTier;
use serde_json::{json, Value};
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::time::Duration;
use tokio::process::Command;

const COMMAND_TIMEOUT: Duration = Duration::from_secs(300);
const OUTPUT_LIMIT: usize = 64 * 1024;

pub struct FormatCodeTool;

#[async_trait]
impl Tool for FormatCodeTool {
    fn name(&self) -> &str {
        "format_code"
    }
    fn description(&self) -> &str {
        "Format explicitly selected source files with the workspace's installed language formatter"
    }
    fn parameters_schema(&self) -> Value {
        json!({
            "type": "object",
            "required": ["paths"],
            "properties": {
                "paths": {
                    "type": "array", "minItems": 1, "maxItems": 100,
                    "items": { "type": "string" }
                }
            }
        })
    }
    fn risk_tier(&self) -> RiskTier {
        RiskTier::Tier2SystemMutating
    }

    async fn execute(&self, ctx: &ToolContext, args: Value) -> Result<Value> {
        let workspace = canonical_workspace(ctx, self.name())?;
        let requested = args
            .get("paths")
            .and_then(Value::as_array)
            .ok_or_else(|| tool_message(self.name(), "paths must be an array"))?;
        if requested.is_empty() || requested.len() > 100 {
            return tool_error(self.name(), "paths must contain between 1 and 100 files");
        }

        let mut groups = BTreeMap::<&str, Vec<PathBuf>>::new();
        for value in requested {
            let input = value
                .as_str()
                .ok_or_else(|| tool_message(self.name(), "every path must be a string"))?;
            let path = resolve_existing(&workspace, input, self.name())?;
            if !path.is_file() {
                return tool_error(self.name(), format!("{} is not a file", path.display()));
            }
            let language = formatter_kind(&path).ok_or_else(|| {
                tool_message(
                    self.name(),
                    format!("no supported formatter for {}", path.display()),
                )
            })?;
            groups.entry(language).or_default().push(path);
        }

        let mut runs = Vec::new();
        for (kind, paths) in groups {
            let (program, arguments) = formatter_command(kind, &workspace, &paths)?;
            let result = run_command(self.name(), &workspace, &program, &arguments).await?;
            if !result.success {
                return Err(command_failure(self.name(), &result));
            }
            runs.push(result);
        }
        Ok(json!({ "formatted": requested.len(), "runs": runs }))
    }
}

pub struct RunChecksTool;

#[async_trait]
impl Tool for RunChecksTool {
    fn name(&self) -> &str {
        "run_checks"
    }
    fn description(&self) -> &str {
        "Detect the project toolchain and run its test or lint command with timeout and captured output"
    }
    fn parameters_schema(&self) -> Value {
        json!({
            "type": "object",
            "required": ["kind"],
            "properties": {
                "kind": { "type": "string", "enum": ["test", "lint"] },
                "path": { "type": "string", "description": "Workspace-relative project directory" }
            }
        })
    }
    fn risk_tier(&self) -> RiskTier {
        RiskTier::Tier2SystemMutating
    }

    async fn execute(&self, ctx: &ToolContext, args: Value) -> Result<Value> {
        let workspace = canonical_workspace(ctx, self.name())?;
        let root = resolve_existing(
            &workspace,
            args.get("path").and_then(Value::as_str).unwrap_or("."),
            self.name(),
        )?;
        if !root.is_dir() {
            return tool_error(
                self.name(),
                format!("{} is not a directory", root.display()),
            );
        }
        let kind = args.get("kind").and_then(Value::as_str).unwrap_or("");
        if kind != "test" && kind != "lint" {
            return tool_error(self.name(), "kind must be 'test' or 'lint'");
        }
        let (project, program, arguments) = detect_check_command(&root, kind)?;
        let result = run_command(self.name(), &root, &program, &arguments).await?;
        if !result.success {
            return Err(command_failure(self.name(), &result));
        }
        Ok(json!({ "project": project, "kind": kind, "result": result }))
    }
}

pub struct TestFileTool;

#[async_trait]
impl Tool for TestFileTool {
    fn name(&self) -> &str {
        "test_file"
    }
    fn description(&self) -> &str {
        "Run the narrowest supported test command for one test file without shell interpolation"
    }
    fn parameters_schema(&self) -> Value {
        json!({
            "type": "object",
            "required": ["file"],
            "properties": {
                "file": { "type": "string", "minLength": 1 }
            }
        })
    }
    fn risk_tier(&self) -> RiskTier {
        RiskTier::Tier2SystemMutating
    }

    async fn execute(&self, ctx: &ToolContext, args: Value) -> Result<Value> {
        let workspace = canonical_workspace(ctx, self.name())?;
        let file = resolve_existing(
            &workspace,
            args.get("file").and_then(Value::as_str).unwrap_or(""),
            self.name(),
        )?;
        if !file.is_file() {
            return tool_error(self.name(), format!("{} is not a file", file.display()));
        }
        let root = find_project_root(&workspace, &file).ok_or_else(|| {
            tool_message(
                self.name(),
                "no supported project manifest found for test file",
            )
        })?;
        let (project, program, arguments) = test_file_command(&root, &file)?;
        let result = run_command(self.name(), &root, &program, &arguments).await?;
        if !result.success {
            return Err(command_failure(self.name(), &result));
        }
        Ok(json!({
            "project": project,
            "file": path_for_output(file.strip_prefix(&workspace).unwrap_or(&file)),
            "result": result
        }))
    }
}

#[derive(Debug, serde::Serialize)]
struct CommandResult {
    program: String,
    arguments: Vec<String>,
    exit_code: Option<i32>,
    success: bool,
    stdout: String,
    stderr: String,
    truncated: bool,
}

async fn run_command(
    tool: &str,
    directory: &Path,
    program: &str,
    args: &[String],
) -> Result<CommandResult> {
    let output = tokio::time::timeout(
        COMMAND_TIMEOUT,
        Command::new(program)
            .args(args)
            .current_dir(directory)
            .kill_on_drop(true)
            .output(),
    )
    .await
    .map_err(|_| {
        tool_message(
            tool,
            format!(
                "command timed out after {} seconds",
                COMMAND_TIMEOUT.as_secs()
            ),
        )
    })?
    .map_err(|error| tool_message(tool, format!("failed to start '{program}': {error}")))?;

    let (stdout, stdout_truncated) = bounded_output(&output.stdout);
    let (stderr, stderr_truncated) = bounded_output(&output.stderr);
    Ok(CommandResult {
        program: program.to_string(),
        arguments: args.to_vec(),
        exit_code: output.status.code(),
        success: output.status.success(),
        stdout,
        stderr,
        truncated: stdout_truncated || stderr_truncated,
    })
}

fn formatter_kind(path: &Path) -> Option<&'static str> {
    match path.extension()?.to_str()?.to_ascii_lowercase().as_str() {
        "rs" => Some("rust"),
        "go" => Some("go"),
        "py" | "pyi" => Some("python"),
        "js" | "jsx" | "ts" | "tsx" | "mjs" | "cjs" | "json" | "css" | "scss" | "md" | "yaml"
        | "yml" => Some("web"),
        _ => None,
    }
}

fn formatter_command(
    kind: &str,
    workspace: &Path,
    paths: &[PathBuf],
) -> Result<(String, Vec<String>)> {
    let relative = paths
        .iter()
        .map(|path| path_for_output(path.strip_prefix(workspace).unwrap_or(path)))
        .collect::<Vec<_>>();
    match kind {
        "rust" => {
            let mut args = vec!["--edition".into(), "2021".into()];
            args.extend(relative);
            Ok(("rustfmt".into(), args))
        }
        "go" => {
            let mut args = vec!["-w".into()];
            args.extend(relative);
            Ok(("gofmt".into(), args))
        }
        "python" => {
            let mut args = vec!["format".into()];
            args.extend(relative);
            Ok(("ruff".into(), args))
        }
        "web" => {
            let biome = local_binary(workspace, "biome");
            let prettier = local_binary(workspace, "prettier");
            if biome.is_file() {
                let mut args = vec!["format".into(), "--write".into()];
                args.extend(relative);
                Ok((path_for_output(&biome), args))
            } else if prettier.is_file() {
                let mut args = vec!["--write".into()];
                args.extend(relative);
                Ok((path_for_output(&prettier), args))
            } else {
                tool_error("format_code", "no local Biome or Prettier executable found")
            }
        }
        _ => tool_error(
            "format_code",
            format!("unsupported formatter kind '{kind}'"),
        ),
    }
}

fn detect_check_command(root: &Path, kind: &str) -> Result<(String, String, Vec<String>)> {
    if root.join("Cargo.toml").is_file() {
        let args = if kind == "test" {
            vec!["test".into()]
        } else {
            vec![
                "clippy".into(),
                "--all-targets".into(),
                "--".into(),
                "-D".into(),
                "warnings".into(),
            ]
        };
        return Ok(("rust".into(), "cargo".into(), args));
    }
    if root.join("package.json").is_file() {
        let package_manager = package_manager(root);
        let script = package_script(root, kind)?;
        return Ok(("node".into(), package_manager, vec!["run".into(), script]));
    }
    if root.join("pyproject.toml").is_file() {
        return if kind == "test" {
            Ok((
                "python".into(),
                "python".into(),
                vec!["-m".into(), "pytest".into()],
            ))
        } else {
            Ok((
                "python".into(),
                "ruff".into(),
                vec!["check".into(), ".".into()],
            ))
        };
    }
    if root.join("go.mod").is_file() {
        return if kind == "test" {
            Ok((
                "go".into(),
                "go".into(),
                vec!["test".into(), "./...".into()],
            ))
        } else {
            Ok(("go".into(), "go".into(), vec!["vet".into(), "./...".into()]))
        };
    }
    tool_error("run_checks", "no supported project manifest found")
}

fn test_file_command(root: &Path, file: &Path) -> Result<(String, String, Vec<String>)> {
    let relative = path_for_output(file.strip_prefix(root).unwrap_or(file));
    if root.join("Cargo.toml").is_file() {
        let stem = file
            .file_stem()
            .and_then(|value| value.to_str())
            .unwrap_or("");
        let is_integration = file
            .strip_prefix(root)
            .ok()
            .and_then(|path| path.components().next())
            .map(|part| part.as_os_str() == "tests")
            .unwrap_or(false);
        let args = if is_integration {
            vec!["test".into(), "--test".into(), stem.into()]
        } else {
            vec!["test".into(), stem.into()]
        };
        return Ok(("rust".into(), "cargo".into(), args));
    }
    if root.join("package.json").is_file() {
        let manager = package_manager(root);
        let script = package_script(root, "test")?;
        return Ok((
            "node".into(),
            manager,
            vec!["run".into(), script, "--".into(), relative],
        ));
    }
    if root.join("pyproject.toml").is_file() {
        return Ok((
            "python".into(),
            "python".into(),
            vec!["-m".into(), "pytest".into(), relative],
        ));
    }
    if root.join("go.mod").is_file() {
        let directory = file
            .parent()
            .unwrap_or(root)
            .strip_prefix(root)
            .unwrap_or(Path::new("."));
        return Ok((
            "go".into(),
            "go".into(),
            vec!["test".into(), format!("./{}", path_for_output(directory))],
        ));
    }
    tool_error("test_file", "no supported project manifest found")
}

fn package_manager(root: &Path) -> String {
    if root.join("pnpm-lock.yaml").is_file() {
        "pnpm".into()
    } else if root.join("yarn.lock").is_file() {
        "yarn".into()
    } else {
        "npm".into()
    }
}

fn package_script(root: &Path, kind: &str) -> Result<String> {
    let bytes = std::fs::read(root.join("package.json"))
        .map_err(|error| tool_message("run_checks", error))?;
    let package: Value =
        serde_json::from_slice(&bytes).map_err(|error| tool_message("run_checks", error))?;
    let scripts = package.get("scripts").and_then(Value::as_object);
    let candidates: &[&str] = if kind == "test" {
        &["test"]
    } else {
        &["lint", "check"]
    };
    candidates
        .iter()
        .find(|name| scripts.and_then(|items| items.get(**name)).is_some())
        .map(|name| (*name).to_string())
        .ok_or_else(|| tool_message("run_checks", format!("package.json has no {kind} script")))
}

fn find_project_root(workspace: &Path, file: &Path) -> Option<PathBuf> {
    let mut current = file.parent()?;
    loop {
        if ["Cargo.toml", "package.json", "pyproject.toml", "go.mod"]
            .iter()
            .any(|manifest| current.join(manifest).is_file())
        {
            return Some(current.to_path_buf());
        }
        if current == workspace {
            return None;
        }
        current = current.parent()?;
        if !current.starts_with(workspace) {
            return None;
        }
    }
}

fn local_binary(root: &Path, name: &str) -> PathBuf {
    let extension = if cfg!(windows) { ".cmd" } else { "" };
    root.join("node_modules")
        .join(".bin")
        .join(format!("{name}{extension}"))
}

fn canonical_workspace(ctx: &ToolContext, tool: &str) -> Result<PathBuf> {
    std::path::Path::new(&ctx.workspace_root)
        .canonicalize()
        .map_err(|error| tool_message(tool, error))
}

fn resolve_existing(workspace: &Path, input: &str, tool: &str) -> Result<PathBuf> {
    let candidate = Path::new(input);
    let joined = if candidate.is_absolute() {
        candidate.to_path_buf()
    } else {
        workspace.join(candidate)
    };
    let resolved = joined
        .canonicalize()
        .map_err(|error| tool_message(tool, error))?;
    if !resolved.starts_with(workspace) {
        return tool_error(tool, "path escapes the workspace boundary");
    }
    Ok(resolved)
}

fn bounded_output(bytes: &[u8]) -> (String, bool) {
    let truncated = bytes.len() > OUTPUT_LIMIT;
    let slice = if truncated {
        &bytes[bytes.len() - OUTPUT_LIMIT..]
    } else {
        bytes
    };
    (String::from_utf8_lossy(slice).into_owned(), truncated)
}

fn command_failure(tool: &str, result: &CommandResult) -> OdonError {
    OdonError::Tool {
        tool: tool.to_string(),
        message: format!(
            "{} exited with code {:?}\nstdout:\n{}\nstderr:\n{}",
            result.program, result.exit_code, result.stdout, result.stderr
        ),
    }
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
    fn output_is_bounded_from_the_tail() {
        let bytes = vec![b'x'; OUTPUT_LIMIT + 10];
        let (output, truncated) = bounded_output(&bytes);
        assert!(truncated);
        assert_eq!(output.len(), OUTPUT_LIMIT);
    }

    #[test]
    fn detects_supported_formatters() {
        assert_eq!(formatter_kind(Path::new("src/lib.rs")), Some("rust"));
        assert_eq!(formatter_kind(Path::new("src/app.tsx")), Some("web"));
        assert_eq!(formatter_kind(Path::new("asset.png")), None);
    }
}
