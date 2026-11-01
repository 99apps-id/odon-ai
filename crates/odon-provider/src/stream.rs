use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", content = "data", rename_all = "snake_case")]
pub enum StreamEvent {
    TextDelta(String),
    ThoughtDelta(String),
    ToolCallDelta {
        index: usize,
        id: Option<String>,
        name: Option<String>,
        arguments_delta: Option<String>,
    },
    Usage {
        prompt_tokens: u32,
        completion_tokens: u32,
    },
    Done,
    Error(String),
}

pub type StreamConsumer = Box<dyn Fn(StreamEvent) -> std::pin::Pin<Box<dyn std::future::Future<Output = ()> + Send>> + Send + Sync>;
