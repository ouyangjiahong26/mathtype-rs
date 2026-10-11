//! Reader and writer for Office Math (OMML), the equation language of Word and PowerPoint.
//!
//! [`parse`] reads an `<m:oMath>` or `<m:oMathPara>` fragment into an [`mt_core::Math`] tree;
//! [`write`] returns an `<m:oMath>` fragment and [`write_paragraph`] wraps it in `<m:oMathPara>`
//! with a justification, both with the `m:` namespace declared so they can be pasted into a Word
//! document.
//!
//! # Canonical forms
//!
//! OMML is a run-based sequence language with far fewer structural hooks than the tree, so a few
//! things cannot be spelled exactly. The round trip is exact for everything else, and these are
//! the documented exceptions:
//!
//! * OMML has no row element: a sequence of items is written inline. A [`mt_core::Node::Row`]
//!   holding exactly one item therefore reads back as that item.
//! * Runs do not carry an atom class. A [`mt_core::Char`] class is derived from the character
//!   itself, so a class that disagrees with the character reads back as the character's own.
//! * [`mt_core::Font::Symbol`] and [`mt_core::Font::Extra`] have no `m:scr` value; they read back
//!   as [`mt_core::Font::Upright`].
//! * `m:nor` (normal text) is what distinguishes literal text from a variable, so a text run
//!   reads back as [`mt_core::Node::Text`] and never as a character.
//! * `m:func` has no node of its own: it reads as a row of the function name and its argument, so
//!   it writes back as a plain sequence.
//! * `m:d`'s `m:sepChr` has no field, and several `m:e` groups read as one body, so the separator
//!   is read but dropped.
//! * `m:sPre` keeps its pre-position in markup only; scripts read as a normal
//!   [`mt_core::Node::Scripts`].
//! * `m:m` cannot say what kind of table it is, and cannot draw cell rules: a
//!   [`mt_core::Matrix`] reads back as [`mt_core::MatrixKind::Matrix`] with `rules` false, except
//!   that `m:eqArr` reads as [`mt_core::MatrixKind::Aligned`]. `m:baseJc` and `m:plcHide` have no
//!   field and are read but dropped.
//! * [`mt_core::Limits`] alignment has no spelling; it reads back centred.
//! * A brace-like accent is drawn on the side its kind names, so `OverBrace`/`OverBracket`/
//!   `OverParen` read back above the base and their `Under…` variants below it; a kind that
//!   disagrees with its side reads back as the variant matching the side.
//! * [`mt_core::Styled`] carries a style and a face that `m:box` cannot hold; only
//!   [`mt_core::Class::Inner`] survives, as `m:borderBox`.
//! * The equation's size preset is not an OMML concept: [`mt_core::Math::style`] always reads
//!   back as [`mt_core::MathStyle::Full`], and `m:oMathPara`'s justification is not kept.
//! * [`mt_core::Node::Error`] has no OMML element; it writes as literal text and reads back as
//!   [`mt_core::Node::Text`].
//! * The width of a [`mt_core::Node::Space`] is written as the matching Unicode space character;
//!   the negative widths become zero-width spaces, which reserve no space when rendered.

#![forbid(unsafe_code)]

mod dom;
mod parse;
mod write;

pub use parse::parse;
pub use write::{write, write_paragraph};

use mt_core::{AccentKind, Class, Delim, Font, Side, Space};

/// The Office Math namespace URI, declared by the writers.
pub const NAMESPACE: &str = "http://schemas.openxmlformats.org/officeDocument/2006/math";

/// Why an OMML document could not be read.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ParseError {
    /// The document is not well-formed XML, or a reference or text node could not be decoded.
    Xml(String),
    /// The root element is neither `<m:oMath>` nor `<m:oMathPara>`.
    Root(String),
    /// An element that has no counterpart in the equation tree.
    UnknownElement(String),
    /// A known element is used in a way the equation tree cannot represent.
    Structure(String),
    /// An attribute value could not be interpreted.
    InvalidAttribute { element: String, attribute: String, value: String },
}

