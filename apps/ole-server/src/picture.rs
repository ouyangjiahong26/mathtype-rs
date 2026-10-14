//! The picture the hosts display: the equation laid out, then packed either as an Enhanced
//! Metafile (vector, what hosts that can draw it get) or a `CF_DIB` bitmap (the fallback).
//!
//! `HIMETRIC` is the unit OLE measures embedded objects in: one unit is 0.01 mm, so a size in
//! points is multiplied by `2540 / 72` to reach it.

use std::ffi::c_void;
use std::mem::size_of;

use mt_core::Math;
use mt_layout::{Box as LayoutBox, LayoutStyle, layout};
use mt_render::{Image, RenderOptions, render, render_emf};
use windows::Win32::Foundation::{GlobalFree, HGLOBAL};
use windows::Win32::Graphics::Gdi::{
    BI_RGB, BITMAPINFO, BITMAPINFOHEADER, CloseMetaFile, CreateMetaFileW, DIB_RGB_COLORS,
    DeleteMetaFile, HMETAFILE, MM_ANISOTROPIC, SRCCOPY, StretchDIBits,
};
use windows::Win32::System::Memory::{GMEM_MOVEABLE, GlobalAlloc, GlobalLock, GlobalUnlock};
use windows::core::PCWSTR;

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
    /// The same equation as an Enhanced Metafile, glyph outlines drawn as Bézier paths.
    pub emf: Vec<u8>,
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
pub fn presentation(
    math: &Math,
    requested_points: f64,
    extent: Option<(i32, i32)>,
) -> Option<Presentation> {
    let natural = natural_size(math, requested_points);
    let size = match extent {
        Some((cx, cy)) if cx > 0 && cy > 0 && natural.0 > 0.0 && natural.1 > 0.0 => {
            let wanted = (
                f64::from(cx) / HIMETRIC_PER_POINT,
                f64::from(cy) / HIMETRIC_PER_POINT,
            );
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
    // Same layout, same size, vector shapes instead of pixels: the two presentations agree
    // on the extent, so hosts see no jump when they pick either.
    let emf = render_emf(
        &boxes,
        &style,
        metrics.face(),
        &RenderOptions {
            scale: 1.0,
            padding_em: 0.0,
            foreground: [0, 0, 0, 255],
            background: None,
        },
    );
    Some(Presentation {
        width_pt,
        height_pt,
        pixels: (image.width, image.height),
        dib: to_dib(&image),
        emf,
    })
}

/// Bytes of the bitmap header a `CF_DIB` opens with, where its pixels start.
pub fn pixel_offset() -> usize {
    size_of::<BITMAPINFOHEADER>()
}

/// A `METAFILEPICT`: the structure `CF_METAFILEPICT` carries in global memory.
///
/// The metafile handle is in the older Windows format, because that is what the format means and
/// because the host deletes it with `DeleteMetaFile` when it releases the medium.
#[repr(C)]
pub struct MetaFilePict {
    /// The mapping mode the picture was created in, `MM_ANISOTROPIC` here.
    pub mapping_mode: i32,
    /// Suggested size in `MM_HIMETRIC` units.
    pub x_extent: i32,
    pub y_extent: i32,
    pub metafile: HMETAFILE,
}

/// The picture as a `CF_METAFILEPICT`: the same equation in the older Windows metafile format,
/// wrapped in a [`MetaFilePict`] the caller owns.
///
/// Hosts that predate `CF_ENHMETAFILE` ask for this format, and Word refuses to embed an object
/// that cannot answer it. The picture is recorded from the bitmap rather than converted out of the
/// enhanced metafile: a Windows-format metafile has no record for the Bézier paths the equation is
/// drawn with, and `GetWinMetaFileBits` refuses the whole conversion because of them.
/// Hands over a handle the host frees with `ReleaseStgMedium`, which deletes the metafile and the
/// memory together.
pub fn metafile_picture(presentation: &Presentation) -> Option<HGLOBAL> {
    let units = |points: f64| (points * HIMETRIC_PER_POINT).round().max(1.0) as i32;
    let dc = unsafe { CreateMetaFileW(PCWSTR::null()) };
    if dc.is_invalid() {
        return None;
    }
    let (width, height) = (presentation.pixels.0 as i32, presentation.pixels.1 as i32);
    let bits = presentation.dib[pixel_offset()..].as_ptr().cast::<c_void>();
    let info = presentation.dib.as_ptr().cast::<BITMAPINFO>();
    let drawn = unsafe {
        StretchDIBits(
            dc,
            0,
            0,
            width,
            height,
            0,
            0,
            width,
            height,
            Some(bits),
            info,
            DIB_RGB_COLORS,
            SRCCOPY,
        )
    };
    let metafile = unsafe { CloseMetaFile(dc) };
    if metafile.is_invalid() || drawn <= 0 {
        if !metafile.is_invalid() {
            let _ = unsafe { DeleteMetaFile(metafile) };
        }
        return None;
    }
    let picture = MetaFilePict {
        mapping_mode: MM_ANISOTROPIC.0,
        x_extent: units(presentation.width_pt),
        y_extent: units(presentation.height_pt),
        metafile,
    };
    let memory = match unsafe { GlobalAlloc(GMEM_MOVEABLE, size_of::<MetaFilePict>()) } {
        Ok(memory) => memory,
        Err(_) => {
            let _ = unsafe { DeleteMetaFile(metafile) };
            return None;
        }
    };
    let target = unsafe { GlobalLock(memory) };
    if target.is_null() {
        let _ = unsafe { GlobalFree(Some(memory)) };
        let _ = unsafe { DeleteMetaFile(metafile) };
        return None;
    }
    unsafe { target.cast::<MetaFilePict>().write(picture) };
    let _ = unsafe { GlobalUnlock(memory) };
    Some(memory)
}

/// Drop a `CF_METAFILEPICT` this server built and kept for itself, the counterpart of
/// [`metafile_picture`]: the host never sees this one, so nothing else frees it.
pub fn discard_metafile_picture(memory: HGLOBAL) {
    let target = unsafe { GlobalLock(memory) };
    if target.is_null() {
        let _ = unsafe { GlobalFree(Some(memory)) };
        return;
    }
    let picture = unsafe { target.cast::<MetaFilePict>().read() };
    let _ = unsafe { GlobalUnlock(memory) };
    let _ = unsafe { DeleteMetaFile(picture.metafile) };
    let _ = unsafe { GlobalFree(Some(memory)) };
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
    let mut out =
        Vec::with_capacity(size_of::<BITMAPINFOHEADER>() + stride * image.height as usize);
    let bytes = unsafe {
        std::slice::from_raw_parts(
            std::ptr::addr_of!(header).cast::<u8>(),
            size_of::<BITMAPINFOHEADER>(),
        )
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
    use windows::Win32::Foundation::GlobalFree;
    use windows::Win32::Graphics::Gdi::{DeleteMetaFile, GetObjectType, HGDIOBJ, OBJ_METAFILE};
    use windows::Win32::System::Memory::{GlobalLock, GlobalUnlock};

    #[test]
    fn the_size_in_himetric_units_matches_the_points_it_was_laid_out_at() {
        let math = parse(r"\frac{a}{b}").unwrap();
        let (width, height) = himetric_size(&math, 18.0);
        let points = |units: i32| f64::from(units) / HIMETRIC_PER_POINT;
        // A fraction is narrow and tall: taller than the 18 point size it is set at, and nowhere
        // near an inch across.
        assert!(
            points(height) > 18.0,
            "height was {} points",
            points(height)
        );
        assert!(
            points(width) > 0.0 && points(width) < 72.0,
            "width was {} points",
            points(width)
        );
    }

    #[test]
    fn the_metafile_picture_holds_a_windows_format_metafile() {
        let math = parse(r"x^2").unwrap();
        let picture = presentation(&math, 18.0, None).unwrap();
        let memory = super::metafile_picture(&picture).expect("a metafile picture");
        let target = unsafe { GlobalLock(memory) };
        assert!(!target.is_null());
        let pict = unsafe { &*target.cast::<super::MetaFilePict>() };
        // The host deletes this handle when it releases the medium, so it has to be a
        // Windows-format metafile rather than the enhanced one it was built from.
        assert_eq!(
            unsafe { GetObjectType(HGDIOBJ(pict.metafile.0)) },
            OBJ_METAFILE.0 as u32
        );
        assert!(
            pict.x_extent > 0 && pict.y_extent > 0,
            "extent was {}x{}",
            pict.x_extent,
            pict.y_extent
        );
        // What `ReleaseStgMedium` does for this medium, done here so the test owns nothing.
        let _ = unsafe { DeleteMetaFile(pict.metafile) };
        let _ = unsafe { GlobalUnlock(memory) };
        let _ = unsafe { GlobalFree(Some(memory)) };
    }

    #[test]
    fn the_bitmap_opens_with_a_thirty_two_bit_bitmap_info_header() {
        let math = parse(r"\frac{a}{b}+\sqrt{x_i^2}").unwrap();
        let picture = presentation(&math, 18.0, None).unwrap();
        assert_eq!(
            u32::from_le_bytes(picture.dib[0..4].try_into().unwrap()),
            40
        );
        assert_eq!(
            i16::from_le_bytes(picture.dib[14..16].try_into().unwrap()),
            32
        );
        assert_eq!(
            picture.dib.len(),
            40 + picture.pixels.0 as usize * picture.pixels.1 as usize * 4
        );
    }

    #[test]
    fn the_metafile_is_a_well_formed_enhanced_metafile() {
        let math = parse(r"\frac{a}{b}+\sqrt{x_i^2}").unwrap();
        let picture = presentation(&math, 18.0, None).unwrap();
        // `EMR_HEADER` as the first record, the " EMF" signature, and the file's own length.
        assert_eq!(u32::from_le_bytes(picture.emf[0..4].try_into().unwrap()), 1);
        assert_eq!(
            u32::from_le_bytes(picture.emf[4..8].try_into().unwrap()),
            88
        );
        assert_eq!(
            u32::from_le_bytes(picture.emf[40..44].try_into().unwrap()),
            0x464D4520
        );
        assert_eq!(
            u32::from_le_bytes(picture.emf[48..52].try_into().unwrap()),
            picture.emf.len() as u32
        );
    }

    #[test]
    fn the_metafile_grows_with_the_extent_like_the_bitmap() {
        let math = parse(r"x^2").unwrap();
        let small = presentation(&math, 18.0, None).unwrap();
        let large = presentation(&math, 18.0, Some((3600, 3600))).unwrap();
        // The frame rectangle, in 0.01 mm, is the picture's size; a bigger extent lays the
        // equation out larger, so the metafile frame grows with it.
        let frame = |emf: &[u8], at: usize| i32::from_le_bytes(emf[at..at + 4].try_into().unwrap());
        assert!(
            frame(&large.emf, 32) > frame(&small.emf, 32),
            "width did not grow"
        );
        assert!(
            frame(&large.emf, 36) > frame(&small.emf, 36),
            "height did not grow"
        );
    }

    #[test]
    fn asking_for_a_larger_extent_gives_a_larger_bitmap() {
        let math = parse(r"x^2").unwrap();
        let small = presentation(&math, 18.0, None).unwrap();
        let large = presentation(&math, 18.0, Some((3600, 3600))).unwrap();
        assert!(
            large.pixels.0 >= small.pixels.0,
            "{:?} then {:?}",
            small.pixels,
            large.pixels
        );
    }
}
