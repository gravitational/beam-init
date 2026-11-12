use std::collections::VecDeque;
use std::io;
use std::num::NonZeroUsize;
use std::os::fd::OwnedFd;
use std::sync::Arc;

use futures_core::Stream;
use tokio::io::{AsyncBufReadExt, BufReader};
use tokio::net::unix::pipe;
use tokio::sync::{Mutex, Notify};
use tokio::task::AbortHandle;

/// A log store with support for multiple async readers and a single pipe writer.
#[derive(Debug)]
pub struct Logs {
    pub(crate) queue: Arc<AsyncRingBuffer>,
    abort_handle: Option<AbortHandle>,
    max_width: NonZeroUsize,
}

#[derive(Debug)]
pub struct AsyncRingBuffer {
    entries: Mutex<RingBuffer>,
    next_entry: Notify,
}

impl AsyncRingBuffer {
    pub fn new(capacity: NonZeroUsize) -> Self {
        Self {
            entries: Mutex::new(RingBuffer::new(capacity)),
            next_entry: Notify::new(),
        }
    }

    pub async fn push(&self, line: String) {
        self.entries.lock().await.push(line);
        self.next_entry.notify_waiters();
    }
}

impl Logs {
    pub fn new(capacity: NonZeroUsize, max_width: NonZeroUsize) -> Self {
        Self {
            queue: Arc::new(AsyncRingBuffer::new(capacity)),
            max_width,
            abort_handle: None,
        }
    }

    pub fn new_writer(&mut self) -> io::Result<OwnedFd> {
        let (tx, rx) = pipe::pipe()?;
        let queue = Arc::clone(&self.queue);
        let max_width = self.max_width.into();

        // Ensure we only have a single writer. There should be a single log
        // pipe per service. If a service passes the log pipe to another process
        // before it is stopped we would otherwise get logs from an old instance
        // of the service interleaved with a new instance if we start it again.
        if let Some(handle) = self.abort_handle.take() {
            handle.abort();
        }

        let handle = tokio::spawn(async move {
            let reader = BufReader::new(rx);
            let mut lines = reader.bounded_split(b'\n', max_width);
            while let Some(line) = lines
                .next_segment()
                .await
                .expect("failed to read from pipe")
            {
                let line = String::from_utf8_lossy_owned(line);
                let line = beam_init::sanitize_with_whitespace(line);
                queue.push(line).await;
            }

            queue.push("[log stream closed]".to_owned()).await;
        });
        self.abort_handle = Some(handle.abort_handle());

        Ok(tx
            .into_blocking_fd()
            .expect("failed to convert pipe to blocking mode"))
    }

    pub fn new_reader(&self) -> impl Stream<Item = String> + 'static {
        let mut reader = RingBufferReader(0);
        let queue = Arc::clone(&self.queue);

        async_stream::stream! {
            let mut next_entry_notified = queue.next_entry.notified();

            loop {
                let entry = queue.entries.lock().await.get(&mut reader);
                // Do not inline the above to ensure the lock is not held over yield points.
                match entry {
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
                next_entry_notified = queue.next_entry.notified();
            }
        }
    }

    pub async fn copy_logs(&self) -> Vec<String> {
        let entries = &self.queue.entries.lock().await.entries;
        entries.iter().cloned().collect()
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
#[derive(Debug)]
struct RingBuffer {
    entries: VecDeque<String>,
    next_idx: u64,
    capacity: NonZeroUsize,
}

impl RingBuffer {
    fn new(capacity: NonZeroUsize) -> Self {
        Self {
            entries: VecDeque::new(),
            next_idx: 0,
            capacity,
        }
    }

    fn push(&mut self, line: String) {
        self.entries.push_back(line);
        while self.entries.len() > self.capacity.get() {
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
            let entry =
                usize::try_from(reader.0 - first_idx).expect("overflow should be handled above");
            let line = self.entries[entry].clone();
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

trait AsyncBufReadSplitExt: Sized {
    fn bounded_split(self, delim: u8, limit: usize) -> BoundedSplit<Self> {
        BoundedSplit {
            reader: self,
            delim,
            limit,
        }
    }
}

impl<T: AsyncBufReadExt> AsyncBufReadSplitExt for T {}

struct BoundedSplit<R> {
    reader: R,
    delim: u8,
    limit: usize,
}

impl<R: AsyncBufReadExt + Unpin> BoundedSplit<R> {
    pub async fn next_segment(&mut self) -> tokio::io::Result<Option<Vec<u8>>> {
        use std::cmp::min;
        let mut vec = Vec::new();

        loop {
            let buf = match self.reader.fill_buf().await {
                Ok(buf) => buf,
                Err(e) if e.kind() == io::ErrorKind::Interrupted => continue,
                Err(e) => Err(e)?,
            };
            let buf_len = buf.len();

            // To cut down on a lot of case analysis, we flatten to one piece of
            // info which is simply "how many bytes to consume".
            // num_bytes == buf.len() indicates no delimiter has been found;
            // num_bytes == 0 indicates EOF
            let num_bytes = buf.iter().position(|b| *b == self.delim).unwrap_or(buf_len);

            vec.extend_from_slice(&buf[..min(num_bytes, self.limit - vec.len())]);

            // we skip one extra to include the delimiter
            self.reader.consume(min(buf_len, num_bytes + 1));

            if buf_len == 0 {
                if vec.is_empty() {
                    return Ok(None);
                } else {
                    return Ok(Some(vec));
                }
            } else if num_bytes < buf_len {
                return Ok(Some(vec));
            }
        }
    }
}

#[cfg(test)]
mod tests {
    #[tokio::test]
    async fn bounded_split_is_compatible_with_split() {
        use super::AsyncBufReadSplitExt;
        use std::io::Cursor;
        use tokio::io::{AsyncBufReadExt, BufReader};

        for test in ["", "hello", "hello-world", "-hello-world", "hello-world-"] {
            let mut r1 = BufReader::new(Cursor::new(test.as_bytes())).bounded_split(b'-', 255);
            let mut r2 = BufReader::new(Cursor::new(test.as_bytes())).split(b'-');
            loop {
                let a = r1.next_segment().await.unwrap();
                let b = r2.next_segment().await.unwrap();
                assert_eq!(a, b);
                if a.is_none() {
                    break;
                }
            }
        }
    }

    #[tokio::test]
    async fn bounded_split_bounds_result() {
        use super::AsyncBufReadSplitExt;
        use std::io::Cursor;
        use tokio::io::BufReader;

        let mut r1 = BufReader::new(Cursor::new(b"a-baa-xxx")).bounded_split(b'-', 2);
        assert_eq!(r1.next_segment().await.unwrap().as_deref(), Some(&b"a"[..]));
        assert_eq!(
            r1.next_segment().await.unwrap().as_deref(),
            Some(&b"ba"[..])
        );
        assert_eq!(
            r1.next_segment().await.unwrap().as_deref(),
            Some(&b"xx"[..])
        );
    }
}
