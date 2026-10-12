//! Encapsulated PostScript backend.
//!
//! The document carries the glyph outlines as filled Bézier paths, so it renders without the font
//! being installed. Layout coordinates are converted to PostScript: `x` shifts to the left edge of
//! the image and `y` is flipped, so the origin is the bottom-left corner of the image box and the
//! integer `%%BoundingBox` is `0 0 ceil(width) ceil(height)` in points.

use mt_layout::constants::{PLACEHOLDER_DASH, PLACEHOLDER_STROKE};

use crate::Item;
use crate::outline::Path;

/// Emit a complete EPS document for the collected items.
///
/// `bounds` is `(x0, y0, x1, y1)` in layout points, as [`crate::image_bounds`] returns it.
pub(crate) fn render(items: &[Item], bounds: (f64, f64, f64, f64), fg: [u8; 4]) -> String {
    let (bx0, by0, bx1, by1) = bounds;
    let width = bx1 - bx0;
    let height = by1 - by0;
    let mut out = String::new();
    out.push_str("%!PS-Adobe-3.0 EPSF-3.0\n");
    out.push_str("%%Creator: mt-render\n");
    out.push_str(&format!(
        "%%BoundingBox: 0 0 {} {}\n",
        width.ceil().max(1.0) as i64,
        height.ceil().max(1.0) as i64
    ));
    out.push_str(&format!(
        "%%HiResBoundingBox: 0 0 {} {}\n",
        number(width),
        number(height)
    ));
    out.push_str("%%EndComments\n");
    out.push_str("%%BeginProlog\n%%EndProlog\n");
    out.push_str(&format!(
        "{} {} {} setrgbcolor\n",
        number(fg[0] as f64 / 255.0),
        number(fg[1] as f64 / 255.0),
        number(fg[2] as f64 / 255.0)
    ));
    out.push_str("%%Page: 1 1\n");

    let origin: Origin = (bx0, by1);
    for item in items {
        match item {
            Item::Glyph { path } => emit_glyph(&mut out, path, origin),
            Item::Rule {
                x,
                y,
                width: rw,
                height,
                depth,
                dashed,
            } => {
                emit_rule(&mut out, *x, *y, *rw, *height, *depth, *dashed, origin);
            }
        }
    }

    out.push_str("showpage\n");
    out.push_str("%%EOF\n");
    out
}

/// PostScript origin of the image: its left edge and its bottom edge, in layout points.
type Origin = (f64, f64);

/// Map a layout point to PostScript coordinates.
fn ps(p: [f64; 2], origin: Origin) -> [f64; 2] {
    [p[0] - origin.0, origin.1 - p[1]]
}

/// Emit one glyph as a set of filled contours.
fn emit_glyph(out: &mut String, path: &Path, origin: Origin) {
    if path.contours.is_empty() {
        return;
    }
    out.push_str("newpath\n");
    for contour in &path.contours {
        if contour.segs.is_empty() {
            continue;
        }
        let start = ps(contour.start, origin);
        out.push_str(&format!(
            "{} {} moveto\n",
            number(start[0]),
            number(start[1])
        ));
        let mut cur = contour.start;
        for seg in &contour.segs {
            match *seg {
                crate::outline::Seg::Line(end) => {
                    let p = ps(end, origin);
                    out.push_str(&format!("{} {} lineto\n", number(p[0]), number(p[1])));
                    cur = end;
                }
                crate::outline::Seg::Quad(ctrl, end) => {
                    // Promote the quadratic to a cubic so every glyph uses `curveto`.
                    let c1 = lerp(cur, ctrl, 2.0 / 3.0);
                    let c2 = lerp(end, ctrl, 2.0 / 3.0);
                    emit_curve(out, ps(c1, origin), ps(c2, origin), ps(end, origin));
                    cur = end;
                }
                crate::outline::Seg::Cubic(a, b, end) => {
                    emit_curve(out, ps(a, origin), ps(b, origin), ps(end, origin));
                    cur = end;
                }
            }
        }
        out.push_str("closepath\n");
    }
    out.push_str("fill\n");
}

/// Emit one `curveto`.
fn emit_curve(out: &mut String, c1: [f64; 2], c2: [f64; 2], end: [f64; 2]) {
    out.push_str(&format!(
        "{} {} {} {} {} {} curveto\n",
        number(c1[0]),
        number(c1[1]),
        number(c2[0]),
        number(c2[1]),
        number(end[0]),
        number(end[1])
    ));
}

/// Emit a rule: a filled rectangle, or a dashed outline for the empty-slot placeholder.
#[allow(clippy::too_many_arguments)]
fn emit_rule(
    out: &mut String,
    x: f64,
    y: f64,
    width: f64,
    height: f64,
    depth: f64,
    dashed: bool,
    origin: Origin,
) {
    let top = y - height;
    let bottom = y + depth;
    // PostScript rectangle lower-left corner plus size, after the `y` flip.
    let ll = ps([x, bottom], origin);
    let w = width;
    let h = bottom - top;
    if dashed {
        out.push_str("newpath\n");
        out.push_str(&format!("{} {} moveto\n", number(ll[0]), number(ll[1])));
        out.push_str(&format!("{} {} lineto\n", number(ll[0] + w), number(ll[1])));
        out.push_str(&format!(
            "{} {} lineto\n",
            number(ll[0] + w),
            number(ll[1] + h)
        ));
        out.push_str(&format!("{} {} lineto\n", number(ll[0]), number(ll[1] + h)));
        out.push_str("closepath\n");
        out.push_str(&format!("{} setlinewidth\n", number(PLACEHOLDER_STROKE)));
        out.push_str(&format!(
            "[{} {}] 0 setdash\n",
            number(PLACEHOLDER_DASH),
            number(PLACEHOLDER_DASH)
        ));
        out.push_str("stroke\n");
        out.push_str("[] 0 setdash\n");
    } else {
        out.push_str(&format!(
            "newpath {} {} {} {} rectfill\n",
            number(ll[0]),
            number(ll[1]),
            number(w),
            number(h)
        ));
    }
}

/// Linear interpolation `a + t * (b - a)`.
fn lerp(a: [f64; 2], b: [f64; 2], t: f64) -> [f64; 2] {
    [a[0] + t * (b[0] - a[0]), a[1] + t * (b[1] - a[1])]
}

/// Format a number with at most three decimals, never `-0`.
fn number(value: f64) -> String {
    let rounded = if value == 0.0 {
        0.0
    } else {
        (value * 1000.0).round() / 1000.0
    };
    format!("{rounded}")
}
