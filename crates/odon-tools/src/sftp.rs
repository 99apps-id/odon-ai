use crate::registry::{Tool, ToolContext};
use odon_core::{OdonError, Result};
use odon_security::RiskTier;
use serde::Deserialize;
use serde_json::json;
use serde_json::Value;
use std::path::{Path, PathBuf};
use std::time::Duration;
use tokio::process::Command;
use tokio::time::timeout;

#[derive(Debug, Deserialize)]
struct SftpUploadPayload {
    profile_id: String,
    local_path: String,
    remote_path: String,
}

#[derive(Debug, Deserialize)]
struct SftpDownloadPayload {
    profile_id: String,
    remote_path: String,
    local_path: String,
}

#[derive(Debug, Deserialize)]
struct SftpListPayload {
    profile_id: String,
    remote_path: String,
}

#[derive(Debug, Deserialize, Clone)]
struct SshProfile {
    id: String,
    alias: String,
    host: String,
    #[serde(default)]
    port: Option<u16>,
    user: String,
    #[serde(default)]
    key_path: Option<String>,
}

struct SshTarget {
    user: String,
    host: String,
    port: Option<u16>,
    key_path: Option<String>,
}

impl SshTarget {
    fn host_target(&self) -> String {
        let host = if self.host.contains(':') && !self.host.starts_with('[') {
            format!("[{}]", self.host)
        } else {
            self.host.clone()
        };
        format!("{}@{}", self.user, host)
    }

    fn base_args(&self) -> Vec<String> {
        let mut args = vec![
            "-o".to_string(),
            "StrictHostKeyChecking=yes".to_string(),
            "-o".to_string(),
            "ConnectTimeout=15".to_string(),
            "-o".to_string(),
            "ServerAliveInterval=10".to_string(),
            "-o".to_string(),
            "BatchMode=yes".to_string(),
        ];
        if let Some(key) = &self.key_path {
            args.push("-i".to_string());
            args.push(key.clone());
        }
        args
    }

    fn ssh_args(&self) -> Vec<String> {
        let mut args = self.base_args();
        if let Some(port) = self.port {
            args.push("-p".to_string());
            args.push(port.to_string());
        }
        args.push(self.host_target());
        args
    }

    fn scp_args(&self) -> Vec<String> {
        let mut args = self.base_args();
        if let Some(port) = self.port {
            args.push("-P".to_string());
            args.push(port.to_string());
        }
        args
    }
}

fn validate_ssh_field(value: &str, label: &str) -> Result<()> {
    if value.trim().is_empty()
        || value.starts_with('-')
        || value.chars().any(|ch| {
            ch.is_whitespace()
                || matches!(
                    ch,
                    '\0' | '\n' | '\r' | '@' | '\'' | '"' | ';' | '|' | '&' | '$' | '`' | '<' | '>'
                )
        })
    {
        return Err(OdonError::Tool {
            tool: "sftp".to_string(),
            message: format!("Invalid SSH {label}: {value:?}"),
        });
    }
    Ok(())
}

fn load_ssh_target(profile_id: &str) -> Result<SshTarget> {
    let profiles_path = dirs::home_dir()
        .ok_or_else(|| OdonError::Tool {
            tool: "sftp".to_string(),
            message: "Cannot locate home directory".to_string(),
        })?
        .join(".odon")
        .join("ssh-profiles.json");

    let data = std::fs::read_to_string(&profiles_path).map_err(|e| OdonError::Tool {
        tool: "sftp".to_string(),
        message: format!("Failed to read SSH profiles: {e}"),
    })?;

    let profiles: Vec<SshProfile> = serde_json::from_str(&data).map_err(|e| OdonError::Tool {
        tool: "sftp".to_string(),
        message: format!("Invalid SSH profiles file: {e}"),
    })?;

    let profile = profiles
        .into_iter()
        .find(|p| p.id == profile_id || p.alias == profile_id)
        .ok_or_else(|| OdonError::Tool {
            tool: "sftp".to_string(),
            message: format!("SSH profile '{profile_id}' not found"),
        })?;

    validate_ssh_field(&profile.user, "username")?;
    validate_ssh_field(&profile.host, "host")?;
    if let Some(key) = &profile.key_path {
        if key.trim().is_empty() || key.contains(['\0', '\n', '\r']) {
            return Err(OdonError::Tool {
                tool: "sftp".to_string(),
                message: "Invalid SSH key path".to_string(),
            });
        }
    }

    Ok(SshTarget {
        user: profile.user,
        host: profile.host,
        port: profile.port,
        key_path: profile.key_path,
    })
}

