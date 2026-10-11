//! Conventions shared by the reader and the writer: record tags, option bits, atom classes,
//! typefaces, delimiter families and the default equation preferences.
//!
//! Everything that is a *convention* rather than a record layout lives here, so the reader and
//! the writer cannot drift apart.

use mt_core::{AccentKind, Class, Delim, Font, Side};

use crate::Unit;

// ---------------------------------------------------------------------------- record tags

pub(crate) const TAG_END: u8 = 0x00;
pub(crate) const TAG_LINE: u8 = 0x01;
pub(crate) const TAG_CHAR: u8 = 0x02;
pub(crate) const TAG_TMPL: u8 = 0x03;
pub(crate) const TAG_PILE: u8 = 0x04;
pub(crate) const TAG_MATRIX: u8 = 0x05;
pub(crate) const TAG_EMBELL: u8 = 0x06;
pub(crate) const TAG_RULER: u8 = 0x07;
pub(crate) const TAG_FONT_STYLE_DEF: u8 = 0x08;
pub(crate) const TAG_SIZE: u8 = 0x09;
/// `FULL` through `SUBSYM` are consecutive one-byte records running up to this tag.
pub(crate) const TAG_TYPESIZE_FIRST: u8 = 0x0A;
pub(crate) const TAG_TYPESIZE_LAST: u8 = 0x0E;
pub(crate) const TAG_COLOR: u8 = 0x0F;
pub(crate) const TAG_COLOR_DEF: u8 = 0x10;
pub(crate) const TAG_FONT_DEF: u8 = 0x11;
pub(crate) const TAG_EQN_PREFS: u8 = 0x12;
pub(crate) const TAG_ENCODING_DEF: u8 = 0x13;
/// Records at or above this tag carry their own length and can therefore be skipped.
pub(crate) const TAG_FUTURE: u8 = 100;

// ------------------------------------------------------------------- option bits

pub(crate) const OPT_NUDGE: u8 = 0x08;
pub(crate) const OPT_LINE_NULL: u8 = 0x01;
pub(crate) const OPT_LINE_LSPACE: u8 = 0x04;
pub(crate) const OPT_LP_RULER: u8 = 0x02;
pub(crate) const OPT_CHAR_EMBELL: u8 = 0x01;
pub(crate) const OPT_CHAR_8BIT: u8 = 0x04;
pub(crate) const OPT_CHAR_16BIT: u8 = 0x10;
pub(crate) const OPT_CHAR_NO_MTCODE: u8 = 0x20;
pub(crate) const OPT_COLOR_CMYK: u8 = 0x01;
pub(crate) const OPT_COLOR_NAME: u8 = 0x04;

/// Character style bit marking a bold face.
pub(crate) const STYLE_BOLD: u8 = 0x01;
/// Character style bit marking an italic face.
pub(crate) const STYLE_ITALIC: u8 = 0x02;

// ------------------------------------------------------------------ typefaces

/// Typeface codes of the named MathType styles.
pub(crate) const TF_TEXT: i32 = 1;
pub(crate) const TF_FUNCTION: i32 = 2;
pub(crate) const TF_VARIABLE: i32 = 3;
pub(crate) const TF_LCGREEK: i32 = 4;
pub(crate) const TF_SYMBOL: i32 = 6;
pub(crate) const TF_VECTOR: i32 = 7;
pub(crate) const TF_MTEXTRA: i32 = 11;
/// Typeface used for the glyphs that decorate templates (fence characters, accents).
pub(crate) const TF_EXPAND: i32 = 22;
/// Typeface of characters used as explicit horizontal space.
pub(crate) const TF_SPACE: i32 = 24;

/// Indices of the encodings every stream has without a definition record.
pub(crate) const ENC_SYMBOL: usize = 3;
pub(crate) const ENC_MT_EXTRA: usize = 4;
/// Index the first `ENCODING_DEF` record in a stream receives.
pub(crate) const ENC_FIRST_DEFINED: usize = 5;

/// Encoding this crate writes its font definitions in.
pub(crate) const DEFAULT_ENCODING: &str = "WinAllBasicCodePages";

