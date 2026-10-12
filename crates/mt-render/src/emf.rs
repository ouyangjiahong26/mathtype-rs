//! Enhanced Metafile (EMF) backend.
//!
//! The picture carries the glyph outlines as filled Bézier paths — the same source as the EPS
//! backend — so it renders without the font being installed and stays sharp at any zoom. No
//! `ExtTextOutW` is emitted, which avoids host-side font substitution entirely.
//!
//! Layout coordinates are already top-down (`y` grows downwards, the baseline at the glyph's
//! `y`), and EMF device coordinates are top-down too, so only the shift to the image box's
//! top-left corner is applied. Device units are points (a 72 dpi reference device), and
//! `rclFrame` carries the physical size in 0.01 mm so hosts can scale the picture to the extent
//! they set with `SetExtent`.

use crate::Item;
use crate::outline::{Path, Seg};

/// EMF record type numbers, from `wingdi.h`.
const EMR_HEADER: u32 = 1;
const EMR_EOF: u32 = 14;
const EMR_SETPOLYFILLMODE: u32 = 19;
const EMR_MOVETOEX: u32 = 27;
const EMR_SELECTOBJECT: u32 = 37;
const EMR_CREATEBRUSHINDIRECT: u32 = 39;
const EMR_LINETO: u32 = 54;
const EMR_BEGINPATH: u32 = 59;
const EMR_ENDPATH: u32 = 60;
const EMR_CLOSEFIGURE: u32 = 61;
const EMR_FILLPATH: u32 = 62;
const EMR_POLYBEZIERTO16: u32 = 88;
const EMR_POLYPOLYGON16: u32 = 91;

/// Handle of the one brush the file creates; handle 0 is reserved.
const BRUSH_HANDLE: u32 = 1;

/// Emit a complete EMF picture for the collected items.
///
/// `bounds` is `(x0, y0, x1, y1)` in layout points, as [`crate::image_bounds`] returns it.
pub(crate) fn render(items: &[Item], bounds: (f64, f64, f64, f64), fg: [u8; 4]) -> Vec<u8> {
    let (bx0, by0, bx1, by1) = bounds;
    let width = (bx1 - bx0).ceil().max(1.0) as i32;
    let height = (by1 - by0).ceil().max(1.0) as i32;
    let origin = (bx0, by0);
    let clip = (0, 0, width - 1, height - 1);

    let mut out = Vec::new();
    header(&mut out, width, height); // nBytes/nRecords patched once the rest is written
    let mut rec = Rec::new(EMR_SETPOLYFILLMODE);
    rec.u32(2); // WINDING, so the holes in glyph outlines fill correctly
    rec.finish(&mut out);
    brush(&mut out, fg);
    let mut rec = Rec::new(EMR_SELECTOBJECT);
    rec.u32(BRUSH_HANDLE);
    rec.finish(&mut out);
    for item in items {
        match item {
            Item::Glyph { path } => emit_glyph(&mut out, path, origin, clip),
            Item::Rule {
                x,
                y,
                width: rw,
                height,
                depth,
                dashed,
            } => emit_rule(
                &mut out,
                Rule {
                    x: *x,
                    y: *y,
                    width: *rw,
                    height: *height,
                    depth: *depth,
                    dashed: *dashed,
                },
                origin,
                clip,
            ),
        }
    }
    eof(&mut out);

    patch_header(&mut out);
    out
}

/// Map a layout point to EMF device units (points, top-down), relative to the image box.
fn dev(p: [f64; 2], origin: (f64, f64)) -> (i32, i32) {
    (
        (p[0] - origin.0).round() as i32,
        (p[1] - origin.1).round() as i32,
    )
}

