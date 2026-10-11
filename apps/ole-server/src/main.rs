//! Out-of-process OLE embedded-object server: an equation that lives inside a document.
//!
//! A host document — Word, WPS 文字, or any other OLE container — starts this executable with
//! `-Embedding`, asks for the class factory registered for the equation class, and gets an
//! equation object from it. The object keeps its equation in the host's own storage and opens the
//! desktop editor on itself when it is activated, which is what double-clicking the equation does.
//!
//! Besides serving the object, the same executable can be pointed at the compound file a document
//! embeds, which is how the verification script reads back what an object stored:
//!
//! ```text
//! mathtype-ole-server.exe --registration                 the class id and programmatic identifier
//! mathtype-ole-server.exe --list-streams <file.bin>      the streams of an embedded object
//! mathtype-ole-server.exe --read-stream <file.bin> <name>  one stream, as text
//! mathtype-ole-server.exe --read-native <file.bin>       the equation of an embedded object
//! mathtype-ole-server.exe --log                          the tail of this server's log
//! ```

#![cfg(windows)]

mod clsid;
mod factory;
mod formats;
mod handoff;
mod metrics;
mod object;
mod picture;
mod server;
mod storage;
mod trace;
mod variant;

use std::process::ExitCode;

const USAGE: &str = "\
mathtype-ole-server: an equation embedded in a document, as an out-of-process OLE server

  -Embedding                     serve the equation class, which is how a host starts this
  --registration                 print the class id, the programmatic identifier and the streams
  --list-streams <file>          list the streams of an embedded object's compound file
  --read-stream <file> <name>    print one stream of an embedded object, as text
  --read-native <file>           print the equation held in an embedded object's equation stream
  --log [<lines>]                print the tail of this server's log (default 40 lines)
  --help                         this text

The editor the object opens is mathtype.exe next to this executable, or the file named by the
environment variable MATHTYPE_RS_EDITOR.
";

fn main() -> ExitCode {
    let arguments: Vec<String> = std::env::args().skip(1).collect();
    if arguments.iter().any(|argument| is_embedding_switch(argument)) {
        return match server::run() {
            Ok(()) => ExitCode::SUCCESS,
            Err(message) => {
                trace::log(&message);
                ExitCode::FAILURE
            }
        };
    }
    let arguments: Vec<&str> = arguments.iter().map(String::as_str).collect();
    match arguments.as_slice() {
        ["--registration" | "-registration", ..] => {
            println!("{}", clsid::registration());
            ExitCode::SUCCESS
        }
        ["--list-streams", file, ..] => report(list_streams(file)),
        ["--read-stream", file, name, ..] => report(read_stream(file, name)),
        ["--read-native", file, ..] => report(read_native(file)),
        ["--log", rest @ ..] => {
            let lines = rest.first().and_then(|count| count.parse().ok()).unwrap_or(40);
            print!("{}", trace::tail(lines));
            ExitCode::SUCCESS
        }
        [] | ["--help" | "-h" | "/?"] => {
            print!("{USAGE}");
            ExitCode::SUCCESS
        }
        _ => {
            eprint!("{USAGE}");
            ExitCode::FAILURE
        }
    }
}

/// Whether a command line is COM asking this server to start as an embedded object.
///
/// COM appends `-Embedding`; some hosts name the same thing `-Automation`, and either spelling may
/// arrive with a slash instead of a dash.
fn is_embedding_switch(argument: &str) -> bool {
    let Some(name) = argument.strip_prefix(['-', '/']) else {
        return false;
    };
    name.eq_ignore_ascii_case("embedding") || name.eq_ignore_ascii_case("automation")
}

fn report(result: Result<String, String>) -> ExitCode {
    match result {
        Ok(text) => {
            print!("{text}");
            if !text.ends_with('\n') {
                println!();
            }
            ExitCode::SUCCESS
        }
        Err(message) => {
            eprintln!("{message}");
            ExitCode::FAILURE
        }
    }
}

/// Open the compound file a document embeds, with COM ready to read it.
fn with_storage<T>(file: &str, read: impl FnOnce(&windows::Win32::System::Com::StructuredStorage::IStorage) -> Result<T, String>) -> Result<T, String> {
    let path = std::path::Path::new(file);
    if !path.is_file() {
        return Err(format!("no such file: {file}"));
    }
    unsafe { windows::Win32::System::Ole::OleInitialize(None) }
        .map_err(|error| format!("OleInitialize failed: {}", storage::error_text(&error)))?;
    let result = storage::open_file(path).and_then(|storage| read(&storage));
    unsafe { windows::Win32::System::Ole::OleUninitialize() };
    result
}

fn list_streams(file: &str) -> Result<String, String> {
    with_storage(file, |storage| {
        let mut out = String::new();
        for (path, size) in storage::walk_streams(storage, "")? {
            out.push_str(&format!("{size:>8}  {path}\n"));
        }
        Ok(out)
    })
}

fn read_stream(file: &str, name: &str) -> Result<String, String> {
    with_storage(file, |storage| match storage::read_stream(storage, name)? {
        Some(bytes) => Ok(storage::text_of(&bytes)),
        None => Err(format!("{file} has no stream called {name}")),
    })
}

fn read_native(file: &str) -> Result<String, String> {
    with_storage(file, |storage| {
        let bytes = storage::read_stream(storage, clsid::STREAM_NATIVE)?
            .ok_or_else(|| format!("{file} has no stream called {}", clsid::STREAM_NATIVE))?;
        let math = mt_mtef::parse(&bytes).map_err(|error| format!("cannot read the equation stream: {error}"))?;
        Ok(mt_tex::to_latex(&math))
    })
}

#[cfg(test)]
mod tests {
    use super::is_embedding_switch;

    #[test]
    fn the_switches_that_mean_embedding_are_recognised() {
        for argument in ["-Embedding", "/Embedding", "-automation", "/AUTOMATION"] {
            assert!(is_embedding_switch(argument), "{argument}");
        }
        for argument in ["--registration", "-embed", "embedding", "--log"] {
            assert!(!is_embedding_switch(argument), "{argument}");
        }
    }
}
