//! Handing the equation to the desktop editor and taking the result back.
//!
//! Activating an object writes its LaTeX to a file in a per-user temporary directory, starts the
//! editor on that file, and returns without waiting. The server then polls the process: when it
//! exits successfully, the file is read back and, if the text changed, the object stores the new
//! equation, commits it and tells the host to save it. An editor that failed is not believed: its
//! file may be half written, so the object keeps the equation it already had and the failure is
//! reported.
//!
//! Everything the verification script needs to find its way around the directory:
//!
//! * `%TEMP%\mathtype-ole-server\current-handoff.txt` — the absolute path of the file the editor
//!   was last started on, one line. This is the documented way to discover it.
//! * `%TEMP%\mathtype-ole-server\handoff-log.txt` — one line per activation: when, which session,
//!   the file, the editor's process id.
//! * `%TEMP%\mathtype-ole-server\server.log` — what the object did, see [`crate::trace`].
//!
//! The editor is read from the environment variable `MATHTYPE_RS_EDITOR` when that is set, and
//! otherwise is `mathtype.exe` next to this server's own executable.

use std::fs::{OpenOptions, create_dir_all};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::{Child, Command};
use std::sync::atomic::{AtomicU64, Ordering};

use crate::trace;

/// Environment variable naming the editor executable.
pub const EDITOR_VARIABLE: &str = "MATHTYPE_RS_EDITOR";

/// The file the editor was last started on, for tooling that has to find it.
pub const POINTER_FILE: &str = "current-handoff.txt";

/// Where every activation is recorded.
pub const LOG_FILE: &str = "handoff-log.txt";

static NEXT_SESSION: AtomicU64 = AtomicU64::new(1);

/// One activation of the editor, and the process running it.
pub struct Session {
    /// Serial number of the activation, unique within this server process.
    pub id: u64,
    /// File the editor was started on and writes back to.
    pub path: PathBuf,
    /// Process id of the editor, which is what "double-clicking the equation" shows up as.
    pub editor_pid: u32,
    child: Option<Child>,
}

impl Session {
    /// Start the editor on `latex`.
    pub fn start(latex: &str) -> Result<Self, String> {
        let directory = trace::directory();
        create_dir_all(&directory).map_err(|error| format!("cannot create {}: {error}", directory.display()))?;
        let id = NEXT_SESSION.fetch_add(1, Ordering::Relaxed);
        let path = directory.join(format!("equation-{}-{id}.tex", std::process::id()));
        std::fs::write(&path, latex).map_err(|error| format!("cannot write {}: {error}", path.display()))?;
        let pointer = directory.join(POINTER_FILE);
        std::fs::write(&pointer, path.display().to_string())
            .map_err(|error| format!("cannot write {}: {error}", pointer.display()))?;

        let editor = editor_path()?;
        let child = Command::new(&editor)
            .arg("--edit")
            .arg(&path)
            .spawn()
            .map_err(|error| format!("cannot start {}: {error}", editor.display()))?;
        let session = Self { id, path, editor_pid: child.id(), child: Some(child) };
        session.record(&editor, "started");
        trace::log(&format!("editor pid={} started on {}", session.editor_pid, session.path.display()));
        Ok(session)
    }

    /// Whether the editor is still running.
    pub fn running(&self) -> bool {
        self.child.is_some()
    }

    /// Poll for the editor having exited, and read the file back when it exited successfully.
    ///
    /// `None` while the editor is still running. A failed exit is reported as an error instead of
    /// the file being read, because what a failed editor left may be truncated.
    pub fn poll(&mut self) -> Option<Result<String, String>> {
        let child = self.child.as_mut()?;
        match child.try_wait() {
            Ok(None) => None,
            Ok(Some(status)) => {
                self.child = None;
                self.record(Path::new(""), &format!("exited {status}"));
                // A failed editor may have died half-way through writing the file, so nothing it
                // left behind is trusted: the object keeps the equation it already had and the
                // failure is reported instead of the file being read back.
                if !status.success() {
                    let message = format!(
                        "editor pid={} exited with {status}; the equation was not changed",
                        self.editor_pid
                    );
                    trace::log(&message);
                    return Some(Err(message));
                }
                trace::log(&format!(
                    "editor pid={} exited successfully, reading {}",
                    self.editor_pid,
                    self.path.display()
                ));
                let text = std::fs::read(&self.path);
                Some(match text {
                    Ok(bytes) => Ok(crate::storage::text_of(&bytes)),
                    Err(error) => Err(format!("cannot read {}: {error}", self.path.display())),
                })
            }
            Err(error) => {
                self.child = None;
                Some(Err(format!("cannot wait for the editor: {error}")))
            }
        }
    }

