use crate::registry::{Tool, ToolContext};
use async_trait::async_trait;
use odon_core::error::{OdonError, Result};
use odon_security::RiskTier;
use serde_json::{json, Value};
use tokio::process::Command;

fn tool_error(name: &str, message: impl Into<String>) -> OdonError {
    OdonError::Tool { tool: name.to_string(), message: message.into() }
}

async fn powershell(name: &str, script: &str, args: &[String]) -> Result<Value> {
    if !cfg!(windows) {
        return Err(tool_error(name, "local application control is currently implemented for Windows only"));
    }
    let output = Command::new("powershell.exe")
        .args(["-NoProfile", "-NonInteractive", "-ExecutionPolicy", "Bypass", "-Command", script, "--"])
        .args(args)
        .output()
        .await
        .map_err(|e| tool_error(name, format!("failed to start PowerShell: {e}")))?;
    let stdout = String::from_utf8_lossy(&output.stdout).trim().to_string();
    let stderr = String::from_utf8_lossy(&output.stderr).trim().to_string();
    if !output.status.success() {
        return Err(tool_error(name, if stderr.is_empty() { stdout } else { stderr }));
    }
    serde_json::from_str(&stdout).map_err(|e| tool_error(name, format!("PowerShell returned invalid JSON: {e}; output={stdout}")))
}

