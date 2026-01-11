//! Output capture utility for TUI
//!
//! Captures stdout/stderr output during planner execution
//! and routes it to the trace timeline.

use std::io::{self, Write};
use std::sync::{Arc, Mutex};

/// A captured output line with metadata
#[derive(Debug, Clone)]
pub struct CapturedLine {
    pub content: String,
    pub is_err: bool,
}

/// Buffer that collects output for later display in TUI
#[derive(Debug, Clone, Default)]
pub struct OutputBuffer {
    lines: Arc<Mutex<Vec<CapturedLine>>>,
}

impl OutputBuffer {
    pub fn new() -> Self {
        Self {
            lines: Arc::new(Mutex::new(Vec::new())),
        }
    }

    /// Add a line to the buffer
    pub fn push(&self, content: String, is_err: bool) {
        if let Ok(mut lines) = self.lines.lock() {
            lines.push(CapturedLine { content, is_err });
        }
    }

    /// Drain all captured lines
    pub fn drain(&self) -> Vec<CapturedLine> {
        if let Ok(mut lines) = self.lines.lock() {
            std::mem::take(&mut *lines)
        } else {
            Vec::new()
        }
    }

    /// Check if there are pending lines
    pub fn has_pending(&self) -> bool {
        if let Ok(lines) = self.lines.lock() {
            !lines.is_empty()
        } else {
            false
        }
    }

    /// Get pending count
    pub fn pending_count(&self) -> usize {
        if let Ok(lines) = self.lines.lock() {
            lines.len()
        } else {
            0
        }
    }
}

/// Writer that routes output to the capture buffer
pub struct CaptureWriter {
    buffer: OutputBuffer,
    is_err: bool,
}

impl CaptureWriter {
    pub fn new(buffer: OutputBuffer, is_err: bool) -> Self {
        Self { buffer, is_err }
    }
}

impl Write for CaptureWriter {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        if let Ok(s) = std::str::from_utf8(buf) {
            for line in s.lines() {
                if !line.trim().is_empty() {
                    self.buffer.push(line.to_string(), self.is_err);
                }
            }
        }
        Ok(buf.len())
    }

    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}

// Thread-local capture buffer for discovery output
thread_local! {
    static CAPTURE_BUFFER: std::cell::RefCell<Option<OutputBuffer>> = const { std::cell::RefCell::new(None) };
}

/// Set the current capture buffer for this thread
pub fn set_capture_buffer(buffer: Option<OutputBuffer>) {
    CAPTURE_BUFFER.with(|b| {
        *b.borrow_mut() = buffer;
    });
}

/// Get the current capture buffer, if any
pub fn get_capture_buffer() -> Option<OutputBuffer> {
    CAPTURE_BUFFER.with(|b| b.borrow().clone())
}

/// Write to the capture buffer if one is active
/// Returns true if captured, false if no buffer is active
pub fn capture_print(msg: &str) -> bool {
    CAPTURE_BUFFER.with(|b| {
        if let Some(ref buffer) = *b.borrow() {
            buffer.push(msg.to_string(), false);
            true
        } else {
            false
        }
    })
}

/// Write error to the capture buffer if one is active
pub fn capture_eprint(msg: &str) -> bool {
    CAPTURE_BUFFER.with(|b| {
        if let Some(ref buffer) = *b.borrow() {
            buffer.push(msg.to_string(), true);
            true
        } else {
            false
        }
    })
}

/// Macro to print to capture buffer OR stdout
#[macro_export]
macro_rules! tui_print {
    ($($arg:tt)*) => {{
        let msg = format!($($arg)*);
        if !$crate::tui::capture::capture_print(&msg) {
            println!("{}", msg);
        }
    }};
}

/// Macro to eprint to capture buffer OR stderr
#[macro_export]
macro_rules! tui_eprint {
    ($($arg:tt)*) => {{
        let msg = format!($($arg)*);
        if !$crate::tui::capture::capture_eprint(&msg) {
            eprintln!("{}", msg);
        }
    }};
}