/// Write `EMR_HEADER`, the 88-byte fixed part: bounds in device units, frame in 0.01 mm.
fn header(out: &mut Vec<u8>, width: i32, height: i32) {
    let himetric = |points: i32| ((f64::from(points) * 2540.0 / 72.0).round() as i32).max(1);
    // `Rec::finish` stamps the record's own `nSize`, so the payload starts at `rclBounds`.
    let mut rec = Rec::new(EMR_HEADER);
    rec.rect_from((0, 0, width - 1, height - 1)); // rclBounds, inclusive, device units
    rec.rect_from((0, 0, himetric(width), himetric(height))); // rclFrame, 0.01 mm
    rec.u32(0x464D4520); // " EMF"
    rec.u32(0x00010000); // version
    rec.u32(0); // nBytes, patched after the file is complete
    rec.u32(0); // nRecords, patched after the file is complete
    rec.u32(2); // nHandles: the brush plus reserved handle 0, and sReserved, two u16 fields
    rec.u32(0); // nDescription
    rec.u32(0); // offDescription
    rec.u32(0); // nPalEntries
    rec.u32(width as u32); // szlDevice.cx, with szlMillimeters below: a 72 dpi device
    rec.u32(height as u32); // szlDevice.cy
    let millimetres = |points: i32| ((himetric(points) as f64 / 100.0).ceil() as u32).max(1);
    rec.u32(millimetres(width)); // szlMillimeters.cx
    rec.u32(millimetres(height)); // szlMillimeters.cy
    rec.finish(out);
}

/// Write `EMR_EOF`.
fn eof(out: &mut Vec<u8>) {
    let mut rec = Rec::new(EMR_EOF);
    rec.u32(0); // nPalEntries
    rec.u32(16); // offPalEntries, just past the fixed fields
    rec.u32(20); // nSizeLast, repeats the record size
    rec.finish(out);
}

/// Write `EMR_CREATEBRUSHINDIRECT` for one solid `fg` brush.
fn brush(out: &mut Vec<u8>, fg: [u8; 4]) {
    let color = u32::from(fg[0]) | u32::from(fg[1]) << 8 | u32::from(fg[2]) << 16; // COLORREF
    let mut rec = Rec::new(EMR_CREATEBRUSHINDIRECT);
    rec.u32(BRUSH_HANDLE); // ihBrush
    rec.u32(0); // BS_SOLID
    rec.u32(color);
    rec.u32(0); // lbHatch
    rec.finish(out);
}

/// Emit one glyph as a filled Bézier path.
fn emit_glyph(out: &mut Vec<u8>, path: &Path, origin: (f64, f64), clip: (i32, i32, i32, i32)) {
    if path.contours.is_empty() {
        return;
    }
    bare_record(out, EMR_BEGINPATH, clip);
    let mut ink = (i32::MAX, i32::MAX, i32::MIN, i32::MIN);
    for contour in &path.contours {
        if contour.segs.is_empty() {
            continue;
        }
        let start = dev(contour.start, origin);
        track(&mut ink, start);
        point_record(out, EMR_MOVETOEX, start);

        // Segments go out in outline order: beziers accumulate in a run and are flushed by the
        // next line, or by the end of the contour, so the drawn contour matches the font's.
        let mut beziers = Vec::new();
        let mut cur = contour.start;
        for seg in &contour.segs {
            match *seg {
                Seg::Line(end) => {
                    flush_beziers(out, &mut beziers, &mut ink, clip);
                    let p = dev(end, origin);
                    track(&mut ink, p);
                    point_record(out, EMR_LINETO, p);
                }
                Seg::Quad(ctrl, end) => {
                    // Promote the quadratic to a cubic so every curve uses one record shape.
                    let c1 = lerp(cur, ctrl, 2.0 / 3.0);
                    let c2 = lerp(end, ctrl, 2.0 / 3.0);
                    for p in [c1, c2, end] {
                        beziers.push(dev(p, origin));
                    }
                }
                Seg::Cubic(a, b, end) => {
                    for p in [a, b, end] {
                        beziers.push(dev(p, origin));
                    }
                }
            }
            cur = end_of(seg);
        }
        flush_beziers(out, &mut beziers, &mut ink, clip);
        bare_record(out, EMR_CLOSEFIGURE, clip);
    }
    bare_record(out, EMR_ENDPATH, clip);
    bare_record(out, EMR_FILLPATH, intersect(ink, clip));
}

