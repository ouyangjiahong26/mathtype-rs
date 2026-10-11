//! The layout engine: walks the equation tree and produces positioned boxes.
//!
//! The engine is a direct translation of the classic math typesetting rules (Knuth, *The TeXbook*,
//! chapters 17 and 18) to the shape of [`mt_core`]'s tree:
//!
//! * a row is laid out left to right, inserting inter-atom spacing between neighbours according to
//!   their [`Class`], with a leading or trailing binary operator demoted to an ordinary atom;
//! * arguments of templates (fraction rule, radicand, scripts, limits, accents, table cells) are
//!   laid out one nesting level deeper, which selects `script_scale` and then
//!   `script_script_scale`;
//! * templates are placed around their arguments by the constants in [`crate::constants`], and
//!   the pieces that have to grow (delimiters, radicals, accents) are fitted to the material they
//!   cover.
//!
//! Every number is in points and every leaf records the position it must be drawn at, so no
//! metric lookup happens a second time when the boxes are drawn.

use std::cell::RefCell;

use mt_core::{
    Accent, AccentKind, BigOp, Char, Class, Delim, Font, Frac, FracKind, Fence, Limits, Math,
    MathStyle, Matrix, MatrixKind, Node, Path, Scripts, Side, Sqrt, Styled, TextRun,
};

use crate::constants::*;
use crate::spacing::Spacing;
use crate::variants::math_variant;
use crate::{Box, FontMetrics, LayoutStyle};

/// A laid-out box together with its extents in points, relative to its own baseline origin.
struct Laid {
    node: Box,
    w: f64,
    h: f64,
    d: f64,
}

impl Laid {
    fn new(node: Box, w: f64, h: f64, d: f64) -> Self {
        Self { node, w, h, d }
    }
}

/// Everything the recursion needs to know about where it is.
#[derive(Clone)]
struct Ctx<'a> {
    metrics: &'a dyn FontMetrics,
    style: &'a LayoutStyle,
    /// Nesting level, which selects the size factor.
    level: u8,
    /// Whether the material is set cramped, which suppresses the extra superscript clearance.
    cramped: bool,
    /// Face imposed by an enclosing [`Node::Styled`].
    font_override: Option<Font>,
    /// Size factor applied on top of the nesting level, used to draw a large operator's own glyph
    /// at display size while its scripts stay at script size.
    scale: f64,
    /// Set while row anchors are being recorded.
    tracking: Option<Tracking<'a>>,
}

/// Where the node being laid out sits in the tree and where its rows are recorded.
#[derive(Clone)]
struct Tracking<'a> {
    recorder: &'a Recorder,
    /// Path of the node being laid out, as [`mt_core::Path`] addresses it.
    path: Path,
}

/// Collects the row anchors of one layout run.
#[derive(Default)]
struct Recorder {
    rows: RefCell<Vec<RowAnchor>>,
}

impl<'a> Ctx<'a> {
    fn at_level(&self, level: u8) -> Self {
        Self { level, ..self.clone() }
    }

    /// Context for material set cramped, which suppresses extra superscript clearance.
    fn cramped(&self) -> Self {
        Self { cramped: true, ..self.clone() }
    }

    /// Context for material drawn `factor` times larger or smaller than this one.
    fn scaled(&self, factor: f64) -> Self {
        Self { scale: self.scale * factor, ..self.clone() }
    }

    /// Context for the node held in slot `slot` of the node being laid out.
    fn slot(&self, slot: usize) -> Self {
        match &self.tracking {
            Some(tracking) => Self {
                tracking: Some(Tracking {
                    recorder: tracking.recorder,
                    path: tracking.path.child(slot),
                }),
                ..self.clone()
            },
            None => self.clone(),
        }
    }

    /// Reserve the anchor of the row about to be laid out, so rows come out outermost first.
    fn begin_row(&self, ox: f64, oy: f64) -> Option<usize> {
        let tracking = self.tracking.as_ref()?;
        let mut rows = tracking.recorder.rows.borrow_mut();
        rows.push(RowAnchor { path: tracking.path.clone(), origin: (ox, oy), offsets: Vec::new() });
        Some(rows.len() - 1)
    }

    /// Fill in the insertion offsets of a row reserved by [`Ctx::begin_row`].
    fn finish_row(&self, anchor: Option<usize>, offsets: Vec<f64>) {
        if let (Some(anchor), Some(tracking)) = (anchor, self.tracking.as_ref()) {
            tracking.recorder.rows.borrow_mut()[anchor].offsets = offsets;
        }
    }

    /// Move the anchors recorded for this subtree, after its box was moved by `(dx, dy)`.
    ///
    /// Templates lay their arguments out at the origin and place them afterwards, so the anchors
    /// of everything below this node — this node's own row included — follow the same shift.
    fn shift_rows(&self, dx: f64, dy: f64) {
        let Some(tracking) = self.tracking.as_ref() else {
            return;
        };
        let prefix = tracking.path.as_slice();
        for anchor in tracking.recorder.rows.borrow_mut().iter_mut() {
            if anchor.path.as_slice().starts_with(prefix) {
                anchor.origin.0 += dx;
                anchor.origin.1 += dy;
            }
        }
    }

    /// Font size in points at this nesting level and scale.
    fn size(&self) -> f64 {
        self.style.size_at(self.level) * self.scale
    }

    /// `units` em expressed in points at this nesting level.
    fn em(&self, units: f64) -> f64 {
        units * self.size()
    }
}

/// Baseline origin of one row box plus the x offset of every insertion point in that row.
///
/// Insertion points are the places an editor can put a caret between two items, so a row of `n`
/// items has `n + 1` of them: the row's left edge, the start of each item, and the row's right
/// edge.
#[derive(Clone, Debug, PartialEq)]
pub struct RowAnchor {
    /// Path of the row, exactly as [`mt_core::Path`] addresses it (root row = `Path::root()`).
    pub path: Path,
    /// `(x, y)` of the row's baseline-left corner, in root-box coordinates.
    pub origin: (f64, f64),
    /// One entry per insertion position: `offsets.len() == row.len() + 1`, offset 0 is the row's
    /// left edge, offset `i` is the x where item `i` starts, the last entry is the row's right
    /// edge. Offsets are relative to `origin.0`.
    pub offsets: Vec<f64>,
}

/// Lay `math` out, using `metrics` for glyph shapes and `style` for sizes.
///
/// The result is always an [`Box::HBox`] carrying the extents of the whole equation. Glyph and
/// rule coordinates are absolute within that box, measured from the baseline of its leftmost item,
/// with `y` growing downwards.
pub fn layout(math: &Math, metrics: &dyn FontMetrics, style: &LayoutStyle) -> Box {
    top_level(layout_tree(math, metrics, style, None))
}

