use mt_core::{Class, Font};

use crate::spacing::Spacing;

/// The functional style slots a preset fills, each naming the face that slot is set in.
///
/// A character of the equation tree is drawn in the face of the slot its role names: variables in
/// `variable`, digits in `number`, Greek in `greek`, bold letters in `vector`, upright
/// operator-like letters in `function`, and every other operator, relation and punctuation in
/// `symbol`. Faces outside this system — script, fraktur, double-struck, sans-serif, monospace,
/// symbol, extra — are explicit formatting and pass through untouched, as does anything under a
/// [`mt_core::Node::Styled`] override.
///
/// [`Default`] is the identity mapping: every slot keeps the face the tree already carries, which
/// is the classic look (italic variables, upright everything else).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct StyleFonts {
    /// Literal text runs.
    pub text: Font,
    /// Function names, e.g. the `sin` of `\sin x`.
    pub function: Font,
    /// Ordinary letters.
    pub variable: Font,
    /// Greek letters set in a standard face.
    pub greek: Font,
    /// Operators, relations and punctuation.
    pub symbol: Font,
    /// Bold letters, the vector-matrix convention.
    pub vector: Font,
    /// Digits.
    pub number: Font,
}

impl Default for StyleFonts {
    fn default() -> Self {
        Self {
            text: Font::Upright,
            function: Font::Upright,
            variable: Font::Italic,
            greek: Font::Italic,
            symbol: Font::Upright,
            vector: Font::Bold,
            number: Font::Upright,
        }
    }
}

impl StyleFonts {
    /// The face a character with `class` and tree font `font` is drawn in.
    pub fn resolve(&self, ch: char, class: Class, font: Font) -> Font {
        if !matches!(
            font,
            Font::Italic | Font::Upright | Font::Bold | Font::BoldItalic
        ) {
            return font;
        }
        if ch.is_ascii_digit() {
            return self.number;
        }
        if is_greek(ch) {
            return if font.is_bold() {
                self.vector
            } else {
                self.greek
            };
        }
        if ch.is_ascii_alphabetic() {
            if font.is_bold() {
                return self.vector;
            }
            if class == Class::Op {
                return self.function;
            }
            return self.variable;
        }
        self.symbol
    }

    /// The face a literal text run set in tree font `font` is drawn in.
    ///
    /// Text runs are roles of their own: never re-read as variables. Only the standard faces take
    /// the `text` slot; an explicit face the author chose is kept.
    pub fn text_font(&self, font: Font) -> Font {
        match font {
            Font::Italic | Font::Upright | Font::Bold | Font::BoldItalic => self.text,
            other => other,
        }
    }
}

/// Whether `ch` is a Greek letter the variant blocks and the `greek` slot cover.
fn is_greek(ch: char) -> bool {
    let code = ch as u32;
    (0x0391..=0x03A1).contains(&code) || code == 0x03A9 || (0x03B1..=0x03C9).contains(&code)
}

/// Size preset for one layout run.
///
/// A style carries the full size and the two scale factors used for nested material; the nesting
/// level of a piece of the equation is tracked separately, so the same style can be reused for
/// every slot. The spacing table and the font slots ride along, so one value describes the whole
/// preset layout is run with; both default to the classic engine behaviour.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct LayoutStyle {
    /// Font size in points at full size.
    pub size: f64,
    /// Size factor for material one level deep, e.g. scripts and fraction arguments.
    pub script_scale: f64,
    /// Size factor for material two or more levels deep.
    pub script_script_scale: f64,
    /// Spacing table, in em units.
    pub spacing: Spacing,
    /// Font slots the functional styles of the tree resolve through.
    pub fonts: StyleFonts,
    /// Whether superscripts are set without the extra clearance that uncramped math gets.
    pub cramped: bool,
    /// Nesting level the equation itself starts at; 0 is full size.
    pub level: u8,
}

impl LayoutStyle {
    /// A full-size style of `size` points, starting at nesting level zero.
    pub fn display(size: f64) -> Self {
        Self {
            size,
            ..Self::default()
        }
    }

