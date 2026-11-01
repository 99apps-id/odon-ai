use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ExecutionMode {
    Autonomous, // Auto-approves Tier 0 and Tier 1, asks Tier 2
    Supervised, // Asks for Tier 1 and Tier 2
    Locked,     // Tier 0 only, all mutating calls rejected
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SecurityPolicy {
    pub mode: ExecutionMode,
    pub allowed_directories: Vec<String>,
    pub forbidden_commands: Vec<String>,
    pub max_execution_time_secs: u64,
}

impl Default for SecurityPolicy {
    fn default() -> Self {
        Self {
            mode: ExecutionMode::Autonomous,
            allowed_directories: Vec::new(),
            forbidden_commands: vec![
                "rm -rf /".to_string(),
                "rm -rf ~".to_string(),
                ":(){ :|:& };:".to_string(), // fork bomb
                "mkfs".to_string(),
                "dd if=/dev/zero".to_string(),
            ],
            max_execution_time_secs: 300,
        }
    }
}

impl SecurityPolicy {
    pub fn is_command_forbidden(&self, command: &str) -> bool {
        let cmd_lower = command.to_lowercase();
        self.forbidden_commands.iter().any(|f| cmd_lower.contains(&f.to_lowercase()))
    }
}
