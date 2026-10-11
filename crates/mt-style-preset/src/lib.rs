//! Style presets: the text format, the built-ins, and how a preset drives layout and streams.
//!
//! A preset is the whole style a run of the editor is set with: the four type sizes in points,
//! the named spacing table in em units, the font each functional style slot is set in, and the
//! layout defaults. The format is a plain INI-style text a person can read and edit:
//!
//! ```ini
//! [meta]
//! name = 紧凑
//!
//! [sizes]
//! full = 12
//! script = 8
//!
//! [spaces]
//! thin-space = 0.13
//!
//! [styles]
//! variable = italic
//! vector = bold
//!
//! [layout]
//! alignment = center
//! ```
//!
//! Every section and key is optional; whatever the file leaves out keeps the engine's default
//! (see [`mt_layout::Spacing::default`] and [`mt_layout::StyleFonts::default`]). Errors name the
//! line they came from. A preset converts to a [`mt_layout::LayoutStyle`] for rendering and to
//! [`mt_mtef::Preferences`] for embedding in an equation stream.

use std::fmt;
use std::sync::LazyLock;

use mt_core::{Font, Side};
use mt_layout::{LayoutStyle, Spacing, StyleFonts};

mod mtef;

pub use mtef::{from_preferences, to_preferences};

// --------------------------------------------------------------------------- format

/// The four type sizes a preset sets, in points.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct PresetSizes {
    /// Full size, the size top-level material is set at.
    pub full: f64,
    /// Text size, the size literal text runs default to.
    pub text: f64,
    /// Script size, one nesting level deep.
    pub script: f64,
    /// Script-script size, two or more levels deep.
    pub script_script: f64,
}

impl Default for PresetSizes {
    fn default() -> Self {
        Self { full: 12.0, text: 12.0, script: 7.0, script_script: 5.0 }
    }
}

/// One style preset: sizes, spacing, style slots and layout defaults.
#[derive(Clone, Debug, PartialEq)]
pub struct StylePreset {
    /// Name shown in the preset list.
    pub name: String,
    /// The four type sizes, in points.
    pub sizes: PresetSizes,
    /// The spacing table, in em units.
    pub spaces: Spacing,
    /// The font each functional style slot is set in.
    pub styles: StyleFonts,
    /// Default alignment of the equation in its container.
    pub alignment: Side,
}

impl Default for StylePreset {
    fn default() -> Self {
        Self {
            name: "unnamed".to_owned(),
            sizes: PresetSizes::default(),
            spaces: Spacing::default(),
            styles: StyleFonts::default(),
            alignment: Side::Center,
        }
    }
}

impl StylePreset {
    /// The layout style this preset runs with: full size, the nesting factors the script sizes
    /// imply, the spacing table and the font slots.
    ///
    /// `script` and `script-script` name points, so the factors are their ratio to `full` —
    /// the customary 0.7 and 0.5 fall out of a 12/8.4/6 preset.
    pub fn layout_style(&self) -> LayoutStyle {
        LayoutStyle {
            size: self.sizes.full,
            script_scale: self.sizes.script / self.sizes.full,
            script_script_scale: self.sizes.script_script / self.sizes.full,
            spacing: self.spaces,
            fonts: self.styles,
            ..LayoutStyle::default()
        }
    }

