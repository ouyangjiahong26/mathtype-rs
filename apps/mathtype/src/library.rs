//! The formula library: named equations kept in a readable text file under the user's data
//! directory, searched by name or contents and inserted back into the editor.
//!
//! The file is a sequence of `[formula]` blocks, so it can be edited or shared by hand:
//!
//! ```ini
//! [formula]
//! name = 二次方程求根
//! latex = x = \frac{-b \pm \sqrt{b^2 - 4ac}}{2a}
//! ```
//!
//! The default location follows the XDG convention: `$XDG_DATA_HOME/mathtype/library.txt`,
//! falling back to `~/.local/share/mathtype/library.txt`.

use std::fmt;
use std::path::{Path, PathBuf};

/// One saved formula: the name it is listed under and the LaTeX it round-trips through.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LibraryEntry {
    pub name: String,
    pub latex: String,
}

/// Why a library file could not be read, with the line it happened on.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LibraryError {
    /// One-based line number the error is about.
    pub line: usize,
    pub message: String,
}

impl fmt::Display for LibraryError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "line {}: {}", self.line, self.message)
    }
}

impl std::error::Error for LibraryError {}

/// The formulas of one library file, plus where they are saved.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Library {
    /// The saved formulas, in the order the file lists them.
    pub entries: Vec<LibraryEntry>,
    /// The file the library is read from and written back to.
    pub path: PathBuf,
}

/// The library file inside `data_dir`, the XDG data home.
pub fn path_under(data_dir: &Path) -> PathBuf {
    data_dir.join("mathtype").join("library.txt")
}

/// The default library location: `$XDG_DATA_HOME/mathtype/library.txt`, or
/// `~/.local/share/mathtype/library.txt` when the variable is unset or relative.
pub fn default_path() -> PathBuf {
    let from_xdg = std::env::var_os("XDG_DATA_HOME")
        .map(PathBuf::from)
        .filter(|path| path.is_absolute());
    let data = from_xdg.or_else(|| {
        std::env::var_os("HOME").map(|home| PathBuf::from(home).join(".local").join("share"))
    });
    // No home directory at all: keep the library beside the working directory.
    data.map(|data| path_under(&data)).unwrap_or_else(|| PathBuf::from("library.txt"))
}

impl Library {
    /// Read the default library; a missing file starts an empty one, a broken one reports the
    /// line that is wrong.
    pub fn load_default() -> Result<Self, LibraryError> {
        Self::load(default_path())
    }

    /// Read the library in `path`; a missing file starts an empty library bound to that path.
    pub fn load(path: impl Into<PathBuf>) -> Result<Self, LibraryError> {
        let path = path.into();
        let bytes = match std::fs::read(&path) {
            Ok(bytes) => bytes,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                return Ok(Self { entries: Vec::new(), path })
            }
            Err(error) => {
                return Err(LibraryError { line: 0, message: format!("{}: {error}", path.display()) })
            }
        };
        let text = String::from_utf8_lossy(&bytes).into_owned();
        Ok(Self { entries: parse(&text)?, path })
    }

    /// Write the library back to its file, creating the directory when needed.
    pub fn save(&self) -> Result<(), String> {
        let text = to_text(&self.entries);
        if let Some(dir) = self.path.parent() {
            std::fs::create_dir_all(dir).map_err(|error| format!("{}: {error}", dir.display()))?;
        }
        std::fs::write(&self.path, text).map_err(|error| format!("{}: {error}", self.path.display()))
    }

    /// Write the library to `path` and remember it as the library's file.
    pub fn save_to(&mut self, path: &Path) -> Result<(), String> {
        let text = to_text(&self.entries);
        if let Some(dir) = path.parent() {
            std::fs::create_dir_all(dir).map_err(|error| format!("{}: {error}", dir.display()))?;
        }
        std::fs::write(path, text).map_err(|error| format!("{}: {error}", path.display()))?;
        self.path = path.to_owned();
        Ok(())
    }

    /// Append a formula, replacing the name when the library already has it.
    pub fn add(&mut self, name: &str, latex: &str) {
        let trimmed = name.trim();
        let name = if trimmed.is_empty() { "未命名公式" } else { trimmed };
        if let Some(existing) = self.entries.iter_mut().find(|entry| entry.name == name) {
            existing.latex = latex.to_owned();
            return;
        }
        self.entries.push(LibraryEntry { name: name.to_owned(), latex: latex.to_owned() });
    }

    /// Drop the entry at `index`, reporting whether there was one.
    pub fn remove(&mut self, index: usize) -> bool {
        if index < self.entries.len() {
            self.entries.remove(index);
            true
        } else {
            false
        }
    }

    /// Indices of the entries whose name or LaTeX contains `query` (case-insensitive), in file
    /// order; every entry when the query is blank.
    pub fn search(&self, query: &str) -> Vec<usize> {
        let needle = query.to_lowercase();
        self.entries
            .iter()
            .enumerate()
            .filter(|(_, entry)| {
                needle.is_empty()
                    || entry.name.to_lowercase().contains(&needle)
                    || entry.latex.to_lowercase().contains(&needle)
            })
            .map(|(index, _)| index)
            .collect()
    }
}