/// One rule to draw: its box in layout point coordinates and whether it is the dashed placeholder.
struct Rule {
    x: f64,
    y: f64,
    width: f64,
    height: f64,
    depth: f64,
    dashed: bool,
}

/// Emit one rule: a filled rectangle, or the dashes of the empty-slot placeholder.
fn emit_rule(out: &mut Vec<u8>, rule: Rule, origin: (f64, f64), clip: (i32, i32, i32, i32)) {
    let polys = crate::rule_polygons(
        rule.x,
        rule.y,
        rule.width,
        rule.height,
        rule.depth,
        rule.dashed,
    );
    let mut points = Vec::with_capacity(polys.len());
    let mut ink = (i32::MAX, i32::MAX, i32::MIN, i32::MIN);
    for poly in &polys {
        let mapped: Vec<(i32, i32)> = poly.iter().map(|p| dev(*p, origin)).collect();
        for p in &mapped {
            track(&mut ink, *p);
        }
        points.push(mapped);
    }
    if points.is_empty() {
        return;
    }
    let mut rec = Rec::new(EMR_POLYPOLYGON16);
    rec.rect_from(intersect(ink, clip));
    rec.u32(points.len() as u32);
    rec.u32(points.iter().map(|p| p.len() as u32).sum());
    for poly in &points {
        rec.u32(poly.len() as u32);
    }
    for poly in &points {
        for p in poly {
            rec.i16(p.0);
            rec.i16(p.1);
        }
    }
    rec.finish(out);
}

/// Emit the pending bezier run, if any, as one `EMR_POLYBEZIERTO16`.
fn flush_beziers(
    out: &mut Vec<u8>,
    beziers: &mut Vec<(i32, i32)>,
    ink: &mut (i32, i32, i32, i32),
    clip: (i32, i32, i32, i32),
) {
    if beziers.is_empty() {
        return;
    }
    for p in beziers.iter() {
        track(ink, *p);
    }
    poly_bezier_to_16(out, beziers, clip);
    beziers.clear();
}

/// Write `EMR_POLYBEZIERTO16` for whole cubic Béziers, `points` holding three points per curve.
fn poly_bezier_to_16(out: &mut Vec<u8>, points: &[(i32, i32)], clip: (i32, i32, i32, i32)) {
    let mut rec = Rec::new(EMR_POLYBEZIERTO16);
    rec.rect_from(clip);
    rec.u32(points.len() as u32);
    for p in points {
        rec.i16(p.0);
        rec.i16(p.1);
    }
    rec.finish(out);
}

/// Write a bounds-only record: `kind` plus a `RECTL`.
fn bare_record(out: &mut Vec<u8>, kind: u32, bounds: (i32, i32, i32, i32)) {
    let mut rec = Rec::new(kind);
    rec.rect_from(bounds);
    rec.finish(out);
}

/// Write a record whose payload is a single `POINTL`.
fn point_record(out: &mut Vec<u8>, kind: u32, p: (i32, i32)) {
    let mut rec = Rec::new(kind);
    rec.i32(p.0);
    rec.i32(p.1);
    rec.finish(out);
}

/// End point of a segment.
fn end_of(seg: &Seg) -> [f64; 2] {
    match *seg {
        Seg::Line(end) | Seg::Quad(_, end) | Seg::Cubic(_, _, end) => end,
    }
}

/// Linear interpolation `a + t * (b - a)`.
fn lerp(a: [f64; 2], b: [f64; 2], t: f64) -> [f64; 2] {
    [a[0] + t * (b[0] - a[0]), a[1] + t * (b[1] - a[1])]
}