/// Fonts every written stream defines, in this order, so that the standard styles can refer to
/// them by index.
pub(crate) const STANDARD_FONTS: [(&str, usize); 4] = [
    ("Times New Roman", ENC_FIRST_DEFINED),
    ("Symbol", ENC_SYMBOL),
    ("Courier New", ENC_FIRST_DEFINED),
    ("MT Extra", ENC_MT_EXTRA),
];

/// The font name and character style a face outside the standard styles is defined with.
///
/// Faces with no MathType style of their own are written as an explicit font definition; the
/// names below are the ones this crate uses for the faces that have no widely available system
/// font of the same shape.
pub(crate) fn explicit_font(font: Font) -> (&'static str, u8) {
    let bits = if font.is_bold() { STYLE_BOLD } else { 0 } | if font.is_italic() { STYLE_ITALIC } else { 0 };
    let name = match font {
        Font::SansSerif | Font::SansSerifBold | Font::SansSerifItalic | Font::SansSerifBoldItalic => "Arial",
        Font::Monospace => "Courier New",
        Font::Script | Font::BoldScript => "MT Script",
        Font::Fraktur | Font::BoldFraktur => "MT Fraktur",
        Font::DoubleStruck => "MT DoubleStruck",
        _ => "Times New Roman",
    };
    (name, bits)
}

/// The face a font definition names, given its character style bits.
pub(crate) fn font_from(name: &str, style: u8) -> Font {
    let bold = style & STYLE_BOLD != 0;
    let italic = style & STYLE_ITALIC != 0;
    let lower = name.to_ascii_lowercase();
    if lower.contains("symbol") {
        Font::Symbol
    } else if lower.contains("extra") {
        Font::Extra
    } else if lower.contains("script") {
        if bold { Font::BoldScript } else { Font::Script }
    } else if lower.contains("fraktur") {
        if bold { Font::BoldFraktur } else { Font::Fraktur }
    } else if lower.contains("doublestruck") {
        Font::DoubleStruck
    } else if lower.contains("courier") || lower.contains("mono") {
        Font::Monospace
    } else if lower.contains("arial") || lower.contains("helvetica") || lower.contains("sans") {
        match (bold, italic) {
            (false, false) => Font::SansSerif,
            (true, false) => Font::SansSerifBold,
            (false, true) => Font::SansSerifItalic,
            (true, true) => Font::SansSerifBoldItalic,
        }
    } else {
        match (bold, italic) {
            (false, false) => Font::Upright,
            (true, false) => Font::Bold,
            (false, true) => Font::Italic,
            (true, true) => Font::BoldItalic,
        }
    }
}

/// The face a named MathType style stands for when no font definition describes it.
pub(crate) fn style_font(typeface: i32) -> Font {
    match typeface {
        TF_VARIABLE | TF_LCGREEK => Font::Italic,
        TF_SYMBOL => Font::Symbol,
        TF_VECTOR => Font::Bold,
        TF_MTEXTRA => Font::Extra,
        _ => Font::Upright,
    }
}

/// The typeface a character is written with, or `None` when the face needs an explicit font
/// definition.
pub(crate) fn standard_typeface(font: Font, class: Class) -> Option<i32> {
    Some(match font {
        Font::Italic => TF_VARIABLE,
        Font::Upright => {
            if class == Class::Op {
                TF_FUNCTION
            } else {
                TF_TEXT
            }
        }
        Font::Symbol => TF_SYMBOL,
        Font::Extra => TF_MTEXTRA,
        _ => return None,
    })
}

// --------------------------------------------------------------------------- classes

/// Characters that take limits when they carry scripts.
const LARGE_OPS: &str = "\u{2211}\u{220F}\u{2210}\u{22C0}\u{22C1}\u{22C2}\u{22C3}\u{2A00}\u{2A01}\u{2A02}\u{2A03}\u{2A04}\u{2A05}\u{2A06}\u{222B}\u{222C}\u{222D}\u{222E}\u{222F}\u{2230}";

const BINARY: &str = "+-*\u{2212}\u{00B1}\u{2213}\u{00D7}\u{00F7}\u{22C5}\u{2217}\u{00B7}\u{2218}\u{2216}\u{2227}\u{2228}\u{2229}\u{222A}\u{2295}\u{2296}\u{2297}\u{2299}\u{2216}";

