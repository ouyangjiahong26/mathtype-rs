//! Taking the equation stream in and out of the compound file around it.
//!
//! A document embeds an equation as a compound file — `word/embeddings/oleObject1.bin` in a
//! Word file, for instance — whose equation lives in a stream called `Equation Native`, usually
//! inside the object's own storage. Both functions here work on the bytes of that compound
//! file, so they can be used on a whole embedded object.

use std::io::{Cursor, Read, Write};
use std::path::PathBuf;

use cfb::CompoundFile;

use crate::MtefError;

/// Stream names an equation is stored under, most specific first. The trailing name is what the
/// equation editor's own objects use in place of a full name.
const STREAM_NAMES: [&str; 3] = ["Equation Native", "EquationNative", "Equation"];

/// The equation stream of a compound file, or `None` when it holds none.
pub fn read_from_ole(bytes: &[u8]) -> Result<Option<Vec<u8>>, MtefError> {
    let mut compound = CompoundFile::open(Cursor::new(bytes)).map_err(ole_error)?;
    let Some(path) = equation_stream(&compound) else {
        return Ok(None);
    };
    let mut stream = compound.open_stream(&path).map_err(ole_error)?;
    let mut contents = Vec::new();
    stream.read_to_end(&mut contents).map_err(ole_error)?;
    Ok(Some(contents))
}

/// Replace the equation stream of a compound file, leaving its other streams untouched.
///
/// The stream keeps its place in the file, so the object that refers to it still finds it.
pub fn write_into_ole(ole: &[u8], stream: &[u8]) -> Result<Vec<u8>, MtefError> {
    let mut compound = CompoundFile::open(Cursor::new(ole.to_vec())).map_err(ole_error)?;
    let path = equation_stream(&compound)
        .ok_or_else(|| MtefError::Ole("the compound file holds no equation stream".to_owned()))?;
    {
        let mut target = compound.create_stream(&path).map_err(ole_error)?;
        target.write_all(stream).map_err(ole_error)?;
        target.flush().map_err(ole_error)?;
    }
    compound.flush().map_err(ole_error)?;
    Ok(compound.into_inner().into_inner())
}

/// The path of the equation stream of a compound file.
fn equation_stream<F>(compound: &CompoundFile<F>) -> Option<PathBuf> {
    compound
        .walk()
        .filter(|entry| entry.is_stream())
        .map(|entry| entry.path().to_path_buf())
        .find(|path| {
            path.file_name()
                .and_then(|name| name.to_str())
                .is_some_and(|name| {
                    STREAM_NAMES.iter().any(|candidate| name.eq_ignore_ascii_case(candidate))
                })
        })
}

fn ole_error(error: std::io::Error) -> MtefError {
    MtefError::Ole(error.to_string())
}
