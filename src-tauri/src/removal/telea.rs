use super::mask::HOLE;
use image::{GrayImage, Rgb, RgbImage};
use std::cmp::Reverse;
use std::collections::BinaryHeap;

const KNOWN: u8 = 0;
const BAND: u8 = 1;
const INSIDE: u8 = 2;
/// Not a source and not filled (e.g. fully transparent pixels).
const IGNORED: u8 = 3;
const SURROUNDED_BY_TRANSPARENT: &str =
    "The selected area is surrounded by transparent pixels, so there is nothing to fill it from";
const INF: f32 = 1.0e6;
const NEIGHBOURS: [(i64, i64); 4] = [(1, 0), (-1, 0), (0, 1), (0, -1)];

struct Grid {
    w: i64,
    h: i64,
    flags: Vec<u8>,
    t: Vec<f32>,
}

impl Grid {
    fn idx(&self, x: i64, y: i64) -> Option<usize> {
        (x >= 0 && y >= 0 && x < self.w && y < self.h).then(|| (y * self.w + x) as usize)
    }

    /// T at (x, y) if it is final or on the front (KNOWN or BAND).
    fn settled_t(&self, x: i64, y: i64) -> Option<f32> {
        self.idx(x, y).filter(|&k| self.flags[k] == KNOWN || self.flags[k] == BAND).map(|k| self.t[k])
    }

    /// Eikonal update from two orthogonal neighbours; mirrors OpenCV's
    /// `FastMarching_solve` (KNOWN and BAND neighbours both contribute).
    fn solve(&self, a: (i64, i64), b: (i64, i64)) -> f32 {
        match (self.settled_t(a.0, a.1), self.settled_t(b.0, b.1)) {
            (Some(t1), Some(t2)) if (t1 - t2).abs() >= 1.0 => 1.0 + t1.min(t2),
            (Some(t1), Some(t2)) => (t1 + t2 + (2.0 - (t1 - t2) * (t1 - t2)).sqrt()) / 2.0,
            (Some(t1), None) => 1.0 + t1,
            (None, Some(t2)) => 1.0 + t2,
            (None, None) => INF,
        }
    }

    fn grad(&self, x: i64, y: i64) -> (f32, f32) {
        let tp = self.t[(y * self.w + x) as usize];
        let axis = |fwd: Option<f32>, back: Option<f32>| match (fwd, back) {
            (Some(a), Some(b)) => (a - b) / 2.0,
            (Some(a), None) => a - tp,
            (None, Some(b)) => tp - b,
            (None, None) => 0.0,
        };
        (
            axis(self.settled_t(x + 1, y), self.settled_t(x - 1, y)),
            axis(self.settled_t(x, y + 1), self.settled_t(x, y - 1)),
        )
    }
}

/// Fast-marching inpainting (Telea 2004) without the image-gradient term.
/// Fills every `HOLE` pixel of `mask` from the known pixels within `radius`.
pub fn inpaint(img: &RgbImage, mask: &GrayImage, radius: u32) -> Result<RgbImage, String> {
    inpaint_with_ignore(img, mask, None, radius)
}

