use async_trait::async_trait;
use odon_core::{OdonError, Result};
use odon_security::RiskTier;
use serde_json::{json, Value};
use std::path::PathBuf;

use crate::registry::{Tool, ToolContext};

fn get_skills_dir() -> PathBuf {
    let base = std::env::var_os("ODON_HOME")
        .map(PathBuf::from)
        .or_else(|| std::env::var_os("USERPROFILE").map(PathBuf::from))
        .or_else(|| std::env::var_os("HOME").map(PathBuf::from))
        .unwrap_or_else(|| PathBuf::from("."));
    base.join(".odon").join("skills")
}

fn normalize_skill_name(name: &str) -> Option<String> {
    let mut slug = String::new();
    let mut pending_separator = false;

    for ch in name.trim().to_ascii_lowercase().chars() {
        if ch.is_ascii_alphanumeric() {
            if pending_separator && !slug.is_empty() {
                slug.push('-');
            }
            slug.push(ch);
            pending_separator = false;
        } else if matches!(ch, '-' | '_' | ' ' | '/' | '\\') && !slug.is_empty() {
            pending_separator = true;
        }
    }

    (!slug.is_empty()).then_some(slug)
}

pub struct SkillListTool;

#[async_trait]
impl Tool for SkillListTool {
    fn name(&self) -> &str {
        "skill_list"
    }
    fn description(&self) -> &str {
        "List all autonomous skills available in ~/.odon/skills or workspace .odon/skills"
    }
    fn parameters_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {},
            "required": []
        })
    }
    fn risk_tier(&self) -> RiskTier {
        RiskTier::Tier0ReadOnly
    }
    async fn execute(&self, _ctx: &ToolContext, _args: Value) -> Result<Value> {
        let skills_dir = get_skills_dir();
        let mut skills = Vec::new();

        if let Ok(entries) = std::fs::read_dir(&skills_dir) {
            for entry in entries.flatten() {
                let path = entry.path();
                if path.extension().and_then(|e| e.to_str()) == Some("md") {
                    let name = path
                        .file_stem()
                        .and_then(|s| s.to_str())
                        .unwrap_or("unknown")
                        .to_string();
                    let content = std::fs::read_to_string(&path).unwrap_or_default();
                    let first_line = content
                        .lines()
                        .next()
                        .unwrap_or("")
                        .trim_start_matches('#')
                        .trim()
                        .to_string();
                    skills.push(json!({
                        "name": name,
                        "description": if first_line.is_empty() { "Autonomous skill procedure".to_string() } else { first_line },
                        "path": path.to_string_lossy()
                    }));
                }
            }
        }

        // Also check current workspace .odon/skills
        let local_dir = PathBuf::from(".odon").join("skills");
        if local_dir != skills_dir {
            if let Ok(entries) = std::fs::read_dir(&local_dir) {
                for entry in entries.flatten() {
                    let path = entry.path();
                    if path.extension().and_then(|e| e.to_str()) == Some("md") {
                        let name = path
                            .file_stem()
                            .and_then(|s| s.to_str())
                            .unwrap_or("unknown")
                            .to_string();
                        if !skills.iter().any(|s| s["name"] == name) {
                            let content = std::fs::read_to_string(&path).unwrap_or_default();
                            let first_line = content
                                .lines()
                                .next()
                                .unwrap_or("")
                                .trim_start_matches('#')
                                .trim()
                                .to_string();
                            skills.push(json!({
                                "name": name,
                                "description": if first_line.is_empty() { "Autonomous skill procedure".to_string() } else { first_line },
                                "path": path.to_string_lossy()
                            }));
                        }
                    }
                }
            }
        }

        Ok(json!({ "skills": skills, "count": skills.len() }))
    }
}

pub struct SkillCreateTool;

#[async_trait]
impl Tool for SkillCreateTool {
    fn name(&self) -> &str {
        "skill_create"
    }
    fn description(&self) -> &str {
        "Create or record a reusable autonomous skill procedure in markdown (.odon/skills/<name>.md)"
    }
    fn parameters_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "name": { "type": "string", "description": "Skill name (e.g. 'deploy-vps', 'audit-smart-contract')" },
                "content": { "type": "string", "description": "Markdown instructions, prerequisites, and execution steps" }
            },
            "required": ["name", "content"]
        })
    }
    fn risk_tier(&self) -> RiskTier {
        RiskTier::Tier1WorkspaceMutating
    }
    async fn execute(&self, _ctx: &ToolContext, args: Value) -> Result<Value> {
        let name = args
            .get("name")
            .and_then(|n| n.as_str())
            .ok_or_else(|| OdonError::Tool {
                tool: self.name().to_string(),
                message: "Missing 'name' parameter".to_string(),
            })?;
        let content = args
            .get("content")
            .and_then(|c| c.as_str())
            .ok_or_else(|| OdonError::Tool {
                tool: self.name().to_string(),
                message: "Missing 'content' parameter".to_string(),
            })?;

        let clean_name = normalize_skill_name(name).ok_or_else(|| OdonError::Tool {
            tool: self.name().to_string(),
            message: "Skill name must contain at least one ASCII letter or number".to_string(),
        })?;
        let dir = get_skills_dir();
        let _ = std::fs::create_dir_all(&dir);
        let file_path = dir.join(format!("{}.md", clean_name));

        std::fs::write(&file_path, content).map_err(|e| OdonError::Tool {
            tool: self.name().to_string(),
            message: format!("Failed to save skill: {}", e),
        })?;

        Ok(json!({
            "saved": true,
            "name": clean_name,
            "path": file_path.to_string_lossy()
        }))
    }
}

pub struct SkillRunTool;

#[async_trait]
impl Tool for SkillRunTool {
    fn name(&self) -> &str {
        "skill_run"
    }
    fn description(&self) -> &str {
        "Load a skill's instructions and execution steps by name"
    }
    fn parameters_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "name": { "type": "string", "description": "Name of the skill to execute" }
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

        let clean_name = normalize_skill_name(name).ok_or_else(|| OdonError::Tool {
            tool: self.name().to_string(),
            message: "Skill name must contain at least one ASCII letter or number".to_string(),
        })?;
        let candidates = vec![
            get_skills_dir().join(format!("{}.md", clean_name)),
            PathBuf::from(".odon")
                .join("skills")
                .join(format!("{}.md", clean_name)),
        ];

        for path in candidates {
            if path.exists() {
                let content = std::fs::read_to_string(&path).unwrap_or_default();
                return Ok(json!({
                    "found": true,
                    "name": clean_name,
                    "path": path.to_string_lossy(),
                    "instructions": content
                }));
            }
        }

        Err(OdonError::Tool {
            tool: self.name().to_string(),
            message: format!("Skill '{}' not found in .odon/skills", name),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::normalize_skill_name;

    #[test]
    fn skill_names_are_safe_and_consistent() {
        assert_eq!(
            normalize_skill_name("Deploy VPS"),
            Some("deploy-vps".to_string())
        );
        assert_eq!(
            normalize_skill_name("../../etc/passwd"),
            Some("etc-passwd".to_string())
        );
        assert_eq!(
            normalize_skill_name("audit\\windows"),
            Some("audit-windows".to_string())
        );
        assert_eq!(normalize_skill_name("..."), None);
    }
}