/// Extend `bounds` with `p`.
fn track(bounds: &mut (i32, i32, i32, i32), p: (i32, i32)) {
    bounds.0 = bounds.0.min(p.0);
    bounds.1 = bounds.1.min(p.1);
    bounds.2 = bounds.2.max(p.0);
    bounds.3 = bounds.3.max(p.1);
}

/// Clamp a drawn bounds to the picture, with the whole picture as the empty-input fallback.
fn intersect(bounds: (i32, i32, i32, i32), clip: (i32, i32, i32, i32)) -> (i32, i32, i32, i32) {
    if bounds.0 > bounds.2 || bounds.1 > bounds.3 {
        return clip;
    }
    (
        bounds.0.max(clip.0),
        bounds.1.max(clip.1),
        bounds.2.min(clip.2),
        bounds.3.min(clip.3),
    )
}

/// Fill in `nBytes` and `nRecords` now that the whole file is written.
fn patch_header(out: &mut [u8]) {
    let mut records = 0u32;
    let mut offset = 0usize;
    while offset + 8 <= out.len() {
        let size = u32::from_le_bytes(out[offset + 4..offset + 8].try_into().unwrap()) as usize;
        if size < 8 || offset + size > out.len() {
            break;
        }
        records += 1;
        offset += size;
    }
    // nBytes/nRecords sit at offsets 48/52, past iType, nSize, both rectangles, the signature
    // and the version.
    let bytes = out.len() as u32;
    out[48..52].copy_from_slice(&bytes.to_le_bytes());
    out[52..56].copy_from_slice(&records.to_le_bytes());
}

/// A record under construction: `iType` and a placeholder `nSize`, then payload words, then up
/// to three zero bytes of padding so the record length stays a multiple of four.
struct Rec {
    buf: Vec<u8>,
}

impl Rec {
    /// Start a record of `kind`.
    fn new(kind: u32) -> Self {
        Self {
            buf: kind.to_le_bytes().iter().chain(&[0; 4]).copied().collect(),
        }
    }

    /// Append a payload word.
    fn u32(&mut self, value: u32) {
        self.buf.extend_from_slice(&value.to_le_bytes());
    }

    /// Append a payload word holding an `i32`.
    fn i32(&mut self, value: i32) {
        self.buf.extend_from_slice(&value.to_le_bytes());
    }

    /// Append a `POINTS`, a 16-bit signed coordinate pair member.
    fn i16(&mut self, value: i32) {
        self.buf.extend_from_slice(&(value as i16).to_le_bytes());
    }

    /// Append a `RECTL`.
    fn rect_from(&mut self, r: (i32, i32, i32, i32)) {
        let (l, t, r, b) = r;
        for v in [l, t, r, b] {
            self.i32(v);
        }
    }

    /// Stamp the real `nSize`, append the padding, and append the record to `out`.
    fn finish(mut self, out: &mut Vec<u8>) {
        let pad = (4 - self.buf.len() % 4) % 4;
        let size = (self.buf.len() + pad) as u32;
        self.buf[4..8].copy_from_slice(&size.to_le_bytes());
        self.buf.extend(std::iter::repeat_n(0, pad));
        out.extend_from_slice(&self.buf);
    }
}

#[cfg(test)]
mod tests {
    use super::render as emf_bytes;
    use crate::{RenderOptions, image_size, render_png};
    use mt_core::{Frac, FracKind, Math, Node};
    use mt_layout::{ApproxMetrics, LayoutStyle, layout};
    use ttf_parser::Face;

    /// A laid-out fraction `a/b`, which draws glyph outlines plus a rule.
    fn tree() -> (mt_layout::Box, LayoutStyle, Face<'static>) {
        let face = Face::parse(
            include_bytes!("../../../assets/fonts/latinmodern-math.otf"),
            0,
        )
        .unwrap();
        let math = Math::new(Node::Frac(Box::new(Frac {
            num: Node::variable('a'),
            den: Node::variable('b'),
            kind: FracKind::Bar,
        })));
        let style = LayoutStyle::display(18.0);
        let tree = layout(&math, &ApproxMetrics, &style);
        (tree, style, face)
    }

