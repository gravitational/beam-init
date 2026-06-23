use std::collections::VecDeque;
use std::io;
use std::os::fd::OwnedFd;
use std::sync::Arc;

use futures_core::Stream;
use tokio::io::{AsyncBufReadExt, BufReader};
use tokio::net::unix::pipe;
use tokio::sync::{Mutex, Notify};
use tokio::task::AbortHandle;

/// The number of log entries kept in the log buffer.
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

        // Ensure we only have a single writer. There should be a single log
        // pipe per service. If a service passes the log pipe to another process
        // before it is stopped we would otherwise get logs from an old instance
        // of the service interleaved with a new instance if we start it again.
        if let Some(handle) = self.abort_handle.take() {
            handle.abort();
        }

        let handle = tokio::spawn(async move {
            let reader = BufReader::new(rx);
            let mut lines = reader.split(b'\n');
            while let Some(line) = lines
                .next_segment()
                .await
                .expect("failed to read from pipe")
            {
                let line = sanitize(line);
                entries.lock().await.push(line);
                next_entry.notify_waiters();
            }

            entries.lock().await.push("[log stream closed]".to_owned());
        });
        self.abort_handle = Some(handle.abort_handle());

        Ok(tx
            .into_blocking_fd()
            .expect("failed to convert pipe to blocking mode"))
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

/// Convert the input to UTF-8 and replace Control codes
fn sanitize(line: Vec<u8>) -> String {
    // Micro-optimization: convert the input without an additional allocation if not absolutely necessary,
    // which it typically won't be.
    // NOTE: If https://doc.rust-lang.org/stable/std/string/struct.String.html#method.from_utf8_lossy_owned gets
    // stabilized, that could replace this construction.
    let line = String::from_utf8(line)
        .unwrap_or_else(|err| String::from_utf8_lossy(&err.into_bytes()).into_owned());

    let is_filtered_control = |ch: char| ch.is_control() && !"\n\t".contains(ch);

    if !line.chars().any(is_filtered_control) {
        return line;
    }

    // Replace control characters (with the exception of TAB and newline)
    line.chars()
        .map(|ch| {
            if is_filtered_control(ch) {
                if ch < '\u{20}' {
                    // This is a CC0 control code, replace it with the corresponding Control Picture,
                    // which is U+2400 + the ASCII code; i.e. '\a' (U+0007) => '␇' (U+2407).
                    //
                    // PANIC: It is not possible for this conversion to fail
                    char::from_u32(u32::from(ch) + 0x2400).unwrap()
                } else {
                    // For CC1 control codes, pictures don't exist so replace with a more generic
                    // replacement.
                    '⍰'
                }
            } else {
                ch
            }
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::sanitize;

    #[test]
    fn log_sanitizer() {
        let hello = b"hello".to_vec();
        let p = hello.as_ptr();
        let hello = sanitize(hello);
        let q = hello.as_ptr();
        assert_eq!(p, q);

        assert_eq!(sanitize(b"hello".to_vec()), "hello");
        assert_eq!(sanitize(b"he\tllo".to_vec()), "he\tllo");
        assert_eq!(sanitize(b"he\nllo".to_vec()), "he\nllo");
        assert_eq!(sanitize(b"he\rllo".to_vec()), "he\u{240D}llo");
        assert_eq!(sanitize(b"he\n\x7Fllo".to_vec()), "he\n⍰llo");
        assert_eq!(sanitize(b"he\t\rllo".to_vec()), "he\t\u{240D}llo");
        assert_eq!(sanitize(b"he\x7Fllo".to_vec()), "he⍰llo");
        assert_eq!(sanitize(b"he\xA0llo".to_vec()), "he\u{FFFD}llo");
    }
}
