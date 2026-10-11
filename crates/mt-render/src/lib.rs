//! Image backends for a laid-out equation: a rasterized RGBA buffer, PNG and EPS.
//!
//! The crate draws the box tree [`mt_layout::layout`] produces. Nothing here re-measures glyphs:
//! every glyph carries the baseline point it must be drawn at, and the outlines come straight from
//! the [`ttf_parser::Face`] the caller supplies, so no installed font is needed.
//!
//! ```
//! use mt_core::{Math, Node};
//! use mt_layout::{layout, ApproxMetrics, LayoutStyle};
//! use mt_render::{render, RenderOptions};
//!
//! let face = ttf_parser::Face::parse(include_bytes!("../../../assets/fonts/latinmodern-math.otf"), 0).unwrap();
//! let math = Math::new(Node::row([Node::variable('x')]));
//! let style = LayoutStyle::display(10.0);
//! let tree = layout(&math, &ApproxMetrics, &style);
//! let image = render(&tree, &style, &face, &RenderOptions::default());
//! assert!(image.width > 0 && image.rgba.len() == image.width as usize * image.height as usize * 4);
//! ```

#![forbid(unsafe_code)]

mod eps;
mod outline;
mod raster;

use mt_layout::constants::{PLACEHOLDER_DASH, PLACEHOLDER_STROKE};
use mt_layout::{Box as LBox, LayoutStyle};
use ttf_parser::Face;

use outline::{Path, glyph_path};
use raster::Mask;

/// How the tree is turned into pixels or points.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct RenderOptions {
    /// Device pixels per point, e.g. `3.0` for a 216 dpi export.
    pub scale: f64,
    /// Padding around the ink box, in em of the full size.
    pub padding_em: f64,
    /// Ink colour, RGBA.
    pub foreground: [u8; 4],
    /// Background colour, RGBA; `None` leaves the background transparent.
    pub background: Option<[u8; 4]>,
}

impl Default for RenderOptions {
    fn default() -> Self {
        Self { scale: 1.0, padding_em: 0.3, foreground: [0, 0, 0, 255], background: None }
    }
}

/// A rendered image: `width * height` RGBA pixels, row-major from the top.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Image {
    pub width: u32,
    pub height: u32,
    /// Row-major RGBA bytes, `width * height * 4` long.
    pub rgba: Vec<u8>,
}

/// Why a render could not be encoded.
#[derive(Debug)]
pub enum RenderError {
    /// PNG encoding failed.
    Png(String),
}

impl std::fmt::Display for RenderError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            RenderError::Png(message) => write!(f, "PNG encoding failed: {message}"),
        }
    }
}

impl std::error::Error for RenderError {}

/// One drawable piece collected from the tree.
pub(crate) enum Item {
    /// A glyph outline in layout point coordinates.
    Glyph { path: Path },
    /// A rule or its dashed placeholder.
    Rule { x: f64, y: f64, width: f64, height: f64, depth: f64, dashed: bool },
}

impl Item {
    /// The polygons of this item, in layout point coordinates.
    fn polygons(&self) -> Vec<Vec<[f64; 2]>> {
        match self {
            Item::Glyph { path } => path.flatten(16),
            Item::Rule { x, y, width, height, depth, dashed } => {
                rule_polygons(*x, *y, *width, *height, *depth, *dashed)
            }
        }
    }

    /// Extend `bbox` with this item's control-point extent.
    fn extend(&self, bbox: &mut (f64, f64, f64, f64)) {
        match self {
            Item::Glyph { path } => {
                if let Some((x0, y0, x1, y1)) = path.bbox {
                    bbox.0 = bbox.0.min(x0);
                    bbox.1 = bbox.1.min(y0);
                    bbox.2 = bbox.2.max(x1);
                    bbox.3 = bbox.3.max(y1);
                }
            }
            Item::Rule { x, y, width, height, depth, .. } => {
                bbox.0 = bbox.0.min(*x);
                bbox.1 = bbox.1.min(*y - height);
                bbox.2 = bbox.2.max(*x + width);
                bbox.3 = bbox.3.max(*y + depth);
            }
        }
    }
}

/// Collect every glyph outline and rule in draw order.
fn collect(tree: &LBox, face: &Face<'_>) -> Vec<Item> {
    let mut items = Vec::new();
    tree.visit(&mut |node| match node {
        LBox::Glyph { ch, x, y, size, yscale, .. } => {
            if let Some(path) = glyph_path(face, *ch, *x, *y, *size, *yscale) {
                items.push(Item::Glyph { path });
            }
        }
        LBox::Rule { x, y, width, height, depth, dashed } => {
            items.push(Item::Rule {
                x: *x,
                y: *y,
                width: *width,
                height: *height,
                depth: *depth,
                dashed: *dashed,
            });
        }
        _ => {}
    });
    items
}

