//! Glyph outline extraction.
//!
//! Every glyph is pulled straight from the OpenType outlines of the face the caller supplies, so
//! rendering never depends on an installed font. `ttf-parser` reports coordinates in font units
//! with `y` growing upwards; [`Builder`] converts each point to the layout coordinate system used
//! by [`mt_layout::Box`]: points, `y` growing downwards, baseline at the glyph's `y`, with the
//! box's `yscale` applied about that baseline.

use ttf_parser::{Face, OutlineBuilder};

/// One outline segment, with its control and end points in layout point coordinates.
///
/// A [`Seg::Line`] holds the end point, [`Seg::Quad`] the control point and the end point, and
/// [`Seg::Cubic`] the two control points and the end point. The start point of a segment is
/// implicit: it is the end point of the previous segment, or the contour's start point.
#[derive(Clone, Copy, Debug)]
pub(crate) enum Seg {
    Line([f64; 2]),
    Quad([f64; 2], [f64; 2]),
    Cubic([f64; 2], [f64; 2], [f64; 2]),
}

/// A closed contour: a start point followed by the segments that reach back to it.
#[derive(Clone, Debug)]
pub(crate) struct Contour {
    pub(crate) start: [f64; 2],
    pub(crate) segs: Vec<Seg>,
}

/// A glyph outline as one or more closed contours, plus the control-point bounding box of every
/// point seen. The bounding box is used to grow the image so that ink sticking out of the glyph's
/// advance still lands inside it.
#[derive(Clone, Debug, Default)]
pub(crate) struct Path {
    pub(crate) contours: Vec<Contour>,
    pub(crate) bbox: Option<(f64, f64, f64, f64)>,
}

impl Path {
    /// Extend the bounding box with `p`.
    fn track(&mut self, p: [f64; 2]) {
        match &mut self.bbox {
            None => self.bbox = Some((p[0], p[1], p[0], p[1])),
            Some(b) => {
                b.0 = b.0.min(p[0]);
                b.1 = b.1.min(p[1]);
                b.2 = b.2.max(p[0]);
                b.3 = b.3.max(p[1]);
            }
        }
    }

    /// Whether the outline carries any segment at all.
    pub(crate) fn is_empty(&self) -> bool {
        self.contours.iter().all(|c| c.segs.is_empty())
    }

    /// Approximate every contour with a closed polyline of line segments.
    ///
    /// Quadratic and cubic segments are sampled `subs` times each; the rasterizer fills the
    /// resulting polygons, so the sample count is the only source of curve error.
    pub(crate) fn flatten(&self, subs: usize) -> Vec<Vec<[f64; 2]>> {
        let mut out = Vec::with_capacity(self.contours.len());
        for contour in &self.contours {
            if contour.segs.is_empty() {
                continue;
            }
            let mut poly = Vec::with_capacity(contour.segs.len() * subs + 1);
            poly.push(contour.start);
            let mut cur = contour.start;
            for seg in &contour.segs {
                match *seg {
                    Seg::Line(end) => {
                        poly.push(end);
                        cur = end;
                    }
                    Seg::Quad(ctrl, end) => {
                        for i in 1..=subs {
                            let t = i as f64 / subs as f64;
                            poly.push(quad(cur, ctrl, end, t));
                        }
                        cur = end;
                    }
                    Seg::Cubic(a, b, end) => {
                        for i in 1..=subs {
                            let t = i as f64 / subs as f64;
                            poly.push(cubic(cur, a, b, end, t));
                        }
                        cur = end;
                    }
                }
            }
            out.push(poly);
        }
        out
    }
}

/// Point on a quadratic Bézier at `t`.
fn quad(a: [f64; 2], c: [f64; 2], b: [f64; 2], t: f64) -> [f64; 2] {
    let u = 1.0 - t;
    [u * u * a[0] + 2.0 * u * t * c[0] + t * t * b[0], u * u * a[1] + 2.0 * u * t * c[1] + t * t * b[1]]
}

/// Point on a cubic Bézier at `t`.
fn cubic(a: [f64; 2], c1: [f64; 2], c2: [f64; 2], b: [f64; 2], t: f64) -> [f64; 2] {
    let u = 1.0 - t;
    [
        u * u * u * a[0] + 3.0 * u * u * t * c1[0] + 3.0 * u * t * t * c2[0] + t * t * t * b[0],
        u * u * u * a[1] + 3.0 * u * u * t * c1[1] + 3.0 * u * t * t * c2[1] + t * t * t * b[1],
    ]
}

/// `ttf-parser` callback that records a glyph as a [`Path`] in layout coordinates.
struct Builder {
    path: Path,
    /// Points per font unit, i.e. `size / units_per_em`.
    unit: f64,
    /// Vertical stretch applied about the baseline.
    yscale: f64,
    /// Pen origin of the glyph.
    origin: [f64; 2],
}

impl Builder {
    /// Map a font-unit point to layout point coordinates.
    fn map(&self, x: f32, y: f32) -> [f64; 2] {
        [self.origin[0] + x as f64 * self.unit, self.origin[1] - y as f64 * self.unit * self.yscale]
    }
}

impl OutlineBuilder for Builder {
    fn move_to(&mut self, x: f32, y: f32) {
        let p = self.map(x, y);
        self.path.track(p);
        self.path.contours.push(Contour { start: p, segs: Vec::new() });
    }

    fn line_to(&mut self, x: f32, y: f32) {
        let p = self.map(x, y);
        self.path.track(p);
        if let Some(contour) = self.path.contours.last_mut() {
            contour.segs.push(Seg::Line(p));
        }
    }

    fn quad_to(&mut self, x1: f32, y1: f32, x: f32, y: f32) {
        let ctrl = self.map(x1, y1);
        let end = self.map(x, y);
        self.path.track(ctrl);
        self.path.track(end);
        if let Some(contour) = self.path.contours.last_mut() {
            contour.segs.push(Seg::Quad(ctrl, end));
        }
    }

    fn curve_to(&mut self, x1: f32, y1: f32, x2: f32, y2: f32, x: f32, y: f32) {
        let a = self.map(x1, y1);
        let b = self.map(x2, y2);
        let end = self.map(x, y);
        self.path.track(a);
        self.path.track(b);
        self.path.track(end);
        if let Some(contour) = self.path.contours.last_mut() {
            contour.segs.push(Seg::Cubic(a, b, end));
        }
    }

    fn close(&mut self) {}
}

/// Extract the outline of `ch` positioned at the baseline point `(x, y)`.
///
/// `size` is the font size in points and `yscale` the vertical stretch about the baseline. Returns
/// `None` when the face has no glyph for `ch` or the glyph carries no outline, e.g. a space.
pub(crate) fn glyph_path(
    face: &Face<'_>,
    ch: char,
    x: f64,
    y: f64,
    size: f64,
    yscale: f64,
) -> Option<Path> {
    let glyph = face.glyph_index(ch)?;
    let units = face.units_per_em() as f64;
    if units <= 0.0 || size <= 0.0 {
        return None;
    }
    let mut builder = Builder {
        path: Path::default(),
        unit: size / units,
        yscale,
        origin: [x, y],
    };
    face.outline_glyph(glyph, &mut builder)?;
    if builder.path.is_empty() {
        None
    } else {
        Some(builder.path)
    }
}
