use std::collections::VecDeque;

use anyhow::Result;
use bytes::Bytes;
use tokio::sync::mpsc;
use tracing::debug;

pub const LOG_BUFFER_CAPACITY: usize = 1024;

enum LogBufferMessage {
    Append(Bytes),
    Clear,
}

#[derive(Debug, Clone)]
pub struct LogBufferHandle {
    tx: mpsc::UnboundedSender<LogBufferMessage>,
}

impl LogBufferHandle {
    /// Append bytes to the log buffer
    pub fn append(&self, b: Bytes) -> Result<()> {
        self.tx.send(LogBufferMessage::Append(b))?;
        Ok(())
    }

    /// Clear the log buffer
    pub fn clear(&self) -> Result<()> {
        self.tx.send(LogBufferMessage::Clear)?;
        Ok(())
    }
}

#[derive(Debug)]
pub struct LogBuffer {
    buffer: VecDeque<Bytes>,
    rx: mpsc::UnboundedReceiver<LogBufferMessage>,
}

impl LogBuffer {
    /// Create a new buffer with specified minimum capacity
    pub fn new(capacity: usize) -> (Self, LogBufferHandle) {
        let (tx, rx) = mpsc::unbounded_channel();
        let buffer = VecDeque::with_capacity(capacity);

        (Self { buffer, rx }, LogBufferHandle { tx })
    }

    /// Dispatch buffer events forever
    pub async fn dispatch_forever(&mut self) -> Result<()> {
        loop {
            match self.rx.recv().await {
                Some(LogBufferMessage::Clear) => {
                    self.buffer.clear();
                    debug!("cleared current log buffer");
                }
                Some(LogBufferMessage::Append(b)) => {
                    self.buffer.push_back(b);
                }
                None => continue,
            }
        }
    }
}