fn normalize_remote_path(path: &str, user: &str) -> Result<String> {
    let path = path.trim();
    if path.is_empty() || path.contains(['\0', '\n', '\r']) {
        return Err(OdonError::Tool {
            tool: "sftp".to_string(),
            message: "Remote path must not be empty or contain control characters".to_string(),
        });
    }
    if path == "~" {
        return Ok(format!("/home/{user}"));
    }
    if let Some(rest) = path.strip_prefix("~/") {
        return Ok(format!("/home/{user}/{rest}"));
    }
    if !path.starts_with('/') {
        return Err(OdonError::Tool {
            tool: "sftp".to_string(),
            message: "Remote path must be absolute or start with '~/'".to_string(),
        });
    }
    Ok(path.to_string())
}

fn shell_quote(value: &str) -> String {
    format!("'{}'", value.replace('\'', "'\\''"))
}

fn local_workspace_path(ctx: &ToolContext, input: &str, for_write: bool) -> Result<PathBuf> {
    let root = std::path::Path::new(&ctx.workspace_root)
        .canonicalize()
        .map_err(|error| OdonError::Tool {
            tool: "sftp".to_string(),
            message: format!("Cannot resolve workspace: {error}"),
        })?;
    let candidate = PathBuf::from(input);
    let joined = if candidate.is_absolute() {
        candidate
    } else {
        root.join(candidate)
    };

    if for_write {
        let parent = joined.parent().unwrap_or(&root);
        std::fs::create_dir_all(parent).map_err(|error| OdonError::Tool {
            tool: "sftp".to_string(),
            message: format!("Cannot create local destination: {error}"),
        })?;
        let parent = parent.canonicalize().map_err(|error| OdonError::Tool {
            tool: "sftp".to_string(),
            message: format!("Cannot resolve local destination: {error}"),
        })?;
        if !parent.starts_with(&root) {
            return Err(OdonError::Tool {
                tool: "sftp".to_string(),
                message: "Local destination is outside the workspace".to_string(),
            });
        }
        return Ok(parent.join(joined.file_name().unwrap_or_default()));
    }

    let canonical = joined.canonicalize().map_err(|error| OdonError::Tool {
        tool: "sftp".to_string(),
        message: format!("Cannot resolve local path: {error}"),
    })?;
    if !canonical.starts_with(&root) {
        return Err(OdonError::Tool {
            tool: "sftp".to_string(),
            message: "Local path is outside the workspace".to_string(),
        });
    }
    Ok(canonical)
}

async fn scp_transfer(
    _ctx: &ToolContext,
    profile_id: &str,
    local: &Path,
    remote: &str,
    to_remote: bool,
) -> Result<Value> {
    let target = load_ssh_target(profile_id)?;
    let remote = normalize_remote_path(remote, &target.user)?;
    let remote_spec = format!("{}:{}", target.host_target(), shell_quote(&remote));

    let mut cmd = Command::new("scp");
    cmd.args(target.scp_args());
    if to_remote {
        cmd.arg(local).arg(&remote_spec);
    } else {
        cmd.arg(&remote_spec).arg(local);
    }

    cmd.stdout(std::process::Stdio::piped());
    cmd.stderr(std::process::Stdio::piped());

    let execution = timeout(Duration::from_secs(120), cmd.output()).await;
    let output = match execution {
        Ok(Ok(output)) => output,
        Ok(Err(e)) => {
            return Err(OdonError::Tool {
                tool: "sftp".to_string(),
                message: format!("scp execution failed: {e}"),
            });
        }
        Err(_) => {
            return Err(OdonError::Tool {
                tool: "sftp".to_string(),
                message: "scp timed out after 120 seconds".to_string(),
            });
        }
    };

    let stdout = String::from_utf8_lossy(&output.stdout).to_string();
    let stderr = String::from_utf8_lossy(&output.stderr).to_string();

    const MAX_OUTPUT_BYTES: usize = 1_048_576;
    let mut capped_stdout = stdout;
    let mut capped_stderr = stderr;
    let mut truncated = false;
    if capped_stdout.len() > MAX_OUTPUT_BYTES {
        capped_stdout.truncate(MAX_OUTPUT_BYTES);
        truncated = true;
    }
    if capped_stderr.len() > MAX_OUTPUT_BYTES {
        capped_stderr.truncate(MAX_OUTPUT_BYTES);
        truncated = true;
    }
    if truncated {
        capped_stderr.push_str("\n[ODON] output capped at 1 MB");
    }

    Ok(json!({
        "success": output.status.success(),
        "exit_code": output.status.code().unwrap_or(-1),
        "stdout": capped_stdout,
        "stderr": capped_stderr,
        "local_path": local.to_string_lossy(),
        "remote_path": remote,
    }))
}