/// Like [`layout`], but also report the caret geometry of every row it visited.
///
/// Anchors come out outermost first, in the same coordinate space as the returned box, and cover
/// the root row as well as every row inside a template slot. An empty row reports a single
/// insertion point at its left edge, matching its zero-length contents.
pub fn layout_with_anchors(
    math: &Math,
    metrics: &dyn FontMetrics,
    style: &LayoutStyle,
) -> (Box, Vec<RowAnchor>) {
    let recorder = Recorder::default();
    let root = top_level(layout_tree(math, metrics, style, Some(&recorder)));
    (root, recorder.rows.into_inner())
}

/// Lay out the body of `math`, recording row anchors into `recorder` when one is given.
fn layout_tree(
    math: &Math,
    metrics: &dyn FontMetrics,
    style: &LayoutStyle,
    recorder: Option<&Recorder>,
) -> Laid {
    let ctx = Ctx {
        metrics,
        style,
        level: style.level.max(level_of(math.style)),
        cramped: style.cramped,
        font_override: None,
        scale: 1.0,
        tracking: recorder
            .map(|recorder| Tracking { recorder, path: Path::root() }),
    };
    layout_node(&math.body, &ctx, 0.0, 0.0)
}

/// Wrap a laid-out body in the top-level horizontal box.
fn top_level(laid: Laid) -> Box {
    match laid.node {
        node @ Box::HBox { .. } => node,
        node => Box::HBox { items: vec![node], width: laid.w, height: laid.h, depth: laid.d },
    }
}

/// Nesting level implied by an author-visible size preset.
fn level_of(style: MathStyle) -> u8 {
    match style {
        MathStyle::Full | MathStyle::Text => 0,
        MathStyle::Script => 1,
        MathStyle::ScriptScript => 2,
    }
}

/// Lay out one node with its origin at `(ox, oy)`.
fn layout_node(node: &Node, ctx: &Ctx, ox: f64, oy: f64) -> Laid {
    match node {
        Node::Row(row) => layout_row(row, ctx, ox, oy),
        Node::Char(character) => layout_char(*character, ctx, ox, oy),
        Node::Text(run) => layout_text(run, ctx, ox, oy),
        Node::Space(space) => {
            let width = ctx.em(space.em());
            Laid::new(Box::Kern { width }, width, 0.0, 0.0)
        }
        Node::Frac(frac) => layout_frac(frac, ctx, ox, oy),
        Node::Sqrt(sqrt) => layout_sqrt(sqrt, ctx, ox, oy),
        Node::Scripts(scripts) => layout_scripts(scripts, ctx, ox, oy),
        Node::Limits(limits) => layout_limits(limits, ctx, ox, oy),
        Node::Fence(fence) => layout_fence(fence, ctx, ox, oy),
        Node::Matrix(matrix) => layout_matrix(matrix, ctx, ox, oy),
        Node::Accent(accent) => layout_accent(accent, ctx, ox, oy),
        Node::Big(big) => layout_big(big, ctx, ox, oy),
        Node::Styled(styled) => layout_styled(styled, ctx, ox, oy),
        Node::Align => zero_box(),
        Node::Error(message) => layout_message(message, ctx, ox, oy),
    }
}

// -------------------------------------------------------------------------------------------
// Rows and inter-atom spacing
// -------------------------------------------------------------------------------------------

/// Lay out a horizontal sequence, inserting the spacing the atom classes call for.
fn layout_row(row: &[Node], ctx: &Ctx, ox: f64, oy: f64) -> Laid {
    let anchor = ctx.begin_row(ox, oy);
    let mut offsets: Vec<f64> = Vec::with_capacity(row.len() + 1);
    if row.is_empty() {
        let laid = placeholder(ctx, ox, oy);
        // An empty row has no items, so the contract `offsets.len() == row.len() + 1` leaves a
        // single insertion point: the row's left edge. Reporting a second one would name caret
        // index `1`, which is outside the `0..=row.len()` the caret accepts.
        offsets.push(0.0);
        ctx.finish_row(anchor, offsets);
        return laid;
    }
    let mut atom_index = Vec::new();
    let mut atom_class = Vec::new();
    for (index, node) in row.iter().enumerate() {
        if let Some(class) = node_class(node) {
            atom_index.push(index);
            atom_class.push(class);
        }
    }
    demote_binary_operators(&mut atom_class);

    let size = ctx.size();
    let mut items: Vec<Box> = Vec::with_capacity(row.len());
    let mut pen = ox;
    let mut height = 0.0_f64;
    let mut depth = 0.0_f64;
    let mut next_atom = 0_usize;
    for (index, child) in row.iter().enumerate() {
        if atom_index.get(next_atom) == Some(&index) {
            if next_atom > 0 {
                let kern =
                    class_space(atom_class[next_atom - 1], atom_class[next_atom], &ctx.style.spacing)
                        * size;
                if kern != 0.0 {
                    items.push(Box::Kern { width: kern });
                    pen += kern;
                }
            }
            next_atom += 1;
        }
        offsets.push(pen - ox);
        let laid = layout_node(child, &ctx.slot(index), pen, oy);
        pen += laid.w;
        height = height.max(laid.h);
        depth = depth.max(laid.d);
        items.push(laid.node);
    }
    let width = pen - ox;
    offsets.push(width);
    ctx.finish_row(anchor, offsets);
    Laid::new(Box::HBox { items, width, height, depth }, width, height, depth)
}

/// Atom class of a node, driving the spacing between neighbours.
///
/// `None` means the node is not an atom at all and takes part in no spacing decision: explicit
/// space, alignment points and empty slots. Templates that TeX treats as a single inner atom
/// (fractions, tables, fenced groups) report [`Class::Inner`], and large operators report
/// [`Class::Op`], so their spacing follows the same table as characters.
fn node_class(node: &Node) -> Option<Class> {
    match node {
        Node::Char(character) => Some(character.class),
        Node::Text(_) | Node::Error(_) => Some(Class::Ord),
        Node::Space(_) | Node::Align => None,
        Node::Frac(_) | Node::Matrix(_) | Node::Fence(_) => Some(Class::Inner),
        Node::Sqrt(_) => Some(Class::Ord),
        Node::Scripts(scripts) => Some(node_class(&scripts.base).unwrap_or(Class::Ord)),
        Node::Limits(limits) => Some(node_class(&limits.base).unwrap_or(Class::Ord)),
        Node::Accent(accent) => Some(node_class(&accent.base).unwrap_or(Class::Ord)),
        Node::Big(_) => Some(Class::Op),
        Node::Styled(styled) => {
            Some(styled.class.or_else(|| node_class(&styled.body)).unwrap_or(Class::Ord))
        }
        Node::Row(row) => (!row.is_empty()).then_some(Class::Ord),
    }
}

