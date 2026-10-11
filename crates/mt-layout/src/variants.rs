//! Mathematical alphanumeric variants: the codepoint a letter takes in a styled face.
//!
//! A math font carries every styled letter shape as its own glyph in the Mathematical
//! Alphanumeric Symbols block (Unicode chart *U+1D400–U+1D7FF*), so "italic a" and "script A" are
//! separate characters (`U+1D44E`, `U+1D49C`). The equation tree stores the plain letter plus a
//! [`Font`]; [`math_variant`] picks the codepoint the face draws it with, which lets a single math
//! font serve every style slot a preset can name.
//!
//! Letters without a variant in their face — italic digits, script Greek, upright everything —
//! keep their plain codepoint, which is also the shape the face gives them.

use mt_core::Font;

/// The codepoint `ch` is drawn with in `font`.
///
/// `ch` is returned unchanged when it is not an ASCII letter, ASCII digit or Greek letter, or
/// when the face has no variant block for it.
pub fn math_variant(ch: char, font: Font) -> char {
    match ch {
        '0'..='9' => digit_variant(ch, font),
        'A'..='Z' | 'a'..='z' => latin_variant(ch, font),
        _ => greek_variant(ch, font),
    }
}

/// Variant of an ASCII letter; reserved letters were handled by [`reserved_latin`].
fn latin_variant(ch: char, font: Font) -> char {
    if let Some(special) = reserved_latin(ch, font) {
        return special;
    }
    let upper = ch.is_ascii_uppercase();
    let index = (ch as u32) - if upper { 0x41 } else { 0x61 };
    let base = match (font, upper) {
        (Font::Upright, _) | (Font::Symbol, _) | (Font::Extra, _) => return ch,
        (Font::Italic, true) => 0x1D434,
        (Font::Italic, false) => 0x1D44E,
        (Font::Bold, true) => 0x1D400,
        (Font::Bold, false) => 0x1D41A,
        (Font::BoldItalic, true) => 0x1D468,
        (Font::BoldItalic, false) => 0x1D482,
        (Font::Script, true) => 0x1D49C,
        (Font::Script, false) => 0x1D4B6,
        (Font::BoldScript, true) => 0x1D4D0,
        (Font::BoldScript, false) => 0x1D4EA,
        (Font::Fraktur, true) => 0x1D504,
        (Font::Fraktur, false) => 0x1D51E,
        (Font::BoldFraktur, true) => 0x1D56C,
        (Font::BoldFraktur, false) => 0x1D586,
        (Font::DoubleStruck, true) => 0x1D538,
        (Font::DoubleStruck, false) => 0x1D552,
        (Font::SansSerif, true) => 0x1D5A0,
        (Font::SansSerif, false) => 0x1D5BA,
        (Font::SansSerifBold, true) => 0x1D5D4,
        (Font::SansSerifBold, false) => 0x1D5EE,
        (Font::SansSerifItalic, true) => 0x1D608,
        (Font::SansSerifItalic, false) => 0x1D622,
        (Font::SansSerifBoldItalic, true) => 0x1D63C,
        (Font::SansSerifBoldItalic, false) => 0x1D656,
        (Font::Monospace, true) => 0x1D670,
        (Font::Monospace, false) => 0x1D68A,
    };
    char::from_u32(base + index).unwrap_or(ch)
}

/// The letters whose variants sit at reserved compatibility codepoints instead of the block.
fn reserved_latin(ch: char, font: Font) -> Option<char> {
    Some(match (font, ch) {
        (Font::Italic, 'h') => '\u{210E}',
        (Font::Script, 'B') => '\u{212C}',
        (Font::Script, 'E') => '\u{2130}',
        (Font::Script, 'F') => '\u{2131}',
        (Font::Script, 'H') => '\u{210B}',
        (Font::Script, 'I') => '\u{2110}',
        (Font::Script, 'L') => '\u{2112}',
        (Font::Script, 'M') => '\u{2133}',
        (Font::Script, 'R') => '\u{211B}',
        (Font::Script, 'e') => '\u{212F}',
        (Font::Script, 'g') => '\u{210A}',
        (Font::Script, 'o') => '\u{2134}',
        (Font::Fraktur, 'C') => '\u{212D}',
        (Font::Fraktur, 'H') => '\u{210C}',
        (Font::Fraktur, 'I') => '\u{2111}',
        (Font::Fraktur, 'R') => '\u{211C}',
        (Font::Fraktur, 'Z') => '\u{2128}',
        (Font::DoubleStruck, 'C') => '\u{2102}',
        (Font::DoubleStruck, 'H') => '\u{210D}',
        (Font::DoubleStruck, 'N') => '\u{2115}',
        (Font::DoubleStruck, 'P') => '\u{2119}',
        (Font::DoubleStruck, 'Q') => '\u{211A}',
        (Font::DoubleStruck, 'R') => '\u{211D}',
        (Font::DoubleStruck, 'Z') => '\u{2124}',
        _ => return None,
    })
}

