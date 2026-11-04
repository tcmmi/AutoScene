//! A plugin's stdout and stderr: each line goes to its log, tagged
//! `[stdout]` or `[stderr]`, as a program's output does.
//!
//! A write is always taken whole: what the log has no room for is dropped
//! there ([`PluginLog`]), so a plugin that floods its output never waits
//! and never fails on it. A line is kept to [`MAX_LINE_BYTES`] and marked
//! cut past it, whatever its encoding.

use std::io;
use std::pin::Pin;
use std::sync::Arc;
use std::task::{Context, Poll};

use parking_lot::Mutex;
use tokio::io::AsyncWrite;
use wasmtime_wasi::cli::{IsTerminal, StdoutStream};

use crate::log::PluginLog;
use crate::runtime::{MAX_LINE_BYTES, decoded, marked};

/// One of a run's output streams, as WASI asks for it.
pub(crate) struct LogStream {
    log: Arc<Mutex<PluginLog>>,
    /// `stdout` or `stderr`.
    tag: &'static str,
}

impl LogStream {
    pub(crate) fn stdout(log: &Arc<Mutex<PluginLog>>) -> Self {
        Self { log: log.clone(), tag: "stdout" }
    }

    pub(crate) fn stderr(log: &Arc<Mutex<PluginLog>>) -> Self {
        Self { log: log.clone(), tag: "stderr" }
    }
}

impl IsTerminal for LogStream {
    fn is_terminal(&self) -> bool {
        false
    }
}

impl StdoutStream for LogStream {
    fn async_stream(&self) -> Box<dyn AsyncWrite + Send + Sync> {
        Box::new(LineWriter { log: self.log.clone(), tag: self.tag, line: Vec::new(), cut: false })
    }
}

/// Writes what it is given to the log line by line; the last line, if it
/// has no line end, when it is shut down or dropped.
struct LineWriter {
    log: Arc<Mutex<PluginLog>>,
    tag: &'static str,
    /// The line so far, at most [`MAX_LINE_BYTES`] and a byte (for a `\r`).
    line: Vec<u8>,
    /// The line had more than that.
    cut: bool,
}

impl LineWriter {
    fn take(&mut self, mut bytes: &[u8]) {
        while !bytes.is_empty() {
            let (len, ends) = match bytes.iter().position(|&b| b == b'\n') {
                Some(i) => (i, true),
                None => (bytes.len(), false),
            };
            let room = MAX_LINE_BYTES + 1 - self.line.len();
            self.cut |= len > room;
            self.line.extend_from_slice(&bytes[..len.min(room)]);
            bytes = &bytes[len + usize::from(ends)..];
            if ends {
                self.end_line();
            }
        }
    }

    fn end_line(&mut self) {
        let (line, cut) = decoded(std::mem::take(&mut self.line), std::mem::take(&mut self.cut), MAX_LINE_BYTES);
        self.log.lock().line(self.tag, &marked(line, cut));
    }

    fn flush_partial(&mut self) {
        if !self.line.is_empty() || self.cut {
            self.end_line();
        }
    }
}

impl AsyncWrite for LineWriter {
    fn poll_write(self: Pin<&mut Self>, _: &mut Context<'_>, buf: &[u8]) -> Poll<io::Result<usize>> {
        self.get_mut().take(buf);
        Poll::Ready(Ok(buf.len()))
    }

    fn poll_flush(self: Pin<&mut Self>, _: &mut Context<'_>) -> Poll<io::Result<()>> {
        Poll::Ready(Ok(()))
    }

    fn poll_shutdown(self: Pin<&mut Self>, _: &mut Context<'_>) -> Poll<io::Result<()>> {
        self.get_mut().flush_partial();
        Poll::Ready(Ok(()))
    }
}

impl Drop for LineWriter {
    fn drop(&mut self) {
        self.flush_partial();
    }
}

#[cfg(test)]
mod tests {
    use std::fs;

    use super::*;

    fn writer(dir: &std::path::Path) -> (LineWriter, std::path::PathBuf) {
        let path = dir.join("plugin.log");
        let log = Arc::new(Mutex::new(PluginLog::open(&path, "run-1", "post").unwrap()));
        (LineWriter { log, tag: "stdout", line: Vec::new(), cut: false }, path)
    }

    #[test]
    fn output_reaches_the_log_line_by_line() {
        let dir = tempfile::tempdir().unwrap();
        let (mut w, path) = writer(dir.path());
        w.take(b"first li");
        w.take(b"ne\r\nsecond\n\xffthird");
        drop(w);
        let text = fs::read_to_string(path).unwrap();
        let lines: Vec<&str> = text.lines().skip(1).collect();
        assert_eq!(lines, ["[stdout] first line", "[stdout] second", "[stdout] \u{FFFD}third"]);
    }

    #[test]
    fn a_long_line_is_cut_and_marked() {
        let dir = tempfile::tempdir().unwrap();
        let (mut w, path) = writer(dir.path());
        let long = vec![b'x'; MAX_LINE_BYTES * 3];
        w.take(&long);
        w.take(b"\nnext\n");
        drop(w);
        let text = fs::read_to_string(path).unwrap();
        let lines: Vec<&str> = text.lines().skip(1).collect();
        assert_eq!(lines.len(), 2);
        assert_eq!(lines[0], format!("[stdout] {} [cut]", "x".repeat(MAX_LINE_BYTES)));
        assert_eq!(lines[1], "[stdout] next");
    }
}
