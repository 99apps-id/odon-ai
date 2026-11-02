use async_trait::async_trait;
use odon_core::{OdonError, Result};
use odon_security::RiskTier;
use serde_json::{json, Value};
use std::path::Path;

use crate::registry::{Tool, ToolContext};

fn project_error(tool: &str, message: String) -> OdonError {
    OdonError::Tool {
        tool: tool.to_string(),
        message,
    }
}

pub struct ProjectDetectTool;
#[async_trait]
impl Tool for ProjectDetectTool {
    fn name(&self) -> &str {
        "project_detect"
    }
    fn description(&self) -> &str {
        "Detect the workspace project type and key configuration files"
    }
    fn parameters_schema(&self) -> Value {
        json!({ "type": "object", "properties": {} })
    }
    fn risk_tier(&self) -> RiskTier {
        RiskTier::Tier0ReadOnly
    }
    async fn execute(&self, ctx: &ToolContext, _args: Value) -> Result<Value> {
        let root = Path::new(&ctx.workspace_root);
        let candidates = [
            ("rust", "Cargo.toml"),
            ("node", "package.json"),
            ("python", "pyproject.toml"),
            ("python", "requirements.txt"),
            ("go", "go.mod"),
            ("dotnet", "*.csproj"),
            ("docker", "docker-compose.yml"),
        ];
        let mut project_types = Vec::new();
        let mut config_files = Vec::new();
        for (kind, file) in candidates {
            let exists = if file.contains('*') {
                std::fs::read_dir(root)
                    .map(|entries| {
                        entries.filter_map(|entry| entry.ok()).any(|entry| {
                            entry
                                .path()
                                .extension()
                                .map(|ext| ext.eq_ignore_ascii_case("csproj"))
                                .unwrap_or(false)
                        })
                    })
                    .unwrap_or(false)
            } else {
                root.join(file).is_file()
            };
            if exists {
                if !project_types.contains(&kind.to_string()) {
                    project_types.push(kind.to_string());
                }
                config_files.push(file.to_string());
            }
        }
        Ok(json!({
            "workspace": ctx.workspace_root,
            "project_types": project_types,
            "config_files": config_files
        }))
    }
}

pub struct DependencyAuditTool;
#[async_trait]
impl Tool for DependencyAuditTool {
    fn name(&self) -> &str {
        "dependency_audit"
    }
    fn description(&self) -> &str {
        "Run a dependency audit using Cargo Audit or npm audit when the project is configured"
    }
    fn parameters_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "timeout_secs": { "type": "integer", "description": "Maximum audit duration (default 60, max 300)" }
            }
        })
    }
    fn risk_tier(&self) -> RiskTier {
        RiskTier::Tier0ReadOnly
    }
    fn requirements(&self) -> crate::registry::ToolRequirements {
        crate::registry::ToolRequirements {
            binaries: Vec::new(),
            any_binaries: vec!["cargo".into(), "npm".into()],
            environment: vec![],
        }
    }
    async fn execute(&self, ctx: &ToolContext, args: Value) -> Result<Value> {
        let timeout_secs = args
            .get("timeout_secs")
            .and_then(Value::as_u64)
            .unwrap_or(60)
            .clamp(1, 300);
        let root = Path::new(&ctx.workspace_root);
        if root.join("Cargo.lock").is_file() {
            let output = tokio::time::timeout(
                std::time::Duration::from_secs(timeout_secs),
                tokio::process::Command::new("cargo")
                    .arg("audit")
                    .arg("--json")
                    .current_dir(root)
                    .stdout(std::process::Stdio::piped())
                    .stderr(std::process::Stdio::piped())
                    .output(),
            )
            .await
            .map_err(|_| project_error(self.name(), "Cargo Audit timed out".to_string()))?
            .map_err(|e| {
                project_error(
                    self.name(),
                    format!("Could not run Cargo Audit; install cargo-audit first: {e}"),
                )
            })?;
            let stdout = String::from_utf8_lossy(&output.stdout).to_string();
            let stderr = String::from_utf8_lossy(&output.stderr).to_string();
            let parsed: Value = serde_json::from_str(&stdout).unwrap_or(Value::Null);
            return Ok(json!({
                "ecosystem": "cargo",
                "command": "cargo audit --json",
                "success": output.status.success(),
                "exit_code": output.status.code().unwrap_or(-1),
                "report": parsed,
                "stdout": stdout.chars().take(100_000).collect::<String>(),
                "stderr": stderr.chars().take(20_000).collect::<String>(),
                "timeout_secs": timeout_secs
            }));
        }
        if root.join("package-lock.json").is_file() {
            let output = tokio::time::timeout(
                std::time::Duration::from_secs(timeout_secs),
                tokio::process::Command::new("npm")
                    .arg("audit")
                    .arg("--json")
                    .current_dir(root)
                    .stdout(std::process::Stdio::piped())
                    .stderr(std::process::Stdio::piped())
                    .output(),
            )
            .await
            .map_err(|_| project_error(self.name(), "npm audit timed out".to_string()))?
            .map_err(|e| project_error(self.name(), format!("Could not run npm audit: {e}")))?;
            let stdout = String::from_utf8_lossy(&output.stdout).to_string();
            let stderr = String::from_utf8_lossy(&output.stderr).to_string();
            let parsed: Value = serde_json::from_str(&stdout).unwrap_or(Value::Null);
            return Ok(json!({
                "ecosystem": "npm",
                "command": "npm audit --json",
                "success": output.status.success(),
                "exit_code": output.status.code().unwrap_or(-1),
                "report": parsed,
                "stdout": stdout.chars().take(100_000).collect::<String>(),
                "stderr": stderr.chars().take(20_000).collect::<String>(),
                "timeout_secs": timeout_secs
            }));
        }
        Err(project_error(
            self.name(),
            "No Cargo.lock or package-lock.json found in the workspace".to_string(),
        ))
    }
}