/// Rewrite binary operators that have nothing to combine into ordinary atoms.
///
/// A `Bin` at the start of a row, after another `Bin`, an `Op`, a `Rel`, an `Open` or a `Punct`,
/// or in front of a `Rel`, a `Close` or a `Punct`, is unary and gets ordinary-atom spacing.
fn demote_binary_operators(classes: &mut [Class]) {
    for index in 0..classes.len() {
        if classes[index] != Class::Bin {
            continue;
        }
        let left_is_bad = index == 0
            || matches!(
                classes[index - 1],
                Class::Bin | Class::Op | Class::Rel | Class::Open | Class::Punct
            );
        let right_is_bad = index + 1 == classes.len()
            || matches!(classes[index + 1], Class::Rel | Class::Close | Class::Punct);
        if left_is_bad || right_is_bad {
            classes[index] = Class::Ord;
        }
    }
}

/// Spacing between two neighbouring atoms, in em units.
///
/// This is the TeXbook's table (chapter 18, rule 20) with the impossible entries — those a `Bin`
/// would need to reach without being demoted — filled in with the ordinary-atom value, since
/// [`demote_binary_operators`] has already rewritten them away. The table stores which kern of
/// the spacing table each pair takes, so a style preset retunes all of them at once.
fn class_space(left: Class, right: Class, spacing: &Spacing) -> f64 {
    match CLASS_SPACING[class_index(left)][class_index(right)] {
        1 => spacing.thin_kern,
        2 => spacing.medium_kern,
        3 => spacing.thick_kern,
        _ => 0.0,
    }
}

fn class_index(class: Class) -> usize {
    match class {
        Class::Ord => 0,
        Class::Op => 1,
        Class::Bin => 2,
        Class::Rel => 3,
        Class::Open => 4,
        Class::Close => 5,
        Class::Punct => 6,
        Class::Inner => 7,
    }
}

/// Rows are the left atom, columns the right one, both in [`class_index`] order; `0` is no kern,
/// `1` thin, `2` medium, `3` thick.
#[rustfmt::skip]
const CLASS_SPACING: [[u8; 8]; 8] = [
    //          Ord    Op     Bin    Rel    Open   Close  Punct  Inner
    /* Ord   */ [0,     1,     2,     3,     0,     0,     1,     0],
    /* Op    */ [1,     1,     2,     3,     1,     0,     1,     1],
    /* Bin    */ [2,     2,     2,     3,     2,     0,     1,     2],
    /* Rel    */ [3,     3,     2,     3,     3,     0,     3,     3],
    /* Open   */ [0,     0,     2,     0,     0,     0,     0,     0],
    /* Close  */ [0,     1,     2,     3,     0,     0,     1,     1],
    /* Punct  */ [1,     1,     2,     3,     1,     0,     1,     1],
    /* Inner  */ [1,     1,     2,     3,     1,     0,     1,     1],
];

// -------------------------------------------------------------------------------------------
// Leaves
// -------------------------------------------------------------------------------------------

/// Lay one glyph of the tree: `ch` in the already-resolved face `font`.
///
/// The codepoint moves to the mathematical alphanumeric variant of the face, so the glyph the
/// metrics measure is the glyph the box draws.
fn glyph_leaf(ch: char, font: Font, ctx: &Ctx, ox: f64, oy: f64) -> Laid {
    let size = ctx.size();
    let ch = math_variant(ch, font);
    let advance = ctx.metrics.advance(ch, font) * size;
    let height = ctx.metrics.height(ch, font) * size;
    let depth = ctx.metrics.depth(ch, font) * size;
    let glyph = Box::Glyph { ch, font, x: ox, y: oy, size, yscale: 1.0 };
    leaf(glyph, advance, height, depth)
}

fn layout_char(character: Char, ctx: &Ctx, ox: f64, oy: f64) -> Laid {
    let font = ctx
        .font_override
        .unwrap_or_else(|| ctx.style.fonts.resolve(character.ch, character.class, character.font));
    glyph_leaf(character.ch, font, ctx, ox, oy)
}

fn layout_text(run: &TextRun, ctx: &Ctx, ox: f64, oy: f64) -> Laid {
    let font = ctx.font_override.unwrap_or_else(|| ctx.style.fonts.text_font(run.font));
    text_leaf(run.text.chars(), font, ctx, ox, oy)
}

/// Unrepresentable construct: its message is set as upright text so a preview still shows it.
fn layout_message(message: &str, ctx: &Ctx, ox: f64, oy: f64) -> Laid {
    let font = ctx.font_override.unwrap_or(Font::Upright);
    text_leaf(message.chars(), font, ctx, ox, oy)
}

/// Lay a run of characters that all share one face, tracking the pen and the extremes.
fn text_leaf(chars: impl Iterator<Item = char>, font: Font, ctx: &Ctx, ox: f64, oy: f64) -> Laid {
    let mut items = Vec::new();
    let mut pen = ox;
    let mut height = 0.0_f64;
    let mut depth = 0.0_f64;
    for ch in chars {
        let laid = glyph_leaf(ch, font, ctx, pen, oy);
        pen += laid.w;
        height = height.max(laid.h);
        depth = depth.max(laid.d);
        items.push(laid.node);
    }
    let width = pen - ox;
    Laid::new(Box::HBox { items, width, height, depth }, width, height, depth)
}

/// The dashed box drawn where an argument slot was left empty.
fn placeholder(ctx: &Ctx, ox: f64, oy: f64) -> Laid {
    let width = ctx.em(EMPTY_SLOT_WIDTH);
    let height = ctx.em(EMPTY_SLOT_HEIGHT);
    let depth = ctx.em(EMPTY_SLOT_DEPTH);
    let rule = Box::Rule { x: ox, y: oy, width, height, depth, dashed: true };
    leaf(rule, width, height, depth)
}

fn zero_box() -> Laid {
    Laid::new(Box::HBox { items: Vec::new(), width: 0.0, height: 0.0, depth: 0.0 }, 0.0, 0.0, 0.0)
}

/// Wrap a leaf in the box that carries its extents.
fn leaf(inner: Box, w: f64, h: f64, d: f64) -> Laid {
    Laid::new(Box::HBox { items: vec![inner], width: w, height: h, depth: d }, w, h, d)
}

