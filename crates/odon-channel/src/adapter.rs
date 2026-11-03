use async_trait::async_trait;
use odon_core::{AgentEvent, MessageAttachment, Result};
use serde::{Deserialize, Serialize};
use tokio::sync::mpsc;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct IncomingChannelMessage {
    pub channel: String,
    pub sender_id: String,
    pub session_id: Option<String>,
    pub text: String,
    #[serde(default)]
    pub attachments: Vec<MessageAttachment>,
}

#[async_trait]
pub trait ChannelAdapter: Send + Sync {
    fn channel_name(&self) -> &str;

    async fn start(&self, incoming_tx: mpsc::Sender<IncomingChannelMessage>) -> Result<()>;

    async fn broadcast_event(&self, event: &AgentEvent) -> Result<()>;

    async fn send_direct_message(&self, recipient: &str, content: &str) -> Result<()>;
}

const MAX_EXTRACTED_TEXT: usize = 180_000;

/// Buffers a response body while enforcing `limit`, so an oversized or
/// unbounded download is rejected without first loading it all into memory.
pub(crate) async fn read_body_capped(mut resp: reqwest::Response, limit: usize) -> Option<Vec<u8>> {
    // Fail fast when the server declares an oversized body.
    if resp
        .content_length()
        .is_some_and(|declared| declared > limit as u64)
    {
        return None;
    }
    let mut body = Vec::new();
    loop {
        match resp.chunk().await {
            Ok(Some(chunk)) => {
                if body.len() + chunk.len() > limit {
                    return None;
                }
                body.extend_from_slice(&chunk);
            }
            Ok(None) => return Some(body),
            Err(_) => return None,
        }
    }
}

pub fn is_extractable_document(name: &str, mime: &str) -> bool {
    let mime = mime.to_ascii_lowercase();
    if mime == "application/pdf" {
        return true;
    }
    let lower = name.to_ascii_lowercase();
    [
        ".pdf", ".doc", ".docx", ".xls", ".xlsx", ".ppt", ".pptx", ".odt", ".ods", ".odp", ".rtf",
    ]
    .iter()
    .any(|extension| lower.ends_with(extension))
}

/// Best-effort text extraction for documents sent through messaging relays.
/// PDF uses `pdftotext`; Office/ODF/RTF uses `pandoc` first and LibreOffice as
/// a fallback. When no converter is installed the caller keeps the binary
/// attachment and tells the user why text could not be extracted.
pub async fn extract_document_text(
    file_name: &str,
    mime: &str,
    bytes: &[u8],
) -> Option<(String, String)> {
    let lower = file_name.to_ascii_lowercase();
    let extension = if mime.eq_ignore_ascii_case("application/pdf") || lower.ends_with(".pdf") {
        "pdf"
    } else {
        lower.rsplit('.').next().unwrap_or("bin")
    };
    let stem: String = lower
        .rsplit('/')
        .next()
        .unwrap_or("document")
        .trim_end_matches(&format!(".{extension}"))
        .chars()
        .filter(|ch| ch.is_ascii_alphanumeric() || matches!(ch, '-' | '_' | ' '))
        .take(64)
        .collect::<String>();
    let stem = if stem.is_empty() {
        "odon-document".to_string()
    } else {
        stem
    };

    let directory = std::env::temp_dir().join(format!(
        "odon-relay-{}",
        chrono::Utc::now().timestamp_millis()
    ));
    std::fs::create_dir_all(&directory).ok()?;
    let input = directory.join(format!("{stem}.{extension}"));
    let output = directory.join(format!("{stem}.txt"));
    std::fs::write(&input, bytes).ok()?;

    let commands: Vec<(&str, Vec<String>)> = if extension == "pdf" {
        vec![(
            "pdftotext",
            vec![
                "-q".to_string(),
                input.to_string_lossy().to_string(),
                output.to_string_lossy().to_string(),
            ],
        )]
    } else {
        vec![
            (
                "pandoc",
                vec![
                    "--from".to_string(),
                    "auto".to_string(),
                    "--to".to_string(),
                    "plain".to_string(),
                    "--output".to_string(),
                    output.to_string_lossy().to_string(),
                    input.to_string_lossy().to_string(),
                ],
            ),
            (
                "soffice",
                vec![
                    "--headless".to_string(),
                    "--convert-to".to_string(),
                    "txt:Text".to_string(),
                    "--outdir".to_string(),
                    directory.to_string_lossy().to_string(),
                    input.to_string_lossy().to_string(),
                ],
            ),
        ]
    };

    for (program, arguments) in commands {
        let executed = tokio::time::timeout(std::time::Duration::from_secs(45), async {
            tokio::process::Command::new(program)
                .args(&arguments)
                .stdout(std::process::Stdio::null())
                .stderr(std::process::Stdio::null())
                .status()
                .await
        })
        .await
        .ok()?
        .ok()?;
        if !executed.success() {
            continue;
        }
        if let Ok(text) = std::fs::read_to_string(&output) {
            let trimmed = text.trim();
            if !trimmed.is_empty() {
                let extracted: String = if trimmed.len() > MAX_EXTRACTED_TEXT {
                    let mut text = trimmed.chars().take(MAX_EXTRACTED_TEXT).collect::<String>();
                    text.push_str("\n\n[document text truncated]");
                    text
                } else {
                    trimmed.to_string()
                };
                let _ = std::fs::remove_dir_all(&directory);
                return Some((extracted, program.to_string()));
            }
        }
    }

    let _ = std::fs::remove_dir_all(&directory);
    None
}
