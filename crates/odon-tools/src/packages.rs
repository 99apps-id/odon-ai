use crate::registry::{Tool, ToolContext};
use async_trait::async_trait;
use odon_core::error::{OdonError, Result};
use odon_security::RiskTier;
use serde_json::{json, Value};
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
        tool: "packages".to_string(),
        message: format!("Command timed out after {}s", timeout.as_secs()),
    })??;
    let (stdout, _) = bounded_output(&output.stdout);
    Ok((output, stdout))
}

fn detect_package_manager(workspace_root: &str) -> Option<&'static str> {
    let root = Path::new(workspace_root);
    if root.join("package.json").exists() {
        if root.join("pnpm-lock.yaml").exists() {
            return Some("pnpm");
        }
        if root.join("yarn.lock").exists() {
            return Some("yarn");
        }
        if root.join("package-lock.json").exists() {
            return Some("npm");
        }
        return Some("npm");
    }
    if root.join("Cargo.toml").exists() {
        return Some("cargo");
    }
    if root.join("pyproject.toml").exists() {
        if root.join("poetry.lock").exists() {
            return Some("poetry");
        }
        if root.join("pdm.lock").exists() {
            return Some("pdm");
        }
        return Some("pip");
    }
    if root.join("requirements.txt").exists() {
        return Some("pip");
    }
    if root.join("go.mod").exists() {
        return Some("go");
    }
    None
}

pub struct SmartInstallTool;

#[async_trait]
impl Tool for SmartInstallTool {
    fn name(&self) -> &str {
        "smart_install"
    }
    fn description(&self) -> &str {
        "Install project dependencies using detected package manager"
    }
    fn parameters_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "manager": {
                    "type": "string",
                    "enum": ["auto", "npm", "pnpm", "yarn", "cargo", "pip", "poetry", "pdm", "go"],
                    "description": "Package manager to use"
                },
                "command": {
                    "type": "string",
                    "description": "Explicit install command override"
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
        let manager = args
            .get("manager")
            .and_then(|v| v.as_str())
            .unwrap_or("auto");
        let command_override = args
            .get("command")
            .and_then(|v| v.as_str())
            .map(|s| s.to_string());
        let timeout = timeout_or_default(&args, COMMAND_TIMEOUT);
        let workspace_root = &ctx.workspace_root;
        let detected = detect_package_manager(workspace_root);
        let manager = match manager {
            "auto" => detected.unwrap_or("npm"),
            other => other,
        };

        let command = if let Some(ref cmd) = command_override {
            cmd.clone()
        } else {
            match manager {
                "npm" => "npm install --silent".to_string(),
                "pnpm" => "pnpm install --silent".to_string(),
                "yarn" => "yarn install --silent".to_string(),
                "cargo" => "cargo fetch".to_string(),
                "pip" => "pip install -r requirements.txt".to_string(),
                "poetry" => "poetry install --no-interaction".to_string(),
                "pdm" => "pdm install".to_string(),
                "go" => "go mod download".to_string(),
                _ => {
                    return Err(OdonError::Tool {
                        tool: self.name().to_string(),
                        message: format!("Unsupported package manager: {manager}"),
                    })
                }
            }
        };

        let mut parts = command.split_whitespace();
        let program = parts.next().unwrap_or(&command);
        let args_vec: Vec<&str> = parts.collect();
        let (output, stdout) = run_command(program, &args_vec, workspace_root, timeout).await?;
        let (stderr, stderr_truncated) = bounded_output(&output.stderr);

        Ok(json!({
            "manager": manager,
            "command": command,
            "success": output.status.success(),
            "exit_code": output.status.code(),
            "stdout": stdout,
            "stderr": stderr,
            "stderr_truncated": stderr_truncated
        }))
    }
}

pub struct SecurityPatchPrTool;

#[async_trait]
impl Tool for SecurityPatchPrTool {
    fn name(&self) -> &str {
        "security_patch_pr"
    }
    fn description(&self) -> &str {
        "Create a draft PR for dependency security patches based on audit results"
    }
    fn parameters_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "audit_tool": {
                    "type": "string",
                    "enum": ["npm", "cargo", "pip"],
                    "description": "Audit source to use"
                },
                "title": {
                    "type": "string",
                    "description": "PR title override"
                },
                "command": {
                    "type": "string",
                    "description": "Explicit command override"
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
        RiskTier::Tier3NetworkAction
    }

