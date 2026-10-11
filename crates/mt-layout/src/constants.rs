//! Every tunable number the layout engine uses, in em units unless stated otherwise.
//!
//! The values follow the classic math typesetting tradition (Knuth, *The TeXbook*, chapters 17
//! and 18): inter-atom spacing is the 3/18, 4/18 and 5/18 em thin/medium/thick trio, and the
//! template gaps are the usual order of magnitude found in Computer Modern.

// ---------------------------------------------------------------------------------------------
// Inter-atom spacing (TeXbook chapter 18, rule 20)
// ---------------------------------------------------------------------------------------------

/// Space between two ordinary atoms, and the unit of the "thin" space (3/18 em).
pub const THIN_KERN: f64 = 3.0 / 18.0;

/// Space around binary operators and relations in the "medium" class (4/18 em).
pub const MEDIUM_KERN: f64 = 4.0 / 18.0;

/// Space around relations in the "thick" class (5/18 em).
pub const THICK_KERN: f64 = 5.0 / 18.0;

// ---------------------------------------------------------------------------------------------
// Fractions
// ---------------------------------------------------------------------------------------------

/// Thickness of the fraction rule.
pub const FRAC_RULE_THICKNESS: f64 = 0.04;

/// Clearance between the rule and the numerator above it, and the denominator below it.
pub const FRAC_RULE_GAP: f64 = 0.1;

/// Clearance used instead of [`FRAC_RULE_GAP`] when the fraction has no rule.
pub const FRAC_NOBAR_GAP: f64 = 0.2;

/// How far the numerator is raised and the denominator lowered for a slash fraction.
pub const FRAC_SLASH_SHIFT: f64 = 0.4;

/// Space between the numerator, the slash and the denominator of a slash fraction.
pub const FRAC_SLASH_KERN: f64 = 0.05;

// ---------------------------------------------------------------------------------------------
// Radicals
// ---------------------------------------------------------------------------------------------

/// Thickness of the radical's overbar.
pub const RADICAL_RULE_THICKNESS: f64 = 0.04;

/// Gap between the top of the radicand and the overbar.
pub const RADICAL_RULE_GAP: f64 = 0.1;

/// Space between the radical sign and the radicand.
pub const RADICAL_KERN: f64 = 0.05;

/// Gap between the raised root index and the overbar.
pub const RADICAL_INDEX_GAP: f64 = 0.1;

// ---------------------------------------------------------------------------------------------
// Scripts and limits
// ---------------------------------------------------------------------------------------------

/// Extra offset added to `(height - depth) / 2` when raising a superscript.
pub const SCRIPT_OFFSET: f64 = 0.15;

/// Extra offset added to the base depth when lowering a subscript.
pub const SUBSCRIPT_OFFSET: f64 = 0.15;

/// Space between a base and the scripts beside it.
pub const SCRIPT_KERN: f64 = 0.05;

/// Minimum clearance kept between a superscript and a subscript.
pub const SCRIPT_MIN_GAP: f64 = 0.07;

/// Clearance between a base and limits stacked above or below it.
pub const LIMIT_GAP: f64 = 0.15;

/// Size factor of a large operator whose limits are stacked above and below it.
///
/// A display-style summation is drawn noticeably larger than the text around it.
pub const BIG_OP_SCALE_LIMITS: f64 = 1.4;

/// Size factor of a large operator whose limits sit beside it.
pub const BIG_OP_SCALE_INLINE: f64 = 1.15;

/// Size factor of an integral with stacked limits, which grows far less than a summation so that
/// it keeps its slim shape.
pub const BIG_OP_SCALE_INTEGRAL: f64 = 1.15;

// ---------------------------------------------------------------------------------------------
// Delimiters, radicals and accents that grow
// ---------------------------------------------------------------------------------------------

/// Delimiter and radical sizes tried in order; the first one tall enough is used.
///
/// When even the last entry is too short the glyph keeps that size and is stretched vertically
/// instead, so that the stretch does not fatten the stroke of an already enlarged glyph.
pub const TALL_GLYPH_SCALES: [f64; 6] = [1.0, 1.2, 1.45, 1.8, 2.2, 2.8];

/// How far a delimiter is moved back towards the body it encloses.
pub const FENCE_KERN: f64 = 0.05;

/// Accent sizes tried in order; the first one wide enough to cover the base is used.
pub const ACCENT_SCALES: [f64; 6] = [1.0, 1.2, 1.45, 1.75, 2.1, 2.6];

/// Clearance between the base and the accent over or under it.
pub const ACCENT_GAP: f64 = 0.1;

/// Thickness of an accent drawn as a rule (`Bar`, `Line`, `Strike`).
pub const ACCENT_RULE_THICKNESS: f64 = 0.06;

// ---------------------------------------------------------------------------------------------
// Matrices
// ---------------------------------------------------------------------------------------------

/// Minimum space between two matrix columns.
pub const MATRIX_COL_SEPARATION: f64 = 0.5;

/// Thickness of the rules drawn when a matrix requests them.
pub const MATRIX_RULE_THICKNESS: f64 = 0.04;

// ---------------------------------------------------------------------------------------------
// Placeholders and previews
// ---------------------------------------------------------------------------------------------

/// Width of the placeholder drawn for an empty argument slot.
pub const EMPTY_SLOT_WIDTH: f64 = 0.5;

/// Height of the placeholder drawn for an empty argument slot, above the baseline.
pub const EMPTY_SLOT_HEIGHT: f64 = 0.7;

/// Depth of the placeholder drawn for an empty argument slot, below the baseline.
pub const EMPTY_SLOT_DEPTH: f64 = 0.2;

/// Padding added around the root box when building an SVG `viewBox`, in em.
pub const SVG_PADDING: f64 = 0.3;
