//! Putting the equation into, and taking it back out of, the storage a document host hands us.
//!
//! Two streams are written for every equation:
//!
//! * [`STREAM_NATIVE`], the equation in the stream format other equation tools read;
//! * [`STREAM_LATEX`], the same equation as UTF-8 LaTeX text.
//!
//! The LaTeX stream is what re-opening an object trusts first — it is the one the editor reads and
//! writes — and the native stream is kept for interoperability. Writing both means a failed read
//! of either still has the other to fall back on, and nothing is ever thrown away.

use std::ffi::c_void;
use std::path::Path;

use windows::Win32::System::Com::StructuredStorage::{IStorage, StgOpenStorage};
use windows::Win32::System::Com::{
    IStream, STATSTG, STGC_DEFAULT, STGM_CREATE, STGM_READ, STGM_SHARE_DENY_WRITE, STGM_SHARE_EXCLUSIVE, STGM_WRITE,
    STGTY_STORAGE, STGTY_STREAM,
};
use windows::core::PCWSTR;

/// `STG_E_FILENOTFOUND`, how a storage says a stream is not there.
const STG_E_FILENOTFOUND: i32 = 0x8003_0002u32 as i32;

/// Report an `HRESULT` failure as a message, keeping the code.
pub fn error_text(error: &windows::core::Error) -> String {
    format!("{:#010X} {}", error.code().0 as u32, error.message())
}

/// The text of a stream or of a hand-off file: UTF-8, decoded lossily, without a leading byte
/// order mark, which a host or an editor may have added.
pub fn text_of(bytes: &[u8]) -> String {
    let text = String::from_utf8_lossy(bytes);
    text.strip_prefix('\u{feff}').unwrap_or(&text).to_owned()
}

/// Write `bytes` as a stream of `storage`, replacing anything already there.
pub fn write_stream(storage: &IStorage, name: &str, bytes: &[u8]) -> Result<(), String> {
    let wide = wide(name);
    unsafe {
        let stream: IStream = storage
            .CreateStream(PCWSTR(wide.as_ptr()), STGM_CREATE | STGM_WRITE | STGM_SHARE_EXCLUSIVE, 0, 0)
            .map_err(|error| error_text(&error))?;
        if !bytes.is_empty() {
            stream
                .Write(bytes.as_ptr().cast::<c_void>(), bytes.len() as u32, None)
                .ok()
                .map_err(|error| error_text(&error))?;
        }
        stream.Commit(STGC_DEFAULT).map_err(|error| error_text(&error))?;
    }
    Ok(())
}

/// Read a stream of `storage`, or `None` when it does not have one under that name.
///
/// A host may keep the object's streams inside a sub-storage of its own, so the name is looked for
/// at any depth before giving up.
pub fn read_stream(storage: &IStorage, name: &str) -> Result<Option<Vec<u8>>, String> {
    if let Some(bytes) = read_named_stream(storage, name)? {
        return Ok(Some(bytes));
    }
    let Some(path) = find_stream(storage, name)? else {
        return Ok(None);
    };
    let (parent, leaf) = split_path(&path);
    let parent = open_sub_storage(storage, parent)?;
    read_named_stream(&parent, leaf)
}

fn read_named_stream(storage: &IStorage, name: &str) -> Result<Option<Vec<u8>>, String> {
    let wide = wide(name);
    unsafe {
        let stream = match storage.OpenStream(
            PCWSTR(wide.as_ptr()),
            None,
            STGM_READ | STGM_SHARE_EXCLUSIVE,
            0,
        ) {
            Ok(stream) => stream,
            Err(error) if error.code().0 == STG_E_FILENOTFOUND => return Ok(None),
            Err(error) => return Err(error_text(&error)),
        };
        let mut out = Vec::new();
        let mut buffer = vec![0u8; 8192];
        loop {
            let mut read = 0u32;
            stream
                .Read(buffer.as_mut_ptr().cast::<c_void>(), buffer.len() as u32, Some(&mut read))
                .ok()
                .map_err(|error| error_text(&error))?;
            if read == 0 {
                break;
            }
            out.extend_from_slice(&buffer[..read as usize]);
        }
        Ok(Some(out))
    }
}

