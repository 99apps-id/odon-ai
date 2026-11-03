use async_trait::async_trait;
use odon_core::{OdonError, Result};
use odon_security::RiskTier;
use serde_json::{json, Value};

use crate::registry::{Tool, ToolContext};

pub struct SystemInfoTool;

#[async_trait]
impl Tool for SystemInfoTool {
    fn name(&self) -> &str {
        "system_info"
    }
    fn description(&self) -> &str {
        "Retrieve host system environment details, OS, architecture, working directory, and resource indicators"
    }
    fn parameters_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {}
        })
    }
    fn risk_tier(&self) -> RiskTier {
        RiskTier::Tier0ReadOnly
    }
    async fn execute(&self, ctx: &ToolContext, _args: Value) -> Result<Value> {
        let os = std::env::consts::OS;
        let arch = std::env::consts::ARCH;
        let family = std::env::consts::FAMILY;
        let current_dir = std::env::current_dir()
            .map(|p| p.to_string_lossy().to_string())
            .unwrap_or_else(|_| "unknown".to_string());
        let cpus = num_cpus();

        Ok(json!({
            "os": os,
            "architecture": arch,
            "family": family,
            "cpu_cores": cpus,
            "current_dir": current_dir,
            "workspace_root": ctx.workspace_root,
            "session_id": ctx.session_id,
        }))
    }
}

pub struct CalcEvalTool;

#[async_trait]
impl Tool for CalcEvalTool {
    fn name(&self) -> &str {
        "calc_eval"
    }
    fn description(&self) -> &str {
        "Evaluate a mathematical or logical expression safely"
    }
    fn parameters_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "expression": { "type": "string", "description": "Mathematical expression (e.g. '128 * 1024', '(50 + 20) / 2')" }
            },
            "required": ["expression"]
        })
    }
    fn risk_tier(&self) -> RiskTier {
        RiskTier::Tier0ReadOnly
    }
    async fn execute(&self, _ctx: &ToolContext, args: Value) -> Result<Value> {
        let expr = args
            .get("expression")
            .and_then(|e| e.as_str())
            .ok_or_else(|| OdonError::Tool {
                tool: self.name().to_string(),
                message: "Missing 'expression' parameter".to_string(),
            })?;

        let res = eval_simple_math(expr).map_err(|e| OdonError::Tool {
            tool: self.name().to_string(),
            message: format!("Evaluation error: {}", e),
        })?;

        Ok(json!({
            "expression": expr,
            "result": res
        }))
    }
}

fn num_cpus() -> usize {
    std::thread::available_parallelism()
        .map(|n| n.get())
        .unwrap_or(1)
}

fn eval_simple_math(expr: &str) -> std::result::Result<f64, String> {
    let clean: String = expr.chars().filter(|c| !c.is_whitespace()).collect();
    if clean.is_empty() {
        return Err("Empty expression".to_string());
    }

    // Evaluate basic arithmetic +, -, *, /, ^
    // Simple recursive descent parser
    let mut chars = clean.chars().peekable();
    let result = parse_addition(&mut chars)?;
    if chars.peek().is_some() {
        return Err(format!(
            "Unexpected trailing input near '{}'",
            chars.collect::<String>()
        ));
    }
    if !result.is_finite() {
        return Err("Expression did not produce a finite number".to_string());
    }
    Ok(result)
}

fn parse_addition(
    chars: &mut std::iter::Peekable<std::str::Chars>,
) -> std::result::Result<f64, String> {
    let mut left = parse_multiplication(chars)?;
    while let Some(&op) = chars.peek() {
        if op == '+' {
            chars.next();
            let right = parse_multiplication(chars)?;
            left += right;
        } else if op == '-' {
            chars.next();
            let right = parse_multiplication(chars)?;
            left -= right;
        } else {
            break;
        }
    }
    Ok(left)
}

