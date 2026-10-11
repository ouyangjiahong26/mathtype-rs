//! Reader and writer for the equation byte stream a document object embeds.
//!
//! A document stores an equation as an OLE compound file whose `Equation Native` stream holds a
//! small object header followed by the equation itself: a one-byte version, the producer's
//! platform, product and version numbers, the application key, an options byte, and then a
//! sequence of records. Every record starts with a tag; some tags also carry an options byte,
//! and the record types that describe equation structure — lines, characters, templates, piles
//! and tables — are followed by an object list terminated by an end record.
//!
//! [`parse`] reads such a stream into the [`mt_core`] equation tree, [`write`] turns a tree back
//! into a stream, and [`read_from_ole`]/[`write_into_ole`] take the stream in and out of the
//! compound file around it. [`Report`] counts the records a parse consumed and the ones it could
//! not represent, and [`Parsed::header`] reports what the stream says about itself: the
//! application that wrote it, the encodings, the font definitions and the equation preferences.
//!
//! # Records
//!
//! The tags this crate reads are listed below with how each one reaches the tree.
//!
//! | tag | record | mapped to |
//! | --- | --- | --- |
//! | `0x00` | end | ends an object list |
//! | `0x01` | line | a row of the surrounding slot |
//! | `0x02` | character | [`mt_core::Node::Char`], or [`mt_core::Node::Space`] in the spacing style |
//! | `0x03` | template | see the template table below |
//! | `0x04` | pile | [`mt_core::Matrix`] with [`mt_core::MatrixKind::Pile`] |
//! | `0x05` | matrix | [`mt_core::Matrix`], or an array when it draws cell rules |
//! | `0x06` | embellishment | [`mt_core::Node::Accent`] over the character it follows |
//! | `0x07` | ruler | skipped |
//! | `0x08` | font style definition | the face an explicit typeface names |
//! | `0x09`–`0x0E` | sizes | the size preset of the equation, or skipped inside a slot |
//! | `0x0F`, `0x10` | colour and colour definition | skipped |
//! | `0x11`, `0x13` | font and encoding definitions | [`Header::fonts`], [`Header::encodings`] |
//! | `0x12` | equation preferences | [`Header::preferences`] |
//! | `>= 0x64` | records for later versions | skipped by their own length |
//!
//! A record this crate does not represent is skipped by its own length and counted in
//! [`Report::skipped`]; a tag that is neither defined nor carries a length is the only input
//! that makes a parse fail.
//!
//! # Templates
//!
//! A template record carries a selector, a variation, and the slots of the construct. The
//! selectors this crate maps are:
//!
//! | selectors | template | mapped to |
//! | --- | --- | --- |
//! | 0–9 | fences and intervals | [`mt_core::Fence`] |
//! | 10 | radical | [`mt_core::Sqrt`] |
//! | 11 | fraction | [`mt_core::Frac`] |
//! | 12, 13 | underbar and overbar | [`mt_core::Accent`] |
//! | 14 | arrow | [`mt_core::Accent`] |
//! | 15–22 | large operators | [`mt_core::Big`] |
//! | 23 | limits | [`mt_core::Limits`] |
//! | 24, 25 | horizontal brace and bracket | [`mt_core::Accent`] with a label, or [`mt_core::Limits`] |
//! | 26 | long division | [`mt_core::Matrix`] with [`mt_core::MatrixKind::LongDivision`] |
//! | 27–29 | scripts | [`mt_core::Scripts`], attached to the neighbouring object |
//! | 30 | bra-ket notation | [`mt_core::Fence`] around the slots |
//! | 31–34, 36 | vectors, tildes, hats, arcs, overstrikes | [`mt_core::Accent`] |
//! | 37 | box | [`mt_core::Fence`] drawing no delimiters |
//!
//! # Canonical forms
//!
//! The equation tree can express a few things this format has no record for, and this format can
//! express a few things the tree has no node for. The round trip is exact for everything else —
//! writing a tree, reading it back and writing it again reproduces the first byte stream exactly,
//! which the fidelity corpus in `tests/fidelity.rs` checks — and these are the documented
//! exceptions:
//!
//! * A style or font override ([`mt_core::Node::Styled`]) has no record of its own. A font
//!   override is written by putting the covered characters in the overridden face, so it reads
//!   back as those characters without the wrapper; a style or class override is dropped, and a
//!   parse never produces one.
//! * The atom class of a character comes from the character itself — the function style forces
//!   [`mt_core::Class::Op`], and every other face recomputes the class from the glyph — so a
//!   class the tree carries but the glyph does not spell is lost.
//! * Literal text ([`mt_core::Node::Text`]) is written as its characters and reads back as a row
//!   of characters, and a construct the reader cannot represent ([`mt_core::Node::Error`]) is
//!   written as its text for the same reason.
//! * Explicit spacing ([`mt_core::Node::Space`]) is written as a space in the spacing style; the
//!   record carries no width, so every kind reads back as [`mt_core::Space::Thin`].
//! * An alignment point ([`mt_core::Node::Align`]) has no record and is dropped.
//! * A delimiter that spells one of the drawn fences ([`mt_core::Delim::Custom`] holding one of
//!   the glyphs a fence template draws) is written with that template and reads back as that
//!   fence's delimiter, and [`mt_core::Delim::Norm`]/[`mt_core::Delim::DoubleBar`] and
//!   [`mt_core::Delim::Arrow`]/[`mt_core::Delim::Angle`] are spellings of the same template.
//! * A [`mt_core::Fence`] whose body is directly a [`mt_core::Matrix`] reads back as a matrix
//!   carrying those delimiters, which is the same construct spelled the other way round.
//! * A [`mt_core::Matrix`] reads back with an empty column list: the format aligns a whole table
//!   in one step. A table that draws cell rules reads back as
//!   [`mt_core::MatrixKind::Array`], one without as [`mt_core::MatrixKind::Matrix`], and
//!   [`mt_core::MatrixKind::Aligned`] has no spelling at all. A long division without a
//!   quotient row reads back with an empty one.
//! * A large operator template takes a single operator glyph, so a [`mt_core::Big`] whose base
//!   spells more — a function name like `lim` — is written as the limits template with its
//!   scripts as under/over material when it carries any, and as the text it spells when it
//!   carries none; both read back without the operator wrapper.
//! * The alignment of a [`mt_core::Limits`] node has no record and reads back as centred.
//! * A slot that is drawn but empty — an empty script, limit or root index — is written as an
//!   empty line and reads back as present and empty; a line that is not drawn at all reads back
//!   as an absent slot.
//! * A row holding a single item is written as that item, because the format has no way to tell
//!   a wrapped item from a bare one.
//! * The size preset of the equation is written as a size record; the text preset has no record
//!   of its own and reads back as the full preset.
//!
//! A doubled bar template reads as [`mt_core::AccentKind::Line`] and writes back as one, and a
//! hat, arc or point accent carries the glyph it draws, so the accent kinds this crate cannot
//! spell as a template are written as the nearest one and read back from that glyph.
//! ```
//! use mt_core::{Math, Node};
//! use mt_mtef::{parse_bare, write_bare};
//!
//! let math = Math::new(Node::row([Node::variable('a'), Node::symbol('+', mt_core::Class::Bin)]));
//! let bytes = write_bare(&math);
//! assert_eq!(parse_bare(&bytes).unwrap(), math);
//! ```