/// Like `inpaint`, but pixels marked HOLE in `ignore` (e.g. fully transparent)
/// are neither used as sources nor filled.
pub fn inpaint_with_ignore(
    img: &RgbImage,
    mask: &GrayImage,
    ignore: Option<&GrayImage>,
    radius: u32,
) -> Result<RgbImage, String> {
    let (w, h) = img.dimensions();
    if mask.dimensions() != (w, h) || ignore.is_some_and(|i| i.dimensions() != (w, h)) {
        return Err("Mask size does not match image size".to_string());
    }
    let mut g = Grid {
        w: w as i64,
        h: h as i64,
        flags: vec![KNOWN; (w * h) as usize],
        t: vec![0.0; (w * h) as usize],
    };
    for (x, y, m) in mask.enumerate_pixels() {
        if m[0] == HOLE {
            let i = (y * w + x) as usize;
            g.flags[i] = INSIDE;
            g.t[i] = INF;
        }
    }
    if let Some(ignore) = ignore {
        for (x, y, m) in ignore.enumerate_pixels() {
            if m[0] == HOLE {
                let i = (y * w + x) as usize;
                g.flags[i] = IGNORED;
                g.t[i] = INF;
            }
        }
    }
    if !g.flags.contains(&KNOWN) {
        return Err(if ignore.is_some() {
            SURROUNDED_BY_TRANSPARENT.to_string()
        } else {
            "Nothing to inpaint from: the mask covers the whole area".to_string()
        });
    }

    let mut px: Vec<[f32; 3]> = img.pixels().map(|p| [p[0] as f32, p[1] as f32, p[2] as f32]).collect();
    let mut heap = BinaryHeap::new();
    for y in 0..g.h {
        for x in 0..g.w {
            let i = (y * g.w + x) as usize;
            let touches_hole = NEIGHBOURS
                .iter()
                .any(|(dx, dy)| g.idx(x + dx, y + dy).is_some_and(|k| g.flags[k] == INSIDE));
            if g.flags[i] == KNOWN && touches_hole {
                g.flags[i] = BAND;
                heap.push(Reverse((0.0f32.to_bits(), i)));
            }
        }
    }

    let r = radius as i64;
    while let Some(Reverse((_, i))) = heap.pop() {
        g.flags[i] = KNOWN;
        let (x, y) = (i as i64 % g.w, i as i64 / g.w);
        for (dx, dy) in NEIGHBOURS {
            let (nx, ny) = (x + dx, y + dy);
            let Some(j) = g.idx(nx, ny) else { continue };
            if g.flags[j] != INSIDE {
                continue;
            }
            g.t[j] = [(-1, -1), (1, -1), (-1, 1), (1, 1)]
                .iter()
                .map(|(sx, sy)| g.solve((nx + sx, ny), (nx, ny + sy)))
                .fold(INF, f32::min);
            g.flags[j] = BAND;
            px[j] = fill_pixel(&g, &px, nx, ny, r);
            heap.push(Reverse((g.t[j].to_bits(), j)));
        }
    }

    // Hole pixels the front never reached are cut off from every source by
    // ignored (transparent) pixels; returning them unchanged would look like success.
    if g.flags.contains(&INSIDE) {
        return Err(SURROUNDED_BY_TRANSPARENT.to_string());
    }

    Ok(RgbImage::from_fn(w, h, |x, y| {
        let p = px[(y * w + x) as usize];
        Rgb([p[0].round() as u8, p[1].round() as u8, p[2].round() as u8])
    }))
}