/// Read the entries of a library file.
///
/// A line must be a comment (`#`), blank, a `[formula]` header or `key = value` inside a
/// formula; `name` and `latex` are required, in that order, and a formula may not reappear once
/// closed. Errors name the line they were found on.
pub fn parse(text: &str) -> Result<Vec<LibraryEntry>, LibraryError> {
    let mut entries = Vec::new();
    let mut current: Option<LibraryEntry> = None;
    for (index, raw) in text.lines().enumerate() {
        let line = index + 1;
        let trimmed = raw.trim();
        if trimmed.is_empty() || trimmed.starts_with('#') || trimmed.starts_with(';') {
            continue;
        }
        if trimmed == "[formula]" {
            entries.extend(current.take());
            current = Some(LibraryEntry { name: String::new(), latex: String::new() });
            continue;
        }
        let (key, value) = trimmed
            .split_once('=')
            .ok_or_else(|| LibraryError { line, message: format!("expected 'key = value' or '[formula]', found: {trimmed}") })?;
        let (key, value) = (key.trim(), value.trim());
        let entry = current.as_mut().ok_or_else(|| {
            LibraryError { line, message: format!("a key outside any [formula] block: {trimmed}") }
        })?;
        match key {
            "name" => {
                if entry.name.is_empty() {
                    entry.name = value.to_owned();
                } else {
                    return Err(LibraryError { line, message: "duplicate key 'name'".to_owned() });
                }
            }
            "latex" => {
                if entry.latex.is_empty() {
                    entry.latex = value.to_owned();
                } else {
                    return Err(LibraryError { line, message: "duplicate key 'latex'".to_owned() });
                }
            }
            _ => {
                return Err(LibraryError {
                    line,
                    message: format!("unknown key '{key}' inside a [formula] block (name, latex)"),
                })
            }
        }
    }
    entries.extend(current);
    for (index, entry) in entries.iter().enumerate() {
        if entry.name.is_empty() {
            return Err(LibraryError {
                line: 0,
                message: format!("formula {} has no name", index + 1),
            });
        }
    }
    Ok(entries)
}

/// Write entries in the library file format.
pub fn to_text(entries: &[LibraryEntry]) -> String {
    let mut out = String::from("# mathtype 公式库 v1\n");
    for entry in entries {
        out.push_str("\n[formula]\n");
        out.push_str(&format!("name = {}\n", entry.name));
        out.push_str(&format!("latex = {}\n", entry.latex));
    }
    out
}