/// The rectangle the image covers, `(x0, y0, x1, y1)`, in points.
///
/// It is the root box's own extent, grown by every glyph outline and rule that sticks out of it,
/// then padded by `padding_em * style.size` on all four sides.
pub fn image_bounds(
    tree: &LBox,
    style: &LayoutStyle,
    face: &Face<'_>,
    options: &RenderOptions,
) -> (f64, f64, f64, f64) {
    let mut bbox = (0.0, -tree.height(), tree.width(), tree.depth());
    for item in collect(tree, face) {
        item.extend(&mut bbox);
    }
    let pad = options.padding_em * style.size;
    (bbox.0 - pad, bbox.1 - pad, bbox.2 + pad, bbox.3 + pad)
}

/// Image dimensions in device pixels for `options`, without rendering.
pub fn image_size(
    tree: &LBox,
    style: &LayoutStyle,
    face: &Face<'_>,
    options: &RenderOptions,
) -> (u32, u32) {
    let (x0, y0, x1, y1) = image_bounds(tree, style, face, options);
    let scale = options.scale.max(0.0);
    (
        ((x1 - x0) * scale).ceil().max(1.0) as u32,
        ((y1 - y0) * scale).ceil().max(1.0) as u32,
    )
}

/// Rasterize `tree` into an RGBA image.
pub fn render(
    tree: &LBox,
    style: &LayoutStyle,
    face: &Face<'_>,
    options: &RenderOptions,
) -> Image {
    let items = collect(tree, face);
    let bounds = {
        let mut bbox = (0.0, -tree.height(), tree.width(), tree.depth());
        for item in &items {
            item.extend(&mut bbox);
        }
        let pad = options.padding_em * style.size;
        (bbox.0 - pad, bbox.1 - pad, bbox.2 + pad, bbox.3 + pad)
    };
    let (bx0, by0, _, _) = bounds;
    let scale = options.scale.max(0.0);
    let (width, height) = (
        (((bounds.2 - bounds.0) * scale).ceil()).max(1.0) as u32,
        (((bounds.3 - bounds.1) * scale).ceil()).max(1.0) as u32,
    );

    let mut image = Image { width, height, rgba: vec![0u8; width as usize * height as usize * 4] };
    if let Some(bg) = options.background {
        for pixel in image.rgba.chunks_exact_mut(4) {
            pixel.copy_from_slice(&bg);
        }
    }

    // Rules first so glyph ink wins where the two overlap.
    for pass in 0..2 {
        for item in &items {
            let is_rule = matches!(item, Item::Rule { .. });
            if (pass == 0) != is_rule {
                continue;
            }
            let polys = to_pixels(&item.polygons(), bx0, by0, scale);
            if let Some(mask) = raster::rasterize(&polys, width, height) {
                blit(&mut image, &mask, options.foreground);
            }
        }
    }
    image
}

/// Encode `tree` as a PNG.
pub fn render_png(
    tree: &LBox,
    style: &LayoutStyle,
    face: &Face<'_>,
    options: &RenderOptions,
) -> Result<Vec<u8>, RenderError> {
    let image = render(tree, style, face, options);
    let mut buffer = Vec::new();
    {
        let mut encoder = png::Encoder::new(&mut buffer, image.width, image.height);
        encoder.set_color(png::ColorType::Rgba);
        encoder.set_depth(png::BitDepth::Eight);
        let mut writer = encoder.write_header().map_err(|e| RenderError::Png(e.to_string()))?;
        writer
            .write_image_data(&image.rgba)
            .map_err(|e| RenderError::Png(e.to_string()))?;
    }
    Ok(buffer)
}

/// Serialise `tree` as an EPS document.
///
/// Coordinates are points and independent of [`RenderOptions::scale`]; `padding_em` and the
/// foreground colour are honoured, the background is not (EPS has no page colour).
pub fn render_eps(
    tree: &LBox,
    style: &LayoutStyle,
    face: &Face<'_>,
    options: &RenderOptions,
) -> String {
    let items = collect(tree, face);
    let bounds = {
        let mut bbox = (0.0, -tree.height(), tree.width(), tree.depth());
        for item in &items {
            item.extend(&mut bbox);
        }
        let pad = options.padding_em * style.size;
        (bbox.0 - pad, bbox.1 - pad, bbox.2 + pad, bbox.3 + pad)
    };
    eps::render(&items, bounds, options.foreground)
}

