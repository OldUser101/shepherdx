use std::{collections::VecDeque, sync::Arc};

use anyhow::Result;
use bytes::Bytes;
use tokio::sync::{Mutex, mpsc};
use tracing::debug;

pub const LOG_BUFFER_CAPACITY: usize = 1024;

enum LogBufferMessage {
    Append(Bytes),
    Clear,
}

#[derive(Debug, Clone)]
pub struct LogBufferHandle {
    buffer: Arc<Mutex<VecDeque<Bytes>>>,
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

    /// Acquire a copy of the current buffer contents
    pub async fn current_log(&self) -> Vec<Bytes> {
        let mut v = Vec::new();

        for b in self.buffer.lock().await.iter() {
            v.push(b.clone());
        }

        v
    }
}

#[derive(Debug)]
pub struct LogBuffer {
    buffer: Arc<Mutex<VecDeque<Bytes>>>,
    rx: mpsc::UnboundedReceiver<LogBufferMessage>,
}

impl LogBuffer {
    /// Create a new buffer for log storage
    pub fn new(capacity: usize) -> (Self, LogBufferHandle) {
        let (tx, rx) = mpsc::unbounded_channel();
        let buffer = Arc::new(Mutex::new(VecDeque::with_capacity(capacity)));

        (
            Self {
                buffer: buffer.clone(),
                rx,
            },
            LogBufferHandle { buffer, tx },
        )
    }

    /// Dispatch buffer events forever
    pub async fn dispatch_forever(&mut self) -> Result<()> {
        loop {
            match self.rx.recv().await {
                Some(LogBufferMessage::Clear) => {
                    self.buffer.lock().await.clear();
                    debug!("cleared current log buffer");
                }
                Some(LogBufferMessage::Append(b)) => {
                    self.buffer.lock().await.push_back(b);
                }
                None => continue,
            }
        }
    }
}
