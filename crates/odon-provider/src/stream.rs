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
    /// The provider stopped because the output budget ran out, not because the
    /// answer was finished. Emitted before `Done` so the runtime can continue
    /// the turn instead of treating a cut answer as a complete one.
    OutputTruncated,
    Done,
    Error(String),
}

pub type StreamConsumer = Box<
    dyn Fn(StreamEvent) -> std::pin::Pin<Box<dyn std::future::Future<Output = ()> + Send>>
        + Send
        + Sync,
>;
