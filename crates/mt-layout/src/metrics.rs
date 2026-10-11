//! Glyph metrics: the interface layout draws its measurements from, plus a deterministic
//! built-in approximation for tests and previews.

use mt_core::Font;

/// Measurements layout needs from a font, all in em units at full size.
pub trait FontMetrics {
    /// Advance width of a glyph in em units at full size.
    fn advance(&self, ch: char, font: Font) -> f64;

    /// Glyph extent above the baseline, in em units at full size.
    fn height(&self, ch: char, font: Font) -> f64;

    /// Glyph extent below the baseline, in em units at full size.
    fn depth(&self, ch: char, font: Font) -> f64;

    /// Extra leading this font wants between stacked material, in em units.
    fn math_leading(&self) -> f64 {
        0.05
    }

    /// Distance from the baseline to the axis of a fraction rule, in em units.
    fn axis_height(&self) -> f64 {
        0.25
    }
}

/// A deterministic approximation of a math font, for tests and for previews that carry no real
/// font metrics.
///
/// The numbers are Computer-Modern-like: a lowercase x-height of 0.45 em, uppercase letters and
/// digits about 0.7 em tall, delimiters 1 em tall, and a math axis 0.25 em above the baseline.
/// The face is ignored, so a bold or italic glyph measures like its upright counterpart.
#[derive(Clone, Copy, Debug, Default)]
pub struct ApproxMetrics;

impl FontMetrics for ApproxMetrics {
    fn advance(&self, ch: char, _font: Font) -> f64 {
        measurement(ch).0
    }

    fn height(&self, ch: char, _font: Font) -> f64 {
        measurement(ch).1
    }

    fn depth(&self, ch: char, _font: Font) -> f64 {
        measurement(ch).2
    }
}

/// Advance, height and depth of `ch`, measuring a styled variant like its plain letter.
fn measurement(ch: char) -> (f64, f64, f64) {
    let ch = crate::math_base(ch).unwrap_or(ch);
    match ch {
        '0'..='9' => (0.5, 0.65, 0.0),
        'A'..='Z' => (0.7, 0.7, 0.0),
        'a'..='z' => match ch {
            'i' | 'l' => (0.28, 0.45, 0.0),
            'j' => (0.28, 0.45, 0.2),
            'g' | 'p' | 'q' | 'y' => (0.5, 0.45, 0.2),
            _ => (0.5, 0.45, 0.0),
        },
        '+' => (0.78, 0.58, 0.08),
        '-' | '\u{2212}' => (0.78, 0.25, 0.25),
        '=' => (0.78, 0.37, 0.37),
        '*' => (0.5, 0.45, 0.0),
        '/' => (0.5, 0.75, 0.25),
        '(' | ')' | '\u{230A}' | '\u{230B}' | '\u{2308}' | '\u{2309}' => (0.389, 0.75, 0.25),
        '[' | ']' | '|' | '\u{2016}' | '\u{27E8}' | '\u{27E9}' => (0.278, 0.75, 0.25),
        '{' | '}' => (0.5, 0.75, 0.25),
        ',' | ';' => (0.278, 0.1, 0.15),
        ':' => (0.278, 0.35, 0.15),
        '.' => (0.278, 0.05, 0.0),
        '\u{221A}' => (0.8, 0.85, 0.15),
        '\u{222B}' | '\u{222E}' => (0.4, 0.85, 0.35),
        '\u{2211}' | '\u{220F}' | '\u{22C3}' => (1.0, 0.75, 0.25),
        '\u{02C6}' | '\u{02DC}' | '~' | '\u{02D9}' | '\u{00A8}' | '\u{02D8}' | '\u{02C7}'
        | '\u{00B4}' | '`' | '\u{02DA}' | '\u{2192}' | '\u{2190}' | '\u{23DE}' | '\u{23DF}'
        | '\u{23B4}' | '\u{23B5}' | '\u{23DC}' | '\u{23DD}' => (0.4, 0.25, 0.0),
        _ => (0.5, 0.6, 0.0),
    }
}