async fn ssh_list(_ctx: &ToolContext, profile_id: &str, remote_path: &str) -> Result<Value> {
    let target = load_ssh_target(profile_id)?;
    let remote = normalize_remote_path(remote_path, &target.user)?;
    let mut cmd = Command::new("ssh");
    cmd.args(target.ssh_args());
    cmd.arg(format!("ls -la -- {}", shell_quote(&remote)));

    cmd.stdout(std::process::Stdio::piped());
    cmd.stderr(std::process::Stdio::piped());

    let execution = timeout(Duration::from_secs(30), cmd.output()).await;
    let output = match execution {
        Ok(Ok(output)) => output,
        Ok(Err(e)) => {
            return Err(OdonError::Tool {
                tool: "sftp".to_string(),
                message: format!("ssh execution failed: {e}"),
            });
        }
        Err(_) => {
            return Err(OdonError::Tool {
                tool: "sftp".to_string(),
                message: "ssh timed out after 30 seconds".to_string(),
            });
        }
    };

    let stdout = String::from_utf8_lossy(&output.stdout).to_string();
    let stderr = String::from_utf8_lossy(&output.stderr).to_string();

    const MAX_OUTPUT_BYTES: usize = 1_048_576;
    let mut capped_stdout = stdout;
    let mut capped_stderr = stderr;
    let mut truncated = false;
    if capped_stdout.len() > MAX_OUTPUT_BYTES {
        capped_stdout.truncate(MAX_OUTPUT_BYTES);
        truncated = true;
    }
    if capped_stderr.len() > MAX_OUTPUT_BYTES {
        capped_stderr.truncate(MAX_OUTPUT_BYTES);
        truncated = true;
    }
    if truncated {
        capped_stderr.push_str("\n[ODON] output capped at 1 MB");
    }

    Ok(json!({
        "success": output.status.success(),
        "exit_code": output.status.code().unwrap_or(-1),
        "remote_path": remote_path,
        "listing": capped_stdout,
        "stderr": capped_stderr,
    }))
}

pub struct SftpUploadTool;

#[async_trait::async_trait]
impl Tool for SftpUploadTool {
    fn name(&self) -> &str {
        "sftp_upload"
    }

    fn description(&self) -> &str {
        "Upload a local workspace file to a remote SSH host via scp"
    }

    fn parameters_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "profile_id": {
                    "type": "string",
                    "description": "Saved SSH profile ID or alias"
                },
                "local_path": {
                    "type": "string",
                    "description": "Local workspace-relative file path to upload"
                },
                "remote_path": {
                    "type": "string",
                    "description": "Absolute remote destination path"
                }
            },
            "required": ["profile_id", "local_path", "remote_path"]
        })
    }

    fn risk_tier(&self) -> RiskTier {
        RiskTier::Tier1WorkspaceMutating
    }

    async fn execute(&self, ctx: &ToolContext, args: Value) -> Result<Value> {
        let payload: SftpUploadPayload =
            serde_json::from_value(args).map_err(|e| OdonError::Tool {
                tool: self.name().to_string(),
                message: format!("Invalid arguments: {e}"),
            })?;

        let local = local_workspace_path(ctx, &payload.local_path, false)?;
        if !local.is_file() {
            return Err(OdonError::Tool {
                tool: self.name().to_string(),
                message: format!("Local file does not exist: {}", payload.local_path),
            });
        }

        scp_transfer(ctx, &payload.profile_id, &local, &payload.remote_path, true).await
    }
}