/// Move every leaf of `node` by `(dx, dy)`.
fn translate(node: &mut Box, dx: f64, dy: f64) {
    match node {
        Box::HBox { items, .. } | Box::VBox { items, .. } => {
            for item in items {
                translate(item, dx, dy);
            }
        }
        Box::Glyph { x, y, .. } | Box::Rule { x, y, .. } => {
            *x += dx;
            *y += dy;
        }
        Box::Kern { .. } => {}
    }
}

/// Move a subtree that was laid out at the origin to `(x, y)`, anchors included.
fn place(node: &mut Box, ctx: &Ctx, x: f64, y: f64) {
    translate(node, x, y);
    ctx.shift_rows(x, y);
}

// -------------------------------------------------------------------------------------------
// Fitting glyphs that have to grow
// -------------------------------------------------------------------------------------------

/// A glyph stretched to a required vertical extent.
struct Fitted {
    /// Advance width at the chosen size.
    advance: f64,
    /// Glyph extent above its own baseline, after stretching.
    h: f64,
    /// Glyph extent below its own baseline, after stretching.
    d: f64,
    /// Font size in points, i.e. the table factor times the current size.
    size: f64,
    /// Vertical stretch applied on top of that size.
    yscale: f64,
}

/// Fit `ch` to a vertical extent of `need` points.
///
/// The first entry of [`TALL_GLYPH_SCALES`] that is tall enough is used; when none is, the largest
/// entry is stretched vertically, so that an already enlarged glyph does not get any fatter.
fn fit_glyph(ctx: &Ctx, ch: char, font: Font, need: f64) -> Fitted {
    let size = ctx.size();
    let advance = ctx.metrics.advance(ch, font) * size;
    let h0 = ctx.metrics.height(ch, font) * size;
    let d0 = ctx.metrics.depth(ch, font) * size;
    let extent = h0 + d0;
    let mut scale = *TALL_GLYPH_SCALES.last().unwrap_or(&1.0);
    for &factor in TALL_GLYPH_SCALES.iter() {
        if extent * factor >= need {
            scale = factor;
            break;
        }
    }
    let yscale =
        if extent <= 0.0 || extent * scale >= need { 1.0 } else { need / (extent * scale) };
    let h = h0 * scale * yscale;
    let d = d0 * scale * yscale;
    Fitted { advance: advance * scale, h, d, size: size * scale, yscale }
}

/// Baseline offset that puts the fitted glyph's top `top` points above the current baseline.
fn align_top(fitted: &Fitted, top: f64) -> f64 {
    fitted.h - top
}

/// Baseline offset that puts the fitted glyph's vertical centre `centre` points below the current
/// baseline, which is how delimiters follow the material they enclose.
fn align_centre(fitted: &Fitted, centre: f64) -> f64 {
    centre + (fitted.h - fitted.d) / 2.0
}

/// Smallest factor of `table` that makes `natural * factor` cover `target`, saturating at the last
/// entry, and 1.0 for degenerate input.
fn fit_scale(table: &[f64], natural: f64, target: f64) -> f64 {
    if natural <= 0.0 || target <= natural {
        return table.first().copied().unwrap_or(1.0);
    }
    for &factor in table {
        if natural * factor >= target {
            return factor;
        }
    }
    table.last().copied().unwrap_or(1.0)
}

/// Extents of a fitted glyph whose baseline sits `y` points below the current baseline.
fn fitted_extents(fitted: &Fitted, y: f64) -> (f64, f64) {
    (fitted.h - y, fitted.d + y)
}

/// Build the single-item box holding a fitted glyph whose baseline sits at `y`.
fn fitted_leaf(ch: char, font: Font, fitted: &Fitted, x: f64, y: f64) -> Box {
    let glyph = Box::Glyph { ch, font, x, y, size: fitted.size, yscale: fitted.yscale };
    leaf(glyph, fitted.advance, fitted.h, fitted.d).node
}

// -------------------------------------------------------------------------------------------
// Fractions
// -------------------------------------------------------------------------------------------

fn layout_frac(frac: &Frac, ctx: &Ctx, ox: f64, oy: f64) -> Laid {
    match frac.kind {
        FracKind::Bar | FracKind::NoBar => layout_stacked_frac(frac, ctx, ox, oy),
        FracKind::Slash => layout_slash_frac(frac, ctx, ox, oy, true),
        FracKind::Linear => layout_slash_frac(frac, ctx, ox, oy, false),
    }
}

/// A stacked fraction, with or without a rule.
///
/// The numerator and the denominator are laid out one level deeper and centred on each other. The
/// rule is put on the math axis and the whole stack is shifted with it, so the fraction's centre
/// ends up near the axis.
fn layout_stacked_frac(frac: &Frac, ctx: &Ctx, ox: f64, oy: f64) -> Laid {
    let level = ctx.level + 1;
    let numerator_ctx = ctx.slot(0).at_level(level);
    let denominator_ctx = ctx.slot(1).at_level(level).cramped();
    let numerator = layout_node(&frac.num, &numerator_ctx, 0.0, 0.0);
    let denominator = layout_node(&frac.den, &denominator_ctx, 0.0, 0.0);

    let spacing = &ctx.style.spacing;
    let thickness = match frac.kind {
        FracKind::Bar => ctx.em(spacing.frac_rule_thickness),
        _ => 0.0,
    };
    let gap = if thickness > 0.0 { ctx.em(spacing.frac_rule_gap) } else { ctx.em(spacing.frac_nobar_gap) };
    let axis = ctx.metrics.axis_height() * ctx.size();

    let width = numerator.w.max(denominator.w);
    let rule_y = oy - axis;
    let numerator_baseline = rule_y - thickness / 2.0 - gap - numerator.d;
    let denominator_baseline = rule_y + thickness / 2.0 + gap + denominator.h;

    let mut items = Vec::new();
    let mut numerator_node = numerator.node;
    let numerator_x = ox + (width - numerator.w) / 2.0;
    place(&mut numerator_node, &numerator_ctx, numerator_x, numerator_baseline);
    items.push(numerator_node);
    if thickness > 0.0 {
        let rule = Box::Rule {
            x: ox,
            y: rule_y,
            width,
            height: thickness / 2.0,
            depth: thickness / 2.0,
            dashed: false,
        };
        items.push(leaf(rule, width, thickness / 2.0, thickness / 2.0).node);
    }
    let mut denominator_node = denominator.node;
    let denominator_x = ox + (width - denominator.w) / 2.0;
    place(&mut denominator_node, &denominator_ctx, denominator_x, denominator_baseline);
    items.push(denominator_node);

    let height = oy - (numerator_baseline - numerator.h);
    let depth = denominator_baseline + denominator.d - oy;
    Laid::new(Box::VBox { items, width, height, depth }, width, height, depth)
}

