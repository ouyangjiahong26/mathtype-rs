//! Conversion between presets and the equation preferences a stream carries.
//!
//! A stream's preferences ([`mt_mtef::Preferences`]) hold a typesize table, a spacing table and
//! one style slot per MathType typeface. The overlap with a preset is the sizes and the style
//! slots; the stream's 30-entry spacing table follows MathType's own percent-of-full semantics
//! that the layout engine does not use, so it keeps the stream defaults in both directions.
//!
//! Style slots can only name one of the four fonts every stream defines (Times New Roman,
//! Symbol, Courier New, MT Extra) plus bold and italic bits, so faces outside that set — script,
//! fraktur, double-struck, sans-serif — convert to the Times slot with their bold bit and read
//! back as the Times face. Sizes survive exactly; `text` has no slot of its own in a stream and
//! reads back as the full size.

use mt_core::Font;
use mt_layout::{Spacing, StyleFonts};
use mt_mtef::{Dimension, Preferences, StyleDef, Unit};

use crate::{PresetSizes, StylePreset, number};

/// The preferences a stream written under this preset declares.
///
/// The typesize table's full, script and script-script entries become the preset's; the style
/// slots follow the preset's seven functional styles in typeface order; everything the preset
/// does not model keeps [`mt_mtef::default_preferences`].
pub fn to_preferences(preset: &StylePreset) -> Preferences {
    let mut preferences = mt_mtef::default_preferences();
    let sizes = [
        ("full", preset.sizes.full),
        ("script", preset.sizes.script),
        ("script-script", preset.sizes.script_script),
    ];
    for (index, (_, value)) in sizes.iter().enumerate() {
        preferences.sizes[index] = Dimension {
            unit: Unit::Points,
            value: number(*value),
        };
    }
    let slots = [
        preset.styles.text,
        preset.styles.function,
        preset.styles.variable,
        preset.styles.greek,
        preset.styles.greek,
        preset.styles.symbol,
        preset.styles.vector,
        preset.styles.number,
    ];
    for (index, font) in slots.iter().enumerate() {
        let (definition, style) = style_def(*font);
        preferences.styles[index] = StyleDef {
            font: Some(definition),
            style,
        };
    }
    preferences
}

/// The preset a stream's preferences describe.
///
/// The name is [`StylePreset::default`]'s, since a stream carries none; sizes convert from
/// whatever unit they were written in, with percent entries read against the full size; style
/// slots past the seven functional ones (user styles, MT Extra) are ignored.
pub fn from_preferences(preferences: &Preferences) -> StylePreset {
    let mut preset = StylePreset::default();
    preset.name = "导入的预设".to_owned();
    let full = dimension(&preferences.sizes.first(), None).unwrap_or(preset.sizes.full);
    preset.sizes = PresetSizes {
        full,
        text: full,
        script: dimension(&preferences.sizes.get(1), Some(full))
            .unwrap_or_else(|| full * 7.0 / 12.0),
        script_script: dimension(&preferences.sizes.get(2), Some(full))
            .unwrap_or_else(|| full * 5.0 / 12.0),
    };
    let slot = |index: usize| preferences.styles.get(index).copied();
    preset.styles = StyleFonts {
        text: font_of(slot(0), Font::Upright),
        function: font_of(slot(1), Font::Upright),
        variable: font_of(slot(2), Font::Italic),
        greek: font_of(slot(3), Font::Italic),
        symbol: font_of(slot(5), Font::Upright),
        vector: font_of(slot(6), Font::Bold),
        number: font_of(slot(7), Font::Upright),
    };
    preset.spaces = Spacing::default();
    preset
}

/// A stream style def as (1-based font definition index, character style bits).
fn style_def(font: Font) -> (usize, u8) {
    let bits = |bold: bool, italic: bool| u8::from(bold) | u8::from(italic) << 1;
    match font {
        Font::Upright => (1, bits(false, false)),
        Font::Italic => (1, bits(false, true)),
        Font::Bold => (1, bits(true, false)),
        Font::BoldItalic => (1, bits(true, true)),
        Font::Symbol => (2, bits(false, false)),
        Font::Monospace => (3, bits(false, false)),
        Font::Extra => (4, bits(false, false)),
        Font::SansSerif | Font::Script | Font::Fraktur | Font::DoubleStruck => {
            (1, bits(false, false))
        }
        Font::SansSerifItalic => (1, bits(false, true)),
        Font::SansSerifBold | Font::BoldScript | Font::BoldFraktur => (1, bits(true, false)),
        Font::SansSerifBoldItalic => (1, bits(true, true)),
    }
}

/// The face a stream style slot names, or `fallback` when the slot is absent.
fn font_of(slot: Option<StyleDef>, fallback: Font) -> Font {
    let Some(slot) = slot else { return fallback };
    let bold = slot.bold();
    let italic = slot.italic();
    match slot.font {
        Some(2) => Font::Symbol,
        Some(3) => Font::Monospace,
        Some(4) => Font::Extra,
        _ => match (bold, italic) {
            (false, false) => Font::Upright,
            (true, false) => Font::Bold,
            (false, true) => Font::Italic,
            (true, true) => Font::BoldItalic,
        },
    }
}

