use odon_core::MessageAttachment;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use tokio::sync::Notify;

/// A user message injected into a running agent turn.
#[derive(Debug, Clone)]
pub struct SteerMessage {
    pub id: String,
    pub text: String,
    pub attachments: Vec<MessageAttachment>,
    /// Priority steers interrupt the current provider stream so the guidance is
    /// applied immediately instead of waiting for the next tool step.
    pub priority: bool,
}

/// Thread-safe steer mailbox shared between the HTTP handlers and the agent
/// loop.
#[derive(Clone, Default)]
pub struct SteerQueue {
    messages: Arc<Mutex<Vec<SteerMessage>>>,
    priority: Arc<AtomicBool>,
    notify: Arc<Notify>,
}

impl SteerQueue {
    pub fn new() -> Self {
        Self::default()
    }

    /// Insert or update a steer by id. Updating an existing steer to priority
    /// wakes the running turn so it can preempt the current stream.
    pub fn push(&self, message: SteerMessage) {
        let mut messages = self.messages.lock().unwrap_or_else(|e| e.into_inner());
        if let Some(existing) = messages.iter_mut().find(|item| item.id == message.id) {
            existing.text = message.text;
            existing.attachments = message.attachments;
            if message.priority {
                existing.priority = true;
                self.priority.store(true, Ordering::SeqCst);
                // notify_one stores a permit when nobody is waiting yet,
                // so the wakeup is not lost in the check-then-wait race.
                self.notify.notify_one();
            }
            return;
        }
        if message.priority {
            self.priority.store(true, Ordering::SeqCst);
        }
        let should_notify = message.priority;
        messages.push(message);
        drop(messages);
        if should_notify {
            self.notify.notify_one();
        }
    }

    /// Remove and return all pending steers.
    pub fn drain(&self) -> Vec<SteerMessage> {
        let mut messages = self.messages.lock().unwrap_or_else(|e| e.into_inner());
        self.priority.store(false, Ordering::SeqCst);
        std::mem::take(&mut *messages)
    }

    pub fn has_pending(&self) -> bool {
        !self
            .messages
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .is_empty()
    }

    /// Wait until at least one steer is marked priority. The priority flag is
    /// checked before waiting, so a notification between polling iterations
    /// cannot be lost.
    pub async fn wait_priority(&self) {
        loop {
            if self.priority.load(Ordering::SeqCst) {
                return;
            }
            self.notify.notified().await;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn upserts_and_drains_steers() {
        let queue = SteerQueue::new();
        queue.push(SteerMessage {
            id: "a".into(),
            text: "first".into(),
            attachments: Vec::new(),
            priority: false,
        });
        queue.push(SteerMessage {
            id: "a".into(),
            text: "updated".into(),
            attachments: Vec::new(),
            priority: true,
        });
        assert!(queue.has_pending());
        let drained = queue.drain();
        assert_eq!(drained.len(), 1);
        assert_eq!(drained[0].text, "updated");
        assert!(drained[0].priority);
        assert!(!queue.has_pending());
    }
}