pub struct LocalAppListTool;
#[async_trait]
impl Tool for LocalAppListTool {
    fn name(&self) -> &str { "app_list" }
    fn description(&self) -> &str { "List visible local Windows processes with process id, title, and executable path" }
    fn parameters_schema(&self) -> Value { json!({"type":"object","properties":{"name":{"type":"string"}}}) }
    fn risk_tier(&self) -> RiskTier { RiskTier::Tier0ReadOnly }
    async fn execute(&self, _ctx: &ToolContext, args: Value) -> Result<Value> {
        let filter = args.get("name").and_then(Value::as_str).unwrap_or("").to_string();
        powershell(self.name(), r#"$f=$args[0]; Get-Process | Where-Object { !$f -or $_.ProcessName -like "*$f*" } | ForEach-Object { [pscustomobject]@{ pid=$_.Id; name=$_.ProcessName; title=$_.MainWindowTitle; path=try { $_.Path } catch { $null } } } | ConvertTo-Json -Depth 3 -Compress"#, &[filter]).await
    }
}

pub struct LocalAppLaunchTool;
#[async_trait]
impl Tool for LocalAppLaunchTool {
    fn name(&self) -> &str { "app_launch" }
    fn description(&self) -> &str { "Launch a local Windows application with an optional argument string" }
    fn parameters_schema(&self) -> Value { json!({"type":"object","required":["executable"],"properties":{"executable":{"type":"string"},"arguments":{"type":"string"},"working_directory":{"type":"string"}}}) }
    fn risk_tier(&self) -> RiskTier { RiskTier::Tier2SystemMutating }
    async fn execute(&self, _ctx: &ToolContext, args: Value) -> Result<Value> {
        let executable = args.get("executable").and_then(Value::as_str).ok_or_else(|| tool_error(self.name(), "executable is required"))?.to_string();
        let arguments = args.get("arguments").and_then(Value::as_str).unwrap_or("").to_string();
        let working = args.get("working_directory").and_then(Value::as_str).unwrap_or("").to_string();
        powershell(self.name(), r#"$p=@{FilePath=$args[0]}; if($args[1]){$p.ArgumentList=$args[1]}; if($args[2]){$p.WorkingDirectory=$args[2]}; $x=Start-Process @p -PassThru; [pscustomobject]@{pid=$x.Id; executable=$args[0]} | ConvertTo-Json -Compress"#, &[executable, arguments, working]).await
    }
}

pub struct LocalAppFocusTool;
#[async_trait]
impl Tool for LocalAppFocusTool {
    fn name(&self) -> &str { "app_focus" }
    fn description(&self) -> &str { "Focus a local Windows application window by process id or title" }
    fn parameters_schema(&self) -> Value { json!({"type":"object","properties":{"pid":{"type":"integer"},"title":{"type":"string"}},"oneOf":[{"required":["pid"]},{"required":["title"]}]}) }
    fn risk_tier(&self) -> RiskTier { RiskTier::Tier2SystemMutating }
    async fn execute(&self, _ctx: &ToolContext, args: Value) -> Result<Value> {
        let pid = args.get("pid").and_then(Value::as_u64).unwrap_or(0).to_string();
        let title = args.get("title").and_then(Value::as_str).unwrap_or("").to_string();
        powershell(self.name(), r#"Add-Type @' using System; using System.Runtime.InteropServices; public static class W { [DllImport("user32.dll")] public static extern bool SetForegroundWindow(IntPtr h); } '@; $p=if($args[0] -ne '0'){Get-Process -Id ([int]$args[0]) -ErrorAction Stop}else{Get-Process | Where-Object {$_.MainWindowTitle -like "*$($args[1])*"} | Select-Object -First 1}; if(!$p -or !$p.MainWindowHandle){throw 'window not found'}; [W]::SetForegroundWindow($p.MainWindowHandle) | Out-Null; [pscustomobject]@{focused=$true;pid=$p.Id;title=$p.MainWindowTitle}|ConvertTo-Json -Compress"#, &[pid, title]).await
    }
}

pub struct LocalAppCloseTool;
#[async_trait]
impl Tool for LocalAppCloseTool {
    fn name(&self) -> &str { "app_close" }
    fn description(&self) -> &str { "Close a local Windows application gracefully by process id" }
    fn parameters_schema(&self) -> Value { json!({"type":"object","required":["pid"],"properties":{"pid":{"type":"integer"},"force":{"type":"boolean","default":false}}}) }
    fn risk_tier(&self) -> RiskTier { RiskTier::Tier2SystemMutating }
    async fn execute(&self, _ctx: &ToolContext, args: Value) -> Result<Value> {
        let pid = args.get("pid").and_then(Value::as_u64).ok_or_else(|| tool_error(self.name(), "pid is required"))?.to_string();
        let force = args.get("force").and_then(Value::as_bool).unwrap_or(false).to_string();
        powershell(self.name(), r#"$p=Get-Process -Id ([int]$args[0]) -ErrorAction Stop; if($args[1] -eq 'True'){$p | Stop-Process -Force}else{if(!$p.CloseMainWindow()){throw 'application has no closable main window'}}; [pscustomobject]@{closed=$true;pid=$p.Id;forced=($args[1] -eq 'True')}|ConvertTo-Json -Compress"#, &[pid, force]).await
    }
}

pub struct LocalAppInputTool;
#[async_trait]
impl Tool for LocalAppInputTool {
    fn name(&self) -> &str { "app_input" }
    fn description(&self) -> &str { "Send a controlled keyboard key or text input to the focused Windows application" }
    fn parameters_schema(&self) -> Value { json!({"type":"object","properties":{"text":{"type":"string"},"key":{"type":"string"}},"oneOf":[{"required":["text"]},{"required":["key"]}]}) }
    fn risk_tier(&self) -> RiskTier { RiskTier::Tier2SystemMutating }
    async fn execute(&self, _ctx: &ToolContext, args: Value) -> Result<Value> {
        let text = args.get("text").and_then(Value::as_str).unwrap_or("").to_string();
        let key = args.get("key").and_then(Value::as_str).unwrap_or("").to_string();
        powershell(self.name(), r#"Add-Type -AssemblyName System.Windows.Forms; if($args[0]){[System.Windows.Forms.SendKeys]::SendWait($args[0])}elseif($args[1]){[System.Windows.Forms.SendKeys]::SendWait("{"+$args[1]+"}")}else{throw 'text or key is required'}; [pscustomobject]@{sent=$true}|ConvertTo-Json -Compress"#, &[text, key]).await
    }
}