    /// Serialize in the preset text format, in the section and key order [`parse`] reads.
    ///
    /// Numbers are written in the shortest form that reads back as the same value, so
    /// `parse(preset.to_text())` is always `Ok(preset)`.
    pub fn to_text(&self) -> String {
        let mut out = String::new();
        out.push_str("# mathtype-rs 样式预设：文本格式，可直接编辑后从编辑器导入。\n\n");
        out.push_str("[meta]\n");
        out.push_str(&format!("name = {}\n\n", self.name));
        out.push_str("[sizes]\n");
        for (key, value) in
            [("full", self.sizes.full), ("text", self.sizes.text), ("script", self.sizes.script), ("script-script", self.sizes.script_script)]
        {
            out.push_str(&format!("{key} = {}\n", number(value)));
        }
        out.push_str("\n[spaces]\n");
        for key in Spacing::KEYS {
            out.push_str(&format!("{key} = {}\n", number(self.spaces.get(key).unwrap_or(0.0))));
        }
        out.push_str("\n[styles]\n");
        for (key, font) in style_slots(&self.styles) {
            out.push_str(&format!("{key} = {}\n", font_name(font)));
        }
        out.push_str("\n[layout]\n");
        out.push_str(&format!("alignment = {}\n", side_name(self.alignment)));
        out
    }
}

/// The preset's style slots as (text key, font) rows, in slot order.
fn style_slots(styles: &StyleFonts) -> [(&'static str, Font); 7] {
    [
        ("text", styles.text),
        ("function", styles.function),
        ("variable", styles.variable),
        ("greek", styles.greek),
        ("symbol", styles.symbol),
        ("vector", styles.vector),
        ("number", styles.number),
    ]
}

/// Format a number the way the format writes it: plain decimal, shortest round-trip form.
fn number(value: f64) -> String {
    format!("{value}")
}

/// The text name of a face, for the `[styles]` section.
pub fn font_name(font: Font) -> &'static str {
    match font {
        Font::Upright => "upright",
        Font::Italic => "italic",
        Font::Bold => "bold",
        Font::BoldItalic => "bold-italic",
        Font::SansSerif => "sans-serif",
        Font::SansSerifBold => "sans-serif-bold",
        Font::SansSerifItalic => "sans-serif-italic",
        Font::SansSerifBoldItalic => "sans-serif-bold-italic",
        Font::Monospace => "monospace",
        Font::Script => "script",
        Font::BoldScript => "bold-script",
        Font::Fraktur => "fraktur",
        Font::BoldFraktur => "bold-fraktur",
        Font::DoubleStruck => "double-struck",
        Font::Symbol => "symbol",
        Font::Extra => "extra",
    }
}

/// The face a `[styles]` value names, or `None` when the value is not a face.
pub fn font_of_name(name: &str) -> Option<Font> {
    Some(match name {
        "upright" => Font::Upright,
        "italic" => Font::Italic,
        "bold" => Font::Bold,
        "bold-italic" => Font::BoldItalic,
        "sans-serif" => Font::SansSerif,
        "sans-serif-bold" => Font::SansSerifBold,
        "sans-serif-italic" => Font::SansSerifItalic,
        "sans-serif-bold-italic" => Font::SansSerifBoldItalic,
        "monospace" => Font::Monospace,
        "script" => Font::Script,
        "bold-script" => Font::BoldScript,
        "fraktur" => Font::Fraktur,
        "bold-fraktur" => Font::BoldFraktur,
        "double-struck" => Font::DoubleStruck,
        "symbol" => Font::Symbol,
        "extra" => Font::Extra,
        _ => return None,
    })
}

fn side_name(side: Side) -> &'static str {
    match side {
        Side::Left => "left",
        Side::Center => "center",
        Side::Right => "right",
    }
}

fn side_of_name(name: &str) -> Option<Side> {
    Some(match name {
        "left" => Side::Left,
        "center" => Side::Center,
        "right" => Side::Right,
        _ => return None,
    })
}

// ---------------------------------------------------------------------------- errors

/// Why a preset file could not be read, with the line it happened on.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PresetError {
    /// One-based line number the error is about.
    pub line: usize,
    /// What is wrong with that line.
    pub message: String,
}

impl fmt::Display for PresetError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "line {}: {}", self.line, self.message)
    }
}

impl std::error::Error for PresetError {}

fn error(line: usize, message: impl Into<String>) -> PresetError {
    PresetError { line, message: message.into() }
}

// ----------------------------------------------------------------------------- parse