const RELATIONS: &str = "=<>\u{2264}\u{2265}\u{2260}\u{2248}\u{2261}\u{2208}\u{2209}\u{2282}\u{2283}\u{2286}\u{2287}\
\u{2192}\u{2190}\u{2194}\u{21D2}\u{21D4}\u{21A6}\u{2191}\u{2193}\u{27F9}\u{27FA}\u{223C}\u{221D}\u{2245}\u{225C}\
\u{2323}\u{2254}\u{2255}\u{220B}\u{2225}\u{2223}\u{2224}\u{22A5}\u{224D}\u{226A}\u{226B}";

const OPENERS: &str = "([{\u{27E8}\u{230A}\u{2308}\u{27E6}\u{301A}";
const CLOSERS: &str = ")]}\u{27E9}\u{230B}\u{2309}\u{27E7}\u{301B}";
const PUNCTUATION: &str = ",;";

/// Whether the character is a large operator.
pub(crate) fn is_large_op(ch: char) -> bool {
    LARGE_OPS.contains(ch)
}

/// The atom class a bare character carries.
///
/// Deliberately mirrors the conventions of the LaTeX codec: a character that is not in one of
/// the tables is an ordinary atom, so a variable and a digit classify the same way.
pub(crate) fn class_of_char(ch: char) -> Class {
    if is_large_op(ch) {
        Class::Op
    } else if OPENERS.contains(ch) {
        Class::Open
    } else if CLOSERS.contains(ch) {
        Class::Close
    } else if PUNCTUATION.contains(ch) {
        Class::Punct
    } else if BINARY.contains(ch) {
        Class::Bin
    } else if RELATIONS.contains(ch) {
        Class::Rel
    } else {
        Class::Ord
    }
}

/// The atom class of a character read from a stream.
///
/// The stream records a typeface but no class, so the class comes from the character, except in
/// the function style, whose characters are operators whatever they spell.
pub(crate) fn class_of(ch: char, typeface: i32) -> Class {
    if typeface == TF_FUNCTION {
        Class::Op
    } else {
        class_of_char(ch)
    }
}

/// The character a typesize record stands for, used when a template carries no operator glyph.
pub(crate) fn default_operator(selector: u8) -> char {
    match selector {
        15 => '\u{222B}',
        16 => '\u{2211}',
        17 => '\u{220F}',
        18 => '\u{2210}',
        19 => '\u{22C3}',
        20 => '\u{22C2}',
        21 => '\u{222B}',
        _ => '\u{2211}',
    }
}

// ------------------------------------------------------------------------ delimiters

/// A delimiter family: the template selector that draws it, with its two glyphs.
struct Family {
    selector: u8,
    open: char,
    close: char,
}

const FAMILIES: [Family; 8] = [
    Family { selector: 0, open: '\u{27E8}', close: '\u{27E9}' },
    Family { selector: 1, open: '(', close: ')' },
    Family { selector: 2, open: '{', close: '}' },
    Family { selector: 3, open: '[', close: ']' },
    Family { selector: 4, open: '|', close: '|' },
    Family { selector: 5, open: '\u{2016}', close: '\u{2016}' },
    Family { selector: 6, open: '\u{230A}', close: '\u{230B}' },
    Family { selector: 7, open: '\u{2308}', close: '\u{2309}' },
];

/// The family a delimiter belongs to, if any.
pub(crate) fn family_of(delim: Delim) -> Option<(u8, char, char)> {
    let found = match delim {
        Delim::Angle | Delim::Arrow => &FAMILIES[0],
        Delim::Paren => &FAMILIES[1],
        Delim::Brace => &FAMILIES[2],
        Delim::Bracket => &FAMILIES[3],
        Delim::Abs => &FAMILIES[4],
        Delim::Norm | Delim::DoubleBar => &FAMILIES[5],
        Delim::Floor => &FAMILIES[6],
        Delim::Ceil => &FAMILIES[7],
        Delim::None | Delim::Custom(_) => return None,
    };
    Some((found.selector, found.open, found.close))
}

