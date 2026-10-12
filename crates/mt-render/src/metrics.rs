//! Glyph measurements taken from the bundled math font.
//!
//! Layout asks for advances and extents in em units, so the font's design units are divided by
//! its units-per-em. The math axis comes from the font's OpenType MATH table when present, which
//! is what makes fractions and delimiters sit the way the font designer intended.

use mt_core::Font;
use mt_layout::FontMetrics;
use ttf_parser::Face;

/// Bundled open-licensed math font, the same face the editor draws with.
pub const MATH_FONT: &[u8] = include_bytes!("../../../assets/fonts/latinmodern-math.otf");

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
        Some(Self {
            face,
            units_per_em,
            axis_height,
            math_leading,
        })
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

    fn glyph(&self, ch: char) -> Option<ttf_parser::GlyphId> {
        self.face
            .glyph_index(ch)
            .or_else(|| self.face.glyph_index('?'))
    }

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

#[cfg(test)]
mod tests {
    use super::{FaceMetrics, MATH_FONT};
    use mt_core::Font;
    use mt_layout::FontMetrics;

    #[test]
    fn the_bundled_face_measures_like_a_math_font() {
        let metrics = FaceMetrics::parse(MATH_FONT, 0).expect("the bundled font parses");
        let advance = metrics.advance('x', Font::Italic);
        assert!((0.3..0.8).contains(&advance), "x advance was {advance}");
        assert!((0.1..0.5).contains(&metrics.axis_height()));
        // The ascender sits above the baseline, as the text layer expects.
        assert!(metrics.ascender() > 0.0);
    }
}