/// A fraction written with a slash: `raised / lowered` when `stacked`, else `a/b` on one line.
fn layout_slash_frac(frac: &Frac, ctx: &Ctx, ox: f64, oy: f64, stacked: bool) -> Laid {
    let level = if stacked { ctx.level + 1 } else { ctx.level };
    let numerator_ctx = ctx.slot(0).at_level(level);
    let denominator_ctx = ctx.slot(1).at_level(level);
    let numerator = layout_node(&frac.num, &numerator_ctx, 0.0, 0.0);
    let denominator = layout_node(&frac.den, &denominator_ctx, 0.0, 0.0);
    let slash = layout_char(Char { ch: '/', class: Class::Ord, font: Font::Upright }, ctx, 0.0, 0.0);
    let spacing = &ctx.style.spacing;
    let kern = ctx.em(spacing.frac_slash_kern);
    let shift = if stacked { ctx.em(spacing.frac_slash_shift) } else { 0.0 };

    let slash_x = ox + numerator.w + kern;
    let denominator_x = slash_x + slash.w + kern;
    let mut slash_node = slash.node;
    // The slash is a bare glyph with nothing under it: `place` would run the fraction's own
    // `shift_rows`, moving the numerator and denominator anchors a second time.
    translate(&mut slash_node, slash_x, oy);
    let mut numerator_node = numerator.node;
    place(&mut numerator_node, &numerator_ctx, ox, oy - shift);
    let mut denominator_node = denominator.node;
    place(&mut denominator_node, &denominator_ctx, denominator_x, oy + shift);

    let width = numerator.w + kern + slash.w + kern + denominator.w;
    let height = (shift + numerator.h).max(slash.h).max(denominator.h - shift);
    let depth = (shift + denominator.d).max(slash.d).max(numerator.d - shift);
    let items = vec![numerator_node, slash_node, denominator_node];
    Laid::new(Box::HBox { items, width, height, depth }, width, height, depth)
}

// -------------------------------------------------------------------------------------------
// Radicals
// -------------------------------------------------------------------------------------------

/// A radical: a stretched sign, an overbar and an optional raised index.
fn layout_sqrt(sqrt: &Sqrt, ctx: &Ctx, ox: f64, oy: f64) -> Laid {
    let radicand_slot = usize::from(sqrt.index.is_some());
    let radicand_ctx = ctx.slot(radicand_slot).cramped();
    let radicand = layout_node(&sqrt.radicand, &radicand_ctx, 0.0, 0.0);

    let font = ctx.font_override.unwrap_or(Font::Upright);
    let spacing = &ctx.style.spacing;
    let gap = ctx.em(spacing.radical_rule_gap);
    let thickness = ctx.em(spacing.radical_rule_thickness);
    let kern = ctx.em(spacing.radical_kern);
    let top = radicand.h + gap + thickness;
    let sign = fit_glyph(ctx, '\u{221A}', font, top + radicand.d);
    let sign_y = align_top(&sign, top);

    let index_ctx = ctx.slot(0).at_level(ctx.level + 1);
    let index = sqrt.index.as_ref().map(|node| layout_node(node, &index_ctx, 0.0, 0.0));
    let index_width = index.as_ref().map_or(0.0, |laid| laid.w);
    // How far the index reaches above the baseline, and where its own baseline goes.
    let index_span = index
        .as_ref()
        .map(|laid| top + ctx.em(spacing.radical_index_gap) + laid.d + laid.h);

    let sign_x = ox + index_width;
    let radicand_x = sign_x + sign.advance + kern;
    let mut items = Vec::new();
    if let Some(index) = index {
        let baseline = oy - top - ctx.em(spacing.radical_index_gap) - index.d;
        let mut node = index.node;
        place(&mut node, &index_ctx, ox, baseline);
        items.push(node);
    }
    items.push(fitted_leaf('\u{221A}', font, &sign, sign_x, oy + sign_y));
    let overbar = Box::Rule {
        x: sign_x,
        y: oy - (top - thickness / 2.0),
        width: radicand.w + kern,
        height: thickness / 2.0,
        depth: thickness / 2.0,
        dashed: false,
    };
    items.push(leaf(overbar, radicand.w + kern, thickness / 2.0, thickness / 2.0).node);
    let mut radicand_node = radicand.node;
    place(&mut radicand_node, &radicand_ctx, radicand_x, oy);
    items.push(radicand_node);

    let width = radicand_x + radicand.w - ox;
    let (sign_h, sign_d) = fitted_extents(&sign, sign_y);
    let height = index_span.map_or(sign_h, |span| sign_h.max(span));
    let depth = radicand.d.max(sign_d);
    Laid::new(Box::HBox { items, width, height, depth }, width, height, depth)
}

// -------------------------------------------------------------------------------------------
// Scripts, limits and large operators
// -------------------------------------------------------------------------------------------

fn layout_scripts(scripts: &Scripts, ctx: &Ctx, ox: f64, oy: f64) -> Laid {
    let base = layout_node(&scripts.base, &ctx.slot(0), 0.0, 0.0);
    let level = ctx.level + 1;
    let sub = scripts
        .sub
        .as_ref()
        .map(|node| layout_node(node, &ctx.slot(1).at_level(level).cramped(), 0.0, 0.0));
    let sup = scripts
        .sup
        .as_ref()
        .map(|node| layout_node(node, &ctx.slot(2).at_level(level).cramped(), 0.0, 0.0));
    attach_scripts(base, sub, sup, ctx, ox, oy)
}

fn layout_limits(limits: &Limits, ctx: &Ctx, ox: f64, oy: f64) -> Laid {
    let base = layout_node(&limits.base, &ctx.slot(0), 0.0, 0.0);
    let level = ctx.level + 1;
    let under = limits
        .under
        .as_ref()
        .map(|node| layout_node(node, &ctx.slot(1).at_level(level), 0.0, 0.0));
    let over = limits
        .over
        .as_ref()
        .map(|node| layout_node(node, &ctx.slot(2).at_level(level), 0.0, 0.0));
    // Unlike a large operator, material that merely carries limits keeps its own size and stays on
    // the baseline.
    stack_limits(base, over, under, limits.align, 0.0, ctx, ox, oy)
}

