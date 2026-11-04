use crate::registry::{Tool, ToolContext};
use async_trait::async_trait;
use odon_core::error::{OdonError, Result};
use odon_security::RiskTier;
use serde_json::{json, Value};
use std::collections::BTreeMap;
use std::path::Path;
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

fn detect_test_runner(workspace_root: &str) -> Option<&'static str> {
    let root = Path::new(workspace_root);
    if root.join("package.json").exists() {
        let content = std::fs::read_to_string(root.join("package.json")).ok()?;
        if content.contains("\"jest\"")
            || content.contains("\"vitest\"")
            || content.contains("\"mocha\"")
        {
            return Some("node");
        }
        if content.contains("\"playwright\"") {
            return Some("playwright");
        }
    }
    if root.join("Cargo.toml").exists() {
        return Some("rust");
    }
    if root.join("pyproject.toml").exists() || root.join("requirements.txt").exists() {
        return Some("python");
    }
    if root.join("go.mod").exists() {
        return Some("go");
    }
    None
}

pub struct TestRunTool;

#[async_trait]
impl Tool for TestRunTool {
    fn name(&self) -> &str {
        "test_run"
    }
    fn description(&self) -> &str {
        "Run project test suite with auto-detected runner and return structured results"
    }
    fn parameters_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "runner": {
                    "type": "string",
                    "enum": ["auto", "node", "rust", "python", "go", "playwright"],
                    "description": "Test runner to use"
                },
                "command": {
                    "type": "string",
                    "description": "Explicit test command override"
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
        let runner = args
            .get("runner")
            .and_then(|v| v.as_str())
            .unwrap_or("auto");
        let command_override = args
            .get("command")
            .and_then(|v| v.as_str())
            .map(|s| s.to_string());
        let timeout = args
            .get("timeout_secs")
            .and_then(|v| v.as_u64())
            .map(Duration::from_secs)
            .unwrap_or(COMMAND_TIMEOUT);

        let workspace_root = &ctx.workspace_root;
        let detected = detect_test_runner(workspace_root);
        let runner = match runner {
            "auto" => detected.unwrap_or("node"),
            other => other,
        };

        let command = if let Some(ref cmd) = command_override {
            cmd.clone()
        } else {
            match runner {
                "node" => "npm test --silent".to_string(),
                "rust" => "cargo test".to_string(),
                "python" => "python -m pytest -q".to_string(),
                "go" => "go test ./...".to_string(),
                "playwright" => "npx playwright test".to_string(),
                _ => {
                    return Err(OdonError::Tool {
                        tool: self.name().to_string(),
                        message: format!("Unsupported test runner: {runner}"),
                    })
                }
            }
        };

        let mut parts = command.split_whitespace();
        let program = parts.next().unwrap_or(&command);
        let args_vec: Vec<&str> = parts.collect();

        let output = tokio::time::timeout(timeout, async {
            Command::new(program)
                .args(&args_vec)
                .current_dir(workspace_root)
                .output()
                .await
        })
        .await
        .map_err(|_| OdonError::Tool {
            tool: self.name().to_string(),
            message: format!("Test command timed out after {}s", timeout.as_secs()),
        })??;

        let (stdout, stdout_truncated) = bounded_output(&output.stdout);
        let (stderr, stderr_truncated) = bounded_output(&output.stderr);

        let exit_code = output.status.code();
        let success = output.status.success();

        Ok(json!({
            "runner": runner,
            "command": command,
            "success": success,
            "exit_code": exit_code,
            "stdout": stdout,
            "stderr": stderr,
            "stdout_truncated": stdout_truncated,
            "stderr_truncated": stderr_truncated
        }))
    }
}

pub struct CoverageRunTool;

