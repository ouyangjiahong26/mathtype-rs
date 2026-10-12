//! Reader and writer for Presentation MathML.
//!
//! [`parse`] turns a MathML 3 presentation document into an [`mt_core::Math`] tree, with or
//! without the conventional `m:` namespace prefix; [`write`] turns a tree back into a MathML
//! document that [`parse`] reads back to an equal tree, up to the canonical forms listed below.
//!
//! Every element, attribute and class decision lives in this crate; `mt_core` is only read.
//!
//! # Canonical forms
//!
//! MathML can express a few things the equation tree cannot, and the equation tree can express a
//! few things MathML has no spelling for. The round trip is exact for everything else, and these
//! are the documented exceptions:
//!
//! * [`mt_core::Delim::DoubleBar`] and [`mt_core::Delim::Norm`] are both the `‖` character, so
//!   `DoubleBar` reads back as `Norm`.
//! * `Delim::Arrow` is written as `→`; a `Delim::Custom('→')` reads back as `Delim::Arrow`.
//! * A fence whose two delimiters are not a matched pair reads back as two custom characters,
//!   because the pair is recognised from the characters alone.
//! * A [`mt_core::Node::Fence`] whose body is directly a [`mt_core::Node::Matrix`] is written as
//!   a table inside delimiters and reads back as a `Matrix` carrying those delimiters, which is
//!   the same construct spelled the other way round.
//! * `mfrac` attributes other than `bevelled`/`linethickness` (`numalign`, `denalign`) and
//!   `mo`'s `stretchy` have no field in the tree and are read but dropped.
//! * `mphantom`, `mpadded` and `menclose` have no node of their own; they read as a
//!   [`mt_core::Node::Styled`] wrapper (with `menclose` marked as an inner atom), which is what
//!   `write` emits for a `Styled` node, so the tree round trips even though the phantom/padding
//!   and enclosure decorations do not.
//! * `ms` reads as literal text; the quotation marks a renderer adds are not part of the tree.
//! * An empty slot inside a script, limit or root index reads as "absent", because MathML writes
//!   both as an empty `mrow`. Only the *empty* form collapses this way.
//! * A `Styled` node that carries only a style is lifted to [`mt_core::Math::style`] when it is
//!   the only child of `math`, which is how `write` records a size preset above full size.
//! * Scripts, limits and accents whose base is a large operator character read as
//!   [`mt_core::Node::Big`], since that is the tree's spelling for an operator with limits.
//!
//! # Slot order
//!
//! Every element writes its children in the order its specification names, which is not always
//! the field order of the tree: `mroot` takes its base first and its index second, while
//! [`mt_core::Sqrt`] lists the index first. Reading and writing both follow the markup order, so
//! the two round trip exactly.
//!
//! # Limits
//!
//! The reader is the boundary this crate stands at, so it refuses input it cannot handle instead
//! of panicking: elements nested deeper than 128 levels are rejected with
//! [`ParseError::Nesting`] before any recursive walk begins.

#![forbid(unsafe_code)]

mod dom;
mod parse;
mod write;

pub use parse::parse;
pub use write::write;

use mt_core::{AccentKind, Class, Delim, Font, MatrixKind, Side, Space};

/// The MathML namespace URI, declared by [`write`] on the root element.
pub const NAMESPACE: &str = "http://www.w3.org/1998/Math/MathML";

/// Why a MathML document could not be read.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ParseError {
    /// The document is not well-formed XML, or a reference or text node could not be decoded.
    Xml(String),
    /// The root element is not a MathML `math` element.
    Root(String),
    /// An element that has no counterpart in the equation tree.
    UnknownElement(String),
    /// A known element is used in a way the equation tree cannot represent.
    Structure(String),
    /// The elements nest deeper than the reader accepts (128 levels); see the crate docs.
    Nesting,
    /// An attribute value could not be interpreted.
    InvalidAttribute {
        element: String,
        attribute: String,
        value: String,
    },
}