/// Map layout-point polygons to image pixels.
fn to_pixels(polys: &[Vec<[f64; 2]>], bx0: f64, by0: f64, scale: f64) -> Vec<Vec<[f64; 2]>> {
    polys
        .iter()
        .map(|poly| {
            poly.iter().map(|p| [(p[0] - bx0) * scale, (p[1] - by0) * scale]).collect()
        })
        .collect()
}

/// Blend a foreground colour through a coverage mask into `image`.
fn blit(image: &mut Image, mask: &Mask, fg: [u8; 4]) {
    for row in 0..mask.h {
        let py = mask.y0 + row as i32;
        if py < 0 || py as u32 >= image.height {
            continue;
        }
        for column in 0..mask.w {
            let px = mask.x0 + column as i32;
            if px < 0 || px as u32 >= image.width {
                continue;
            }
            let coverage = mask.cov[row as usize * mask.w as usize + column as usize];
            if coverage <= 0.0 {
                continue;
            }
            let index = (py as u32 * image.width + px as u32) as usize * 4;
            blend_pixel(&mut image.rgba[index..index + 4], fg, coverage.min(1.0));
        }
    }
}

/// Composite `fg` at coverage `coverage` over one RGBA pixel, source-over.
fn blend_pixel(dst: &mut [u8], fg: [u8; 4], coverage: f64) {
    let src_a = coverage * fg[3] as f64 / 255.0;
    if src_a <= 0.0 {
        return;
    }
    let dst_a = dst[3] as f64 / 255.0;
    let out_a = src_a + dst_a * (1.0 - src_a);
    if out_a <= 0.0 {
        dst.copy_from_slice(&[0, 0, 0, 0]);
        return;
    }
    for channel in 0..3 {
        let src = fg[channel] as f64;
        let back = dst[channel] as f64;
        let value = (src * src_a + back * dst_a * (1.0 - src_a)) / out_a;
        dst[channel] = value.round().clamp(0.0, 255.0) as u8;
    }
    dst[3] = (out_a * 255.0).round().clamp(0.0, 255.0) as u8;
}

/// Polygons for a rule: a filled rectangle, or dashes around its outline.
fn rule_polygons(
    x: f64,
    y: f64,
    width: f64,
    height: f64,
    depth: f64,
    dashed: bool,
) -> Vec<Vec<[f64; 2]>> {
    let top = y - height;
    let bottom = y + depth;
    if !dashed {
        return vec![vec![[x, top], [x + width, top], [x + width, bottom], [x, bottom]]];
    }

    let corners = [[x, top], [x + width, top], [x + width, bottom], [x, bottom]];
    let edges = [(0, 1), (1, 2), (2, 3), (3, 0)];
    let cycle = 2.0 * PLACEHOLDER_DASH;
    let half = PLACEHOLDER_STROKE / 2.0;
    let mut out = Vec::new();
    let mut travelled = 0.0;
    for (a, b) in edges {
        let start = corners[a];
        let end = corners[b];
        let length = ((end[0] - start[0]).powi(2) + (end[1] - start[1]).powi(2)).sqrt();
        if length <= 0.0 {
            continue;
        }
        let dir = [(end[0] - start[0]) / length, (end[1] - start[1]) / length];
        let mut t = 0.0;
        while t < length {
            let phase = (travelled + t) % cycle;
            if phase < PLACEHOLDER_DASH {
                let segment = (PLACEHOLDER_DASH - phase).min(length - t);
                let p = [start[0] + dir[0] * t, start[1] + dir[1] * t];
                let q = [p[0] + dir[0] * segment, p[1] + dir[1] * segment];
                out.push(thick_segment(p, q, half));
                t += segment;
            } else {
                t += (cycle - phase).min(length - t);
            }
        }
        travelled += length;
    }
    out
}

/// A four-corner rectangle around the segment `p .. q`, `half` points thick on each side.
fn thick_segment(p: [f64; 2], q: [f64; 2], half: f64) -> Vec<[f64; 2]> {
    let dx = q[0] - p[0];
    let dy = q[1] - p[1];
    let length = (dx * dx + dy * dy).sqrt();
    let (nx, ny) = if length > 0.0 { (-dy / length * half, dx / length * half) } else { (0.0, 0.0) };
    vec![
        [p[0] + nx, p[1] + ny],
        [q[0] + nx, q[1] + ny],
        [q[0] - nx, q[1] - ny],
        [p[0] - nx, p[1] - ny],
    ]
}