    /// Collect + bound the same way `render_emf` does, straight from the crate root.
    fn picture() -> (Vec<u8>, (f64, f64, f64, f64)) {
        let (tree, style, face) = tree();
        let items = crate::collect(&tree, &face);
        let bounds = crate::ink_bounds(&items, &tree, 0.3 * style.size);
        (emf_bytes(&items, bounds, [0, 0, 0, 255]), bounds)
    }

    /// Walk the records: `(iType, nSize)` pairs in file order.
    fn records(bytes: &[u8]) -> Vec<(u32, u32)> {
        let mut out = Vec::new();
        let mut offset = 0;
        while offset + 8 <= bytes.len() {
            let kind = u32::from_le_bytes(bytes[offset..offset + 4].try_into().unwrap());
            let size = u32::from_le_bytes(bytes[offset + 4..offset + 8].try_into().unwrap());
            assert!(size >= 8 && size % 4 == 0, "record {kind} has size {size}");
            assert!(
                offset + size as usize <= bytes.len(),
                "record {kind} overruns the file"
            );
            out.push((kind, size));
            offset += size as usize;
        }
        assert_eq!(offset, bytes.len(), "trailing bytes after the last record");
        out
    }

    #[test]
    fn the_header_counts_what_the_file_contains() {
        let (bytes, _) = picture();
        assert_eq!(u32::from_le_bytes(bytes[0..4].try_into().unwrap()), 1); // EMR_HEADER
        assert_eq!(u32::from_le_bytes(bytes[4..8].try_into().unwrap()), 88); // its size
        assert_eq!(
            u32::from_le_bytes(bytes[40..44].try_into().unwrap()),
            0x464D4520
        ); // " EMF"
        let walked = records(&bytes);
        let count = walked.len() as u32;
        assert_eq!(u32::from_le_bytes(bytes[52..56].try_into().unwrap()), count);
        assert_eq!(
            u32::from_le_bytes(bytes[48..52].try_into().unwrap()),
            bytes.len() as u32
        );
        assert_eq!(walked.last().copied(), Some((super::EMR_EOF, 20)));
    }

    #[test]
    fn the_bounds_match_the_png_size_and_the_frame_is_himetric() {
        let (bytes, bounds) = picture();
        let (width, height) =
            image_size(&tree().0, &tree().1, &tree().2, &RenderOptions::default());
        let field = |off: usize| i32::from_le_bytes(bytes[off..off + 4].try_into().unwrap());
        assert_eq!(
            (field(8), field(12), field(16), field(20)),
            (0, 0, width as i32 - 1, height as i32 - 1)
        );
        // rclFrame is the same rectangle in 0.01 mm, i.e. points * 2540 / 72.
        let expect = (width as f64 * 2540.0 / 72.0).round() as i32;
        assert!(
            (field(32) - expect).abs() <= 1,
            "frame right was {}",
            field(32)
        );
        let expect = (height as f64 * 2540.0 / 72.0).round() as i32;
        assert!(
            (field(36) - expect).abs() <= 1,
            "frame bottom was {}",
            field(36)
        );
        assert!(bounds.2 > bounds.0 && bounds.3 > bounds.1);

        // The PNG of the same tree keeps the EMF picture's aspect ratio.
        let png = render_png(&tree().0, &tree().1, &tree().2, &RenderOptions::default()).unwrap();
        let (png_w, png_h) = png_dimensions(&png);
        let emf_ratio = f64::from(width) / f64::from(height);
        assert!(
            (png_w / png_h - emf_ratio).abs() / emf_ratio < 0.05,
            "aspect drifted: png {png_w}x{png_h}, emf {width}x{height}"
        );
    }