/// A dimension as points, given the full size percent entries are relative to.
///
/// Entries that do not parse, are not finite or fall outside sane bounds read as `None`.
fn dimension(entry: &Option<&Dimension>, full: Option<f64>) -> Option<f64> {
    let entry = (*entry)?;
    let value: f64 = entry.value.parse().ok()?;
    if !value.is_finite() {
        return None;
    }
    let points = match entry.unit {
        Unit::Points => value,
        Unit::Inches => value * 72.0,
        Unit::Centimeters => value * 72.0 / 2.54,
        Unit::Picas => value * 12.0,
        Unit::Percent => value / 100.0 * full?,
        Unit::Other(_) => return None,
    };
    (points.is_finite() && points > 0.0 && points < 1000.0).then_some(points)
}

#[cfg(test)]
mod tests {
    use super::super::{builtins, parse};
    use super::*;
    use mt_core::Font;

    /// A preset whose slots the stream format can name exactly.
    fn expressible() -> StylePreset {
        parse(
            "[meta]\nname = 可表达的预设\n\
             [sizes]\nfull = 14\ntext = 14\nscript = 9\nscript-script = 6\n\
             [styles]\ntext = upright\nfunction = upright\nvariable = bold-italic\n\
             greek = italic\nsymbol = symbol\nvector = bold\nnumber = monospace\n",
        )
        .unwrap()
    }

    #[test]
    fn sizes_and_slots_travel_into_the_preferences() {
        let preferences = to_preferences(&expressible());
        let value = |index: usize| preferences.sizes[index].value.as_str();
        assert_eq!((value(0), value(1), value(2)), ("14", "9", "6"));
        assert_eq!(preferences.sizes[0].unit, Unit::Points);
        let def = |index: usize| preferences.styles[index];
        assert_eq!(
            def(0),
            StyleDef {
                font: Some(1),
                style: 0
            }
        );
        assert_eq!(
            def(2),
            StyleDef {
                font: Some(1),
                style: 3
            }
        );
        assert_eq!(
            def(5),
            StyleDef {
                font: Some(2),
                style: 0
            }
        );
        assert_eq!(
            def(6),
            StyleDef {
                font: Some(1),
                style: 1
            }
        );
        assert_eq!(
            def(7),
            StyleDef {
                font: Some(3),
                style: 0
            }
        );
        // Slots the preset has no say in keep the stream defaults.
        assert_eq!(def(8), mt_mtef::default_preferences().styles[8]);
    }

    #[test]
    fn expressible_presets_survive_the_stream_round_trip() {
        let preset = expressible();
        let back = from_preferences(&to_preferences(&preset));
        assert_eq!(back.sizes, preset.sizes);
        assert_eq!(back.styles, preset.styles);
        assert_eq!(back.spaces, preset.spaces);
    }

    #[test]
    fn exotic_faces_convert_to_the_times_slot_with_their_bold_bit() {
        let preset = parse("[styles]\nvariable = script\nvector = bold-fraktur\n").unwrap();
        let preferences = to_preferences(&preset);
        assert_eq!(
            preferences.styles[2],
            StyleDef {
                font: Some(1),
                style: 0
            }
        );
        assert_eq!(
            preferences.styles[6],
            StyleDef {
                font: Some(1),
                style: 1
            }
        );
        // Reading back yields the Times face, by design.
        let back = from_preferences(&preferences);
        assert_eq!(back.styles.variable, Font::Upright);
        assert_eq!(back.styles.vector, Font::Bold);
    }

    #[test]
    fn dimensions_convert_from_any_unit() {
        let mut preferences = mt_mtef::default_preferences();
        preferences.sizes[0] = Dimension {
            unit: Unit::Centimeters,
            value: "0.5".to_owned(),
        };
        preferences.sizes[1] = Dimension {
            unit: Unit::Percent,
            value: "50".to_owned(),
        };
        let preset = from_preferences(&preferences);
        assert!((preset.sizes.full - 0.5 * 72.0 / 2.54).abs() < 1e-9);
        assert!((preset.sizes.script - preset.sizes.full * 0.5).abs() < 1e-9);
    }

    #[test]
    fn the_builtins_embed_as_valid_stream_preferences() {
        let math = mt_core::Math::new(mt_core::Node::variable('x'));
        for preset in builtins() {
            let stream = mt_mtef::write_with_preferences(&math, &to_preferences(preset));
            let read = mt_mtef::read(&stream).expect("the stream must read back");
            assert_eq!(
                read.header.preferences.sizes[0].value,
                number(preset.sizes.full),
                "{}: the full size must be embedded",
                preset.name
            );
        }
    }
}