/// The delimiter a fence template draws, given its selector and the sides that are present.
pub(crate) fn delimiter_of(selector: u8, left: bool, right: bool) -> (Delim, Delim) {
    let family = FAMILIES.iter().find(|family| family.selector == selector);
    let open = match (family, left) {
        (Some(family), true) => delim_from_glyph(family, family.open, true),
        _ => Delim::None,
    };
    let close = match (family, right) {
        (Some(family), true) => delim_from_glyph(family, family.close, false),
        _ => Delim::None,
    };
    (open, close)
}

/// The delimiter a glyph stands for inside a fence of `family`.
fn delim_from_glyph(family: &Family, glyph: char, opening: bool) -> Delim {
    match (family.selector, opening, glyph) {
        (0, true, _) => Delim::Angle,
        (0, false, _) => Delim::Angle,
        (1, _, '(') => Delim::Paren,
        (1, _, ')') => Delim::Paren,
        (2, _, '{') => Delim::Brace,
        (2, _, '}') => Delim::Brace,
        (3, _, '[') => Delim::Bracket,
        (3, _, ']') => Delim::Bracket,
        (4, _, '|') => Delim::Abs,
        (5, _, _) => Delim::DoubleBar,
        (6, _, '\u{230A}') => Delim::Floor,
        (6, _, '\u{230B}') => Delim::Floor,
        (7, _, '\u{2308}') => Delim::Ceil,
        (7, _, '\u{2309}') => Delim::Ceil,
        _ => Delim::Custom(glyph),
    }
}

/// The interval template codes for a delimiter, or `None` when it is not one of the four
/// characters the interval template can spell.
pub(crate) fn interval_code(delim: Delim, left: bool) -> Option<u32> {
    let ch = match delim {
        Delim::Paren => if left { '(' } else { ')' },
        Delim::Bracket => if left { '[' } else { ']' },
        Delim::Custom(ch) => ch,
        _ => return None,
    };
    let code = match ch {
        '(' => 0,
        ')' => 1,
        '[' => 2,
        ']' => 3,
        _ => return None,
    };
    Some(if left { code } else { code << 4 })
}

/// The delimiter an interval template code stands for.
///
/// An interval can draw a delimiter pointing "the wrong way" — a closing bracket as the left
/// delimiter, say — which is a custom delimiter in the equation tree.
pub(crate) fn interval_delim(code: u32, left: bool) -> Delim {
    if left {
        match code & 0x03 {
            0 => Delim::Paren,
            1 => Delim::Custom(')'),
            2 => Delim::Bracket,
            _ => Delim::Custom(']'),
        }
    } else {
        match code & 0x30 {
            0x00 => Delim::Custom('('),
            0x10 => Delim::Paren,
            0x20 => Delim::Custom('['),
            _ => Delim::Bracket,
        }
    }
}

/// Horizontal alignment from an MTEF alignment value.
pub(crate) fn side_of(value: u8) -> Side {
    match value {
        1 => Side::Left,
        3 => Side::Right,
        _ => Side::Center,
    }
}

/// MTEF alignment value for a horizontal side.
pub(crate) fn side_value(side: Side) -> u8 {
    match side {
        Side::Left => 1,
        Side::Right => 3,
        Side::Center => 2,
    }
}

// --------------------------------------------------------------------------- accents

/// The accent a template draws when its glyph is not decisive.
pub(crate) fn accent_of_template(selector: u8, variation: u32) -> (AccentKind, bool) {
    match selector {
        12 => (if variation & 0x0001 != 0 { AccentKind::Line } else { AccentKind::Bar }, true),
        13 => (if variation & 0x0001 != 0 { AccentKind::Line } else { AccentKind::Bar }, false),
        14 => (AccentKind::ArrowRight, false),
        24 => (if variation & 0x0001 != 0 { AccentKind::OverBrace } else { AccentKind::UnderBrace }, variation & 0x0001 == 0),
        25 => (if variation & 0x0001 != 0 { AccentKind::OverBracket } else { AccentKind::UnderBracket }, variation & 0x0001 == 0),
        31 => (AccentKind::Vector, variation & 0x0004 != 0),
        32 => (AccentKind::Tilde, false),
        33 => (AccentKind::Hat, false),
        34 => (AccentKind::OverParen, false),
        _ => (AccentKind::Strike, false),
    }
}

