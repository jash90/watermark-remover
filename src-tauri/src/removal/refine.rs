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
    let strokes = GrayImage::from_fn(w, h, |x, y| Luma([if diff.get_pixel(x, y)[0] > level { HOLE } else { 0 }]));
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