#[async_trait]
impl Tool for CoverageRunTool {
    fn name(&self) -> &str {
        "coverage_run"
    }
    fn description(&self) -> &str {
        "Run project coverage and return parsed metrics"
    }
    fn parameters_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "runner": {
                    "type": "string",
                    "enum": ["auto", "node", "rust", "python", "go"],
                    "description": "Coverage runner to use"
                },
                "command": {
                    "type": "string",
                    "description": "Explicit coverage command override"
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
        let runner = args
            .get("runner")
            .and_then(|v| v.as_str())
            .unwrap_or("auto");
        let command_override = args
            .get("command")
            .and_then(|v| v.as_str())
            .map(|s| s.to_string());
        let timeout = args
            .get("timeout_secs")
            .and_then(|v| v.as_u64())
            .map(Duration::from_secs)
            .unwrap_or(COMMAND_TIMEOUT);

        let workspace_root = &ctx.workspace_root;
        let detected = detect_test_runner(workspace_root);
        let runner = match runner {
            "auto" => detected.unwrap_or("node"),
            other => other,
        };

        let command = if let Some(ref cmd) = command_override {
            cmd.clone()
        } else {
            match runner {
                "node" => {
                    "npx jest --coverage --coverageReporters=json-summary --silent".to_string()
                }
                "rust" => "cargo tarpaulin --quiet --out=Json".to_string(),
                "python" => "python -m pytest --cov --cov-report=json -q".to_string(),
                "go" => "go test -coverprofile=coverage.out ./...".to_string(),
                _ => {
                    return Err(OdonError::Tool {
                        tool: self.name().to_string(),
                        message: format!("Unsupported coverage runner: {runner}"),
                    })
                }
            }
        };

        let mut parts = command.split_whitespace();
        let program = parts.next().unwrap_or(&command);
        let args_vec: Vec<&str> = parts.collect();

        let output = tokio::time::timeout(timeout, async {
            Command::new(program)
                .args(&args_vec)
                .current_dir(workspace_root)
                .output()
                .await
        })
        .await
        .map_err(|_| OdonError::Tool {
            tool: self.name().to_string(),
            message: format!("Coverage command timed out after {}s", timeout.as_secs()),
        })??;

        let (stdout, stdout_truncated) = bounded_output(&output.stdout);
        let (stderr, stderr_truncated) = bounded_output(&output.stderr);

        let mut metrics = BTreeMap::<&str, Value>::new();
        metrics.insert("runner", Value::String(runner.to_string()));
        metrics.insert("command", Value::String(command));
        metrics.insert("success", Value::Bool(output.status.success()));
        metrics.insert("exit_code", Value::from(output.status.code().unwrap_or(-1)));
        metrics.insert("stdout", Value::String(stdout));
        metrics.insert("stderr", Value::String(stderr));
        metrics.insert("stdout_truncated", Value::Bool(stdout_truncated));
        metrics.insert("stderr_truncated", Value::Bool(stderr_truncated));

        if runner == "python" {
            if let Ok(raw) =
                std::fs::read_to_string(Path::new(workspace_root).join("coverage.json"))
            {
                if let Ok(parsed) = serde_json::from_str::<Value>(&raw) {
                    metrics.insert("coverage_report", parsed);
                }
            }
        }

        if runner == "go" {
            metrics.insert(
                "note",
                Value::String(
                    "go coverage report generated; parse coverage.out for details".to_string(),
                ),
            );
        }

        Ok(Value::Object(
            metrics
                .into_iter()
                .map(|(k, v)| (k.to_string(), v))
                .collect(),
        ))
    }
}

pub struct CoverageThresholdCheckTool;