/// The section a line of the file is in.
#[derive(Clone, Copy, PartialEq)]
enum Section {
    Meta,
    Sizes,
    Spaces,
    Styles,
    Layout,
}

impl Section {
    fn from_header(header: &str) -> Option<Self> {
        Some(match header {
            "meta" => Section::Meta,
            "sizes" => Section::Sizes,
            "spaces" => Section::Spaces,
            "styles" => Section::Styles,
            "layout" => Section::Layout,
            _ => return None,
        })
    }

    fn name(self) -> &'static str {
        match self {
            Section::Meta => "meta",
            Section::Sizes => "sizes",
            Section::Spaces => "spaces",
            Section::Styles => "styles",
            Section::Layout => "layout",
        }
    }
}

/// Read a preset from its text format.
///
/// Sections and keys may appear in any order; every one of them is optional. Unknown sections,
/// unknown keys, duplicated keys and malformed or out-of-range values are errors that name the
/// line they were found on.
pub fn parse(text: &str) -> Result<StylePreset, PresetError> {
    let mut preset = StylePreset::default();
    let mut section: Option<Section> = None;
    let mut seen_sections: Vec<Section> = Vec::new();
    let mut seen: Vec<(Section, String)> = Vec::new();

    for (index, raw) in text.lines().enumerate() {
        let line = index + 1;
        let trimmed = raw.trim();
        if trimmed.is_empty() || trimmed.starts_with('#') || trimmed.starts_with(';') {
            continue;
        }
        if let Some(header) = trimmed.strip_prefix('[') {
            let name = header.strip_suffix(']').ok_or_else(|| {
                error(line, format!("a section header must end in ']': {trimmed}"))
            })?;
            let parsed = Section::from_header(name).ok_or_else(|| {
                error(line, format!("unknown section [{name}] (expected meta, sizes, spaces, styles or layout)"))
            })?;
            if seen_sections.contains(&parsed) {
                return Err(error(line, format!("section [{}] appears twice", parsed.name())));
            }
            seen_sections.push(parsed);
            section = Some(parsed);
            continue;
        }
        let Some(section) = section else {
            return Err(error(line, format!("a key before any section: {trimmed}")));
        };
        let (key, value) = trimmed
            .split_once('=')
            .ok_or_else(|| error(line, format!("expected 'key = value', found no '=': {trimmed}")))?;
        let (key, value) = (key.trim(), value.trim());
        if key.is_empty() {
            return Err(error(line, "the key before '=' is empty"));
        }
        if value.is_empty() {
            return Err(error(line, format!("the value of '{key}' is empty")));
        }
        if seen.contains(&(section, key.to_owned())) {
            return Err(error(line, format!("duplicate key '{key}' in section [{}]", section.name())));
        }
        seen.push((section, key.to_owned()));
        match section {
            Section::Meta => {
                if key == "name" {
                    preset.name = value.to_owned();
                } else {
                    return Err(error(line, format!("unknown key '{key}' in section [meta] (only name)")));
                }
            }
            Section::Sizes => {
                let value = positive(line, key, value)?;
                match key {
                    "full" => preset.sizes.full = value,
                    "text" => preset.sizes.text = value,
                    "script" => preset.sizes.script = value,
                    "script-script" => preset.sizes.script_script = value,
                    _ => {
                        return Err(error(
                            line,
                            format!("unknown key '{key}' in section [sizes] (full, text, script, script-script)"),
                        ))
                    }
                }
            }
            Section::Spaces => {
                let value = non_negative(line, key, value)?;
                if !preset.spaces.set(key, value) {
                    return Err(error(line, format!("unknown spacing '{key}' in section [spaces]")));
                }
            }
            Section::Styles => {
                let font = font_of_name(value)
                    .ok_or_else(|| error(line, format!("unknown font '{value}' for style '{key}'")))?;
                let slots = style_slots(&preset.styles);
                if !slots.iter().any(|(slot, _)| *slot == key) {
                    return Err(error(
                        line,
                        format!("unknown style '{key}' in section [styles] (text, function, variable, greek, symbol, vector, number)"),
                    ));
                }
                let target = &mut preset.styles;
                match key {
                    "text" => target.text = font,
                    "function" => target.function = font,
                    "variable" => target.variable = font,
                    "greek" => target.greek = font,
                    "symbol" => target.symbol = font,
                    "vector" => target.vector = font,
                    _ => target.number = font,
                }
            }
            Section::Layout => {
                if key == "alignment" {
                    preset.alignment = side_of_name(value).ok_or_else(|| {
                        error(line, format!("alignment must be left, center or right, not '{value}'"))
                    })?;
                } else {
                    return Err(error(line, format!("unknown key '{key}' in section [layout] (only alignment)")));
                }
            }
        }
    }
    Ok(preset)
}

