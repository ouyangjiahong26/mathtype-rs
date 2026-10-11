//! A line-per-event log next to the hand-off files.
//!
//! The object runs out of a host's process with nowhere to print, so everything worth knowing
//! about a session goes to `%TEMP%\mathtype-ole-server\server.log`, which the verification script
//! prints when a step fails.

use std::fs::OpenOptions;
use std::io::Write;
use std::path::PathBuf;

use windows::Win32::Foundation::SYSTEMTIME;
use windows::Win32::System::SystemInformation::GetLocalTime;

/// Directory holding the log, the hand-off files and the pointer file.
pub fn directory() -> PathBuf {
    std::env::temp_dir().join("mathtype-ole-server")
}

/// Log the events of this server's session, appended to across processes.
pub fn log_path() -> PathBuf {
    directory().join("server.log")
}

/// Write one line, both to the log file and to standard error.
pub fn log(message: &str) {
    let line = format!("{} pid={} {message}", timestamp(), std::process::id());
    eprintln!("[mathtype-ole-server] {line}");
    let _ = append(&line);
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
    let mut file = OpenOptions::new().create(true).append(true).open(directory.join("server.log"))?;
    writeln!(file, "{line}")
}

fn timestamp() -> String {
    let time: SYSTEMTIME = unsafe { GetLocalTime() };
    format!(
        "{:04}-{:02}-{:02} {:02}:{:02}:{:02}.{:03}",
        time.wYear, time.wMonth, time.wDay, time.wHour, time.wMinute, time.wSecond, time.wMilliseconds
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