#![forbid(unsafe_code)]

mod ole;
mod read;
mod tables;
mod write;

pub use mt_core::Math;
pub use ole::{create_ole, read_from_ole, write_into_ole};

use std::fmt;

// -------------------------------------------------------------------------- stream

/// The equation stream as it sits inside a document object: the object header in front, then
/// the equation data. Both parts are kept so that a stream can be taken apart, understood, and
/// put back together without disturbing the header.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Stream {
    /// The object header, whose first four bytes give the offset of the equation data.
    pub preamble: Vec<u8>,
    /// The equation data, starting at the version byte.
    pub body: Vec<u8>,
}

impl Stream {
    /// Split a stream into its header and its equation data.
    ///
    /// The header opens with the offset of the equation data; some writers count that field
    /// itself, others do not, so both readings are tried.
    pub fn split(bytes: &[u8]) -> Result<Self, MtefError> {
        let field: [u8; 4] = bytes.get(..4).and_then(|raw| raw.try_into().ok()).ok_or(MtefError::Truncated)?;
        let offset = usize::try_from(u32::from_le_bytes(field)).unwrap_or(usize::MAX);
        let start = [offset, offset + 4]
            .into_iter()
            .find(|&start| bytes.get(start) == Some(&VERSION))
            .ok_or(MtefError::Header)?;
        Ok(Self {
            preamble: bytes[..start].to_vec(),
            body: bytes[start..].to_vec(),
        })
    }