fn layout_big(big: &BigOp, ctx: &Ctx, ox: f64, oy: f64) -> Laid {
    let base_ctx = ctx.slot(0).scaled(operator_scale(&big.base, big.limits));
    let base = layout_node(&big.base, &base_ctx, 0.0, 0.0);
    let script_ctx = |slot: usize| {
        let ctx = ctx.slot(slot).at_level(ctx.level + 1);
        if big.limits { ctx } else { ctx.cramped() }
    };
    let sub = big.sub.as_ref().map(|node| layout_node(node, &script_ctx(1), 0.0, 0.0));
    let sup = big.sup.as_ref().map(|node| layout_node(node, &script_ctx(2), 0.0, 0.0));
    if big.limits {
        // Stacked limits are centred on the operator, and the operator's own centre is put on the
        // math axis, so the axis passes through it.
        let axis = ctx.metrics.axis_height() * ctx.size();
        let shift = (base.h - base.d) / 2.0 - axis;
        stack_limits(base, sup, sub, Side::Center, shift, ctx, ox, oy)
    } else {
        attach_scripts(base, sub, sup, ctx, ox, oy)
    }
}

/// Whether a large operator is an integral, which keeps a much slimmer display shape.
fn is_integral(node: &Node) -> bool {
    match node {
        Node::Char(character) => {
            matches!(character.ch, '\u{222B}' | '\u{222E}' | '\u{222F}' | '\u{2230}')
        }
        Node::Row(row) => row.iter().any(is_integral),
        Node::Styled(styled) => is_integral(&styled.body),
        Node::Scripts(scripts) => is_integral(&scripts.base),
        _ => false,
    }
}

/// Size factor the glyph of a large operator is drawn at.
///
/// A display-style operator is set larger than the text around it — noticeably so when its limits
/// are stacked above and below — while an integral grows far less, so that it keeps its slim
/// shape. The factor applies to the operator itself only; its scripts stay at script size.
fn operator_scale(node: &Node, limits: bool) -> f64 {
    if is_integral(node) {
        if limits {
            BIG_OP_SCALE_INTEGRAL
        } else {
            1.0
        }
    } else if limits {
        BIG_OP_SCALE_LIMITS
    } else {
        BIG_OP_SCALE_INLINE
    }
}

/// Attach a subscript and a superscript beside their base.
///
/// The superscript is raised by `(height - depth) / 2` plus [`SCRIPT_OFFSET`]; the subscript is
/// lowered below the base depth by [`SUBSCRIPT_OFFSET`]. When both exist the superscript is raised
/// further if it would otherwise collide with the subscript. Scripts are laid out one level deeper
/// and start [`SCRIPT_KERN`] after the base.
fn attach_scripts(
    base: Laid,
    sub: Option<Laid>,
    sup: Option<Laid>,
    ctx: &Ctx,
    ox: f64,
    oy: f64,
) -> Laid {
    let spacing = &ctx.style.spacing;
    let kern = ctx.em(spacing.script_kern);
    let offset = if ctx.cramped { 0.0 } else { ctx.em(spacing.script_offset) };
    let mut raise = (base.h - base.d) / 2.0 + offset;
    let drop = base.d + ctx.em(spacing.subscript_offset);
    if let (Some(sub), Some(sup)) = (&sub, &sup) {
        let needed = sup.d + sub.h + ctx.em(spacing.script_min_gap) - drop;
        if raise < needed {
            raise = needed;
        }
    }

    let script_x = ox + base.w + kern;
    let widest = sub.as_ref().map_or(0.0, |laid| laid.w).max(sup.as_ref().map_or(0.0, |laid| laid.w));
    let width = base.w + kern + widest;
    let mut height = base.h;
    let mut depth = base.d;
    let mut items = Vec::new();
    let mut base_node = base.node;
    place(&mut base_node, &ctx.slot(0), ox, oy);
    items.push(base_node);
    if let Some(sub) = sub {
        let mut node = sub.node;
        place(&mut node, &ctx.slot(1), script_x, oy + drop);
        height = height.max(sub.h - drop);
        depth = depth.max(drop + sub.d);
        items.push(node);
    }
    if let Some(sup) = sup {
        let mut node = sup.node;
        place(&mut node, &ctx.slot(2), script_x, oy - raise);
        height = height.max(raise + sup.h);
        depth = depth.max(sup.d - raise);
        items.push(node);
    }
    Laid::new(Box::HBox { items, width, height, depth }, width, height, depth)
}

/// Stack limits above and below a base, aligned as `align` asks for.
///
/// The narrower of the base, the over-script and the under-script is aligned inside the widest of
/// them, so the base, the limits and their common centre line up. `base_shift` moves the base's
/// baseline away from the current one, which is how a large operator puts its own centre on the
/// math axis; the limits follow the base's edges, so all three keep that one centre.
fn stack_limits(
    base: Laid,
    over: Option<Laid>,
    under: Option<Laid>,
    align: Side,
    base_shift: f64,
    ctx: &Ctx,
    ox: f64,
    oy: f64,
) -> Laid {
    let spacing = &ctx.style.spacing;
    let gap = ctx.em(spacing.limit_gap);
    let width = base
        .w
        .max(over.as_ref().map_or(0.0, |laid| laid.w))
        .max(under.as_ref().map_or(0.0, |laid| laid.w));
    let aligned = |part: f64| match align {
        Side::Center => (width - part) / 2.0,
        Side::Left => 0.0,
        Side::Right => width - part,
    };
    let base_top = base_shift - base.h;
    let base_bottom = base_shift + base.d;

    let mut items = Vec::new();
    let mut height = -base_top;
    let mut depth = base_bottom;
    if let Some(over) = over {
        let mut node = over.node;
        place(&mut node, &ctx.slot(2), ox + aligned(over.w), oy + base_top - gap - over.d);
        height = height.max(-(base_top - gap - over.d) + over.h);
        items.push(node);
    }
    let mut base_node = base.node;
    place(&mut base_node, &ctx.slot(0), ox + aligned(base.w), oy + base_shift);
    items.push(base_node);
    if let Some(under) = under {
        let mut node = under.node;
        place(&mut node, &ctx.slot(1), ox + aligned(under.w), oy + base_bottom + gap + under.h);
        depth = depth.max(base_bottom + gap + under.h + under.d);
        items.push(node);
    }
    Laid::new(Box::VBox { items, width, height, depth }, width, height, depth)
}

// -------------------------------------------------------------------------------------------
// Fences
// -------------------------------------------------------------------------------------------