/// Variant of an ASCII digit, for the faces that have digit variants.
fn digit_variant(ch: char, font: Font) -> char {
    let index = (ch as u32) - '0' as u32;
    let base = match font {
        Font::Bold => 0x1D7CE,
        Font::SansSerif => 0x1D7E2,
        Font::SansSerifBold => 0x1D7EC,
        Font::DoubleStruck => 0x1D7D8,
        Font::Monospace => 0x1D6F8,
        _ => return ch,
    };
    char::from_u32(base + index).unwrap_or(ch)
}

/// Variant of a Greek letter, for the faces that have Greek variant blocks.
///
/// Upright and italic Greek keep their base codepoint: a math font already draws those shapes
/// there, and no reserved codepoints are involved. The capitals run `U+0391..=U+03A1` for
/// Α..Ψ with Ω at `U+03A9`; the small letters `U+03B1..=U+03C9` are contiguous.
fn greek_variant(ch: char, font: Font) -> char {
    let code = ch as u32;
    let capital = (0x0391..=0x03A1).contains(&code) || code == 0x03A9;
    let lower = (0x03B1..=0x03C9).contains(&code);
    if !capital && !lower {
        return ch;
    }
    let index = if capital { code - 0x0391 } else { code - 0x03B1 };
    let base = match (font, capital) {
        (Font::Bold, true) => 0x1D6A8,
        (Font::Bold, false) => 0x1D6C2,
        (Font::BoldItalic, true) => 0x1D71C,
        (Font::BoldItalic, false) => 0x1D736,
        (Font::SansSerifBold, true) => 0x1D756,
        (Font::SansSerifBold, false) => 0x1D770,
        (Font::SansSerifBoldItalic, true) => 0x1D790,
        (Font::SansSerifBoldItalic, false) => 0x1D7AA,
        _ => return ch,
    };
    char::from_u32(base + index).unwrap_or(ch)
}

/// The plain letter or digit a variant codepoint came from, or `None` for anything else.
///
/// This is [`math_variant`] read backwards, for measuring: a glyph approximation that has no
/// shapes of its own measures a styled letter like the plain letter it replaces.
pub fn math_base(ch: char) -> Option<char> {
    let code = ch as u32;
    for base in LATIN_UPPER_BASES {
        if (base..base + 26).contains(&code) {
            return char::from_u32(0x41 + code - base);
        }
    }
    for base in LATIN_LOWER_BASES {
        if (base..base + 26).contains(&code) {
            return char::from_u32(0x61 + code - base);
        }
    }
    for base in DIGIT_BASES {
        if (base..base + 10).contains(&code) {
            return char::from_u32('0' as u32 + code - base);
        }
    }
    for base in GREEK_GROUP_BASES {
        let offset = code.checked_sub(base)?;
        if offset < 25 {
            // The capitals Α..Ψ then Σ..Ω; the alphabet skips the codepoint U+03A2 never used.
            let greek = if offset <= 16 { 0x0391 + offset } else { 0x0392 + offset };
            return char::from_u32(greek);
        }
        if (26..51).contains(&offset) {
            return char::from_u32(0x03B1 + offset - 26);
        }
    }
    match ch {
        '\u{210E}' => Some('h'),
        '\u{212C}' | '\u{2130}' | '\u{2131}' | '\u{210B}' | '\u{2110}' | '\u{2112}'
        | '\u{2133}' | '\u{211B}' => char::from_u32(0x42 + "BEFHILMR".find(ch).unwrap() as u32),
        '\u{212F}' | '\u{210A}' | '\u{2134}' => char::from_u32(0x61 + "ego".find(ch).unwrap() as u32),
        '\u{212D}' | '\u{210C}' | '\u{2111}' | '\u{211C}' | '\u{2128}' => {
            char::from_u32(0x41 + "CHIRZ".find(ch).unwrap() as u32)
        }
        '\u{2102}' | '\u{210D}' | '\u{2115}' | '\u{2119}' | '\u{211A}' | '\u{211D}'
        | '\u{2124}' => char::from_u32(0x41 + "CHNPQRZ".find(ch).unwrap() as u32),
        _ => None,
    }
}