    /// Abandon the session without waiting for the editor.
    ///
    /// The editor keeps running: it is the user's window, and the file it is editing still holds
    /// the equation, so nothing is lost by letting it finish.
    pub fn forget(&mut self) {
        if self.child.is_some() {
            trace::log(&format!(
                "session {} left running (editor pid={}, file {})",
                self.id,
                self.editor_pid,
                self.path.display()
            ));
        }
        self.child = None;
    }

    fn record(&self, editor: &Path, what: &str) {
        let line = format!(
            "session {} {what}: file={} editor={} pid={}\n",
            self.id,
            self.path.display(),
            editor.display(),
            self.editor_pid
        );
        if let Ok(mut file) = OpenOptions::new()
            .create(true)
            .append(true)
            .open(trace::directory().join(LOG_FILE))
        {
            let _ = file.write_all(line.as_bytes());
        }
    }
}

/// The editor executable: the override when it names one, otherwise the one next to this binary.
pub fn editor_path() -> Result<PathBuf, String> {
    if let Ok(named) = std::env::var(EDITOR_VARIABLE) {
        if !named.trim().is_empty() {
            let path = PathBuf::from(named);
            return if path.is_file() {
                Ok(path)
            } else {
                Err(format!("{EDITOR_VARIABLE} names {}, which is not a file", path.display()))
            };
        }
    }
    let own = std::env::current_exe().map_err(|error| format!("cannot find this executable: {error}"))?;
    let directory = own.parent().ok_or_else(|| format!("{} has no directory", own.display()))?;
    let editor = directory.join("mathtype.exe");
    if editor.is_file() {
        Ok(editor)
    } else {
        Err(format!("no equation editor at {} (set {EDITOR_VARIABLE})", editor.display()))
    }
}

#[cfg(test)]
mod tests {
    use super::{EDITOR_VARIABLE, LOG_FILE, POINTER_FILE, Session, editor_path};
    use std::path::PathBuf;
    use std::time::Duration;

    #[test]
    fn the_editor_is_looked_up_next_to_this_binary() {
        // A test binary lives in target/debug/deps, where there is no editor; the override is the
        // supported way to name one, so an unset override must report where it looked.
        let named = editor_path();
        match named {
            Ok(path) => assert!(path.is_file()),
            Err(message) => assert!(message.contains("mathtype.exe"), "{message}"),
        }
    }

    #[test]
    fn the_discovery_files_have_the_documented_names() {
        assert_eq!(POINTER_FILE, "current-handoff.txt");
        assert_eq!(LOG_FILE, "handoff-log.txt");
    }

    #[test]
    fn a_failed_editor_is_not_accepted() {
        // A stand-in editor that exits non-zero without touching the hand-off file: `where.exe`
        // rejects `--edit`. That is what an editor that crashed half-way through writing looks like
        // to the server, whose file may then be truncated, so its text must not be adopted.
        let Some(editor) = failing_editor() else {
            return;
        };
        // SAFETY: the variable is set and cleared around the session. The other test that reads it
        // accepts either result, so setting it here cannot make that test fail.
        unsafe { std::env::set_var(EDITOR_VARIABLE, &editor) };
        let started = Session::start("x");
        unsafe { std::env::remove_var(EDITOR_VARIABLE) };
        let mut session = started.expect("the stand-in editor starts");

        let outcome = loop {
            if let Some(outcome) = session.poll() {
                break outcome;
            }
            std::thread::sleep(Duration::from_millis(20));
        };

        let error = outcome.expect_err("a failed editor's hand-off file is not adopted");
        assert!(error.contains("exited with"), "{error}");
    }

    /// A Windows tool that exits non-zero when handed `--edit <file>`, or `None` when there is no
    /// such tool on this machine.
    fn failing_editor() -> Option<PathBuf> {
        let root = std::env::var_os("SystemRoot")
            .or_else(|| std::env::var_os("windir"))
            .map(PathBuf::from)?;
        let editor = root.join("System32").join("where.exe");
        editor.is_file().then_some(editor)
    }
}