fn parse_multiplication(
    chars: &mut std::iter::Peekable<std::str::Chars>,
) -> std::result::Result<f64, String> {
    let mut left = parse_power(chars)?;
    while let Some(&op) = chars.peek() {
        if op == '*' {
            chars.next();
            let right = parse_power(chars)?;
            left *= right;
        } else if op == '/' {
            chars.next();
            let right = parse_power(chars)?;
            if right == 0.0 {
                return Err("Division by zero".to_string());
            }
            left /= right;
        } else if op == '%' {
            chars.next();
            let right = parse_power(chars)?;
            if right == 0.0 {
                return Err("Modulo by zero".to_string());
            }
            left %= right;
        } else {
            break;
        }
    }
    Ok(left)
}

fn parse_power(
    chars: &mut std::iter::Peekable<std::str::Chars>,
) -> std::result::Result<f64, String> {
    let left = parse_primary(chars)?;
    if chars.peek() == Some(&'^') {
        chars.next();
        let right = parse_power(chars)?;
        let result = left.powf(right);
        if !result.is_finite() {
            return Err("Exponentiation did not produce a finite number".to_string());
        }
        return Ok(result);
    }
    Ok(left)
}

fn parse_primary(
    chars: &mut std::iter::Peekable<std::str::Chars>,
) -> std::result::Result<f64, String> {
    match chars.peek() {
        Some('(') => {
            chars.next();
            let val = parse_addition(chars)?;
            if chars.next() != Some(')') {
                return Err("Missing closing parenthesis".to_string());
            }
            Ok(val)
        }
        Some('-') => {
            chars.next();
            let val = parse_primary(chars)?;
            Ok(-val)
        }
        Some('+') => {
            chars.next();
            parse_primary(chars)
        }
        Some(c) if c.is_ascii_digit() || *c == '.' => {
            let mut num_str = String::new();
            while let Some(&ch) = chars.peek() {
                if ch.is_ascii_digit() || ch == '.' {
                    num_str.push(ch);
                    chars.next();
                } else {
                    break;
                }
            }
            num_str.parse::<f64>().map_err(|e| e.to_string())
        }
        Some(other) => Err(format!("Unexpected character: '{}'", other)),
        None => Err("Unexpected end of expression".to_string()),
    }
}

pub struct EnvGetTool;

#[async_trait]
impl Tool for EnvGetTool {
    fn name(&self) -> &str {
        "env_get"
    }
    fn description(&self) -> &str {
        "Read one environment variable (sensitive keys redacted)"
    }
    fn parameters_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "name": { "type": "string", "description": "Environment variable name" }
            },
            "required": ["name"]
        })
    }
    fn risk_tier(&self) -> RiskTier {
        RiskTier::Tier0ReadOnly
    }
    async fn execute(&self, _ctx: &ToolContext, args: Value) -> Result<Value> {
        let name = args
            .get("name")
            .and_then(|n| n.as_str())
            .ok_or_else(|| OdonError::Tool {
                tool: self.name().to_string(),
                message: "Missing 'name' parameter".to_string(),
            })?;

        let val = std::env::var(name).unwrap_or_default();
        let upper = name.to_uppercase();
        let safe_val = if upper.contains("KEY")
            || upper.contains("SECRET")
            || upper.contains("TOKEN")
            || upper.contains("PASS")
        {
            if val.is_empty() {
                "".to_string()
            } else {
                "REDACTED".to_string()
            }
        } else {
            val
        };

        Ok(json!({ "name": name, "value": safe_val }))
    }
}

pub struct EnvListTool;

#[async_trait]
impl Tool for EnvListTool {
    fn name(&self) -> &str {
        "env_list"
    }
    fn description(&self) -> &str {
        "List environment variable names currently visible to the agent"
    }
    fn parameters_schema(&self) -> Value {
        json!({ "type": "object", "properties": {} })
    }
    fn risk_tier(&self) -> RiskTier {
        RiskTier::Tier0ReadOnly
    }
    async fn execute(&self, _ctx: &ToolContext, _args: Value) -> Result<Value> {
        let sensitive = |name: &str| {
            let upper = name.to_uppercase();
            upper.ends_with("KEY")
                || upper.ends_with("TOKEN")
                || upper.ends_with("SECRET")
                || upper.ends_with("PASSWORD")
                || upper.ends_with("PASSWD")
                || upper.ends_with("API_KEY")
                || upper.contains("SECRET")
                || upper.contains("PASSWORD")
                || upper.contains("PRIVATE_KEY")
        };
        let vars: Vec<String> = std::env::vars()
            .map(|(k, _)| k)
            .filter(|name| !sensitive(name))
            .collect();
        let filtered: usize = std::env::vars().count().saturating_sub(vars.len());
        Ok(json!({ "variables": vars, "count": vars.len(), "filtered_sensitive": filtered }))
    }
}

