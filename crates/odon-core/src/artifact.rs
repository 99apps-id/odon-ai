use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ArtifactKind {
    Code,
    Document,
    Image,
    Patch,
    Automation,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Artifact {
    pub id: String,
    pub session_id: String,
    pub title: String,
    pub kind: ArtifactKind,
    pub summary: String,
    pub payload: ArtifactPayload,
    pub created_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(tag = "type", content = "data", rename_all = "snake_case")]
pub enum ArtifactPayload {
    Code {
        language: String,
        framework: Option<String>,
        entry_file: Option<String>,
        files: Vec<ArtifactFile>,
    },
    Document {
        doc_type: String, // PRD, Architecture, Report, Meeting Notes
        markdown: String,
        size_bytes: usize,
    },
    Image {
        prompt: String,
        width: u32,
        height: u32,
        file_path: Option<String>,
        preview_url: Option<String>,
        size_bytes: usize,
        model: String,
    },
    Patch {
        git_target: String,
        diff: String,
        files_changed: usize,
        additions: usize,
        deletions: usize,
    },
    Automation {
        trigger: String,
        schedule: Option<String>,
        action: String,
        is_active: bool,
    },
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ArtifactFile {
    pub path: String,
    pub content: String,
}

impl Artifact {
    pub fn new_code(
        session_id: impl Into<String>,
        title: impl Into<String>,
        summary: impl Into<String>,
        language: impl Into<String>,
        framework: Option<String>,
        files: Vec<ArtifactFile>,
    ) -> Self {
        Self {
            id: Uuid::new_v4().to_string(),
            session_id: session_id.into(),
            title: title.into(),
            kind: ArtifactKind::Code,
            summary: summary.into(),
            payload: ArtifactPayload::Code {
                language: language.into(),
                framework,
                entry_file: files.first().map(|f| f.path.clone()),
                files,
            },
            created_at: Utc::now(),
        }
    }

    pub fn new_document(
        session_id: impl Into<String>,
        title: impl Into<String>,
        doc_type: impl Into<String>,
        markdown: impl Into<String>,
    ) -> Self {
        let doc_type = doc_type.into();
        let md = markdown.into();
        let bytes = md.len();
        Self {
            id: Uuid::new_v4().to_string(),
            session_id: session_id.into(),
            title: title.into(),
            kind: ArtifactKind::Document,
            summary: format!("{} ({} KB)", doc_type, bytes / 1024),
            payload: ArtifactPayload::Document {
                doc_type,
                markdown: md,
                size_bytes: bytes,
            },
            created_at: Utc::now(),
        }
    }

    pub fn new_image(
        session_id: impl Into<String>,
        title: impl Into<String>,
        prompt: impl Into<String>,
        width: u32,
        height: u32,
        model: impl Into<String>,
        file_path: Option<String>,
        preview_url: Option<String>,
        size_bytes: usize,
    ) -> Self {
        let model = model.into();
        Self {
            id: Uuid::new_v4().to_string(),
            session_id: session_id.into(),
            title: title.into(),
            kind: ArtifactKind::Image,
            summary: format!("{}x{} Image - {}", width, height, model),
            payload: ArtifactPayload::Image {
                prompt: prompt.into(),
                width,
                height,
                file_path,
                preview_url,
                size_bytes,
                model,
            },
            created_at: Utc::now(),
        }
    }

    pub fn new_patch(
        session_id: impl Into<String>,
        title: impl Into<String>,
        git_target: impl Into<String>,
        diff: impl Into<String>,
        files_changed: usize,
        additions: usize,
        deletions: usize,
    ) -> Self {
        Self {
            id: Uuid::new_v4().to_string(),
            session_id: session_id.into(),
            title: title.into(),
            kind: ArtifactKind::Patch,
            summary: format!(
                "+{} -{} lines ({} files)",
                additions, deletions, files_changed
            ),
            payload: ArtifactPayload::Patch {
                git_target: git_target.into(),
                diff: diff.into(),
                files_changed,
                additions,
                deletions,
            },
            created_at: Utc::now(),
        }
    }
}
