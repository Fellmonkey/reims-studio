//! Log ring buffer and file rotation for the diagnostics panel.
//!
//! # Quick start
//!
//! ```
//! use reims_core::log::LogBuffer;
//!
//! let mut buffer = LogBuffer::new(128);
//! buffer.push("stamp wait");
//! buffer.push("stamp wait");
//! assert_eq!(buffer.page(0, 8), "stamp wait (x2)");
//! ```

use std::ffi::OsString;
use std::fs::File;
use std::io::ErrorKind;
use std::path::{Path, PathBuf};

use thiserror::Error;

/// Default number of retained logical lines.
pub const DEFAULT_CAPACITY: usize = 256;
/// Maximum size of one log file before rotation.
pub const MAX_LOG_BYTES: u64 = 25 * 1024 * 1024;
/// Number of rotated files to keep.
pub const KEEP_ROTATED: usize = 5;

#[derive(Debug, Clone, PartialEq, Eq)]
struct LogEntry {
    text: String,
    repeats: usize,
}

/// A bounded ring buffer that collapses consecutive duplicate lines.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LogBuffer {
    entries: std::collections::VecDeque<LogEntry>,
    capacity: usize,
}

impl LogBuffer {
    /// Creates an empty buffer that keeps at most `capacity` logical lines.
    pub fn new(capacity: usize) -> Self {
        Self {
            entries: std::collections::VecDeque::new(),
            capacity: capacity.max(1),
        }
    }

    /// Appends a line, merging it into the previous entry when identical.
    pub fn push(&mut self, line: impl Into<String>) {
        let line = line.into();
        if let Some(last) = self.entries.back_mut()
            && last.text == line
        {
            last.repeats += 1;
            return;
        }
        if self.entries.len() == self.capacity {
            self.entries.pop_front();
        }
        self.entries.push_back(LogEntry {
            text: line,
            repeats: 1,
        });
    }

    /// Returns the number of logical lines in the buffer.
    pub fn len(&self) -> usize {
        self.entries.len()
    }

    /// Returns true when the buffer is empty.
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    /// Renders `count` lines starting at `start` as one text block.
    ///
    /// The returned string is intended for a single `iced::widget::text`
    /// without `scrollable` or clipping. Duplicates are rendered as
    /// `<line> (xN)`.
    pub fn page(&self, start: usize, count: usize) -> String {
        self.entries
            .iter()
            .skip(start)
            .take(count)
            .map(|entry| {
                if entry.repeats == 1 {
                    entry.text.clone()
                } else {
                    format!("{} (x{})", entry.text, entry.repeats)
                }
            })
            .collect::<Vec<_>>()
            .join("\n")
    }

    /// Removes all buffered lines.
    pub fn clear(&mut self) {
        self.entries.clear();
    }
}

impl Default for LogBuffer {
    fn default() -> Self {
        Self::new(DEFAULT_CAPACITY)
    }
}

/// Failures while rotating a log file.
#[derive(Debug, Error)]
pub enum LogError {
    /// A file operation failed.
    #[error("log rotation failed for {path}: {source}")]
    Io {
        /// Path that failed.
        path: String,
        /// Underlying I/O failure.
        #[source]
        source: std::io::Error,
    },
    /// `keep` was zero.
    #[error("log rotation must keep at least one file")]
    NoRetention,
}

fn numbered(path: &Path, index: usize) -> PathBuf {
    let mut name = OsString::from(path.as_os_str());
    name.push(format!(".{index}"));
    PathBuf::from(name)
}

fn rotate_error(path: &Path, source: std::io::Error) -> LogError {
    LogError::Io {
        path: path.display().to_string(),
        source,
    }
}

/// Rotates `path` when it reached `max_bytes`, retaining `keep` old files.
///
/// Rotation renames `path` to `path.1`, `path.1` to `path.2`, and so on. The
/// oldest file is dropped. A missing file is not an error.
///
/// # Errors
///
/// Returns [`LogError::NoRetention`] when `keep` is zero, or [`LogError::Io`]
/// on a filesystem failure.
pub fn rotate_if_needed(path: &Path, max_bytes: u64, keep: usize) -> Result<bool, LogError> {
    if keep == 0 {
        return Err(LogError::NoRetention);
    }
    let size = match std::fs::metadata(path) {
        Ok(metadata) => metadata.len(),
        Err(error) if error.kind() == ErrorKind::NotFound => return Ok(false),
        Err(source) => return Err(rotate_error(path, source)),
    };
    if size < max_bytes {
        return Ok(false);
    }

    // Drop the oldest retained file before shifting, so no file outside
    // `1..=keep` is ever created.
    let oldest = numbered(path, keep);
    if oldest.exists() {
        std::fs::remove_file(&oldest).map_err(|source| rotate_error(&oldest, source))?;
    }
    for index in (1..keep).rev() {
        let source_path = numbered(path, index);
        if source_path.exists() {
            let target_path = numbered(path, index + 1);
            std::fs::rename(&source_path, &target_path)
                .map_err(|source| rotate_error(&source_path, source))?;
        }
    }
    std::fs::rename(path, numbered(path, 1)).map_err(|source| rotate_error(path, source))?;
    File::create(path).map_err(|source| rotate_error(path, source))?;
    Ok(true)
}

#[cfg(test)]
mod tests {
    use std::io::Write as _;

    use super::*;

    #[test]
    fn duplicates_collapse_into_a_counter() {
        let mut buffer = LogBuffer::new(4);
        for _ in 0..3 {
            buffer.push("stamp wait");
        }
        buffer.push("reims-vgpu: fatal: VK_DEVICE_LOST");
        assert_eq!(buffer.len(), 2);
        assert_eq!(
            buffer.page(0, 10),
            "stamp wait (x3)\nreims-vgpu: fatal: VK_DEVICE_LOST"
        );
    }

    #[test]
    fn capacity_evicts_the_oldest_logical_line() {
        let mut buffer = LogBuffer::new(2);
        buffer.push("one");
        buffer.push("two");
        buffer.push("three");
        assert_eq!(buffer.page(0, 10), "two\nthree");
    }

    #[test]
    fn page_slices_from_an_offset() {
        let mut buffer = LogBuffer::new(8);
        buffer.push("one");
        buffer.push("two");
        buffer.push("three");
        assert_eq!(buffer.page(1, 1), "two");
    }

    #[test]
    fn rotation_keeps_the_configured_number_of_files() {
        let dir = tempfile::tempdir().expect("temp dir");
        let log = dir.path().join("session.log");
        for _ in 0..3 {
            let mut file = File::create(&log).expect("create log");
            file.write_all(b"0123456789").expect("write log");
            assert!(rotate_if_needed(&log, 10, 2).expect("rotate"));
        }
        assert!(log.exists());
        assert!(numbered(&log, 1).exists());
        assert!(numbered(&log, 2).exists());
        assert!(!numbered(&log, 3).exists());
    }

    #[test]
    fn rotation_ignores_small_and_missing_files() {
        let dir = tempfile::tempdir().expect("temp dir");
        let log = dir.path().join("missing.log");
        assert!(!rotate_if_needed(&log, 10, 1).expect("missing file"));
        let mut file = File::create(&log).expect("create log");
        file.write_all(b"short").expect("write log");
        drop(file);
        assert!(!rotate_if_needed(&log, 10, 1).expect("small file"));
    }

    #[test]
    fn rotation_requires_retention() {
        let dir = tempfile::tempdir().expect("temp dir");
        let log = dir.path().join("session.log");
        assert!(matches!(
            rotate_if_needed(&log, 10, 0),
            Err(LogError::NoRetention)
        ));
    }
}
