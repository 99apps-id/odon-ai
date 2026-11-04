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

fn bounded_output(raw: &[u8]) -> (String, bool) {
    if raw.len() <= OUTPUT_LIMIT {
        (String::from_utf8_lossy(raw).to_string(), false)
    } else {
        (
            String::from_utf8_lossy(&raw[..OUTPUT_LIMIT]).to_string(),
            true,
        )
    }
}

fn timeout_or_default(args: &Value, default: Duration) -> Duration {
    args.get("timeout_secs")
        .and_then(|v| v.as_u64())
        .map(Duration::from_secs)
        .unwrap_or(default)
}

async fn run_command(
    program: &str,
    args: &[&str],
    cwd: &str,
    timeout: Duration,
) -> Result<(std::process::Output, String)> {
    let output = tokio::time::timeout(timeout, async {
        Command::new(program)
            .args(args)
            .current_dir(cwd)
            .output()
            .await
    })
    .await
    .map_err(|_| OdonError::Tool {
        tool: "cicd".to_string(),
        message: format!("Command timed out after {}s", timeout.as_secs()),
    })??;
    let (stdout, _) = bounded_output(&output.stdout);
    Ok((output, stdout))
}

pub struct PipelineTriggerTool;

#[async_trait]
impl Tool for PipelineTriggerTool {
    fn name(&self) -> &str {
        "pipeline_trigger"
    }
    fn description(&self) -> &str {
        "Trigger CI pipeline execution using detected workflow definition"
    }
    fn parameters_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "workflow": {
                    "type": "string",
                    "description": "Workflow file or identifier to trigger"
                },
                "command": {
                    "type": "string",
                    "description": "Explicit trigger command override"
                },
                "timeout_secs": {
                    "type": "integer",
                    "minimum": 1,
                    "maximum": 900,
                    "description": "Optional timeout in seconds"
                }
            }
        })
    }
    fn risk_tier(&self) -> RiskTier {
        RiskTier::Tier2SystemMutating
    }

    async fn execute(&self, ctx: &ToolContext, args: Value) -> Result<Value> {
        let command_override = args
            .get("command")
            .and_then(|v| v.as_str())
            .map(|s| s.to_string());
        let timeout = timeout_or_default(&args, COMMAND_TIMEOUT);
        let workspace_root = &ctx.workspace_root;
        let workflow = args
            .get("workflow")
            .and_then(|v| v.as_str())
            .map(|s| s.to_string());

        let command = if let Some(cmd) = command_override {
            cmd
        } else {
            let workflow_hint = workflow.as_deref().unwrap_or("");
            if Path::new(workspace_root).join(".github/workflows").exists() {
                if !workflow_hint.is_empty() {
                    format!("gh workflow run {workflow_hint}")
                } else {
                    "gh workflow run".to_string()
                }
            } else if Path::new(workspace_root).join("Jenkinsfile").exists() {
                "jenkins-cli build".to_string()
            } else if Path::new(workspace_root).join(".gitlab-ci.yml").exists() {
                "gitlab-runner exec".to_string()
            } else {
                "echo 'No CI workflow definition detected'".to_string()
            }
        };

        let mut parts = command.split_whitespace();
        let program = parts.next().unwrap_or(&command);
        let args_vec: Vec<&str> = parts.collect();
        let (output, stdout) = run_command(program, &args_vec, workspace_root, timeout).await?;
        let (stderr, stderr_truncated) = bounded_output(&output.stderr);

        Ok(json!({
            "command": command,
            "success": output.status.success(),
            "exit_code": output.status.code(),
            "stdout": stdout,
            "stderr": stderr,
            "stderr_truncated": stderr_truncated
        }))
    }
}

pub struct DeployAutomationTool;

#[async_trait]
impl Tool for DeployAutomationTool {
    fn name(&self) -> &str {
        "deploy_automation"
    }
    fn description(&self) -> &str {
        "Run deployment automation scripts with bounded output and timeout"
    }
    fn parameters_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "command": {
                    "type": "string",
                    "description": "Deployment command to run"
                },
                "timeout_secs": {
                    "type": "integer",
                    "minimum": 1,
                    "maximum": 900,
                    "description": "Optional timeout in seconds"
                }
            },
            "required": ["command"]
        })
    }
    fn risk_tier(&self) -> RiskTier {
        RiskTier::Tier2SystemMutating
    }

    async fn execute(&self, ctx: &ToolContext, args: Value) -> Result<Value> {
        let command = args
            .get("command")
            .and_then(|v| v.as_str())
            .ok_or_else(|| OdonError::Tool {
                tool: self.name().to_string(),
                message: "command is required".to_string(),
            })?;
        let timeout = timeout_or_default(&args, COMMAND_TIMEOUT);
        let workspace_root = &ctx.workspace_root;

        let mut parts = command.split_whitespace();
        let program = parts.next().unwrap_or(command);
        let args_vec: Vec<&str> = parts.collect();
        let (output, stdout) = run_command(program, &args_vec, workspace_root, timeout).await?;
        let (stderr, stderr_truncated) = bounded_output(&output.stderr);

        Ok(json!({
            "command": command,
            "success": output.status.success(),
            "exit_code": output.status.code(),
            "stdout": stdout,
            "stderr": stderr,
            "stderr_truncated": stderr_truncated
        }))
    }
}

pub struct ArtifactManagementTool;