/// Insert a library entry into `editor` at the caret, as the tree its LaTeX parses to.
pub fn insert_entry(editor: &mut mt_edit::Editor, entry: &LibraryEntry) -> Result<(), String> {
    let math = mt_tex::parse(&entry.latex)
        .map_err(|error| format!("LaTeX error at byte {}: {}", error.offset, error.message))?;
    editor.insert_node(math.body);
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    const SAMPLE: &str = "# mathtype 公式库 v1\n\
                          \n\
                          [formula]\n\
                          name = 二次方程求根\n\
                          latex = x = \\frac{-b \\pm \\sqrt{b^2 - 4ac}}{2a}\n\
                          \n\
                          [formula]\n\
                          name = Euler\n\
                          latex = e^{i\\pi} + 1 = 0\n";

    #[test]
    fn entries_round_trip_through_the_file_format() {
        let entries = parse(SAMPLE).expect("the sample parses");
        assert_eq!(entries.len(), 2);
        assert_eq!(entries[0].name, "二次方程求根");
        assert_eq!(entries[1].latex, "e^{i\\pi} + 1 = 0");
        assert_eq!(parse(&to_text(&entries)).unwrap(), entries);
    }

    #[test]
    fn latex_may_contain_equals_signs() {
        let entries = parse("[formula]\nname = a\nlatex = a = b = c\n").unwrap();
        assert_eq!(entries[0].latex, "a = b = c");
    }

    #[test]
    fn errors_name_their_line() {
        let error = parse("name = x\n").unwrap_err();
        assert_eq!(error.line, 1);
        assert!(error.message.contains("[formula]"));

        let error = parse("[formula]\nname\n").unwrap_err();
        assert_eq!(error.line, 2);
        assert!(error.message.contains("key = value"));

        let error = parse("[formula]\nname = a\nname = b\n").unwrap_err();
        assert_eq!(error.line, 3);
        assert!(error.message.contains("duplicate"));

        let error = parse("[formula]\nname = a\ncolor = red\n").unwrap_err();
        assert_eq!(error.line, 3);
        assert!(error.message.contains("color"));

        let error = parse("[formula]\nlatex = x\n").unwrap_err();
        assert!(error.message.contains("no name"));
    }

    #[test]
    fn search_matches_names_and_contents_case_insensitively() {
        let library = Library { entries: parse(SAMPLE).unwrap(), path: PathBuf::new() };
        assert_eq!(library.search(""), vec![0, 1]);
        assert_eq!(library.search("euler"), vec![1]);
        assert_eq!(library.search("EULER"), vec![1]);
        assert_eq!(library.search("frac"), vec![0]);
        assert!(library.search("quadratic").is_empty());
    }

    #[test]
    fn add_replaces_by_name_and_remove_bounds_check() {
        let mut library = Library { entries: Vec::new(), path: PathBuf::new() };
        library.add("勾股定理", "a^2 + b^2 = c^2");
        library.add("临时", "x");
        library.add("临时", "y + 1");
        assert_eq!(library.entries.len(), 2);
        assert_eq!(library.entries[1].latex, "y + 1");
        assert!(library.remove(1));
        assert!(!library.remove(9));
        assert_eq!(library.entries.len(), 1);
        // A blank name falls back rather than saving an unnamed entry.
        library.add("", "z");
        assert_eq!(library.entries[1].name, "未命名公式");
    }

    #[test]
    fn the_library_lives_in_the_xdg_data_dir() {
        assert_eq!(path_under(Path::new("/data")), Path::new("/data/mathtype/library.txt"));
    }

    #[test]
    fn saving_and_loading_round_trip_through_disk() {
        let dir = std::env::temp_dir().join(format!("mathtype-library-{}", std::process::id()));
        let path = dir.join("nested").join("library.txt");
        let mut library = Library { entries: Vec::new(), path: path.clone() };
        library.add("求和", "\\sum_{i=1}^{n} i");
        library.save().expect("saving creates the directories");
        let reloaded = Library::load(&path).expect("the saved file reads back");
        assert_eq!(reloaded.entries, library.entries);
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn a_missing_file_starts_an_empty_library() {
        let missing = std::env::temp_dir().join("mathtype-library-missing-9431.txt");
        let library = Library::load(&missing).expect("a missing file is an empty library");
        assert!(library.entries.is_empty());
        assert_eq!(library.path, missing);
    }

    #[test]
    fn entries_insert_as_trees_at_the_caret() {
        let mut editor = mt_edit::Editor::new();
        insert_entry(&mut editor, &LibraryEntry {
            name: "分式".to_owned(),
            latex: "\\frac{a}{b}".to_owned(),
        })
        .expect("the LaTeX parses");
        assert_eq!(editor.math().body.slot_count(), 1);
        let mut found = false;
        editor.math().body.walk(&mut |node| found |= matches!(node, mt_core::Node::Frac(_)));
        assert!(found, "the inserted formula tree must contain the fraction");

        let error = insert_entry(&mut editor, &LibraryEntry {
            name: "坏".to_owned(),
            latex: "\\frac{a".to_owned(),
        })
        .unwrap_err();
        assert!(error.contains("LaTeX error"));
    }
}