pub struct ProcessListTool;

#[async_trait]
impl Tool for ProcessListTool {
    fn name(&self) -> &str {
        "process_list"
    }
    fn description(&self) -> &str {
        "List running processes on the host"
    }
    fn parameters_schema(&self) -> Value {
        json!({ "type": "object", "properties": {} })
    }
    fn risk_tier(&self) -> RiskTier {
        RiskTier::Tier0ReadOnly
    }
    async fn execute(&self, ctx: &ToolContext, _args: Value) -> Result<Value> {
        #[cfg(target_os = "windows")]
        let output = tokio::process::Command::new("powershell")
            .arg("-NoProfile")
            .arg("-Command")
            .arg("Get-Process | Select-Object -First 25 Id, ProcessName, WS | ConvertTo-Json")
            .current_dir(&ctx.workspace_root)
            .output()
            .await;

        #[cfg(not(target_os = "windows"))]
        let output = tokio::process::Command::new("ps")
            .arg("aux")
            .current_dir(&ctx.workspace_root)
            .output()
            .await;

        match output {
            Ok(out) => {
                let stdout = String::from_utf8_lossy(&out.stdout).to_string();
                Ok(json!({ "processes": stdout }))
            }
            Err(e) => Err(OdonError::Tool {
                tool: self.name().to_string(),
                message: format!("Failed to list processes: {}", e),
            }),
        }
    }
}

pub struct DiskUsageTool;

#[async_trait]
impl Tool for DiskUsageTool {
    fn name(&self) -> &str {
        "disk_usage"
    }
    fn description(&self) -> &str {
        "Inspect workspace disk usage with a bounded recursive size scan"
    }
    fn parameters_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "max_entries": { "type": "integer", "description": "Maximum entries to scan (default 20000)" }
            }
        })
    }
    fn risk_tier(&self) -> RiskTier {
        RiskTier::Tier0ReadOnly
    }
    async fn execute(&self, ctx: &ToolContext, args: Value) -> Result<Value> {
        let max_entries = args
            .get("max_entries")
            .and_then(Value::as_u64)
            .unwrap_or(20_000)
            .clamp(1, 200_000);
        let root = std::path::Path::new(&ctx.workspace_root).to_path_buf();
        let mut stack = vec![root.clone()];
        let mut files = 0u64;
        let mut directories = 0u64;
        let mut total_bytes = 0u64;
        let mut truncated = false;

        while let Some(dir) = stack.pop() {
            if files + directories >= max_entries {
                truncated = true;
                break;
            }
            let Ok(entries) = std::fs::read_dir(&dir) else {
                continue;
            };
            directories += 1;
            for entry in entries.filter_map(|entry| entry.ok()) {
                if files + directories >= max_entries {
                    truncated = true;
                    break;
                }
                let Ok(metadata) = entry.metadata() else {
                    continue;
                };
                if metadata.is_dir() {
                    stack.push(entry.path());
                } else if metadata.is_file() {
                    files += 1;
                    total_bytes += metadata.len();
                }
            }
        }

        Ok(json!({
            "workspace": ctx.workspace_root,
            "total_bytes": total_bytes,
            "files": files,
            "directories": directories,
            "max_entries": max_entries,
            "truncated": truncated,
            "status": "measured"
        }))
    }
}

#[cfg(test)]
mod tests {
    use super::eval_simple_math;

    #[test]
    fn calc_eval_rejects_trailing_input_and_division_by_zero() {
        assert!(eval_simple_math("1+2abc").is_err());
        assert!(eval_simple_math("1/0").is_err());
    }

    #[test]
    fn calc_eval_supports_exponents_parentheses_and_precedence() {
        assert_eq!(eval_simple_math("2^3").unwrap(), 8.0);
        assert_eq!(eval_simple_math("(2+3)*4").unwrap(), 20.0);
        assert_eq!(eval_simple_math("2^2^2").unwrap(), 16.0);
    }
}