#[async_trait]
impl Tool for ArtifactManagementTool {
    fn name(&self) -> &str {
        "artifact_management"
    }
    fn description(&self) -> &str {
        "List or package build artifacts from the workspace"
    }
    fn parameters_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "action": {
                    "type": "string",
                    "enum": ["list", "package"],
                    "description": "Artifact management action"
                },
                "source": {
                    "type": "string",
                    "description": "Artifact source path"
                },
                "destination": {
                    "type": "string",
                    "description": "Destination path for packaged artifact"
                }
            },
            "required": ["action"]
        })
    }
    fn risk_tier(&self) -> RiskTier {
        RiskTier::Tier2SystemMutating
    }

    async fn execute(&self, ctx: &ToolContext, args: Value) -> Result<Value> {
        let action =
            args.get("action")
                .and_then(|v| v.as_str())
                .ok_or_else(|| OdonError::Tool {
                    tool: self.name().to_string(),
                    message: "action is required".to_string(),
                })?;
        let source = args
            .get("source")
            .and_then(|v| v.as_str())
            .unwrap_or("./dist");
        let destination = args.get("destination").and_then(|v| v.as_str());

        match action {
            "list" => {
                let source_path = Path::new(&ctx.workspace_root).join(source);
                if !source_path.exists() {
                    return Ok(json!({"action": "list", "source": source, "artifacts": []}));
                }
                let mut entries = Vec::new();
                for entry in std::fs::read_dir(source_path).map_err(|e| OdonError::Tool {
                    tool: self.name().to_string(),
                    message: format!("Failed to read artifact directory: {e}"),
                })? {
                    let entry = entry.map_err(|e| OdonError::Tool {
                        tool: self.name().to_string(),
                        message: format!("Failed to read artifact entry: {e}"),
                    })?;
                    entries.push(json!({
                        "name": entry.file_name().to_string_lossy().to_string(),
                        "path": entry.path().to_string_lossy().to_string(),
                        "size": entry.metadata().map(|m| m.len()).unwrap_or(0)
                    }));
                }
                Ok(json!({"action": "list", "source": source, "artifacts": entries}))
            }
            "package" => {
                let source_path = Path::new(&ctx.workspace_root).join(source);
                let destination_path = destination
                    .map(|d| {
                        let p = Path::new(d);
                        if p.is_absolute() {
                            p.to_path_buf()
                        } else {
                            Path::new(&ctx.workspace_root).join(p)
                        }
                    })
                    .unwrap_or_else(|| Path::new(&ctx.workspace_root).join("artifacts.zip"));
                std::fs::create_dir_all(destination_path.parent().unwrap_or(Path::new(".")))
                    .map_err(|e| OdonError::Tool {
                        tool: self.name().to_string(),
                        message: format!("Failed to prepare destination: {e}"),
                    })?;
                zip_artifact(&source_path, &destination_path)?;
                Ok(
                    json!({"action": "package", "source": source, "destination": destination_path.to_string_lossy().to_string()}),
                )
            }
            other => Err(OdonError::Tool {
                tool: self.name().to_string(),
                message: format!("Unsupported action: {other}"),
            }),
        }
    }
}

fn zip_artifact(source: &Path, destination: &Path) -> Result<()> {
    use std::io::Write;
    use zip::write::FileOptions;
    use zip::CompressionMethod::Deflated;

    let file = std::fs::File::create(destination).map_err(|e| OdonError::Tool {
        tool: "artifact_management".to_string(),
        message: format!("Failed to create zip file: {e}"),
    })?;
    let mut zip = zip::ZipWriter::new(file);
    let options: zip::write::SimpleFileOptions = FileOptions::default().compression_method(Deflated);

    if source.is_file() {
        zip.start_file(
            source
                .file_name()
                .and_then(|n| n.to_str())
                .unwrap_or("artifact"),
            options,
        )
        .map_err(|e| OdonError::Tool {
            tool: "artifact_management".to_string(),
            message: format!("Failed to add file to zip: {e}"),
        })?;
        let data = std::fs::read(source).map_err(|e| OdonError::Tool {
            tool: "artifact_management".to_string(),
            message: format!("Failed to read artifact file: {e}"),
        })?;
        zip.write_all(&data).map_err(|e| OdonError::Tool {
            tool: "artifact_management".to_string(),
            message: format!("Failed to write zip entry: {e}"),
        })?;
    } else if source.is_dir() {
        for entry in walkdir::WalkDir::new(source) {
            let entry = entry.map_err(|e| OdonError::Tool {
                tool: "artifact_management".to_string(),
                message: format!("Failed to walk artifact directory: {e}"),
            })?;
            let path = entry.path();
            let name = path.strip_prefix(source).map_err(|e| OdonError::Tool {
                tool: "artifact_management".to_string(),
                message: format!("Failed to compute zip entry name: {e}"),
            })?;
            if path.is_file() {
                zip.start_file(name.to_string_lossy().to_string(), options)
                    .map_err(|e| OdonError::Tool {
                        tool: "artifact_management".to_string(),
                        message: format!("Failed to add file to zip: {e}"),
                    })?;
                let data = std::fs::read(path).map_err(|e| OdonError::Tool {
                    tool: "artifact_management".to_string(),
                    message: format!("Failed to read artifact file: {e}"),
                })?;
                zip.write_all(&data).map_err(|e| OdonError::Tool {
                    tool: "artifact_management".to_string(),
                    message: format!("Failed to write zip entry: {e}"),
                })?;
            }
        }
    }

    zip.finish().map_err(|e| OdonError::Tool {
        tool: "artifact_management".to_string(),
        message: format!("Failed to finalize zip archive: {e}"),
    })?;
    Ok(())
}
