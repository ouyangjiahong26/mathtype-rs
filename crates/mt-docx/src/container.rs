//! The zip container of a .docx file: read parts, change parts, write everything back.
//!
//! The one guarantee worth stating: a part this layer has not been asked to change is written
//! back with its compressed bytes copied verbatim, so untouched parts are byte-for-byte identical
//! after a read–write cycle — not merely equal after decompression. Changed and added parts are
//! deflated afresh and appended in place or at the end of the archive.

use std::io::{Cursor, Read, Write};

use zip::CompressionMethod;
use zip::read::ZipArchive;
use zip::write::SimpleFileOptions;

use crate::error::DocxError;

/// The part entries of a container, in archive order.
pub struct Package {
    /// The bytes of the archive as it was opened, the source of every untouched entry.
    source: Vec<u8>,
    entries: Vec<Entry>,
}

struct Entry {
    name: String,
    /// Index into the source archive; `usize::MAX` for parts added later.
    index: usize,
    data: EntryData,
}

enum EntryData {
    /// Still the source bytes; copy raw on write.
    Original,
    /// Replaced content, written as a fresh deflated entry.
    Fresh(Vec<u8>),
}

fn zip_error(error: impl std::fmt::Display) -> DocxError {
    DocxError::Zip(error.to_string())
}

impl Package {
    /// Reads a whole container into memory. Duplicate part names keep the first entry, the
    /// convention Word itself follows.
    pub fn open(bytes: &[u8]) -> Result<Self, DocxError> {
        let mut archive = ZipArchive::new(Cursor::new(bytes)).map_err(zip_error)?;
        let mut entries = Vec::with_capacity(archive.len());
        for index in 0..archive.len() {
            let file = archive.by_index_raw(index).map_err(zip_error)?;
            let name = file.name().map_err(zip_error)?.into_owned();
            if entries.iter().any(|entry: &Entry| entry.name == name) {
                continue;
            }
            entries.push(Entry {
                name,
                index,
                data: EntryData::Original,
            });
        }
        Ok(Self {
            source: bytes.to_vec(),
            entries,
        })
    }

    /// The decompressed bytes of a part, or `None` when the container holds no such part.
    pub fn part(&self, name: &str) -> Option<Result<Vec<u8>, DocxError>> {
        let entry = self.entries.iter().find(|entry| entry.name == name)?;
        Some(match &entry.data {
            EntryData::Fresh(data) => Ok(data.clone()),
            EntryData::Original => {
                let mut archive = ZipArchive::new(Cursor::new(&self.source[..])).ok()?;
                let mut file = archive.by_index(entry.index).ok()?;
                let mut data = Vec::new();
                match file.read_to_end(&mut data) {
                    Ok(_) => Ok(data),
                    Err(error) => Err(zip_error(error)),
                }
            }
        })
    }

    /// The decompressed bytes of a part, with a missing part reported as an error.
    pub fn required(&self, name: &str) -> Result<Vec<u8>, DocxError> {
        self.part(name)
            .unwrap_or(Err(DocxError::Missing(name.to_owned())))
    }

    /// Adds a part, or replaces the content of one that exists.
    pub fn set_part(&mut self, name: &str, data: Vec<u8>) {
        if let Some(entry) = self.entries.iter_mut().find(|entry| entry.name == name) {
            entry.data = EntryData::Fresh(data);
        } else {
            self.entries.push(Entry {
                name: name.to_owned(),
                index: usize::MAX,
                data: EntryData::Fresh(data),
            });
        }
    }

    /// Drops a part from the container.
    pub fn remove_part(&mut self, name: &str) {
        self.entries.retain(|entry| entry.name != name);
    }

    /// The part names, in archive order with later additions last.
    pub fn part_names(&self) -> impl Iterator<Item = &str> {
        self.entries.iter().map(|entry| entry.name.as_str())
    }

    /// Writes the container: untouched entries raw-copied, fresh ones deflated.
    pub fn into_bytes(self) -> Result<Vec<u8>, DocxError> {
        let mut writer = zip::ZipWriter::new(Cursor::new(Vec::new()));
        let options = SimpleFileOptions::default().compression_method(CompressionMethod::Deflated);
        for entry in &self.entries {
            match &entry.data {
                EntryData::Original => {
                    let mut archive =
                        ZipArchive::new(Cursor::new(&self.source[..])).map_err(zip_error)?;
                    let file = archive.by_index_raw(entry.index).map_err(zip_error)?;
                    writer.raw_copy_file(file).map_err(zip_error)?;
                }
                EntryData::Fresh(data) => {
                    writer
                        .start_file(entry.name.as_str(), options)
                        .map_err(zip_error)?;
                    writer.write_all(data).map_err(zip_error)?;
                }
            }
        }
        writer
            .finish()
            .map_err(zip_error)
            .map(|cursor| cursor.into_inner())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use zip::write::SimpleFileOptions;

    /// A container with a deflated text part and a stored binary part.
    fn sample() -> Vec<u8> {
        let mut writer = zip::ZipWriter::new(Cursor::new(Vec::new()));
        let deflated = SimpleFileOptions::default().compression_method(CompressionMethod::Deflated);
        let stored = SimpleFileOptions::default().compression_method(CompressionMethod::Stored);
        writer.start_file("a.txt", deflated).unwrap();
        writer.write_all(b"alpha").unwrap();
        writer.start_file("dir/b.bin", stored).unwrap();
        writer.write_all(&[0, 1, 2, 250]).unwrap();
        writer.finish().unwrap().into_inner()
    }

    fn parts(bytes: &[u8]) -> Vec<(String, Vec<u8>)> {
        let mut archive = ZipArchive::new(Cursor::new(bytes)).unwrap();
        (0..archive.len())
            .map(|index| {
                let mut file = archive.by_index(index).unwrap();
                let mut data = Vec::new();
                file.read_to_end(&mut data).unwrap();
                (file.name().unwrap().into_owned(), data)
            })
            .collect()
    }

    #[test]
    fn untouched_parts_come_back_byte_identical() {
        let sample = sample();
        let written = Package::open(&sample).unwrap().into_bytes().unwrap();
        assert_eq!(sample, written);
    }

    #[test]
    fn replacing_one_part_leaves_the_other_alone() {
        let sample = sample();
        let mut package = Package::open(&sample).unwrap();
        package.set_part("a.txt", b"beta".to_vec());
        let written = package.into_bytes().unwrap();
        assert_eq!(parts(&written)[0], ("a.txt".to_owned(), b"beta".to_vec()));
        assert_eq!(
            parts(&written)[1],
            ("dir/b.bin".to_owned(), vec![0, 1, 2, 250])
        );
    }

    #[test]
    fn added_parts_append_and_missing_parts_report_none() {
        let mut package = Package::open(&sample()).unwrap();
        package.set_part("c.txt", b"gamma".to_vec());
        assert!(package.part("nope").is_none());
        let written = package.into_bytes().unwrap();
        assert_eq!(parts(&written).len(), 3);
    }
}
