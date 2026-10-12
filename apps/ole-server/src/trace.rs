//! A line-per-event log next to the hand-off files.
//!
//! The object runs out of a host's process with nowhere to print, so everything worth knowing
//! about a session goes to `%TEMP%\mathtype-ole-server\server.log`, which the verification script
//! prints when a step fails.

use std::fs::OpenOptions;
use std::io::Write;
use std::path::PathBuf;
use std::sync::LazyLock;
use std::sync::mpsc::{SyncSender, sync_channel};

use windows::Win32::Foundation::SYSTEMTIME;
use windows::Win32::System::SystemInformation::GetLocalTime;

/// How many lines may be waiting for standard error before new ones are dropped there.
const STDERR_QUEUE: usize = 64;

/// Lines waiting for standard error, drained by a thread of their own.
///
/// The queue is bounded on purpose: a reader that stops reading must never be able to block the
/// thread that writes the log, because that thread is the apartment answering the host's calls. A
/// line that does not fit is dropped from standard error and kept in the file, which is the record
/// that has to survive.
static TO_STDERR: LazyLock<SyncSender<String>> = LazyLock::new(|| {
    let (sender, receiver) = sync_channel::<String>(STDERR_QUEUE);
    std::thread::spawn(move || {
        for line in receiver {
            eprintln!("[mathtype-ole-server] {line}");
        }
    });
    sender
});

/// Directory holding the log, the hand-off files and the pointer file.
pub fn directory() -> PathBuf {
    std::env::temp_dir().join("mathtype-ole-server")
}

/// Log the events of this server's session, appended to across processes.
pub fn log_path() -> PathBuf {
    directory().join("server.log")
}

/// Write one line to the log file and, without waiting for it, to standard error.
///
/// The file comes first and is written synchronously: whatever happens to the process afterwards,
/// the line is on disk. Standard error goes through a queue drained by another thread, so a console
/// or a wrapper sees the line live while a pipe whose reader has gone away cannot stall the object.
pub fn log(message: &str) {
    let line = format!("{} pid={} {message}", timestamp(), std::process::id());
    let _ = append(&line);
    let _ = TO_STDERR.try_send(line);
}

/// The tail of the log, for a report.
pub fn tail(max_lines: usize) -> String {
    let Ok(text) = std::fs::read_to_string(log_path()) else {
        return String::new();
    };
    let lines: Vec<&str> = text.lines().collect();
    lines[lines.len().saturating_sub(max_lines)..].join("\n")
}

fn append(line: &str) -> std::io::Result<()> {
    let directory = directory();
    std::fs::create_dir_all(&directory)?;
    let mut file = OpenOptions::new()
        .create(true)
        .append(true)
        .open(directory.join("server.log"))?;
    writeln!(file, "{line}")
}

fn timestamp() -> String {
    let time: SYSTEMTIME = unsafe { GetLocalTime() };
    format!(
        "{:04}-{:02}-{:02} {:02}:{:02}:{:02}.{:03}",
        time.wYear,
        time.wMonth,
        time.wDay,
        time.wHour,
        time.wMinute,
        time.wSecond,
        time.wMilliseconds
    )
}

#[cfg(test)]
mod tests {
    use super::timestamp;

    #[test]
    fn a_timestamp_is_a_readable_date_and_time() {
        let stamp = timestamp();
        assert_eq!(stamp.len(), "2026-10-11 08:30:00.000".len());
        assert!(stamp.starts_with("20"), "{stamp}");
        assert_eq!(stamp.as_bytes()[10], b' ');
    }
}