/// The accent a decorative glyph stands for.
pub(crate) fn accent_of_glyph(glyph: char) -> Option<(AccentKind, bool)> {
    Some(match glyph {
        '\u{02C6}' => (AccentKind::Hat, false),
        '\u{0302}' => (AccentKind::WideHat, false),
        '~' => (AccentKind::Tilde, false),
        '\u{0303}' => (AccentKind::WideTilde, false),
        '\u{02D9}' => (AccentKind::Dot, false),
        '\u{00A8}' => (AccentKind::DoubleDot, false),
        '\u{20DB}' => (AccentKind::TripleDot, false),
        '\u{02D8}' => (AccentKind::Breve, false),
        '\u{02C7}' => (AccentKind::Check, false),
        '\u{00B4}' => (AccentKind::Acute, false),
        '`' => (AccentKind::Grave, false),
        '\u{02DA}' | '\u{00B0}' => (AccentKind::Circle, false),
        '\u{20D7}' => (AccentKind::Vector, false),
        '\u{2192}' => (AccentKind::ArrowRight, false),
        '\u{2190}' => (AccentKind::ArrowLeft, false),
        '\u{23DC}' | '\u{2322}' => (AccentKind::OverParen, false),
        '\u{23DD}' | '\u{2323}' => (AccentKind::UnderParen, true),
        '\u{23DE}' => (AccentKind::OverBrace, false),
        '\u{23DF}' => (AccentKind::UnderBrace, true),
        '\u{23B4}' => (AccentKind::OverBracket, false),
        '\u{23B5}' => (AccentKind::UnderBracket, true),
        _ => return None,
    })
}

/// The template and variation a written accent uses, with the decorative glyph to emit.
///
/// The template decides which side of its contents the mark is drawn on, and the glyph decides
/// which mark it is, so an accent that goes under its contents and has no template of its own is
/// written as an underbar carrying its own glyph.
pub(crate) fn template_of_accent(kind: AccentKind, under: bool) -> (u8, u32, Option<char>) {
    let glyph = kind.glyph();
    match kind {
        AccentKind::Bar | AccentKind::Line => {
            (if under { 12 } else { 13 }, u32::from(kind == AccentKind::Line), None)
        }
        AccentKind::OverBrace => (24, 0x0001, glyph),
        AccentKind::UnderBrace => (24, 0x0000, glyph),
        AccentKind::OverBracket => (25, 0x0001, glyph),
        AccentKind::UnderBracket => (25, 0x0000, glyph),
        AccentKind::OverParen | AccentKind::UnderParen => (34, 0, glyph),
        AccentKind::Vector => (31, 0x0002 | if under { 0x0004 } else { 0 }, glyph),
        AccentKind::Strike => (36, 0x0001, None),
        AccentKind::ArrowRight | AccentKind::ArrowLeft if !under => {
            (14, if kind == AccentKind::ArrowLeft { 0x10 } else { 0x20 }, glyph)
        }
        // Hats, arcs, points and the accents with no template of their own share the hat
        // template, which draws over its contents, or the underbar when they go under them.
        _ => (if under { 12 } else { 33 }, 0, glyph),
    }
}

/// Whether a template decides the side of the material it decorates itself.
pub(crate) fn template_has_side(selector: u8) -> bool {
    matches!(selector, 12 | 13 | 24 | 25 | 31)
}

// --------------------------------------------------------------------- embellishments