    #[test]
    fn the_drawing_uses_outlined_paths_and_rule_polygons() {
        let (bytes, _) = picture();
        let kinds: Vec<u32> = records(&bytes).into_iter().map(|(kind, _)| kind).collect();
        for needed in [
            super::EMR_SETPOLYFILLMODE,
            super::EMR_CREATEBRUSHINDIRECT,
            super::EMR_SELECTOBJECT,
            super::EMR_BEGINPATH,
            super::EMR_MOVETOEX,
            super::EMR_POLYBEZIERTO16,
            super::EMR_CLOSEFIGURE,
            super::EMR_ENDPATH,
            super::EMR_FILLPATH,
            super::EMR_POLYPOLYGON16,
        ] {
            assert!(
                kinds.contains(&needed),
                "missing record type {needed} in {kinds:?}"
            );
        }
        // No text output: everything is outlines, so no host font substitution can occur.
        assert!(!kinds.contains(&84) && !kinds.contains(&83));
    }

    #[test]
    fn libreoffice_can_render_the_picture() {
        let (bytes, _) = picture();
        let dir = std::env::temp_dir().join(format!("mt-emf-test-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("equation.emf");
        std::fs::write(&path, &bytes).unwrap();
        let profile = dir.join("lo-profile");
        let status = std::process::Command::new("soffice")
            .args([
                "-env:UserInstallation=file://".to_string() + profile.to_str().unwrap(),
                "--headless".to_string(),
                "--convert-to".to_string(),
                "png".to_string(),
                "--outdir".to_string(),
            ])
            .arg(&dir)
            .arg(&path)
            .output()
            .expect("soffice must be installed");
        assert!(
            status.status.success(),
            "soffice failed: {}",
            String::from_utf8_lossy(&status.stderr)
        );
        let png_path = dir.join("equation.png");
        let png = std::fs::read(&png_path).unwrap_or_else(|_| {
            panic!(
                "soffice wrote no png; stdout: {}",
                String::from_utf8_lossy(&status.stdout)
            )
        });
        assert!(png.len() > 100, "the png is suspiciously small");

        // LibreOffice places the picture on a page of its own choosing, so only that it
        // decoded the file into a real image with actual ink is asserted here; the aspect
        // ratio itself is checked against the PNG backend in
        // `the_bounds_match_the_png_size_and_the_frame_is_himetric`.
        assert!(ink_pixels(&png) > 50, "the png carries no dark ink at all");

        let _ = std::fs::remove_dir_all(&dir);
    }

    /// Count pixels darker than mid-grey, i.e. the ink LibreOffice drew.
    fn ink_pixels(bytes: &[u8]) -> usize {
        let decoder = png::Decoder::new(std::io::Cursor::new(bytes.to_vec()));
        let mut reader = decoder
            .read_info()
            .expect("soffice produced a readable png");
        let mut buffer = vec![0; reader.output_buffer_size().unwrap_or_default()];
        let info = reader.next_frame(&mut buffer).expect("the png decodes");
        let (w, h) = (info.width as usize, info.height as usize);
        match info.color_type {
            png::ColorType::Grayscale | png::ColorType::Rgb => buffer
                .chunks(info.color_type.samples())
                .filter(|px| px[..3.min(px.len())].iter().any(|c| *c < 128))
                .count(),
            png::ColorType::Rgba => buffer
                .chunks(4)
                .take(w * h)
                .filter(|px| px[0] < 128 && px[3] > 0)
                .count(),
            _ => 0,
        }
    }

    /// Width and height of a PNG from its `IHDR` chunk.
    fn png_dimensions(bytes: &[u8]) -> (f64, f64) {
        let decoder = png::Decoder::new(std::io::Cursor::new(bytes));
        let reader = decoder
            .read_info()
            .expect("soffice produced a readable png");
        (
            f64::from(reader.info().width),
            f64::from(reader.info().height),
        )
    }
}
