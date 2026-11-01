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
        let expr = args.get("expression").and_then(|e| e.as_str()).ok_or_else(|| {
            OdonError::Tool {
                tool: self.name().to_string(),
                message: "Missing 'expression' parameter".to_string(),
            }
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
    std::thread::available_parallelism().map(|n| n.get()).unwrap_or(1)
}

fn eval_simple_math(expr: &str) -> std::result::Result<f64, String> {
    let clean: String = expr.chars().filter(|c| !c.is_whitespace()).collect();
    if clean.is_empty() {
        return Err("Empty expression".to_string());
    }

    // Evaluate basic arithmetic +, -, *, /, ^
    // Simple recursive descent parser
    let mut chars = clean.chars().peekable();
    parse_addition(&mut chars)
}

fn parse_addition(chars: &mut std::iter::Peekable<std::str::Chars>) -> std::result::Result<f64, String> {
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

fn parse_multiplication(chars: &mut std::iter::Peekable<std::str::Chars>) -> std::result::Result<f64, String> {
    let mut left = parse_primary(chars)?;
    while let Some(&op) = chars.peek() {
        if op == '*' {
            chars.next();
            let right = parse_primary(chars)?;
            left *= right;
        } else if op == '/' {
            chars.next();
            let right = parse_primary(chars)?;
            if right == 0.0 {
                return Err("Division by zero".to_string());
            }
            left /= right;
        } else if op == '%' {
            chars.next();
            let right = parse_primary(chars)?;
            left %= right;
        } else {
            break;
        }
    }
    Ok(left)
}

fn parse_primary(chars: &mut std::iter::Peekable<std::str::Chars>) -> std::result::Result<f64, String> {
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
        Some(c) if c.is_digit(10) || *c == '.' => {
            let mut num_str = String::new();
            while let Some(&ch) = chars.peek() {
                if ch.is_digit(10) || ch == '.' {
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