#[async_trait]
impl Tool for CoverageThresholdCheckTool {
    fn name(&self) -> &str {
        "coverage_threshold_check"
    }
    fn description(&self) -> &str {
        "Check coverage report against thresholds and return pass/fail summary"
    }
    fn parameters_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "report_path": {
                    "type": "string",
                    "description": "Path to coverage JSON report"
                },
                "threshold": {
                    "type": "number",
                    "minimum": 0.0,
                    "maximum": 100.0,
                    "description": "Minimum acceptable coverage percentage"
                }
            },
            "required": ["report_path"]
        })
    }
    fn risk_tier(&self) -> RiskTier {
        RiskTier::Tier0ReadOnly
    }

    async fn execute(&self, ctx: &ToolContext, args: Value) -> Result<Value> {
        let report_path = args
            .get("report_path")
            .and_then(|v| v.as_str())
            .ok_or_else(|| OdonError::Tool {
                tool: self.name().to_string(),
                message: "report_path is required".to_string(),
            })?;
        let threshold = args
            .get("threshold")
            .and_then(|v| v.as_f64())
            .unwrap_or(80.0);

        // Tier0 must never reach outside the workspace: this tool runs without
        // approval and stays usable in plan mode, so an absolute path would let a
        // model read arbitrary files such as credentials or private keys.
        let resolved = crate::workspace_path::resolve_existing(&ctx.workspace_root, report_path)
            .map_err(|error| OdonError::Tool {
                tool: self.name().to_string(),
                message: format!("coverage report is outside the workspace: {error}"),
            })?;

        let raw = std::fs::read_to_string(&resolved).map_err(|e| OdonError::Tool {
            tool: self.name().to_string(),
            message: format!("Failed to read coverage report: {e}"),
        })?;
        let parsed: Value = serde_json::from_str(&raw).map_err(|e| OdonError::Tool {
            tool: self.name().to_string(),
            message: format!("Invalid coverage report JSON: {e}"),
        })?;

        let mut summary = BTreeMap::<&str, Value>::new();
        summary.insert("threshold", Value::from(threshold));
        summary.insert(
            "report_path",
            Value::String(resolved.to_string_lossy().to_string()),
        );

        let total = parsed
            .get("totals")
            .and_then(|v| v.get("lines"))
            .and_then(|v| v.get("pct"))
            .and_then(|v| v.as_f64());
        let coverage_pct: Option<f64> = total;

        if let Some(total) = coverage_pct {
            summary.insert("coverage_pct", Value::from(total));
            summary.insert("passed", Value::Bool(total >= threshold));
        } else {
            summary.insert("coverage_pct", Value::Null);
            summary.insert("passed", Value::Bool(false));
            summary.insert(
                "note",
                Value::String("Could not parse coverage percentage".to_string()),
            );
        }

        Ok(Value::Object(
            summary
                .into_iter()
                .map(|(k, v)| (k.to_string(), v))
                .collect(),
        ))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::registry::ToolContext;
    use odon_security::{ApprovalEngine, AuditLogger, SecurityPolicy};
    use std::sync::Arc;

    fn context(workspace: &Path) -> ToolContext {
        ToolContext {
            session_id: "s1".to_string(),
            workspace_root: workspace.to_string_lossy().to_string(),
            approval_engine: Arc::new(ApprovalEngine::new(SecurityPolicy::default())),
            audit_logger: Arc::new(AuditLogger::new()),
            storage: None,
        }
    }

    #[tokio::test]
    async fn coverage_threshold_reads_a_report_inside_the_workspace() {
        let dir = std::env::temp_dir().join(format!("odon-cov-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(
            dir.join("coverage.json"),
            r#"{"totals":{"lines":{"pct":91.5}}}"#,
        )
        .unwrap();

        let ctx = context(&dir);
        let result = CoverageThresholdCheckTool
            .execute(&ctx, json!({ "report_path": "coverage.json", "threshold": 80.0 }))
            .await
            .expect("report inside the workspace must be readable");

        assert_eq!(result["coverage_pct"], 91.5);
        assert_eq!(result["passed"], true);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[tokio::test]
    async fn coverage_threshold_rejects_a_report_outside_the_workspace() {
        // This tool is Tier0: it runs without approval and stays usable in plan
        // mode. An absolute path used to reach anything on the machine, so this
        // asserts the sandbox holds.
        let workspace = std::env::temp_dir().join(format!("odon-cov-ws-{}", std::process::id()));
        std::fs::create_dir_all(&workspace).unwrap();
        let outside = std::env::temp_dir().join(format!("odon-cov-outside-{}.json", std::process::id()));
        std::fs::write(&outside, r#"{"totals":{"lines":{"pct":99.0}}}"#).unwrap();

        let ctx = context(&workspace);
        let result = CoverageThresholdCheckTool
            .execute(
                &ctx,
                json!({ "report_path": outside.to_string_lossy() }),
            )
            .await;

        assert!(
            result.is_err(),
            "a Tier0 tool must not read files outside the workspace"
        );
        let _ = std::fs::remove_dir_all(&workspace);
        let _ = std::fs::remove_file(&outside);
    }
}