pub struct SftpDownloadTool;

#[async_trait::async_trait]
impl Tool for SftpDownloadTool {
    fn name(&self) -> &str {
        "sftp_download"
    }

    fn description(&self) -> &str {
        "Download a remote file from an SSH host into the local workspace via scp"
    }

    fn parameters_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "profile_id": {
                    "type": "string",
                    "description": "Saved SSH profile ID or alias"
                },
                "remote_path": {
                    "type": "string",
                    "description": "Absolute remote source path"
                },
                "local_path": {
                    "type": "string",
                    "description": "Local workspace-relative destination path"
                }
            },
            "required": ["profile_id", "remote_path", "local_path"]
        })
    }

    fn risk_tier(&self) -> RiskTier {
        RiskTier::Tier1WorkspaceMutating
    }

    async fn execute(&self, ctx: &ToolContext, args: Value) -> Result<Value> {
        let payload: SftpDownloadPayload =
            serde_json::from_value(args).map_err(|e| OdonError::Tool {
                tool: self.name().to_string(),
                message: format!("Invalid arguments: {e}"),
            })?;

        let local = local_workspace_path(ctx, &payload.local_path, true)?;
        scp_transfer(
            ctx,
            &payload.profile_id,
            &local,
            &payload.remote_path,
            false,
        )
        .await
    }
}

pub struct SftpListTool;

#[async_trait::async_trait]
impl Tool for SftpListTool {
    fn name(&self) -> &str {
        "sftp_list"
    }

    fn description(&self) -> &str {
        "List files in a remote SSH directory"
    }

    fn parameters_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "profile_id": {
                    "type": "string",
                    "description": "Saved SSH profile ID or alias"
                },
                "remote_path": {
                    "type": "string",
                    "description": "Absolute remote directory path"
                }
            },
            "required": ["profile_id", "remote_path"]
        })
    }

    fn risk_tier(&self) -> RiskTier {
        RiskTier::Tier0ReadOnly
    }

    async fn execute(&self, ctx: &ToolContext, args: Value) -> Result<Value> {
        let payload: SftpListPayload =
            serde_json::from_value(args).map_err(|e| OdonError::Tool {
                tool: self.name().to_string(),
                message: format!("Invalid arguments: {e}"),
            })?;

        ssh_list(ctx, &payload.profile_id, &payload.remote_path).await
    }
}

fn clip_ssh_output(raw: &[u8], limit: usize) -> String {
    if raw.len() <= limit {
        String::from_utf8_lossy(raw).to_string()
    } else {
        String::from_utf8_lossy(&raw[..limit]).to_string()
    }
}

async fn run_ssh_command(
    profile_id: &str,
    command: &str,
    timeout_secs: u64,
    tool_name: &str,
) -> Result<Value> {
    if command.trim().is_empty() {
        return Err(OdonError::Tool {
            tool: tool_name.to_string(),
            message: "command is required".to_string(),
        });
    }
    if command.len() > 10_000 {
        return Err(OdonError::Tool {
            tool: tool_name.to_string(),
            message: "command is too long".to_string(),
        });
    }
    let target = load_ssh_target(profile_id)?;
    let mut cmd = Command::new("ssh");
    cmd.args(target.ssh_args());
    cmd.arg(command);
    cmd.stdout(std::process::Stdio::piped());
    cmd.stderr(std::process::Stdio::piped());

    let started = std::time::Instant::now();
    let execution = timeout(Duration::from_secs(timeout_secs), cmd.output()).await;
    let output = match execution {
        Ok(Ok(output)) => output,
        Ok(Err(e)) => {
            return Err(OdonError::Tool {
                tool: tool_name.to_string(),
                message: format!("ssh execution failed: {e}"),
            });
        }
        Err(_) => {
            return Err(OdonError::Tool {
                tool: tool_name.to_string(),
                message: format!("ssh command timed out after {timeout_secs} seconds"),
            });
        }
    };
    Ok(json!({
        "profile_id": profile_id,
        "command": command,
        "success": output.status.success(),
        "exit_code": output.status.code().unwrap_or(-1),
        "stdout": clip_ssh_output(&output.stdout, 256 * 1024),
        "stderr": clip_ssh_output(&output.stderr, 256 * 1024),
        "duration_ms": started.elapsed().as_millis()
    }))
}