/// A strictly positive finite number.
fn positive(line: usize, key: &str, value: &str) -> Result<f64, PresetError> {
    let parsed = value
        .parse::<f64>()
        .map_err(|_| error(line, format!("'{key}' needs a number of points, found '{value}'")))?;
    if parsed.is_finite() && parsed > 0.0 {
        Ok(parsed)
    } else {
        Err(error(line, format!("'{key}' must be greater than zero")))
    }
}

/// A finite number that is at least zero.
fn non_negative(line: usize, key: &str, value: &str) -> Result<f64, PresetError> {
    let parsed = value
        .parse::<f64>()
        .map_err(|_| error(line, format!("'{key}' needs a number of em, found '{value}'")))?;
    if parsed.is_finite() && parsed >= 0.0 {
        Ok(parsed)
    } else {
        Err(error(line, format!("'{key}' must not be negative")))
    }
}

// --------------------------------------------------------------------------- builtins

/// One built-in preset: its short id and the file it is read from.
struct Builtin {
    id: &'static str,
    text: &'static str,
}

const BUILTINS: [Builtin; 4] = [
    Builtin { id: "default", text: include_str!("../presets/default.ini") },
    Builtin { id: "classic", text: include_str!("../presets/classic.ini") },
    Builtin { id: "compact", text: include_str!("../presets/compact.ini") },
    Builtin { id: "large-print", text: include_str!("../presets/large-print.ini") },
];

static PARSED_BUILTINS: LazyLock<Vec<StylePreset>> =
    LazyLock::new(|| BUILTINS.iter().map(|builtin| parse(builtin.text).expect("builtin presets parse")).collect());

/// The built-in presets in menu order: 默认, 经典 MathType, 紧凑, 大字号.
pub fn builtins() -> &'static [StylePreset] {
    &PARSED_BUILTINS
}

/// The id of a built-in preset, for stable naming of exported files.
pub fn builtin_id(preset: &StylePreset) -> Option<&'static str> {
    PARSED_BUILTINS
        .iter()
        .zip(BUILTINS.iter())
        .find(|(parsed, _)| *parsed == preset)
        .map(|(_, builtin)| builtin.id)
}

#[cfg(test)]
mod tests {
    use super::*;
    use mt_core::Font;

    #[test]
    fn a_full_preset_round_trips_through_text() {
        for preset in builtins() {
            let text = preset.to_text();
            let back = parse(&text).unwrap_or_else(|error| panic!("{}: {error}", preset.name));
            assert_eq!(&back, preset, "{} must survive its own text form", preset.name);
        }
    }

    #[test]
    fn four_builtins_exist_and_are_valid() {
        let presets = builtins();
        assert!(presets.len() >= 4, "at least four built-in presets");
        let names: Vec<&str> = presets.iter().map(|preset| preset.name.as_str()).collect();
        assert_eq!(names, ["默认", "经典 MathType", "紧凑", "大字号"]);
        for preset in presets {
            assert!(preset.sizes.full > 0.0);
            assert!(preset.sizes.script <= preset.sizes.full);
            assert!(preset.sizes.script_script <= preset.sizes.script);
        }
        assert_eq!(builtin_id(&presets[0]), Some("default"));
        assert_eq!(builtin_id(&presets[3]), Some("large-print"));
    }