impl std::fmt::Display for ParseError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ParseError::Xml(reason) => write!(f, "malformed XML: {reason}"),
            ParseError::Root(name) => write!(f, "expected a <math> root element, found <{name}>"),
            ParseError::UnknownElement(name) => write!(f, "unsupported MathML element <{name}>"),
            ParseError::Structure(detail) => f.write_str(detail),
            ParseError::Nesting => write!(f, "elements nest deeper than the reader accepts"),
            ParseError::InvalidAttribute {
                element,
                attribute,
                value,
            } => {
                write!(
                    f,
                    "unsupported value {value:?} for {attribute} on <{element}>"
                )
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

/// Atom class MathML's own defaults would give the bare character, used to decide when a written
/// `mo` needs an explicit class and to read a bare `mo` back.
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

/// The class an `mo` marked `fence="true"` takes, for characters that can act as a fence.
pub(crate) fn fence_class(ch: char) -> Option<Class> {
    if "([{\u{27E8}\u{230A}\u{2308}\u{27E6}|".contains(ch) {
        Some(Class::Open)
    } else if ")]}\u{27E9}\u{230B}\u{2309}\u{27E7}|".contains(ch) {
        Some(Class::Close)
    } else {
        None
    }
}

/// Class marker carried in a `class` attribute when the tree's class differs from MathML's own.
pub(crate) fn class_marker(class: Class) -> &'static str {
    match class {
        Class::Ord => "mt-class-ord",
        Class::Op => "mt-class-op",
        Class::Bin => "mt-class-bin",
        Class::Rel => "mt-class-rel",
        Class::Open => "mt-class-open",
        Class::Close => "mt-class-close",
        Class::Punct => "mt-class-punct",
        Class::Inner => "mt-class-inner",
    }
}

/// Inverse of [`class_marker`].
pub(crate) fn class_of_marker(token: &str) -> Option<Class> {
    Some(match token {
        "mt-class-ord" => Class::Ord,
        "mt-class-op" => Class::Op,
        "mt-class-bin" => Class::Bin,
        "mt-class-rel" => Class::Rel,
        "mt-class-open" => Class::Open,
        "mt-class-close" => Class::Close,
        "mt-class-punct" => Class::Punct,
        "mt-class-inner" => Class::Inner,
        _ => return None,
    })
}

/// Marker for the two faces `mathvariant` cannot name.
pub(crate) fn font_marker(font: Font) -> Option<&'static str> {
    match font {
        Font::Symbol => Some("mt-font-symbol"),
        Font::Extra => Some("mt-font-extra"),
        _ => None,
    }
}

/// Inverse of [`font_marker`].
pub(crate) fn font_of_marker(token: &str) -> Option<Font> {
    match token {
        "mt-font-symbol" => Some(Font::Symbol),
        "mt-font-extra" => Some(Font::Extra),
        _ => None,
    }
}

/// The `mathvariant` value naming `font`.
pub(crate) fn variant_of(font: Font) -> Option<&'static str> {
    Some(match font {
        Font::Italic => "italic",
        Font::Upright => "normal",
        Font::Bold => "bold",
        Font::BoldItalic => "bold-italic",
        Font::SansSerif => "sans-serif",
        Font::SansSerifBold => "bold-sans-serif",
        Font::SansSerifItalic => "sans-serif-italic",
        Font::SansSerifBoldItalic => "sans-serif-bold-italic",
        Font::Monospace => "monospace",
        Font::Script => "script",
        Font::BoldScript => "bold-script",
        Font::Fraktur => "fraktur",
        Font::BoldFraktur => "bold-fraktur",
        Font::DoubleStruck => "double-struck",
        Font::Symbol | Font::Extra => return None,
    })
}

/// Inverse of [`variant_of`]. The MathML values that only vary the glyph shape map to
/// [`Font::Upright`].
pub(crate) fn font_of_variant(value: &str) -> Option<Font> {
    Some(match value {
        "italic" => Font::Italic,
        "normal" => Font::Upright,
        "bold" => Font::Bold,
        "bold-italic" => Font::BoldItalic,
        "sans-serif" => Font::SansSerif,
        "bold-sans-serif" => Font::SansSerifBold,
        "sans-serif-italic" => Font::SansSerifItalic,
        "sans-serif-bold-italic" => Font::SansSerifBoldItalic,
        "monospace" => Font::Monospace,
        "script" => Font::Script,
        "bold-script" => Font::BoldScript,
        "fraktur" => Font::Fraktur,
        "bold-fraktur" => Font::BoldFraktur,
        "double-struck" => Font::DoubleStruck,
        "initial" | "tailed" | "looped" | "stretched" => Font::Upright,
        _ => return None,
    })
}