/// Where each styled face's uppercase run starts.
const LATIN_UPPER_BASES: [u32; 13] = [
    0x1D400, 0x1D434, 0x1D468, 0x1D49C, 0x1D4D0, 0x1D504, 0x1D538, 0x1D56C, 0x1D5A0, 0x1D5D4,
    0x1D608, 0x1D63C, 0x1D670,
];

/// Where each styled face's lowercase run starts.
const LATIN_LOWER_BASES: [u32; 13] = [
    0x1D41A, 0x1D44E, 0x1D482, 0x1D4B6, 0x1D4EA, 0x1D51E, 0x1D552, 0x1D586, 0x1D5BA, 0x1D5EE,
    0x1D622, 0x1D656, 0x1D68A,
];

/// Where each styled face's digit run starts.
const DIGIT_BASES: [u32; 5] = [0x1D7CE, 0x1D7D8, 0x1D7E2, 0x1D7EC, 0x1D6F8];

/// Where each styled Greek group starts: 25 capitals, a nabla, 25 small letters, then symbols.
const GREEK_GROUP_BASES: [u32; 5] = [0x1D6A8, 0x1D6E2, 0x1D71C, 0x1D756, 0x1D790];

#[cfg(test)]
mod tests {
    use super::math_variant;
    use mt_core::Font;

    #[test]
    fn latin_letters_take_the_variant_of_their_face() {
        assert_eq!(math_variant('a', Font::Italic), '\u{1D44E}');
        assert_eq!(math_variant('A', Font::Italic), '\u{1D434}');
        assert_eq!(math_variant('a', Font::Bold), '\u{1D41A}');
        assert_eq!(math_variant('A', Font::Script), '\u{1D49C}');
        assert_eq!(math_variant('x', Font::Script), '\u{1D4CD}');
        assert_eq!(math_variant('A', Font::Fraktur), '\u{1D504}');
        assert_eq!(math_variant('a', Font::DoubleStruck), '\u{1D552}');
        assert_eq!(math_variant('A', Font::SansSerif), '\u{1D5A0}');
        assert_eq!(math_variant('a', Font::Monospace), '\u{1D68A}');
        assert_eq!(math_variant('A', Font::BoldScript), '\u{1D4D0}');
        assert_eq!(math_variant('A', Font::SansSerifBoldItalic), '\u{1D63C}');
    }

    #[test]
    fn reserved_variants_sit_outside_the_block() {
        assert_eq!(math_variant('h', Font::Italic), '\u{210E}');
        assert_eq!(math_variant('B', Font::Script), '\u{212C}');
        assert_eq!(math_variant('e', Font::Script), '\u{212F}');
        assert_eq!(math_variant('g', Font::Script), '\u{210A}');
        assert_eq!(math_variant('o', Font::Script), '\u{2134}');
        assert_eq!(math_variant('R', Font::Script), '\u{211B}');
        // A letter after a reserved one continues the block where it left off.
        assert_eq!(math_variant('C', Font::Fraktur), '\u{212D}');
        assert_eq!(math_variant('D', Font::Fraktur), '\u{1D507}');
        assert_eq!(math_variant('J', Font::Fraktur), '\u{1D50D}');
        assert_eq!(math_variant('R', Font::DoubleStruck), '\u{211D}');
        assert_eq!(math_variant('S', Font::DoubleStruck), '\u{1D54A}');
    }

    #[test]
    fn digits_and_greek_have_variants_only_where_unicode_defines_them() {
        assert_eq!(math_variant('7', Font::Bold), '\u{1D7D5}');
        assert_eq!(math_variant('7', Font::Italic), '7');
        assert_eq!(math_variant('7', Font::Script), '7');
        assert_eq!(math_variant('α', Font::Bold), '\u{1D6C2}');
        assert_eq!(math_variant('ω', Font::Bold), '\u{1D6DA}');
        assert_eq!(math_variant('Ω', Font::Bold), '\u{1D6C0}');
        assert_eq!(math_variant('Ω', Font::BoldItalic), '\u{1D734}');
        assert_eq!(math_variant('α', Font::Italic), 'α');
        assert_eq!(math_variant('α', Font::Script), 'α');
        assert_eq!(math_variant('α', Font::SansSerif), 'α');
    }

    #[test]
    fn everything_else_keeps_its_codepoint() {
        for ch in ['+', '=', '(', '∑', '√', '×', ' ', '中'] {
            assert_eq!(math_variant(ch, Font::Italic), ch);
            assert_eq!(math_variant(ch, Font::Bold), ch);
        }
        assert_eq!(math_variant('a', Font::Upright), 'a');
        assert_eq!(math_variant('a', Font::Symbol), 'a');
        assert_eq!(math_variant('5', Font::Upright), '5');
    }
}