impl std::fmt::Display for ParseError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ParseError::Xml(reason) => write!(f, "malformed XML: {reason}"),
            ParseError::Root(name) => {
                write!(f, "expected an <oMath> or <oMathPara> root element, found <{name}>")
            }
            ParseError::UnknownElement(name) => write!(f, "unsupported Office Math element <{name}>"),
            ParseError::Structure(detail) => f.write_str(detail),
            ParseError::InvalidAttribute { element, attribute, value } => {
                write!(f, "unsupported value {value:?} for {attribute} on <{element}>")
            }
        }
    }
}

impl std::error::Error for ParseError {}

/// Characters that take limits above and below by default, i.e. the large operators.
pub(crate) fn is_large_op(ch: char) -> bool {
    "\u{2211}\u{220F}\u{222B}\u{222C}\u{222D}\u{222E}\u{222F}\u{2230}\u{22C3}\u{22C2}\u{2210}\u{2A01}\u{2A02}\u{22C1}\u{22C0}"
        .contains(ch)
}

/// Atom class Office Math does not record and therefore infers from the character.
pub(crate) fn class_of(ch: char) -> Class {
    const BIN: &str = "+-*/\u{2212}\u{00B1}\u{2213}\u{00D7}\u{00F7}\u{22C5}\u{2217}\u{00B7}\u{2218}\u{2216}\u{2227}\u{2228}\u{2295}\u{2296}\u{2297}\u{2299}";
    const REL: &str = "=<>\u{2264}\u{2265}\u{2260}\u{2248}\u{2261}\u{2208}\u{2209}\u{2192}\u{2190}\u{21D2}\u{21D4}\u{223C}\u{221D}\u{2282}\u{2283}\u{2286}\u{2287}\u{2245}\u{225C}\u{2323}\u{2254}\u{2255}";
    const OPEN: &str = "([{\u{27E8}\u{230A}\u{2308}\u{27E6}\u{301A}";
    const CLOSE: &str = ")]}\u{27E9}\u{230B}\u{2309}\u{27E7}\u{301B}";
    const PUNCT: &str = ",;:";
    if is_large_op(ch) {
        Class::Op
    } else if OPEN.contains(ch) {
        Class::Open
    } else if CLOSE.contains(ch) {
        Class::Close
    } else if PUNCT.contains(ch) {
        Class::Punct
    } else if BIN.contains(ch) {
        Class::Bin
    } else if REL.contains(ch) {
        Class::Rel
    } else {
        Class::Ord
    }
}

/// The character that stands for an accent kind, the inverse of [`accent_kind`].
pub(crate) fn accent_char(kind: AccentKind) -> char {
    match kind {
        AccentKind::Hat => '^',
        AccentKind::WideHat => '\u{0302}',
        AccentKind::Tilde => '~',
        AccentKind::WideTilde => '\u{0303}',
        AccentKind::Bar => '\u{00AF}',
        AccentKind::Vector => '\u{20D7}',
        AccentKind::Line => '\u{203E}',
        AccentKind::Strike => '\u{0336}',
        AccentKind::Dot => '\u{02D9}',
        AccentKind::DoubleDot => '\u{00A8}',
        AccentKind::TripleDot => '\u{20DB}',
        AccentKind::Breve => '\u{02D8}',
        AccentKind::Check => '\u{02C7}',
        AccentKind::Acute => '\u{00B4}',
        AccentKind::Grave => '`',
        AccentKind::Circle => '\u{02DA}',
        AccentKind::OverBrace => '\u{23DE}',
        AccentKind::UnderBrace => '\u{23DF}',
        AccentKind::OverBracket => '\u{23B4}',
        AccentKind::UnderBracket => '\u{23B5}',
        AccentKind::OverParen => '\u{23DC}',
        AccentKind::UnderParen => '\u{23DD}',
        AccentKind::ArrowRight => '\u{2192}',
        AccentKind::ArrowLeft => '\u{2190}',
    }
}

