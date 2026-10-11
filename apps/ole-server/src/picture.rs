//! The picture the hosts display: the equation laid out, rasterised and packed as a `CF_DIB`.
//!
//! `HIMETRIC` is the unit OLE measures embedded objects in: one unit is 0.01 mm, so a size in
//! points is multiplied by `2540 / 72` to reach it.

use std::mem::size_of;

use mt_core::Math;
use mt_layout::{Box as LayoutBox, LayoutStyle, layout};
use mt_render::{Image, RenderOptions, render};
use windows::Win32::Graphics::Gdi::{BI_RGB, BITMAPINFOHEADER};

use mt_render::metrics::{FaceMetrics, MATH_FONT};

/// HIMETRIC units in one point.
pub const HIMETRIC_PER_POINT: f64 = 2540.0 / 72.0;

/// Device pixels per point the presentation bitmap is drawn at: 288 dpi, crisp when a host scales
/// the picture up and cheap enough to build on every request.
pub const DIB_SCALE: f64 = 4.0;

/// Font size of an equation that was never laid out before, in points.
pub const DEFAULT_SIZE_POINTS: f64 = 18.0;

/// A laid-out equation packed as a device-independent bitmap.
pub struct Presentation {
    /// The equation as it was laid out, in points.
    pub width_pt: f64,
    pub height_pt: f64,
    /// `BITMAPINFOHEADER` followed by bottom-up 32-bit BGRA pixels.
    pub dib: Vec<u8>,
    /// Pixel size of the bitmap, which is the extent rounded to whole pixels.
    pub pixels: (u32, u32),
}

/// Lay the equation out at the device-independent size.
pub fn lay_out(math: &Math, size_points: f64) -> Option<(LayoutBox, LayoutStyle, FaceMetrics)> {
    let metrics = FaceMetrics::parse(MATH_FONT, 0)?;
    let style = LayoutStyle::display(size_points);
    let boxes = layout(math, &metrics, &style);
    Some((boxes, style, metrics))
}

/// The equation's natural size at `points` points, in points.
pub fn natural_size(math: &Math, points: f64) -> (f64, f64) {
    match lay_out(math, points) {
        Some((boxes, _, _)) => (boxes.width(), boxes.height() + boxes.depth()),
        None => (0.0, 0.0),
    }
}

/// The equation's size in `HIMETRIC` units, which is what `GetExtent` reports.
pub fn himetric_size(math: &Math, points: f64) -> (i32, i32) {
    let (width, height) = natural_size(math, points);
    let units = |points: f64| (points * HIMETRIC_PER_POINT).round().max(1.0) as i32;
    (units(width), units(height))
}

/// Rasterise the equation and pack it as a `CF_DIB`.
///
/// When the host has asked for a size with `SetExtent`, the equation is laid out at the size that
/// fills that box, so the bitmap it gets back is the size it asked for.
pub fn presentation(math: &Math, requested_points: f64, extent: Option<(i32, i32)>) -> Option<Presentation> {
    let natural = natural_size(math, requested_points);
    let size = match extent {
        Some((cx, cy)) if cx > 0 && cy > 0 && natural.0 > 0.0 && natural.1 > 0.0 => {
            let wanted = (f64::from(cx) / HIMETRIC_PER_POINT, f64::from(cy) / HIMETRIC_PER_POINT);
            let factor = (wanted.0 / natural.0).min(wanted.1 / natural.1);
            requested_points * factor.clamp(0.05, 8.0)
        }
        _ => requested_points,
    };
    let (boxes, style, metrics) = lay_out(math, size)?;
    let options = RenderOptions {
        scale: DIB_SCALE,
        padding_em: 0.0,
        foreground: [0, 0, 0, 255],
        background: Some([255, 255, 255, 255]),
    };
    let image = render(&boxes, &style, metrics.face(), &options);
    let width_pt = boxes.width();
    let height_pt = boxes.height() + boxes.depth();
    Some(Presentation { width_pt, height_pt, pixels: (image.width, image.height), dib: to_dib(&image) })
}

/// Bytes of the bitmap header a `CF_DIB` opens with, where its pixels start.
pub fn pixel_offset() -> usize {
    size_of::<BITMAPINFOHEADER>()
}

/// Pack an RGBA image as a bottom-up 32-bit `BI_RGB` DIB: header first, then the pixels.
fn to_dib(image: &Image) -> Vec<u8> {
    let stride = image.width as usize * 4;
    let header = BITMAPINFOHEADER {
        biSize: size_of::<BITMAPINFOHEADER>() as u32,
        biWidth: image.width as i32,
        biHeight: image.height as i32,
        biPlanes: 1,
        biBitCount: 32,
        biCompression: BI_RGB.0,
        biSizeImage: (stride * image.height as usize) as u32,
        ..Default::default()
    };
    let mut out = Vec::with_capacity(size_of::<BITMAPINFOHEADER>() + stride * image.height as usize);
    let bytes = unsafe {
        std::slice::from_raw_parts(std::ptr::addr_of!(header).cast::<u8>(), size_of::<BITMAPINFOHEADER>())
    };
    out.extend_from_slice(bytes);
    // `CF_DIB` rows run bottom-up; the rendered image runs top-down.
    for row in (0..image.height as usize).rev() {
        for pixel in image.rgba[row * stride..(row + 1) * stride].chunks_exact(4) {
            out.extend_from_slice(&[pixel[2], pixel[1], pixel[0], 255]);
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::{HIMETRIC_PER_POINT, himetric_size, presentation};
    use mt_tex::parse;

    #[test]
    fn the_size_in_himetric_units_matches_the_points_it_was_laid_out_at() {
        let math = parse(r"\frac{a}{b}").unwrap();
        let (width, height) = himetric_size(&math, 18.0);
        let points = |units: i32| f64::from(units) / HIMETRIC_PER_POINT;
        // A fraction is narrow and tall: taller than the 18 point size it is set at, and nowhere
        // near an inch across.
        assert!(points(height) > 18.0, "height was {} points", points(height));
        assert!(points(width) > 0.0 && points(width) < 72.0, "width was {} points", points(width));
    }

    #[test]
    fn the_bitmap_opens_with_a_thirty_two_bit_bitmap_info_header() {
        let math = parse(r"\frac{a}{b}+\sqrt{x_i^2}").unwrap();
        let picture = presentation(&math, 18.0, None).unwrap();
        assert_eq!(u32::from_le_bytes(picture.dib[0..4].try_into().unwrap()), 40);
        assert_eq!(i16::from_le_bytes(picture.dib[14..16].try_into().unwrap()), 32);
        assert_eq!(picture.dib.len(), 40 + picture.pixels.0 as usize * picture.pixels.1 as usize * 4);
    }

    #[test]
    fn asking_for_a_larger_extent_gives_a_larger_bitmap() {
        let math = parse(r"x^2").unwrap();
        let small = presentation(&math, 18.0, None).unwrap();
        let large = presentation(&math, 18.0, Some((3600, 3600))).unwrap();
        assert!(large.pixels.0 >= small.pixels.0, "{:?} then {:?}", small.pixels, large.pixels);
    }
}