    async fn execute(&self, ctx: &ToolContext, args: Value) -> Result<Value> {
        let audit_tool = args
            .get("audit_tool")
            .and_then(|v| v.as_str())
            .unwrap_or("npm");
        let title = args
            .get("title")
            .and_then(|v| v.as_str())
            .map(|s| s.to_string());
        let command_override = args
            .get("command")
            .and_then(|v| v.as_str())
            .map(|s| s.to_string());
        let timeout = timeout_or_default(&args, COMMAND_TIMEOUT);
        let workspace_root = &ctx.workspace_root;

        let title = title.unwrap_or_else(|| format!("security: patch {audit_tool} dependencies"));
        let command = if let Some(ref cmd) = command_override {
            cmd.clone()
        } else {
            match audit_tool {
                "npm" => format!(
                    "npm audit fix && git checkout -b security/patch-{audit_tool}-deps || true"
                ),
                "cargo" => format!(
                    "cargo audit fix && git checkout -b security/patch-{audit_tool}-deps || true"
                ),
                "pip" => format!(
                    "pip-audit --fix && git checkout -b security/patch-{audit_tool}-deps || true"
                ),
                _ => {
                    return Err(OdonError::Tool {
                        tool: self.name().to_string(),
                        message: format!("Unsupported audit tool: {audit_tool}"),
                    })
                }
            }
        };

        let mut parts = command.split_whitespace();
        let program = parts.next().unwrap_or(&command);
        let args_vec: Vec<&str> = parts.collect();
        let (output, stdout) = run_command(program, &args_vec, workspace_root, timeout).await?;
        let (stderr, stderr_truncated) = bounded_output(&output.stderr);

        Ok(json!({
            "audit_tool": audit_tool,
            "title": title,
            "command": command,
            "success": output.status.success(),
            "exit_code": output.status.code(),
            "stdout": stdout,
            "stderr": stderr,
            "stderr_truncated": stderr_truncated
        }))
    }
}

pub struct LicenseScanTool;

#[async_trait]
impl Tool for LicenseScanTool {
    fn name(&self) -> &str {
        "license_scan"
    }
    fn description(&self) -> &str {
        "Scan project dependencies for license metadata and unknown licenses"
    }
    fn parameters_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "manager": {
                    "type": "string",
                    "enum": ["auto", "npm", "cargo", "pip"],
                    "description": "Package manager to scan"
                },
                "command": {
                    "type": "string",
                    "description": "Explicit command override"
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
        let manager = args
            .get("manager")
            .and_then(|v| v.as_str())
            .unwrap_or("auto");
        let command_override = args
            .get("command")
            .and_then(|v| v.as_str())
            .map(|s| s.to_string());
        let timeout = timeout_or_default(&args, COMMAND_TIMEOUT);
        let workspace_root = &ctx.workspace_root;
        let detected = detect_package_manager(workspace_root);
        let manager = match manager {
            "auto" => detected.unwrap_or("npm"),
            other => other,
        };

        let command = if let Some(ref cmd) = command_override {
            cmd.clone()
        } else {
            match manager {
                "npm" => "npx license-checker --json".to_string(),
                "cargo" => "cargo deny --format json".to_string(),
                "pip" => "pip-licenses --format json".to_string(),
                _ => {
                    return Err(OdonError::Tool {
                        tool: self.name().to_string(),
                        message: format!("Unsupported manager for license scan: {manager}"),
                    })
                }
            }
        };

        let mut parts = command.split_whitespace();
        let program = parts.next().unwrap_or(&command);
        let args_vec: Vec<&str> = parts.collect();
        let (output, stdout) = run_command(program, &args_vec, workspace_root, timeout).await?;
        let (stderr, stderr_truncated) = bounded_output(&output.stderr);

        Ok(json!({
            "manager": manager,
            "command": command,
            "success": output.status.success(),
            "exit_code": output.status.code(),
            "stdout": stdout,
            "stderr": stderr,
            "stderr_truncated": stderr_truncated
        }))
    }
}