/// Inverse of [`accent_char`], also accepting the plain caret for a hat.
pub(crate) fn accent_kind(ch: char) -> Option<AccentKind> {
    const KINDS: [AccentKind; 24] = [
        AccentKind::Hat,
        AccentKind::WideHat,
        AccentKind::Tilde,
        AccentKind::WideTilde,
        AccentKind::Bar,
        AccentKind::Vector,
        AccentKind::Line,
        AccentKind::Strike,
        AccentKind::Dot,
        AccentKind::DoubleDot,
        AccentKind::TripleDot,
        AccentKind::Breve,
        AccentKind::Check,
        AccentKind::Acute,
        AccentKind::Grave,
        AccentKind::Circle,
        AccentKind::OverBrace,
        AccentKind::UnderBrace,
        AccentKind::OverBracket,
        AccentKind::UnderBracket,
        AccentKind::OverParen,
        AccentKind::UnderParen,
        AccentKind::ArrowRight,
        AccentKind::ArrowLeft,
    ];
    if ch == '\u{02C6}' {
        return Some(AccentKind::Hat);
    }
    KINDS.into_iter().find(|kind| accent_char(*kind) == ch)
}

/// Whether the accent character itself is drawn below the base, whatever `m:pos` says.
pub(crate) fn accent_is_under(ch: char) -> bool {
    matches!(ch, '\u{23DF}' | '\u{23DD}' | '\u{23B5}')
}

/// Whether an accent kind is one of the brace-like shapes Office Math draws with `m:groupChr`.
pub(crate) fn is_group_character(kind: AccentKind) -> bool {
    matches!(
        kind,
        AccentKind::OverBrace
            | AccentKind::UnderBrace
            | AccentKind::OverBracket
            | AccentKind::UnderBracket
            | AccentKind::OverParen
            | AccentKind::UnderParen
    )
}

/// The brace-like accent a group character draws, accepting both the plain and the stretched
/// spellings Office Math uses.
pub(crate) fn group_character_kind(ch: char) -> Option<AccentKind> {
    Some(match ch {
        '{' | '}' => AccentKind::OverBrace,
        '[' | ']' => AccentKind::OverBracket,
        '(' | ')' => AccentKind::OverParen,
        other => accent_kind(other).filter(|kind| is_group_character(*kind))?,
    })
}

/// The Unicode space a width is written as. Office Math has no spacing element, so a run holding
/// the matching space character stands in for one.
pub(crate) fn space_text(space: Space) -> &'static str {
    match space {
        Space::Thin => "\u{2009}",
        Space::Medium => "\u{2005}",
        Space::Thick => "\u{2008}",
        Space::Sixteenth => "\u{2004}",
        Space::Quad => "\u{2003}",
        Space::DoubleQuad => "\u{2003}\u{2003}",
        Space::NegativeThin => "\u{200B}",
        Space::NegativeMedium => "\u{200B}\u{200B}",
        Space::NegativeThick => "\u{200B}\u{200B}\u{200B}",
    }
}

/// Inverse of [`space_text`].
pub(crate) fn space_of_text(text: &str) -> Option<Space> {
    Some(match text {
        "\u{2009}" => Space::Thin,
        "\u{2005}" => Space::Medium,
        "\u{2008}" => Space::Thick,
        "\u{2004}" => Space::Sixteenth,
        "\u{2003}" => Space::Quad,
        "\u{2003}\u{2003}" => Space::DoubleQuad,
        "\u{200B}" => Space::NegativeThin,
        "\u{200B}\u{200B}" => Space::NegativeMedium,
        "\u{200B}\u{200B}\u{200B}" => Space::NegativeThick,
        _ => return None,
    })
}

/// The `m:scr` and `m:sty` values that spell a face, the inverse of [`font_of_properties`].
pub(crate) fn font_properties(font: Font) -> (&'static str, &'static str) {
    match font {
        Font::Italic => ("roman", "i"),
        Font::Upright => ("roman", "p"),
        Font::Bold => ("roman", "b"),
        Font::BoldItalic => ("roman", "bi"),
        Font::SansSerif => ("sans-serif", "p"),
        Font::SansSerifItalic => ("sans-serif", "i"),
        Font::SansSerifBold => ("sans-serif", "b"),
        Font::SansSerifBoldItalic => ("sans-serif", "bi"),
        Font::Monospace => ("monospace", "p"),
        Font::Script => ("script", "p"),
        Font::BoldScript => ("script", "b"),
        Font::Fraktur => ("fraktur", "p"),
        Font::BoldFraktur => ("fraktur", "b"),
        Font::DoubleStruck => ("double-struck", "p"),
        // These two faces have no `m:scr` value; they are written as plain roman.
        Font::Symbol | Font::Extra => ("roman", "p"),
    }
}

