//! Anti-aliased scanline rasterizer.
//!
//! A glyph or rule is flattened to closed polygons and filled with the non-zero winding rule.
//! Coverage is accumulated with vertical supersampling: every pixel row is sampled on a fixed
//! number of sub-scanlines, each sub-scanline yields the spans that are inside the outline, and a
//! span contributes its horizontal overlap with each pixel column. The resulting coverage buffer
//! holds a value in `[0, 1]` per pixel.

/// Number of sub-scanlines per pixel row. Four is enough for the coverage values the tests look
/// at and keeps the cost proportional to the ink area rather than the image area.
const SUB_ROWS: usize = 4;

/// Coverage of an axis-aligned region of the image, plus the pixel offset it sits at.
pub(crate) struct Mask {
    /// Left edge of the mask, in image pixels.
    pub(crate) x0: i32,
    /// Top edge of the mask, in image pixels.
    pub(crate) y0: i32,
    /// Width of the mask, in pixels.
    pub(crate) w: u32,
    /// Height of the mask, in pixels.
    pub(crate) h: u32,
    /// Row-major coverage values in `[0, 1]`, `w * h` of them.
    pub(crate) cov: Vec<f64>,
}

/// Rasterize `polys` into coverage clipped to a `clip_w` by `clip_h` image.
///
/// Points are in image pixel coordinates. Returns `None` when the polygons are empty or fall
/// entirely outside the clip rectangle.
pub(crate) fn rasterize(polys: &[Vec<[f64; 2]>], clip_w: u32, clip_h: u32) -> Option<Mask> {
    let (mut min_x, mut min_y) = (f64::INFINITY, f64::INFINITY);
    let (mut max_x, mut max_y) = (f64::NEG_INFINITY, f64::NEG_INFINITY);
    for poly in polys {
        for p in poly {
            min_x = min_x.min(p[0]);
            min_y = min_y.min(p[1]);
            max_x = max_x.max(p[0]);
            max_y = max_y.max(p[1]);
        }
    }
    if !min_x.is_finite() || !min_y.is_finite() {
        return None;
    }

    let cx0 = (min_x.floor() as i64).max(0);
    let cy0 = (min_y.floor() as i64).max(0);
    let cx1 = (max_x.ceil() as i64).min(clip_w as i64);
    let cy1 = (max_y.ceil() as i64).min(clip_h as i64);
    if cx1 <= cx0 || cy1 <= cy0 {
        return None;
    }
    let w = (cx1 - cx0) as usize;
    let h = (cy1 - cy0) as usize;
    let mut cov = vec![0.0f64; w * h];

    let mut crossings: Vec<(f64, i32)> = Vec::new();
    let weight = 1.0 / SUB_ROWS as f64;
    for row in 0..h {
        let row_top = cy0 as f64 + row as f64;
        let row_cov = &mut cov[row * w..row * w + w];
        for sub in 0..SUB_ROWS {
            let sy = row_top + (sub as f64 + 0.5) * weight;
            crossings.clear();
            for poly in polys {
                let n = poly.len();
                if n < 2 {
                    continue;
                }
                for i in 0..n {
                    let a = poly[i];
                    let b = poly[(i + 1) % n];
                    let (ay, by) = (a[1], b[1]);
                    let dir = if ay <= sy && sy < by {
                        1
                    } else if by <= sy && sy < ay {
                        -1
                    } else {
                        continue;
                    };
                    let x = a[0] + (sy - ay) * (b[0] - a[0]) / (by - ay);
                    crossings.push((x, dir));
                }
            }
            if crossings.is_empty() {
                continue;
            }
            crossings.sort_by(|p, q| p.0.partial_cmp(&q.0).unwrap_or(std::cmp::Ordering::Equal));
            let mut winding = 0i32;
            let mut prev = 0.0;
            for &(x, dir) in crossings.iter() {
                if winding != 0 {
                    add_span(row_cov, prev - cx0 as f64, x - cx0 as f64, weight);
                }
                winding += dir;
                prev = x;
            }
        }
    }

    Some(Mask { x0: cx0 as i32, y0: cy0 as i32, w: w as u32, h: h as u32, cov })
}

/// Add the horizontal coverage of the span `xa .. xb` (in mask-local pixel coordinates) to one
/// coverage row, weighting it by `weight` (the share of one sub-scanline).
fn add_span(row: &mut [f64], xa: f64, xb: f64, weight: f64) {
    let w = row.len() as f64;
    let a = xa.max(0.0);
    let b = xb.min(w);
    if b <= a {
        return;
    }
    let start = a.floor() as usize;
    let end = b.ceil() as usize;
    for column in start..end.min(row.len()) {
        let left = column as f64;
        let right = left + 1.0;
        let overlap = b.min(right) - a.max(left);
        if overlap > 0.0 {
            row[column] += overlap * weight;
        }
    }
}
