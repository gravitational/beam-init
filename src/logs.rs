use std::collections::VecDeque;
use std::io;
use std::os::fd::OwnedFd;
use std::sync::Arc;

use futures_core::Stream;
use tokio::io::{AsyncBufReadExt, BufReader};
use tokio::net::unix::pipe;
use tokio::sync::{Mutex, Notify};
use tokio::task::AbortHandle;

const LOG_COUNT: usize = 100;

/// A log store with support for multiple async readers and a single pipe writer.
#[derive(Debug)]
pub struct Logs {
    entries: Arc<Mutex<RingBuffer>>,
    next_entry: Arc<Notify>,
    abort_handle: Option<AbortHandle>,
}

impl Logs {
    pub fn new() -> Self {
        Self {
            entries: Arc::new(Mutex::new(RingBuffer::default())),
            next_entry: Arc::new(Notify::new()),
            abort_handle: None,
        }
    }

    pub fn new_writer(&mut self) -> io::Result<OwnedFd> {
        let (tx, rx) = pipe::pipe()?;
        let entries = Arc::clone(&self.entries);
        let next_entry = Arc::clone(&self.next_entry);

        if let Some(handle) = self.abort_handle.take() {
            handle.abort();
        }

        let handle = tokio::spawn(async move {
            let reader = BufReader::new(rx);
            let mut lines = reader.split(b'\n');
            while let Some(line) = lines.next_segment().await.unwrap() {
                let line = String::from_utf8_lossy(&line).into_owned();
                entries.lock().await.push(line);
                next_entry.notify_waiters();
            }

            entries.lock().await.push("[log stream closed]".to_owned());
        });
        self.abort_handle = Some(handle.abort_handle());

        Ok(tx.into_blocking_fd().unwrap())
    }

    pub fn new_reader(&self) -> impl Stream<Item = String> + 'static {
        let entries = Arc::clone(&self.entries);
        let mut reader = RingBufferReader(0);
        let next_entry = Arc::clone(&self.next_entry);

        async_stream::stream! {
            let mut next_entry_notified = next_entry.notified();

            loop {
                match entries.lock().await.get(&mut reader) {
                    RingBufferEntry::Line(line) => {
                        yield line;
                        continue;
                    }
                    RingBufferEntry::Lost => {
                        yield "[log entries lost]".to_owned();
                        continue;
                    }
                    RingBufferEntry::Empty => {},
                }

                next_entry_notified.await;
                next_entry_notified = next_entry.notified();
            }
        }
    }

    pub async fn copy_logs(&self) -> Vec<String> {
        self.entries.lock().await.entries.iter().cloned().collect()
    }
}

impl Drop for Logs {
    fn drop(&mut self) {
        if let Some(handle) = self.abort_handle.take() {
            handle.abort();
        }
    }
}

/// A sync ring buffer for log entries.
///
/// This ring buffer emulates a list with the first `next_idx - entries.len()`
/// entries being evicted from memory. This way readers can refer to entries
/// using a stable index.
///
/// ```plain
/// xxxx <---->
/// ^^^^ ^^^^^^ ^
/// |    |      next_idx
/// |    entries in the VecDeque
/// first `next_idx - entries.len()` entries are gone
/// ```
#[derive(Debug, Default)]
struct RingBuffer {
    entries: VecDeque<String>,
    next_idx: u64,
}

impl RingBuffer {
    fn push(&mut self, line: String) {
        self.entries.push_back(line);
        while self.entries.len() > LOG_COUNT {
            self.entries.pop_front();
        }
        self.next_idx += 1;
    }

    fn get(&self, reader: &mut RingBufferReader) -> RingBufferEntry {
        let first_idx = self.next_idx - self.entries.len() as u64;

        // FIXME handle wrap
        if reader.0 >= self.next_idx {
            // xxxx <---->
            //             ^
            //             next_idx
            //             reader
            debug_assert_eq!(reader.0, self.next_idx);
            RingBufferEntry::Empty
        } else if reader.0 < first_idx {
            // xxxx <---->
            //   ^         ^
            //   |         next_idx
            //   reader
            reader.0 = first_idx;
            RingBufferEntry::Lost
        } else {
            // xxxx <---->
            //         ^   ^
            //         |   next_idx
            //         reader
            let line = self.entries[usize::try_from(reader.0 - first_idx).unwrap()].clone();
            reader.0 += 1;
            RingBufferEntry::Line(line)
        }
    }
}

struct RingBufferReader(u64);

enum RingBufferEntry {
    Line(String),

    /// One or more log entries were removed from the ring buffer before we had
    /// a chance to read them.
    Lost,

    /// No new entries have been added after the reader position.
    Empty,
}