/// Delimiters stretched to the body and kerned back towards it.
fn layout_fence(fence: &Fence, ctx: &Ctx, ox: f64, oy: f64) -> Laid {
    let body = layout_node(&fence.body, &ctx.slot(0), 0.0, 0.0);
    let font = ctx.font_override.unwrap_or(Font::Upright);
    let spacing = &ctx.style.spacing;
    let kern = ctx.em(spacing.fence_kern);
    let need = body.h + body.d;
    let centre = (body.d - body.h) / 2.0;
    let open = fence.open.open().map(|ch| (ch, fit_glyph(ctx, ch, font, need)));
    let close = fence.close.close().map(|ch| (ch, fit_glyph(ctx, ch, font, need)));
    // A delimiter contributes its advance less the kern that pulls the body towards it; a missing
    // one contributes nothing at all.
    let open_part = open.as_ref().map_or(0.0, |(_, fitted)| fitted.advance - kern);
    let close_part = close.as_ref().map_or(0.0, |(_, fitted)| fitted.advance - kern);
    let close_advance = close.as_ref().map_or(0.0, |(_, fitted)| fitted.advance);

    let body_x = ox + open_part;
    let close_x = body_x + body.w + close_part - close_advance;
    let width = open_part + body.w + close_part;

    let mut items = Vec::new();
    let mut height = body.h;
    let mut depth = body.d;
    if let Some((ch, fitted)) = &open {
        let y = align_centre(fitted, centre);
        items.push(fitted_leaf(*ch, font, fitted, ox, oy + y));
        let (h, d) = fitted_extents(fitted, y);
        height = height.max(h);
        depth = depth.max(d);
    }
    let mut body_node = body.node;
    place(&mut body_node, &ctx.slot(0), body_x, oy);
    items.push(body_node);
    if let Some((ch, fitted)) = &close {
        let y = align_centre(fitted, centre);
        items.push(fitted_leaf(*ch, font, fitted, close_x, oy + y));
        let (h, d) = fitted_extents(fitted, y);
        height = height.max(h);
        depth = depth.max(d);
    }
    Laid::new(Box::HBox { items, width, height, depth }, width, height, depth)
}

// -------------------------------------------------------------------------------------------
// Matrices
// -------------------------------------------------------------------------------------------

/// A table: per-column widths, per-row baselines, alignment from the matrix.
///
/// Columns are separated by at least [`MATRIX_COL_SEPARATION`] em, rows by the font's math leading,
/// and the whole table is centred on the math axis before its delimiters are fitted around it.
fn layout_matrix(matrix: &Matrix, ctx: &Ctx, ox: f64, oy: f64) -> Laid {
    let size = ctx.size();
    let rows = matrix.row_count();
    let cols = matrix.col_count();
    if rows == 0 || cols == 0 {
        return zero_box();
    }

    let mut cells: Vec<Vec<Laid>> = Vec::with_capacity(rows);
    let mut col_w = vec![0.0_f64; cols];
    let mut row_h = vec![0.0_f64; rows];
    let mut row_d = vec![0.0_f64; rows];
    let mut flat = 0_usize;
    for (r, cells_row) in matrix.rows.iter().enumerate() {
        let mut laid_row = Vec::with_capacity(cols);
        for c in 0..cols {
            match cells_row.get(c) {
                Some(cell) => {
                    // Cells are addressed row-major across the whole table, the way
                    // `Node::slot` numbers them.
                    let laid = layout_node(cell, &ctx.slot(flat + c), 0.0, 0.0);
                    col_w[c] = col_w[c].max(laid.w);
                    row_h[r] = row_h[r].max(laid.h);
                    row_d[r] = row_d[r].max(laid.d);
                    laid_row.push(laid);
                }
                None => laid_row.push(zero_box()),
            }
        }
        flat += cells_row.len();
        cells.push(laid_row);
    }

    let spacing = &ctx.style.spacing;
    let separation = ctx.em(spacing.matrix_col_separation);
    let mut col_x = vec![0.0_f64; cols];
    let mut x = 0.0;
    for c in 0..cols {
        col_x[c] = x;
        x += col_w[c] + separation;
    }
    let table_width = x - separation;

    let leading = ctx.metrics.math_leading() * size;
    let mut row_y = vec![0.0_f64; rows];
    let mut y = 0.0;
    for r in 0..rows {
        row_y[r] = y;
        y += row_h[r] + leading + row_d[r];
    }
    let stack_h = row_h[0];
    // Depth below the baseline of the last row, not below the leading that follows it.
    let stack_d = row_y[rows - 1] + row_d[rows - 1];
    let axis = ctx.metrics.axis_height() * size;
    let shift = -axis - (stack_d - stack_h) / 2.0;
    let table_h = stack_h - shift;
    let table_d = stack_d + shift;

    let font = ctx.font_override.unwrap_or(Font::Upright);
    let kern = ctx.em(spacing.fence_kern);
    let (open_delim, close_delim) = matrix.delims.unwrap_or_else(|| default_delims(matrix.kind));
    let need = table_h + table_d;
    let centre = (table_d - table_h) / 2.0;
    let open = open_delim.open().map(|ch| (ch, fit_glyph(ctx, ch, font, need)));
    let close = close_delim.close().map(|ch| (ch, fit_glyph(ctx, ch, font, need)));
    let open_part = open.as_ref().map_or(0.0, |(_, fitted)| fitted.advance - kern);
    let close_part = close.as_ref().map_or(0.0, |(_, fitted)| fitted.advance - kern);
    let close_advance = close.as_ref().map_or(0.0, |(_, fitted)| fitted.advance);
    let body_x = ox + open_part;
    let close_x = body_x + table_width + close_part - close_advance;
    let width = open_part + table_width + close_part;

    let mut flat = 0_usize;
    for r in 0..rows {
        let filled = matrix.rows[r].len();
        for c in 0..cols {
            let offset = match column_side(matrix, c) {
                Side::Center => (col_w[c] - cells[r][c].w) / 2.0,
                Side::Left => 0.0,
                Side::Right => col_w[c] - cells[r][c].w,
            };
            let x = body_x + col_x[c] + offset;
            let y = oy + shift + row_y[r];
            if c < filled {
                place(&mut cells[r][c].node, &ctx.slot(flat + c), x, y);
            } else {
                translate(&mut cells[r][c].node, x, y);
            }
        }
        flat += filled;
    }

    let mut row_boxes = Vec::with_capacity(rows);
    for r in 0..rows {
        let row = std::mem::take(&mut cells[r]);
        let items = row.into_iter().map(|cell| cell.node).collect();
        row_boxes.push(Box::HBox { items, width: table_width, height: row_h[r], depth: row_d[r] });
    }

    let mut items: Vec<Box> = Vec::new();
    let mut height = table_h;
    let mut depth = table_d;
    if let Some((ch, fitted)) = &open {
        let y = align_centre(fitted, centre);
        items.push(fitted_leaf(*ch, font, fitted, ox, oy + y));
        let (h, d) = fitted_extents(fitted, y);
        height = height.max(h);
        depth = depth.max(d);
    }
    items.push(Box::VBox { items: row_boxes, width: table_width, height: table_h, depth: table_d });
    if matrix.rules {
        let thickness = ctx.em(spacing.matrix_rule_thickness);
        for r in 1..rows {
            let boundary = (row_y[r - 1] + row_d[r - 1] + row_y[r] - row_h[r]) / 2.0;
            let rule = Box::Rule {
                x: body_x,
                y: oy + shift + boundary,
                width: table_width,
                height: thickness / 2.0,
                depth: thickness / 2.0,
                dashed: false,
            };
            items.push(leaf(rule, table_width, thickness / 2.0, thickness / 2.0).node);
        }
        for c in 1..cols {
            let boundary = (col_x[c - 1] + col_w[c - 1] + col_x[c]) / 2.0;
            let rule = Box::Rule {
                x: body_x + boundary,
                y: oy,
                width: thickness,
                height: table_h,
                depth: table_d,
                dashed: false,
            };
            items.push(leaf(rule, thickness, table_h, table_d).node);
        }
    }
    if let Some((ch, fitted)) = &close {
        let y = align_centre(fitted, centre);
        items.push(fitted_leaf(*ch, font, fitted, close_x, oy + y));
        let (h, d) = fitted_extents(fitted, y);
        height = height.max(h);
        depth = depth.max(d);
    }
    Laid::new(Box::HBox { items, width, height, depth }, width, height, depth)
}