pub struct SshExecTool;
#[async_trait::async_trait]
impl Tool for SshExecTool {
    fn name(&self) -> &str {
        "ssh_exec"
    }
    fn description(&self) -> &str {
        "Execute a single command on a configured SSH host"
    }
    fn parameters_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "profile_id": { "type": "string", "description": "Saved SSH profile ID or alias" },
                "command": { "type": "string", "description": "Remote command to execute" },
                "timeout_secs": { "type": "integer", "description": "Execution timeout (default 30, max 300)" }
            },
            "required": ["profile_id", "command"]
        })
    }
    fn risk_tier(&self) -> RiskTier {
        RiskTier::Tier2SystemMutating
    }
    async fn execute(&self, _ctx: &ToolContext, args: Value) -> Result<Value> {
        let profile_id = args
            .get("profile_id")
            .and_then(Value::as_str)
            .ok_or_else(|| OdonError::Tool {
                tool: self.name().to_string(),
                message: "Missing 'profile_id' parameter".to_string(),
            })?;
        let command =
            args.get("command")
                .and_then(Value::as_str)
                .ok_or_else(|| OdonError::Tool {
                    tool: self.name().to_string(),
                    message: "Missing 'command' parameter".to_string(),
                })?;
        let timeout_secs = args
            .get("timeout_secs")
            .and_then(Value::as_u64)
            .unwrap_or(30)
            .clamp(1, 300);
        run_ssh_command(profile_id, command, timeout_secs, self.name()).await
    }
}

pub struct SshProfileTestTool;
#[async_trait::async_trait]
impl Tool for SshProfileTestTool {
    fn name(&self) -> &str {
        "ssh_profile_test"
    }
    fn description(&self) -> &str {
        "Verify that a saved SSH profile can authenticate and execute a harmless echo command"
    }
    fn parameters_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "profile_id": { "type": "string", "description": "Saved SSH profile ID or alias" }
            },
            "required": ["profile_id"]
        })
    }
    fn risk_tier(&self) -> RiskTier {
        RiskTier::Tier0ReadOnly
    }
    async fn execute(&self, _ctx: &ToolContext, args: Value) -> Result<Value> {
        let profile_id = args
            .get("profile_id")
            .and_then(Value::as_str)
            .ok_or_else(|| OdonError::Tool {
                tool: self.name().to_string(),
                message: "Missing 'profile_id' parameter".to_string(),
            })?;
        let mut result = run_ssh_command(profile_id, "echo odon-ssh-ok", 15, self.name()).await?;
        if let Some(object) = result.as_object_mut() {
            object.insert(
                "authenticated".into(),
                json!(object
                    .get("success")
                    .and_then(Value::as_bool)
                    .unwrap_or(false)),
            );
            object.remove("command");
        }
        Ok(result)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn remote_home_paths_expand_for_the_profile_user() {
        assert_eq!(normalize_remote_path("~", "alice").unwrap(), "/home/alice");
        assert_eq!(
            normalize_remote_path("~/app/log.txt", "alice").unwrap(),
            "/home/alice/app/log.txt"
        );
        assert_eq!(
            normalize_remote_path("/var/log/app.log", "alice").unwrap(),
            "/var/log/app.log"
        );
        assert!(normalize_remote_path("relative/path", "alice").is_err());
        assert!(normalize_remote_path("/tmp/bad\npath", "alice").is_err());
    }

    #[test]
    fn shell_quoting_handles_single_quotes() {
        assert_eq!(shell_quote("/tmp/a'b"), "'/tmp/a'\\''b'");
    }

    #[test]
    fn ssh_fields_reject_injection_shaped_values() {
        assert!(validate_ssh_field("alice", "username").is_ok());
        assert!(validate_ssh_field("example.com", "host").is_ok());
        assert!(validate_ssh_field("2001:db8::1", "host").is_ok());
        assert!(validate_ssh_field("-oProxyCommand=x", "host").is_err());
        assert!(validate_ssh_field("host;rm -rf /", "host").is_err());
        assert!(validate_ssh_field("user@host", "username").is_err());
    }
}
