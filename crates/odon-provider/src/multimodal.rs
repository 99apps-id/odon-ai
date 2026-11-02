//! Builders that translate stored attachments into each provider's native
//! multimodal envelope. Attachments are stored as raw base64 so they can be
//! replayed on every turn of a conversation.

use odon_core::{MessageAttachment, MessageContent};
use serde_json::{json, Value};

pub fn is_image(mime: &str) -> bool {
    mime.starts_with("image/")
}

pub fn is_pdf(mime: &str) -> bool {
    mime == "application/pdf"
}

pub fn data_url(att: &MessageAttachment) -> String {
    format!(
        "data:{};base64,{}",
        if att.mime_type.is_empty() {
            "application/octet-stream"
        } else {
            &att.mime_type
        },
        att.data_base64
    )
}

/// OpenAI Chat Completions content. Text-only messages stay a plain string so
/// gateways that reject content arrays keep working.
pub fn openai_content(content: &MessageContent) -> Value {
    let mut parts = Vec::new();
    if !content.text.is_empty() {
        parts.push(json!({ "type": "text", "text": content.text }));
    }
    for att in &content.attachments {
        if is_image(&att.mime_type) {
            parts.push(json!({
                "type": "image_url",
                "image_url": { "url": data_url(att) }
            }));
        } else if is_pdf(&att.mime_type) {
            parts.push(json!({
                "type": "file",
                "file": { "filename": att.name, "file_data": data_url(att) }
            }));
        }
    }
    match parts.len() {
        0 => json!(""),
        1 if parts[0].get("type").and_then(|t| t.as_str()) == Some("text") => {
            parts[0]["text"].clone()
        }
        _ => json!(parts),
    }
}

/// Anthropic messages content blocks (text, image, document).
pub fn anthropic_blocks(content: &MessageContent) -> Vec<Value> {
    let mut blocks = Vec::new();
    if !content.text.is_empty() {
        blocks.push(json!({ "type": "text", "text": content.text }));
    }
    for att in &content.attachments {
        if is_image(&att.mime_type) {
            blocks.push(json!({
                "type": "image",
                "source": {
                    "type": "base64",
                    "media_type": att.mime_type,
                    "data": att.data_base64
                }
            }));
        } else if is_pdf(&att.mime_type) {
            blocks.push(json!({
                "type": "document",
                "source": {
                    "type": "base64",
                    "media_type": "application/pdf",
                    "data": att.data_base64
                }
            }));
        }
    }
    blocks
}

/// Gemini `parts` array (inlineData supports images and PDFs).
pub fn gemini_parts(content: &MessageContent) -> Vec<Value> {
    let mut parts = Vec::new();
    if !content.text.is_empty() {
        parts.push(json!({ "text": content.text }));
    }
    for att in &content.attachments {
        if is_image(&att.mime_type) || is_pdf(&att.mime_type) {
            parts.push(json!({
                "inlineData": {
                    "mimeType": att.mime_type,
                    "data": att.data_base64
                }
            }));
        }
    }
    if parts.is_empty() {
        parts.push(json!({ "text": "" }));
    }
    parts
}

/// OpenAI Responses API (`/responses`) input content parts.
pub fn responses_content(content: &MessageContent) -> Value {
    let mut parts = Vec::new();
    if !content.text.is_empty() {
        parts.push(json!({ "type": "input_text", "text": content.text }));
    }
    for att in &content.attachments {
        if is_image(&att.mime_type) {
            parts.push(json!({
                "type": "input_image",
                "image_url": data_url(att)
            }));
        } else if is_pdf(&att.mime_type) {
            parts.push(json!({
                "type": "input_file",
                "filename": att.name,
                "file_data": data_url(att)
            }));
        }
    }
    if parts.len() == 1 && parts[0].get("type").and_then(|t| t.as_str()) == Some("input_text") {
        parts[0]["text"].clone()
    } else {
        json!(parts)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use odon_core::{MessageAttachment, MessageContent};

    fn content(text: &str, attachments: Vec<MessageAttachment>) -> MessageContent {
        MessageContent {
            text: text.to_string(),
            thought: None,
            tool_calls: Vec::new(),
            artifact_ids: Vec::new(),
            action_chips: Vec::new(),
            attachments,
        }
    }

    fn image() -> MessageAttachment {
        MessageAttachment {
            name: "shot.png".into(),
            mime_type: "image/png".into(),
            data_base64: "aGVsbG8=".into(),
        }
    }

    fn pdf() -> MessageAttachment {
        MessageAttachment {
            name: "spec.pdf".into(),
            mime_type: "application/pdf".into(),
            data_base64: "JVBERi0=".into(),
        }
    }

    #[test]
    fn text_only_stays_a_plain_string() {
        let value = openai_content(&content("hello", Vec::new()));
        assert_eq!(value, json!("hello"));
    }

    #[test]
    fn openai_builds_image_and_file_parts() {
        let value = openai_content(&content("look", vec![image(), pdf()]));
        let parts = value.as_array().unwrap();
        assert_eq!(parts[0]["type"], "text");
        assert_eq!(parts[1]["type"], "image_url");
        assert_eq!(parts[1]["image_url"]["url"], "data:image/png;base64,aGVsbG8=");
        assert_eq!(parts[2]["type"], "file");
        assert_eq!(parts[2]["file"]["filename"], "spec.pdf");
    }

    #[test]
    fn anthropic_uses_native_image_and_document_blocks() {
        let blocks = anthropic_blocks(&content("read", vec![image(), pdf()]));
        assert_eq!(blocks[1]["type"], "image");
        assert_eq!(blocks[2]["type"], "document");
        assert_eq!(blocks[2]["source"]["media_type"], "application/pdf");
    }

    #[test]
    fn gemini_uses_inline_data() {
        let parts = gemini_parts(&content("read", vec![image(), pdf()]));
        assert_eq!(parts[1]["inlineData"]["mimeType"], "image/png");
        assert_eq!(parts[2]["inlineData"]["mimeType"], "application/pdf");
    }

    #[test]
    fn responses_api_uses_input_parts() {
        let value = responses_content(&content("look", vec![image()]));
        let parts = value.as_array().unwrap();
        assert_eq!(parts[1]["type"], "input_image");
    }
}
