//! Everything that can go wrong while reading or editing a document.

use std::fmt;

/// The failure modes of [`crate::Document`](crate::Document) and the parts it is built from.
#[derive(Debug)]
pub enum DocxError {
    /// The zip container could not be opened, read, or written.
    Zip(String),
    /// A part's XML is malformed, or does not have the shape this crate expects.
    Xml(String),
    /// A part this crate insists on — `word/document.xml`, for instance — is missing.
    Missing(String),
    /// A part is not valid UTF-8. Word documents always are.
    NotUtf8(String),
}

impl fmt::Display for DocxError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            DocxError::Zip(message) => write!(f, "zip container: {message}"),
            DocxError::Xml(message) => write!(f, "xml: {message}"),
            DocxError::Missing(name) => write!(f, "the document has no part named {name}"),
            DocxError::NotUtf8(name) => write!(f, "the part {name} is not valid UTF-8"),
        }
    }
}

impl std::error::Error for DocxError {}

/// Reading XML events failed, with the position in the part appended by the caller.
pub(crate) fn xml_error(error: impl fmt::Display) -> DocxError {
    DocxError::Xml(error.to_string())
}