pub struct ProjectScriptsTool;
#[async_trait]
impl Tool for ProjectScriptsTool {
    fn name(&self) -> &str {
        "project_scripts"
    }
    fn description(&self) -> &str {
        "List runnable project scripts from package.json, Cargo, and known Make targets"
    }
    fn parameters_schema(&self) -> Value {
        json!({ "type": "object", "properties": {} })
    }
    fn risk_tier(&self) -> RiskTier {
        RiskTier::Tier0ReadOnly
    }
    async fn execute(&self, ctx: &ToolContext, _args: Value) -> Result<Value> {
        let root = Path::new(&ctx.workspace_root);
        let mut scripts = Vec::new();

        if let Ok(content) = std::fs::read_to_string(root.join("package.json")) {
            if let Ok(value) = serde_json::from_str::<Value>(&content) {
                if let Some(map) = value.get("scripts").and_then(Value::as_object) {
                    for (name, value) in map {
                        scripts.push(json!({
                            "kind": "npm",
                            "name": name,
                            "command": format!("npm run {name}"),
                            "script": value
                        }));
                    }
                }
            }
        }
        if root.join("Cargo.toml").is_file() {
            for (name, command) in [
                ("build", "cargo build"),
                ("check", "cargo check"),
                ("test", "cargo test"),
                ("format", "cargo fmt --all -- --check"),
                (
                    "lint",
                    "cargo clippy --workspace --all-targets -- -D warnings",
                ),
            ] {
                scripts.push(json!({ "kind": "cargo", "name": name, "command": command }));
            }
        }
        if root.join("Makefile").is_file() {
            if let Ok(content) = std::fs::read_to_string(root.join("Makefile")) {
                for line in content.lines() {
                    let Some((target, rest)) = line.split_once(':') else {
                        continue;
                    };
                    if target.contains(char::is_whitespace)
                        || target.starts_with(['.', '#'])
                        || !rest.trim().is_empty()
                    {
                        continue;
                    }
                    scripts.push(json!({
                        "kind": "make",
                        "name": target.trim(),
                        "command": format!("make {}", target.trim())
                    }));
                }
            }
        }
        if scripts.is_empty() {
            return Err(project_error(
                self.name(),
                "No package.json, Cargo.toml, or Makefile scripts found".to_string(),
            ));
        }
        Ok(json!({ "scripts": scripts, "count": scripts.len() }))
    }
}
