use super::mask::{dilate, hole_count, HOLE};
use super::telea;
use image::{GrayImage, Luma, RgbImage};
use imageproc::contrast::otsu_level;

/// Differences below this are treated as noise or compression, not watermark.
const MIN_CONTRAST: u8 = 12;
/// If more than this share of the rectangle looks like watermark, keep the rectangle.
const MAX_KEEP_RATIO: f32 = 0.7;

pub struct Refined {
    pub mask: GrayImage,
    pub used_refinement: bool,
}

/// Shrinks a rectangular hole to the pixels that differ from the background
/// predicted from the rectangle's surroundings (watermark strokes).
pub fn refine_mask(crop: &RgbImage, rect_mask: &GrayImage, grow: u8) -> Result<Refined, String> {
    let predicted = telea::inpaint(crop, rect_mask, 5)?;
    let (w, h) = crop.dimensions();
    let diff = GrayImage::from_fn(w, h, |x, y| {
        if rect_mask.get_pixel(x, y)[0] != HOLE {
            return Luma([0]);
        }
        let (a, b) = (crop.get_pixel(x, y), predicted.get_pixel(x, y));
        let d = (0..3).map(|c| (a[c] as i16 - b[c] as i16).unsigned_abs()).max().unwrap_or(0);
        Luma([d.min(255) as u8])
    });
    let inside: Vec<u8> = diff
        .enumerate_pixels()
        .filter(|(x, y, _)| rect_mask.get_pixel(*x, *y)[0] == HOLE)
        .map(|(_, _, p)| p[0])
        .collect();
    let fallback = || Ok(Refined { mask: rect_mask.clone(), used_refinement: false });
    let Some(samples) = GrayImage::from_raw(inside.len() as u32, 1, inside) else { return fallback() };
    let level = otsu_level(&samples).max(MIN_CONTRAST);
    // Anti-aliased stroke edges fall below Otsu's level and would stay as a ghost
    // outline. Accept weaker pixels too, but only where they connect to a strong
    // stroke, and never below twice the typical background difference.
    let low = (level / 2).max(MIN_CONTRAST).max(median(samples.as_raw()).saturating_mul(2));
    let strokes = hysteresis(&diff, level, low);
    let grown = dilate(&strokes, grow);
    let refined = GrayImage::from_fn(w, h, |x, y| {
        let keep = grown.get_pixel(x, y)[0] == HOLE && rect_mask.get_pixel(x, y)[0] == HOLE;
        Luma([if keep { HOLE } else { 0 }])
    });
    let kept = hole_count(&refined);
    if kept == 0 || kept as f32 / hole_count(rect_mask) as f32 > MAX_KEEP_RATIO {
        return fallback();
    }
    Ok(Refined { mask: refined, used_refinement: true })
}

fn median(values: &[u8]) -> u8 {
    let mut histogram = [0usize; 256];
    for &v in values {
        histogram[v as usize] += 1;
    }
    let mut seen = 0;
    for (value, &count) in histogram.iter().enumerate() {
        seen += count;
        if seen * 2 >= values.len() {
            return value as u8;
        }
    }
    0
}

/// Pixels above `high`, plus pixels above `low` 8-connected to them.
fn hysteresis(diff: &GrayImage, high: u8, low: u8) -> GrayImage {
    let (w, h) = diff.dimensions();
    let mut out = GrayImage::new(w, h);
    let mut stack: Vec<(u32, u32)> = diff
        .enumerate_pixels()
        .filter(|(_, _, p)| p[0] > high)
        .map(|(x, y, _)| (x, y))
        .collect();
    for &(x, y) in &stack {
        out.put_pixel(x, y, Luma([HOLE]));
    }
    while let Some((x, y)) = stack.pop() {
        for ny in y.saturating_sub(1)..=(y + 1).min(h - 1) {
            for nx in x.saturating_sub(1)..=(x + 1).min(w - 1) {
                if out.get_pixel(nx, ny)[0] != HOLE && diff.get_pixel(nx, ny)[0] > low {
                    out.put_pixel(nx, ny, Luma([HOLE]));
                    stack.push((nx, ny));
                }
            }
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::removal::geometry::Rect;
    use crate::removal::mask::rect_mask;
    use image::Rgb;

    fn setup() -> (RgbImage, GrayImage) {
        let crop = Rect { x: 0, y: 0, w: 80, h: 60 };
        (RgbImage::from_pixel(80, 60, Rgb([90, 110, 130])), rect_mask(crop, Rect { x: 20, y: 20, w: 40, h: 20 }))
    }

    #[test]
    fn narrows_mask_to_thin_stroke() {
        let (mut img, rect) = setup();
        for x in 25..55 {
            for y in 29..31 {
                img.put_pixel(x, y, Rgb([250, 250, 250]));
            }
        }
        let r = refine_mask(&img, &rect, 1).unwrap();
        assert!(r.used_refinement);
        assert_eq!(r.mask.get_pixel(30, 30)[0], HOLE);
        assert_eq!(r.mask.get_pixel(30, 22)[0], 0, "background inside the rectangle is kept");
        assert!(hole_count(&r.mask) < hole_count(&rect) / 2);
    }

    #[test]
    fn includes_soft_anti_aliased_edges_of_a_stroke() {
        // Bright 2 px core with a long fading halo, like blurred text on a photo.
        const LIFT: [u8; 7] = [150, 115, 85, 60, 42, 28, 18];
        let (mut img, rect) = setup();
        for x in 26..54 {
            for y in 20..40 {
                let d = if y < 30 { 29 - y } else { y - 30 };
                let lift = LIFT.get(d as usize).copied().unwrap_or(0);
                img.put_pixel(x, y, Rgb([90 + lift, 110u8.saturating_add(lift), 130 + lift / 2]));
            }
        }
        let r = refine_mask(&img, &rect, 1).unwrap();
        assert!(r.used_refinement);
        let masked: Vec<u8> = (20..40).map(|y| (r.mask.get_pixel(40, y)[0] == HOLE) as u8).collect();
        // Every ring still lifted by 28+ levels (visible as a ghost) must be masked.
        for y in 20u32..40 {
            let d = if y < 30 { 29 - y } else { y - 30 };
            if LIFT.get(d as usize).is_some_and(|&l| l >= 28) {
                assert_eq!(r.mask.get_pixel(40, y)[0], HOLE, "ring d={d} left unmasked; column: {masked:?}");
            }
        }
    }

    #[test]
    fn falls_back_to_rectangle_for_opaque_block() {
        let (mut img, rect) = setup();
        for x in 20..60 {
            for y in 20..40 {
                img.put_pixel(x, y, Rgb([0, 0, 0]));
            }
        }
        let r = refine_mask(&img, &rect, 1).unwrap();
        assert!(!r.used_refinement);
        assert_eq!(r.mask, rect);
    }
}