    #[test]
    fn the_default_preset_keeps_the_engine_values() {
        let preset = &builtins()[0];
        assert_eq!(preset.sizes.full, 18.0);
        assert_eq!(preset.spaces, Spacing::default());
        assert_eq!(preset.styles, StyleFonts::default());
        let style = preset.layout_style();
        assert_eq!(style.size, 18.0);
        assert!((style.script_scale - 0.7).abs() < 1e-12);
        assert!((style.script_script_scale - 0.5).abs() < 1e-12);
    }

    #[test]
    fn a_preset_with_every_face_and_partial_sections_parses() {
        let text = "[styles]\n\
                    text = fraktur\n\
                    function = sans-serif\n\
                    variable = script\n\
                    greek = bold-italic\n\
                    symbol = symbol\n\
                    vector = bold-fraktur\n\
                    number = double-struck\n\
                    [sizes]\n\
                    full = 24\n";
        let preset = parse(text).expect("parses");
        assert_eq!(preset.styles.text, Font::Fraktur);
        assert_eq!(preset.styles.variable, Font::Script);
        assert_eq!(preset.styles.greek, Font::BoldItalic);
        assert_eq!(preset.styles.vector, Font::BoldFraktur);
        assert_eq!(preset.styles.number, Font::DoubleStruck);
        assert_eq!(preset.sizes.full, 24.0);
        // Whatever the file leaves out keeps the defaults, and the name falls back.
        assert_eq!(preset.name, "unnamed");
        assert_eq!(preset.sizes.script, PresetSizes::default().script);
        assert_eq!(preset.spaces, Spacing::default());
        assert_eq!(preset.alignment, Side::Center);
    }

    #[test]
    fn errors_name_their_line() {
        let bad = |text: &str| parse(text).unwrap_err();

        let error = bad("[sizes]\nfull = twelve\n");
        assert_eq!(error.line, 2);
        assert!(error.message.contains("full"));

        let error = bad("[spaces]\nthick-space = -1\n");
        assert_eq!(error.line, 2);
        assert!(error.message.contains("negative"));

        let error = bad("[styles]\nvariable = copperplate\n");
        assert_eq!(error.line, 2);
        assert!(error.message.contains("copperplate"));

        let error = bad("[layout]\nalignment = justified\n");
        assert_eq!(error.line, 2);

        let error = bad("[spaces]\nwide-space = 0.2\n");
        assert_eq!(error.line, 2);
        assert!(error.message.contains("wide-space"));

        let error = bad("[styles]\nvector = bold\nvector = italic\n");
        assert_eq!(error.line, 3);
        assert!(error.message.contains("duplicate"));

        let error = bad("[cymbals]\nfull = 1\n");
        assert_eq!(error.line, 1);
        assert!(error.message.contains("cymbals"));

        let error = bad("full = 12\n");
        assert_eq!(error.line, 1);
        assert!(error.message.contains("section"));

        let error = bad("[sizes]\nfull 12\n");
        assert_eq!(error.line, 2);
        assert!(error.message.contains("'='"));

        let error = bad("[sizes]\n[sizes]\n");
        assert_eq!(error.line, 2);
        assert!(error.message.contains("twice"));

        let error = bad("[sizes]\n= 12\n");
        assert_eq!(error.line, 2);
        assert!(error.message.contains("key"));

        let error = bad("[meta]\nname =\n");
        assert_eq!(error.line, 2);
        assert!(error.message.contains("empty"));
    }

    #[test]
    fn comments_and_blank_lines_are_ignored() {
        let text = "# a comment\n\n;syntax from another editor\n[meta]\nname = X\n";
        assert_eq!(parse(text).unwrap().name, "X");
    }
}