/// Open the storage at a `a/b/` path inside `storage`.
fn open_sub_storage(storage: &IStorage, path: &str) -> Result<IStorage, String> {
    let mut current: IStorage = storage.clone();
    for part in path.trim_end_matches('/').split('/').filter(|part| !part.is_empty()) {
        let wide = wide(part);
        current = unsafe {
            current
                .OpenStorage(PCWSTR(wide.as_ptr()), None, STGM_READ | STGM_SHARE_EXCLUSIVE, std::ptr::null(), 0)
                .map_err(|error| error_text(&error))?
        };
    }
    Ok(current)
}

fn split_path(path: &str) -> (&str, &str) {
    match path.rfind('/') {
        Some(at) => (&path[..=at], &path[at + 1..]),
        None => ("", path),
    }
}

/// Commit the streams written so far into the storage.
pub fn commit(storage: &IStorage) -> Result<(), String> {
    unsafe { storage.Commit(0).map_err(|error| error_text(&error)) }
}

/// Open a compound file on disk, for the offline checks the verification script runs.
pub fn open_file(path: &Path) -> Result<IStorage, String> {
    let text = path.to_str().ok_or_else(|| format!("not a usable path: {}", path.display()))?;
    let wide = wide(text);
    unsafe {
        StgOpenStorage(PCWSTR(wide.as_ptr()), None, STGM_READ | STGM_SHARE_DENY_WRITE, None, 0)
            .map_err(|error| error_text(&error))
    }
}

/// Every stream in a storage, including the ones inside sub-storages.
pub fn walk_streams(storage: &IStorage, prefix: &str) -> Result<Vec<(String, u64)>, String> {
    let mut found = Vec::new();
    collect_streams(storage, prefix, &mut found)?;
    Ok(found)
}

/// Path of the first stream whose last path segment is `name`, for hosts that nest the object's
/// own streams inside a storage of their own.
pub fn find_stream(storage: &IStorage, name: &str) -> Result<Option<String>, String> {
    Ok(walk_streams(storage, "")?
        .into_iter()
        .find(|(path, _)| path.rsplit('/').next().is_some_and(|leaf| leaf == name))
        .map(|(path, _)| path))
}

fn collect_streams(storage: &IStorage, prefix: &str, found: &mut Vec<(String, u64)>) -> Result<(), String> {
    for (name, kind, size) in entries(storage)? {
        let path = format!("{prefix}{name}");
        if kind == STGTY_STREAM.0 as u32 {
            found.push((path, size));
        } else if kind == STGTY_STORAGE.0 as u32 {
            let wide = wide(&name);
            let inner = unsafe {
                storage.OpenStorage(PCWSTR(wide.as_ptr()), None, STGM_READ | STGM_SHARE_EXCLUSIVE, std::ptr::null(), 0)
            };
            if let Ok(inner) = inner {
                collect_streams(&inner, &format!("{path}/"), found)?;
            }
        }
    }
    Ok(())
}

/// The entries of one storage: name, type and size.
fn entries(storage: &IStorage) -> Result<Vec<(String, u32, u64)>, String> {
    unsafe {
        let enumerator = storage.EnumElements(None, None, None).map_err(|error| error_text(&error))?;
        let mut entries = Vec::new();
        loop {
            let mut entry = [STATSTG::default()];
            let mut fetched = 0u32;
            if enumerator.Next(&mut entry, Some(&mut fetched)).is_err() || fetched == 0 {
                break;
            }
            let entry = &entry[0];
            let name = if entry.pwcsName.is_null() {
                String::new()
            } else {
                let name = entry.pwcsName.to_string().unwrap_or_default();
                windows::Win32::System::Com::CoTaskMemFree(Some(entry.pwcsName.0.cast::<c_void>()));
                name
            };
            entries.push((name, entry.r#type, entry.cbSize));
        }
        Ok(entries)
    }
}

fn wide(text: &str) -> Vec<u16> {
    text.encode_utf16().chain(std::iter::once(0)).collect()
}