    /// The stream as one byte string.
    pub fn into_bytes(self) -> Vec<u8> {
        let mut out = Vec::with_capacity(self.preamble.len() + self.body.len());
        out.extend_from_slice(&self.preamble);
        out.extend_from_slice(&self.body);
        out
    }

    /// Read the equation data of the stream.
    pub fn parse(&self) -> Result<Math, MtefError> {
        parse_bare(&self.body)
    }

    /// Read the equation and everything the stream says about itself.
    pub fn read(&self) -> Result<Parsed, MtefError> {
        read_bare(&self.body)
    }
}

/// The MTEF version this crate reads and writes.
const VERSION: u8 = 5;

// ------------------------------------------------------------------------- reading

/// Read the equation of a complete stream.
pub fn parse(stream: &[u8]) -> Result<Math, MtefError> {
    Ok(read(stream)?.math)
}

/// Read the equation of a bare equation stream, i.e. one starting at the version byte.
pub fn parse_bare(body: &[u8]) -> Result<Math, MtefError> {
    Ok(read_bare(body)?.math)
}

/// Read the equation of a complete stream, with what the stream says about itself and what the
/// reader had to leave out.
pub fn read(stream: &[u8]) -> Result<Parsed, MtefError> {
    Stream::split(stream)?.read()
}

/// Read a bare equation stream, with what the stream says about itself and what the reader had
/// to leave out.
pub fn read_bare(body: &[u8]) -> Result<Parsed, MtefError> {
    read::document(body)
}

/// What a parse recovered besides the tree.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Parsed {
    /// The equation.
    pub math: Math,
    /// What the stream says about itself.
    pub header: Header,
    /// What the reader consumed.
    pub report: Report,
}

/// The stream header and the records that describe the equation rather than build it.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Header {
    /// Version of the equation format, always 5 for a stream this crate reads.
    pub version: u8,
    /// Platform the stream was written on: 0 for the Macintosh, 1 for Windows.
    pub platform: u8,
    /// Product that wrote the stream: 0 for MathType, 1 for Equation Editor.
    pub product: u8,
    /// Version of that product.
    pub product_version: u8,
    /// Subversion of that product.
    pub product_subversion: u8,
    /// Application key of the writing application, for example `DSMT6`.
    pub creator: String,
    /// Equation options; bit 0 is set for an inline equation.
    pub options: u8,
    /// Encoding definitions, in the order they were read.
    pub encodings: Vec<String>,
    /// Font definitions, in the order they were read.
    pub fonts: Vec<FontDef>,
    /// The typesizes, spacing and styles the equation was written with.
    pub preferences: Preferences,
}

/// A font name together with the encoding its characters are written in.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct FontDef {
    /// Index of the encoding definition this font is written in.
    pub encoding: usize,
    /// Name of the font.
    pub name: String,
}

/// One character style: the font it is drawn in and its bold and italic bits.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct StyleDef {
    /// Index of the font definition the style is drawn in, or `None` when the style is unused.
    pub font: Option<usize>,
    /// Character style bits: bit 0 is bold, bit 1 is italic.
    pub style: u8,
}

impl StyleDef {
    /// Whether the style is bold.
    pub fn bold(self) -> bool {
        self.style & 0x01 != 0
    }

    /// Whether the style is italic.
    pub fn italic(self) -> bool {
        self.style & 0x02 != 0
    }
}

