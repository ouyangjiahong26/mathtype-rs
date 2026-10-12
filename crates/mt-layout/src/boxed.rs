//! The positioned box tree that layout produces.

use mt_core::Font;

/// A positioned piece of a laid-out equation.
///
/// Coordinates are in points and absolute within the root box of the equation: `x` grows to the
/// right, `y` grows downwards with the baseline of the leftmost item at zero, which is also the
/// orientation SVG uses. Every variant that can contain children reports its own width, height
/// (above the baseline) and depth (below it).
///
/// Leaves are always placed inside an [`Box::HBox`] that carries their extents, so
/// [`Box::width`], [`Box::height`] and [`Box::depth`] are exact for every box layout returns.
#[derive(Clone, Debug, PartialEq)]
pub enum Box {
    /// Horizontal sequence, laid out left to right.
    HBox {
        items: Vec<Box>,
        width: f64,
        height: f64,
        depth: f64,
    },
    /// Vertical sequence, listed top to bottom.
    VBox {
        items: Vec<Box>,
        width: f64,
        height: f64,
        depth: f64,
    },
    /// One glyph, positioned by the baseline point of its left edge.
    ///
    /// `size` is the font size in points. `yscale` is the vertical stretch factor applied about
    /// the anchor: 1.0 for ordinary glyphs, larger for delimiters and radicals that had to grow
    /// beyond the largest glyph of the size table. Horizontal stretch is expressed through
    /// `size` instead, so that a stretched glyph keeps its proportions.
    Glyph {
        ch: char,
        font: Font,
        x: f64,
        y: f64,
        size: f64,
        yscale: f64,
    },
    /// Filled rectangle, spanning `x .. x + width` horizontally and
    /// `y - height .. y + depth` vertically. `dashed` marks the placeholder of an empty slot.
    Rule {
        x: f64,
        y: f64,
        width: f64,
        height: f64,
        depth: f64,
        dashed: bool,
    },
    /// Horizontal advance without ink, either an explicit space or inter-atom spacing.
    Kern { width: f64 },
}

impl Box {
    /// Horizontal extent in points.
    ///
    /// Zero for a bare glyph, whose extent lives in the box that encloses it.
    pub fn width(&self) -> f64 {
        match self {
            Box::HBox { width, .. } | Box::VBox { width, .. } | Box::Rule { width, .. } => *width,
            Box::Kern { width } => *width,
            Box::Glyph { .. } => 0.0,
        }
    }

    /// Extent above the baseline in points.
    ///
    /// Zero for a bare glyph, whose extent lives in the box that encloses it.
    pub fn height(&self) -> f64 {
        match self {
            Box::HBox { height, .. } | Box::VBox { height, .. } => *height,
            Box::Rule { height, .. } => *height,
            Box::Kern { .. } | Box::Glyph { .. } => 0.0,
        }
    }

    /// Extent below the baseline in points.
    ///
    /// Zero for a bare glyph, whose extent lives in the box that encloses it.
    pub fn depth(&self) -> f64 {
        match self {
            Box::HBox { depth, .. } | Box::VBox { depth, .. } => *depth,
            Box::Rule { depth, .. } => *depth,
            Box::Kern { .. } | Box::Glyph { .. } => 0.0,
        }
    }

    /// Whether the box contains no children.
    pub fn is_empty(&self) -> bool {
        match self {
            Box::HBox { items, .. } | Box::VBox { items, .. } => items.is_empty(),
            _ => false,
        }
    }

    /// Visit this box and every box inside it, parents before children.
    pub fn visit(&self, visit: &mut impl FnMut(&Box)) {
        visit(self);
        if let Box::HBox { items, .. } | Box::VBox { items, .. } = self {
            for item in items {
                item.visit(visit);
            }
        }
    }
}