    /// Size factor applied at `level`: full at 0, `script_scale` at 1, `script_script_scale`
    /// from 2 onwards.
    pub fn scale(&self, level: u8) -> f64 {
        match level {
            0 => 1.0,
            1 => self.script_scale,
            _ => self.script_script_scale,
        }
    }

    /// Font size in points at `level`.
    pub fn size_at(&self, level: u8) -> f64 {
        self.size * self.scale(level)
    }
}

impl Default for LayoutStyle {
    /// Ten point at full size, with the customary 0.7 and 0.5 nesting factors.
    fn default() -> Self {
        Self {
            size: 10.0,
            script_scale: 0.7,
            script_script_scale: 0.5,
            spacing: Spacing::default(),
            fonts: StyleFonts::default(),
            cramped: false,
            level: 0,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{LayoutStyle, StyleFonts};
    use mt_core::{Class, Font};

    #[test]
    fn scale_saturates_at_the_second_factor() {
        let style = LayoutStyle::display(12.0);
        assert_eq!(style.scale(0), 1.0);
        assert_eq!(style.scale(1), 0.7);
        assert_eq!(style.scale(2), 0.5);
        assert_eq!(style.scale(9), 0.5);
        assert_eq!(style.size_at(0), 12.0);
        assert_eq!(style.size_at(1), 12.0 * 0.7);
    }

    #[test]
    fn cramped_and_level_survive_the_struct_update_syntax() {
        let style = LayoutStyle {
            cramped: true,
            level: 2,
            ..LayoutStyle::display(8.0)
        };
        assert!(style.cramped);
        assert_eq!(style.level, 2);
        assert_eq!(style.size, 8.0);
        assert_eq!(style.script_scale, 0.7);
    }

    #[test]
    fn default_slots_keep_the_tree_face() {
        let slots = StyleFonts::default();
        assert_eq!(slots.resolve('x', Class::Ord, Font::Italic), Font::Italic);
        assert_eq!(slots.resolve('7', Class::Ord, Font::Upright), Font::Upright);
        assert_eq!(slots.resolve('+', Class::Bin, Font::Upright), Font::Upright);
        assert_eq!(slots.resolve('α', Class::Ord, Font::Italic), Font::Italic);
        assert_eq!(slots.resolve('v', Class::Ord, Font::Bold), Font::Bold);
        assert_eq!(slots.resolve('x', Class::Ord, Font::Script), Font::Script);
        assert_eq!(slots.resolve('∑', Class::Op, Font::Upright), Font::Upright);
    }

    #[test]
    fn slots_restyle_the_roles_of_the_tree() {
        let mut slots = StyleFonts {
            variable: Font::Script,
            number: Font::Bold,
            greek: Font::Upright,
            symbol: Font::BoldItalic,
            vector: Font::BoldItalic,
            function: Font::SansSerif,
            ..StyleFonts::default()
        };
        assert_eq!(slots.resolve('x', Class::Ord, Font::Italic), Font::Script);
        assert_eq!(slots.resolve('x', Class::Ord, Font::Upright), Font::Script);
        assert_eq!(slots.resolve('7', Class::Ord, Font::Upright), Font::Bold);
        assert_eq!(slots.resolve('α', Class::Ord, Font::Italic), Font::Upright);
        assert_eq!(
            slots.resolve('+', Class::Bin, Font::Upright),
            Font::BoldItalic
        );
        assert_eq!(slots.resolve('v', Class::Ord, Font::Bold), Font::BoldItalic);
        assert_eq!(
            slots.resolve('d', Class::Op, Font::Upright),
            Font::SansSerif
        );
        // Explicit faces always pass through.
        assert_eq!(slots.resolve('x', Class::Ord, Font::Fraktur), Font::Fraktur);
        assert_eq!(
            slots.resolve('x', Class::Ord, Font::SansSerif),
            Font::SansSerif
        );
        // Text runs follow the text slot, never the variable rules.
        assert_eq!(slots.text_font(Font::Upright), slots.text);
        slots.text = Font::Bold;
        assert_eq!(slots.text_font(Font::Italic), Font::Bold);
        assert_eq!(slots.text_font(Font::Monospace), Font::Monospace);
    }
}