/// A dimensional value as the equation was authored, kept as text.
///
/// The stream records the value the author typed rather than a rounded number, so it is reported
/// as a string together with the unit it is in. A digit nibble the format does not define is
/// reported as `?`.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Dimension {
    /// Unit the value is in.
    pub unit: Unit,
    /// The value as written, for example `12` or `0.5`.
    pub value: String,
}

/// A dimensional unit.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum Unit {
    /// Inches.
    Inches,
    /// Centimetres.
    Centimeters,
    /// Points.
    #[default]
    Points,
    /// Picas.
    Picas,
    /// Percent of the size the value is relative to.
    Percent,
    /// A unit the format does not define, reported as the nibble that carried it.
    Other(u8),
}

impl Unit {
    /// The nibble this unit is written as.
    pub(crate) fn code(self) -> u8 {
        match self {
            Unit::Inches => 0,
            Unit::Centimeters => 1,
            Unit::Points => 2,
            Unit::Picas => 3,
            Unit::Percent => 4,
            Unit::Other(code) => code,
        }
    }

    /// The unit a nibble stands for.
    pub(crate) fn from_code(code: u8) -> Self {
        match code {
            0 => Unit::Inches,
            1 => Unit::Centimeters,
            2 => Unit::Points,
            3 => Unit::Picas,
            4 => Unit::Percent,
            other => Unit::Other(other),
        }
    }
}

/// The preferences an equation carries: its typesize table, its spacing table and its styles.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Preferences {
    /// Typesizes, in the order the stream lists them.
    pub sizes: Vec<Dimension>,
    /// Spacing values, in the order the stream lists them.
    pub spaces: Vec<Dimension>,
    /// One entry per character style, in typeface order.
    pub styles: Vec<StyleDef>,
}

/// What a parse consumed, and how much of it reached the tree.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Report {
    /// Records read, including the ends of object lists and the records that only describe the
    /// equation.
    pub records: usize,
    /// Records read whose contents were dropped: records for later versions, rulers, colours,
    /// size records inside a slot, and embellishments with no accent to map to.
    pub skipped: usize,
}

// ------------------------------------------------------------------------- writing

/// Write an equation as a complete stream, object header included.
pub fn write(math: &Math) -> Vec<u8> {
    write::stream(math)
}

/// Write an equation as a bare equation stream, without the object header.
pub fn write_bare(math: &Math) -> Vec<u8> {
    write::document(math)
}

/// Write an equation as a complete stream, declaring the given equation preferences.
///
/// The preferences are the sizes, spacings and styles the stream lists before the equation, so
/// a stream saved with the preferences a reader captured round-trips unchanged.
pub fn write_with_preferences(math: &Math, preferences: &Preferences) -> Vec<u8> {
    write::stream_with_preferences(math, preferences)
}

/// Write an equation as a bare equation stream, declaring the given equation preferences.
pub fn write_bare_with_preferences(math: &Math, preferences: &Preferences) -> Vec<u8> {
    write::document_with_preferences(math, preferences)
}

// --------------------------------------------------------------------------- errors

/// Why a stream could not be read or written.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum MtefError {
    /// The stream ended in the middle of a record.
    Truncated,
    /// The stream header does not open with the version this crate reads.
    Version(u8),
    /// The header does not say where the equation data starts.
    Header,
    /// A record tag that is neither defined nor carries its own length.
    UnknownRecord(u8),
    /// The object lists nest deeper than the reader accepts.
    Nesting,
    /// The compound file around the equation could not be read or written.
    Ole(String),
}

impl fmt::Display for MtefError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            MtefError::Truncated => f.write_str("the stream ends in the middle of a record"),
            MtefError::Version(version) => {
                write!(f, "unsupported equation format version {version}")
            }
            MtefError::Header => f.write_str("the stream header does not give the offset of the equation data"),
            MtefError::UnknownRecord(tag) => write!(f, "unknown record tag {tag:#04X}"),
            MtefError::Nesting => f.write_str("the object lists nest too deeply"),
            MtefError::Ole(reason) => write!(f, "compound file error: {reason}"),
        }
    }
}

impl std::error::Error for MtefError {}

impl From<std::io::Error> for MtefError {
    fn from(error: std::io::Error) -> Self {
        MtefError::Ole(error.to_string())
    }
}