/// The accent an embellishment type draws.
pub(crate) fn accent_of_embellishment(kind: u8) -> Option<(AccentKind, bool)> {
    Some(match kind {
        2 => (AccentKind::Dot, false),
        3 => (AccentKind::DoubleDot, false),
        4 => (AccentKind::TripleDot, false),
        8 => (AccentKind::Tilde, false),
        9 => (AccentKind::Hat, false),
        10 => (AccentKind::Strike, false),
        11 => (AccentKind::ArrowRight, false),
        12 => (AccentKind::ArrowLeft, false),
        16 | 17 => (AccentKind::Bar, false),
        19 | 20 => (AccentKind::OverParen, false),
        21..=23 => (AccentKind::Strike, false),
        25 => (AccentKind::Dot, true),
        26 => (AccentKind::DoubleDot, true),
        27 => (AccentKind::TripleDot, true),
        29 => (AccentKind::Bar, true),
        30 => (AccentKind::Tilde, true),
        31 | 32 => (AccentKind::UnderParen, true),
        33 => (AccentKind::ArrowRight, true),
        34 => (AccentKind::ArrowLeft, true),
        _ => return None,
    })
}

/// The embellishment an accent is spelled as, for accents the format draws over or under a
/// single character.
///
/// This is how MathType itself writes an accent on one character; accents over anything longer
/// are written as templates instead.
pub(crate) fn embellishment_of_accent(kind: AccentKind, under: bool) -> Option<u8> {
    Some(match (kind, under) {
        (AccentKind::Dot, false) => 2,
        (AccentKind::DoubleDot, false) => 3,
        (AccentKind::TripleDot, false) => 4,
        (AccentKind::Tilde, false) => 8,
        (AccentKind::Hat, false) => 9,
        (AccentKind::Strike, false) => 10,
        (AccentKind::ArrowRight, false) => 11,
        (AccentKind::ArrowLeft, false) => 12,
        (AccentKind::Dot, true) => 25,
        (AccentKind::DoubleDot, true) => 26,
        (AccentKind::TripleDot, true) => 27,
        (AccentKind::Tilde, true) => 30,
        (AccentKind::UnderParen, true) => 31,
        (AccentKind::ArrowRight, true) => 33,
        (AccentKind::ArrowLeft, true) => 34,
        _ => return None,
    })
}

// ------------------------------------------------------------------------ preferences

/// The default typesizes every written stream declares, in the order the stream lists them.
pub(crate) const DEFAULT_SIZES: [(Unit, &str); 8] = [
    (Unit::Points, "12"),
    (Unit::Points, "7"),
    (Unit::Points, "5"),
    (Unit::Points, "18"),
    (Unit::Points, "12"),
    (Unit::Percent, "75"),
    (Unit::Percent, "150"),
    (Unit::Points, "1"),
];

/// The default spacing every written stream declares, in the order the stream lists them.
pub(crate) const DEFAULT_SPACES: [(Unit, &str); 30] = [
    (Unit::Percent, "150"),
    (Unit::Percent, "150"),
    (Unit::Percent, "100"),
    (Unit::Percent, "45"),
    (Unit::Percent, "25"),
    (Unit::Percent, "8"),
    (Unit::Percent, "25"),
    (Unit::Percent, "100"),
    (Unit::Percent, "100"),
    (Unit::Percent, "35"),
    (Unit::Percent, "100"),
    (Unit::Points, "1"),
    (Unit::Points, "0.5"),
    (Unit::Points, "0.25"),
    (Unit::Percent, "8"),
    (Unit::Points, "1"),
    (Unit::Percent, "100"),
    (Unit::Percent, "100"),
    (Unit::Percent, "0"),
    (Unit::Percent, "8"),
    (Unit::Percent, "17"),
    (Unit::Percent, "8"),
    (Unit::Percent, "100"),
    (Unit::Points, "1.5"),
    (Unit::Percent, "45"),
    (Unit::Percent, "5"),
    (Unit::Percent, "5"),
    (Unit::Percent, "5"),
    (Unit::Percent, "5"),
    (Unit::Percent, "10"),
];

/// The character style each standard style is defined with, in typeface order.
pub(crate) const DEFAULT_STYLES: [(Option<usize>, u8); 12] = [
    (Some(1), 0),
    (Some(1), 0),
    (Some(1), STYLE_ITALIC),
    (Some(2), STYLE_ITALIC),
    (Some(2), 0),
    (Some(2), 0),
    (Some(1), STYLE_BOLD),
    (Some(1), 0),
    (Some(3), 0),
    (Some(1), 0),
    (Some(4), 0),
    (None, 0),
];