/// Alignment of column `col`: an explicit per-column rule wins, else the kind decides.
fn column_side(matrix: &Matrix, col: usize) -> Side {
    if let Some(side) = matrix.columns.get(col) {
        return *side;
    }
    match matrix.kind {
        MatrixKind::Cases | MatrixKind::Aligned => Side::Left,
        MatrixKind::LongDivision => Side::Right,
        _ => matrix.align,
    }
}

/// Delimiters a table kind draws when it does not carry an explicit pair.
fn default_delims(kind: MatrixKind) -> (Delim, Delim) {
    match kind {
        MatrixKind::Matrix => (Delim::Paren, Delim::Paren),
        MatrixKind::Cases => (Delim::Brace, Delim::None),
        MatrixKind::Aligned | MatrixKind::Array | MatrixKind::Pile | MatrixKind::LongDivision => {
            (Delim::None, Delim::None)
        }
    }
}

// -------------------------------------------------------------------------------------------
// Accents
// -------------------------------------------------------------------------------------------

/// An accent over or under its base.
///
/// Glyph accents are enlarged until they cover the base and centred on it; `Bar`, `Line` and
/// `Strike` are drawn as rules, and `Vector` uses an arrow glyph.
fn layout_accent(accent: &Accent, ctx: &Ctx, ox: f64, oy: f64) -> Laid {
    let base = layout_node(&accent.base, &ctx.slot(0), 0.0, 0.0);
    let size = ctx.size();
    let spacing = &ctx.style.spacing;
    let gap = ctx.em(spacing.accent_gap);
    let over = !accent.under;
    let font = ctx.font_override.unwrap_or(Font::Upright);

    let mut accent_node: Box;
    let (accent_w, accent_h, accent_d);
    if matches!(accent.kind, AccentKind::Bar | AccentKind::Line | AccentKind::Strike) {
        let thickness = ctx.em(spacing.accent_rule_thickness);
        let y = match accent.kind {
            AccentKind::Strike => (base.d - base.h) / 2.0,
            _ if over => -(base.h + gap + thickness / 2.0),
            _ => base.d + gap + thickness / 2.0,
        };
        let rule = Box::Rule {
            x: 0.0,
            y: oy + y,
            width: base.w,
            height: thickness / 2.0,
            depth: thickness / 2.0,
            dashed: false,
        };
        accent_node = leaf(rule, base.w, thickness / 2.0, thickness / 2.0).node;
        accent_w = base.w;
        accent_h = thickness / 2.0;
        accent_d = thickness / 2.0;
    } else {
        let ch = accent_glyph(accent.kind).unwrap_or('~');
        let natural = ctx.metrics.advance(ch, font) * size;
        let scale = fit_scale(&ACCENT_SCALES, natural, base.w);
        let w = natural * scale;
        let h = ctx.metrics.height(ch, font) * size * scale;
        let d = ctx.metrics.depth(ch, font) * size * scale;
        let y = if over { -(base.h + gap + d) } else { base.d + gap + h };
        let glyph = Box::Glyph { ch, font, x: 0.0, y: oy + y, size: size * scale, yscale: 1.0 };
        accent_node = leaf(glyph, w, h, d).node;
        accent_w = w;
        accent_h = h;
        accent_d = d;
    }
    translate(&mut accent_node, ox + (base.w - accent_w) / 2.0, 0.0);

    let strike = accent.kind == AccentKind::Strike;
    let height = if over && !strike { base.h + gap + accent_d + accent_h } else { base.h };
    let depth = if !over && !strike { base.d + gap + accent_h + accent_d } else { base.d };
    let width = base.w.max(accent_w);

    let mut items = Vec::new();
    let mut base_node = base.node;
    place(&mut base_node, &ctx.slot(0), ox, oy);
    if over {
        items.push(accent_node);
        items.push(base_node);
    } else {
        items.push(base_node);
        items.push(accent_node);
    }
    Laid::new(Box::VBox { items, width, height, depth }, width, height, depth)
}

/// The glyph an accent is drawn with.
///
/// The wide accents use the spacing shapes rather than the combining ones the model stores, since
/// layout stretches them itself.
fn accent_glyph(kind: AccentKind) -> Option<char> {
    match kind {
        AccentKind::WideHat => Some('\u{02C6}'),
        AccentKind::WideTilde => Some('~'),
        AccentKind::Vector => Some('\u{2192}'),
        other => other.glyph(),
    }
}

// -------------------------------------------------------------------------------------------
// Style overrides
// -------------------------------------------------------------------------------------------

fn layout_styled(styled: &Styled, ctx: &Ctx, ox: f64, oy: f64) -> Laid {
    let mut child = ctx.clone();
    if let Some(style) = styled.style {
        child.level = level_of(style);
    }
    if let Some(font) = styled.font {
        child.font_override = Some(font);
    }
    layout_node(&styled.body, &child.slot(0), ox, oy)
}