/// Named length MathML uses for `mspace` widths.
pub(crate) fn space_width(space: Space) -> &'static str {
    match space {
        Space::Thin => "thinmathspace",
        Space::Medium => "mediummathspace",
        Space::Thick => "thickmathspace",
        Space::Sixteenth => "verythickmathspace",
        Space::Quad => "1em",
        Space::DoubleQuad => "2em",
        Space::NegativeThin => "negativethinmathspace",
        Space::NegativeMedium => "negativemediummathspace",
        Space::NegativeThick => "negativethickmathspace",
    }
}

/// Inverse of [`space_width`], also accepting the equivalent numeric lengths.
pub(crate) fn space_of_width(width: &str) -> Option<Space> {
    let named = match width.trim() {
        "thinmathspace" => Some(Space::Thin),
        "mediummathspace" => Some(Space::Medium),
        "thickmathspace" => Some(Space::Thick),
        "verythickmathspace" => Some(Space::Sixteenth),
        "negativethinmathspace" => Some(Space::NegativeThin),
        "negativemediummathspace" => Some(Space::NegativeMedium),
        "negativethickmathspace" => Some(Space::NegativeThick),
        _ => None,
    };
    if named.is_some() {
        return named;
    }
    // Numeric lengths: `em` is the unit of the model, `mu` is 1/18 em.
    let (number, unit) = match width.trim().strip_suffix("em") {
        Some(number) => (number, 1.0),
        None => {
            let number = width.trim().strip_suffix("mu")?;
            (number, 1.0 / 18.0)
        }
    };
    let em = number.trim().parse::<f64>().ok()? * unit;
    const GRID: [(f64, Space); 9] = [
        (3.0 / 18.0, Space::Thin),
        (4.0 / 18.0, Space::Medium),
        (5.0 / 18.0, Space::Thick),
        (6.0 / 18.0, Space::Sixteenth),
        (1.0, Space::Quad),
        (2.0, Space::DoubleQuad),
        (-3.0 / 18.0, Space::NegativeThin),
        (-4.0 / 18.0, Space::NegativeMedium),
        (-5.0 / 18.0, Space::NegativeThick),
    ];
    GRID.iter()
        .find(|(grid, _)| (grid - em).abs() < 1e-4)
        .map(|(_, space)| *space)
}

/// The character that stands for an accent kind in markup, the inverse of [`accent_kind`].
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

/// The delimiter pair spelled by an opening and a closing character, `None` meaning "draw
/// nothing".
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

/// Horizontal alignment as MathML writes it.
pub(crate) fn side_name(side: Side) -> &'static str {
    match side {
        Side::Center => "center",
        Side::Left => "left",
        Side::Right => "right",
    }
}

/// Inverse of [`side_name`].
pub(crate) fn side_of_name(name: &str) -> Option<Side> {
    match name.trim() {
        "center" => Some(Side::Center),
        "left" => Some(Side::Left),
        "right" => Some(Side::Right),
        _ => None,
    }
}

/// Cluster marker for the table kinds MathML has no attribute for.
pub(crate) fn kind_marker(kind: MatrixKind) -> Option<&'static str> {
    match kind {
        MatrixKind::Matrix => None,
        MatrixKind::Cases => Some("mt-cases"),
        MatrixKind::Aligned => Some("mt-aligned"),
        MatrixKind::Array => Some("mt-array"),
        MatrixKind::Pile => Some("mt-pile"),
        MatrixKind::LongDivision => Some("mt-long-division"),
    }
}

/// Inverse of [`kind_marker`].
pub(crate) fn kind_of_marker(token: &str) -> Option<MatrixKind> {
    Some(match token {
        "mt-cases" => MatrixKind::Cases,
        "mt-aligned" => MatrixKind::Aligned,
        "mt-array" => MatrixKind::Array,
        "mt-pile" => MatrixKind::Pile,
        "mt-long-division" => MatrixKind::LongDivision,
        _ => return None,
    })
}

/// Fold a list of per-column alignments into the tree's `align`/`columns` pair.
///
/// A list whose entries all agree is the same construct as an `align` default, so it becomes one;
/// a mixed list keeps every entry and neutralises `align`, since `columns` overrides it anyway.
pub(crate) fn fold_columns(columns: Vec<Side>) -> (Side, Vec<Side>) {
    match columns.split_first() {
        None => (Side::Center, Vec::new()),
        Some((first, rest)) if rest.iter().all(|side| side == first) => (*first, Vec::new()),
        Some(_) => (Side::Center, columns),
    }
}
