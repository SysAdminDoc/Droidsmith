//! Bounded, sanitized capture of a child process's piped output.
//!
//! Shared by the process supervisors ([`crate::scrcpy`], [`crate::gnirehtet`]):
//! a background thread drains a reader into a rolling byte tail, and snapshots
//! are control-character-scrubbed and length-bounded before they are exposed to
//! the renderer. Keeping this in one place means a fix to the capture or
//! redaction logic cannot silently diverge between supervisors.

use std::io::Read;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::{Duration, Instant};

const LOG_CAPTURE_BYTES: usize = 64 * 1024;
const EXPOSED_LOG_CHARS: usize = 16 * 1024;

#[derive(Clone)]
pub(crate) struct CapturedTail {
    bytes: Arc<Mutex<Vec<u8>>>,
    done: Arc<AtomicBool>,
}

impl CapturedTail {
    pub(crate) fn spawn<R>(mut reader: R) -> Self
    where
        R: Read + Send + 'static,
    {
        let capture = Self {
            bytes: Arc::new(Mutex::new(Vec::new())),
            done: Arc::new(AtomicBool::new(false)),
        };
        let bytes = Arc::clone(&capture.bytes);
        let done = Arc::clone(&capture.done);
        thread::spawn(move || {
            let mut buffer = [0_u8; 4096];
            loop {
                match reader.read(&mut buffer) {
                    Ok(0) | Err(_) => break,
                    Ok(count) => {
                        let mut captured = bytes.lock().unwrap_or_else(|error| error.into_inner());
                        append_tail(&mut captured, &buffer[..count], LOG_CAPTURE_BYTES);
                    }
                }
            }
            done.store(true, Ordering::Release);
        });
        capture
    }

    pub(crate) fn snapshot(&self) -> String {
        let bytes = self.bytes.lock().unwrap_or_else(|error| error.into_inner());
        sanitize_log(&String::from_utf8_lossy(&bytes))
    }

    pub(crate) fn wait_for_eof(&self) {
        let started = Instant::now();
        while !self.done.load(Ordering::Acquire) && started.elapsed() < Duration::from_millis(150) {
            thread::sleep(Duration::from_millis(10));
        }
    }
}

fn append_tail(target: &mut Vec<u8>, bytes: &[u8], limit: usize) {
    if bytes.len() >= limit {
        target.clear();
        target.extend_from_slice(&bytes[bytes.len() - limit..]);
        return;
    }
    let overflow = target
        .len()
        .saturating_add(bytes.len())
        .saturating_sub(limit);
    if overflow > 0 {
        target.drain(..overflow);
    }
    target.extend_from_slice(bytes);
}

/// Strip control characters (keeping tab/newline/carriage-return) and bound the
/// exposed length so captured process output is safe to surface in the UI.
pub(crate) fn sanitize_log(value: &str) -> String {
    let mut chars: Vec<char> = value
        .chars()
        .filter(|character| !character.is_control() || matches!(character, '\n' | '\r' | '\t'))
        .collect();
    if chars.len() > EXPOSED_LOG_CHARS {
        chars.drain(..chars.len() - EXPOSED_LOG_CHARS);
    }
    chars.into_iter().collect::<String>().trim().to_string()
}

#[cfg(test)]
mod tests {
    use super::{append_tail, sanitize_log, EXPOSED_LOG_CHARS, LOG_CAPTURE_BYTES};

    #[test]
    fn append_tail_keeps_only_the_trailing_window() {
        let mut buf = Vec::new();
        append_tail(
            &mut buf,
            &vec![b'a'; LOG_CAPTURE_BYTES + 500],
            LOG_CAPTURE_BYTES,
        );
        assert_eq!(buf.len(), LOG_CAPTURE_BYTES);
        // A follow-up write evicts the oldest bytes to stay within the limit.
        append_tail(&mut buf, b"tail", LOG_CAPTURE_BYTES);
        assert_eq!(buf.len(), LOG_CAPTURE_BYTES);
        assert!(buf.ends_with(b"tail"));
    }

    #[test]
    fn sanitize_log_scrubs_control_chars_and_bounds_length() {
        assert_eq!(sanitize_log("a\u{0007}b\tc\n"), "ab\tc");
        let long = "x".repeat(EXPOSED_LOG_CHARS + 100);
        assert_eq!(sanitize_log(&long).chars().count(), EXPOSED_LOG_CHARS);
    }
}