/// Inverse of [`font_properties`]: `None` when a value is not one Office Math defines.
pub(crate) fn font_of_properties(script: Option<&str>, style: Option<&str>, normal_text: bool) -> Option<Font> {
    let (bold, italic) = match style {
        None => (false, !normal_text),
        Some("p") => (false, false),
        Some("b") => (true, false),
        Some("i") => (false, true),
        Some("bi") => (true, true),
        Some(_) => return None,
    };
    Some(match script.unwrap_or("roman") {
        "roman" => match (bold, italic) {
            (false, false) => Font::Upright,
            (false, true) => Font::Italic,
            (true, false) => Font::Bold,
            (true, true) => Font::BoldItalic,
        },
        "script" => {
            if bold {
                Font::BoldScript
            } else {
                Font::Script
            }
        }
        "fraktur" => {
            if bold {
                Font::BoldFraktur
            } else {
                Font::Fraktur
            }
        }
        "double-struck" => Font::DoubleStruck,
        "sans-serif" => match (bold, italic) {
            (false, false) => Font::SansSerif,
            (false, true) => Font::SansSerifItalic,
            (true, false) => Font::SansSerifBold,
            (true, true) => Font::SansSerifBoldItalic,
        },
        "monospace" => Font::Monospace,
        _ => return None,
    })
}

/// The delimiter pair spelled by a `m:begChr`/`m:endChr` pair, `None` meaning "draw nothing".
pub(crate) fn delim_pair(open: Option<char>, close: Option<char>) -> (Delim, Delim) {
    match (open, close) {
        (None, None) => (Delim::None, Delim::None),
        (Some('('), Some(')')) => (Delim::Paren, Delim::Paren),
        (Some('['), Some(']')) => (Delim::Bracket, Delim::Bracket),
        (Some('{'), Some('}')) => (Delim::Brace, Delim::Brace),
        (Some('|'), Some('|')) => (Delim::Abs, Delim::Abs),
        (Some('\u{2016}'), Some('\u{2016}')) => (Delim::Norm, Delim::Norm),
        (Some('\u{27E8}'), Some('\u{27E9}')) => (Delim::Angle, Delim::Angle),
        (Some('\u{230A}'), Some('\u{230B}')) => (Delim::Floor, Delim::Floor),
        (Some('\u{2308}'), Some('\u{2309}')) => (Delim::Ceil, Delim::Ceil),
        (Some('\u{2192}'), Some('\u{2192}')) => (Delim::Arrow, Delim::Arrow),
        (Some(open), Some(close)) => (Delim::Custom(open), Delim::Custom(close)),
        (Some(open), None) => (Delim::Custom(open), Delim::None),
        (None, Some(close)) => (Delim::None, Delim::Custom(close)),
    }
}

/// The opening and closing characters a delimiter pair is written with.
pub(crate) fn delim_chars(open: Delim, close: Delim) -> (Option<char>, Option<char>) {
    let open = match open {
        Delim::None => None,
        Delim::Arrow => Some('\u{2192}'),
        other => other.open(),
    };
    let close = match close {
        Delim::None => None,
        Delim::Arrow => Some('\u{2192}'),
        other => other.close(),
    };
    (open, close)
}

/// Horizontal alignment as Office Math writes it.
pub(crate) fn side_name(side: Side) -> &'static str {
    match side {
        Side::Center => "center",
        Side::Left => "left",
        Side::Right => "right",
    }
}

/// Inverse of [`side_name`], also accepting the group spelling of "centre".
pub(crate) fn side_of_name(name: &str) -> Option<Side> {
    match name.trim() {
        "center" | "centerGroup" => Some(Side::Center),
        "left" => Some(Side::Left),
        "right" => Some(Side::Right),
        _ => None,
    }
}

/// Fold per-column alignments into the tree's `align`/`columns` pair, exactly as MathML does, so
/// that both readers agree on one canonical spelling.
pub(crate) fn fold_columns(columns: Vec<Side>) -> (Side, Vec<Side>) {
    match columns.split_first() {
        None => (Side::Center, Vec::new()),
        Some((first, rest)) if rest.iter().all(|side| side == first) => (*first, Vec::new()),
        Some(_) => (Side::Center, columns),
    }
}