fn fill_pixel(g: &Grid, px: &[[f32; 3]], x: i64, y: i64, r: i64) -> [f32; 3] {
    let i = (y * g.w + x) as usize;
    let (gx, gy) = g.grad(x, y);
    let mut acc = [0.0f32; 3];
    let mut wsum = 0.0f32;
    for qy in (y - r).max(0)..=(y + r).min(g.h - 1) {
        for qx in (x - r).max(0)..=(x + r).min(g.w - 1) {
            let k = (qy * g.w + qx) as usize;
            if k == i || g.flags[k] == INSIDE || g.flags[k] == IGNORED {
                continue;
            }
            let (rx, ry) = ((x - qx) as f32, (y - qy) as f32);
            let d2 = rx * rx + ry * ry;
            if d2 > (r * r) as f32 {
                continue;
            }
            let mut dir = rx * gx + ry * gy;
            if dir.abs() <= 0.01 {
                dir = 1.0e-6;
            }
            let dst = 1.0 / (d2 * d2.sqrt());
            let lev = 1.0 / (1.0 + (g.t[k] - g.t[i]).abs());
            let wgt = (dir * dst * lev).abs();
            for c in 0..3 {
                acc[c] += wgt * px[k][c];
            }
            wsum += wgt;
        }
    }
    if wsum > 0.0 {
        acc.map(|a| a / wsum)
    } else {
        px[i]
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use image::Luma;

    fn hole_mask(w: u32, h: u32, x0: u32, y0: u32, hw: u32, hh: u32) -> GrayImage {
        GrayImage::from_fn(w, h, |x, y| {
            let inside = x >= x0 && x < x0 + hw && y >= y0 && y < y0 + hh;
            Luma([if inside { HOLE } else { 0 }])
        })
    }

    #[test]
    fn reconstructs_flat_colour_exactly() {
        let img = RgbImage::from_pixel(40, 30, Rgb([120, 80, 200]));
        let mut damaged = img.clone();
        for y in 10..20 {
            for x in 12..28 {
                damaged.put_pixel(x, y, Rgb([255, 255, 255]));
            }
        }
        let out = inpaint(&damaged, &hole_mask(40, 30, 12, 10, 16, 10), 5).unwrap();
        assert_eq!(out, img);
    }

    #[test]
    fn approximates_horizontal_gradient() {
        // Loose bounds on purpose. Quality is measured by examples/eval.rs, not here.
        let img = RgbImage::from_fn(100, 40, |x, _| {
            let v = (x as f32 * 2.55) as u8;
            Rgb([v, v, v])
        });
        let mut damaged = img.clone();
        for y in 15..25 {
            for x in 45..55 {
                damaged.put_pixel(x, y, Rgb([0, 0, 0]));
            }
        }
        let out = inpaint(&damaged, &hole_mask(100, 40, 45, 15, 10, 10), 5).unwrap();
        let mut total = 0u32;
        for y in 15..25 {
            for x in 45..55 {
                let d = (out.get_pixel(x, y)[0] as i32 - img.get_pixel(x, y)[0] as i32).unsigned_abs();
                assert!(d <= 25, "pixel ({x},{y}) off by {d}");
                total += d;
            }
        }
        assert!(total as f32 / 100.0 < 8.0, "mean error {}", total as f32 / 100.0);
    }

    #[test]
    fn leaves_known_pixels_untouched() {
        let img = RgbImage::from_fn(30, 30, |x, y| Rgb([(x * 8) as u8, (y * 8) as u8, 50]));
        let mask = hole_mask(30, 30, 10, 10, 5, 5);
        let out = inpaint(&img, &mask, 5).unwrap();
        for (x, y, p) in out.enumerate_pixels() {
            if mask.get_pixel(x, y)[0] != HOLE {
                assert_eq!(p, img.get_pixel(x, y));
            }
        }
    }

    #[test]
    fn fills_hole_touching_border() {
        let img = RgbImage::from_pixel(20, 20, Rgb([10, 20, 30]));
        let out = inpaint(&img, &hole_mask(20, 20, 0, 0, 6, 20), 5).unwrap();
        assert_eq!(out.get_pixel(0, 0), &Rgb([10, 20, 30]));
    }

    #[test]
    fn rejects_mask_covering_everything() {
        let img = RgbImage::new(8, 8);
        assert!(inpaint(&img, &hole_mask(8, 8, 0, 0, 8, 8), 5).is_err());
    }

    fn transparent_left(w: u32, h: u32, cols: u32) -> GrayImage {
        GrayImage::from_fn(w, h, |x, _| Luma([if x < cols { HOLE } else { 0 }]))
    }

    #[test]
    fn ignored_pixels_are_not_fill_sources() {
        // Left 8 columns: transparent (RGB black underneath). Right: opaque red.
        let img = RgbImage::from_fn(24, 12, |x, _| if x < 8 { Rgb([0, 0, 0]) } else { Rgb([200, 0, 0]) });
        let mask = hole_mask(24, 12, 8, 3, 4, 6);
        let out = inpaint_with_ignore(&img, &mask, Some(&transparent_left(24, 12, 8)), 5).unwrap();
        for y in 3..9 {
            for x in 8..12 {
                assert_eq!(out.get_pixel(x, y), &Rgb([200, 0, 0]), "({x},{y}) picked up transparent black");
            }
        }
    }

    #[test]
    fn hole_inside_transparent_area_is_left_alone() {
        let img = RgbImage::from_pixel(12, 12, Rgb([7, 8, 9]));
        let mask = hole_mask(12, 12, 2, 2, 3, 3);
        let ignore = GrayImage::from_pixel(12, 12, Luma([HOLE]));
        // Nothing known anywhere → error, not a panic.
        assert!(inpaint_with_ignore(&img, &mask, Some(&ignore), 5).is_err());
    }

    #[test]
    fn hole_walled_off_by_transparent_ring_is_an_error_not_a_silent_no_op() {
        // Opaque 10x10 logo (the hole) inside a transparent ring, opaque background outside.
        let img = RgbImage::from_pixel(30, 30, Rgb([40, 90, 140]));
        let inner = |x: u32, y: u32| (10..20).contains(&x) && (10..20).contains(&y);
        let ring = |x: u32, y: u32| (6..24).contains(&x) && (6..24).contains(&y) && !inner(x, y);
        let mask = GrayImage::from_fn(30, 30, |x, y| Luma([if inner(x, y) { HOLE } else { 0 }]));
        let ignore = GrayImage::from_fn(30, 30, |x, y| Luma([if ring(x, y) { HOLE } else { 0 }]));
        let err = inpaint_with_ignore(&img, &mask, Some(&ignore), 5).unwrap_err();
        assert!(err.contains("surrounded by transparent"), "{err}");
    }

    #[test]
    fn rejects_mismatched_mask() {
        assert!(inpaint(&RgbImage::new(8, 8), &GrayImage::new(4, 4), 5).is_err());
    }
}
