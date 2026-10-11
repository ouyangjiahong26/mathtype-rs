//! Glyph measurements taken from the bundled math font.
//!
//! Layout asks for advances and extents in em units, so the font's design units are divided by
//! its units-per-em. The math axis comes from the font's OpenType MATH table when present, which
//! is what makes fractions and delimiters sit the way the font designer intended.

use mt_core::Font;
use mt_layout::FontMetrics;
use ttf_parser::Face;

/// Metrics read from a single face.
pub struct FaceMetrics {
    face: Face<'static>,
    units_per_em: f64,
    axis_height: f64,
    math_leading: f64,
}

impl FaceMetrics {
    /// Parse `data` as a font face. Returns `None` when the data is not a usable font.
    pub fn parse(data: &'static [u8], index: u32) -> Option<Self> {
        let face = Face::parse(data, index).ok()?;
        let units_per_em = f64::from(face.units_per_em());
        if units_per_em <= 0.0 {
            return None;
        }
        let constants = face.tables().math.and_then(|math| math.constants);
        let axis_height = constants
            .map(|constants| f64::from(constants.axis_height().value) / units_per_em)
            .unwrap_or(0.25);
        let math_leading = constants
            .map(|constants| f64::from(constants.math_leading().value) / units_per_em)
            .unwrap_or(0.05);
        Some(Self { face, units_per_em, axis_height, math_leading })
    }

    fn glyph(&self, ch: char) -> Option<ttf_parser::GlyphId> {
        self.face.glyph_index(ch).or_else(|| self.face.glyph_index('?'))
    }

    /// Distance from the top of the line box down to the baseline, in em units.
    ///
    /// Drawing positions glyphs from the baseline, while the text layer positions them from the
    /// top of the line box, so this is the conversion between the two.
    pub fn ascender(&self) -> f64 {
        f64::from(self.face.ascender()) / self.units_per_em
    }

    /// The parsed face, for callers that rasterize or outline glyphs themselves.
    pub fn face(&self) -> &Face<'static> {
        &self.face
    }
}

impl FontMetrics for FaceMetrics {
    fn advance(&self, ch: char, _font: Font) -> f64 {
        self.glyph(ch)
            .and_then(|glyph| self.face.glyph_hor_advance(glyph))
            .map_or(0.5, |advance| f64::from(advance) / self.units_per_em)
    }

    fn height(&self, ch: char, _font: Font) -> f64 {
        self.extent(ch, true)
    }

    fn depth(&self, ch: char, _font: Font) -> f64 {
        self.extent(ch, false)
    }

    fn axis_height(&self) -> f64 {
        self.axis_height
    }

    fn math_leading(&self) -> f64 {
        self.math_leading
    }
}

impl FaceMetrics {
    fn extent(&self, ch: char, above: bool) -> f64 {
        let Some(glyph) = self.glyph(ch) else {
            return if above { 0.6 } else { 0.0 };
        };
        let Some(bounds) = self.face.glyph_bounding_box(glyph) else {
            return 0.0;
        };
        let value = if above { bounds.y_max } else { -bounds.y_min };
        (f64::from(value) / self.units_per_em).max(0.0)
    }
}
