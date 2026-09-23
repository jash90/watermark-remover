use super::geometry::Rect;
use super::mask::{dilate, HOLE};
use image::{GrayImage, RgbImage};
use imageproc::filter::gaussian_blur_f32;

/// Blends `patch` into `base` at `crop`. HOLE pixels are fully replaced,
/// a ~`feather` px band around them is blended, the rest stays byte-identical.
pub fn composite(base: &mut RgbImage, patch: &RgbImage, crop: Rect, mask: &GrayImage, feather: u8) {
    assert_eq!(patch.dimensions(), (crop.w, crop.h), "patch must be crop-sized");
    assert_eq!(mask.dimensions(), (crop.w, crop.h), "mask must be crop-sized");
    let soft = (feather > 0).then(|| gaussian_blur_f32(&dilate(mask, feather), feather as f32 / 2.0));
    for (x, y, m) in mask.enumerate_pixels() {
        let a = match (&soft, m[0] == HOLE) {
            (_, true) => 1.0,
            (Some(s), false) => s.get_pixel(x, y)[0] as f32 / 255.0,
            (None, false) => 0.0,
        };
        if a <= 0.0 {
            continue;
        }
        let src = patch.get_pixel(x, y);
        let dst = base.get_pixel_mut(crop.x + x, crop.y + y);
        for c in 0..3 {
            dst[c] = (src[c] as f32 * a + dst[c] as f32 * (1.0 - a)).round() as u8;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::removal::mask::rect_mask;
    use image::Rgb;

    #[test]
    fn replaces_hole_and_keeps_far_pixels_identical() {
        let mut base = RgbImage::from_pixel(100, 100, Rgb([10, 10, 10]));
        let crop = Rect { x: 20, y: 20, w: 60, h: 60 };
        let hole = Rect { x: 45, y: 45, w: 10, h: 10 };
        let mask = rect_mask(crop, hole);
        let patch = RgbImage::from_pixel(60, 60, Rgb([200, 200, 200]));
        composite(&mut base, &patch, crop, &mask, 3);
        assert_eq!(base.get_pixel(50, 50), &Rgb([200, 200, 200]));
        assert_eq!(base.get_pixel(45, 45), &Rgb([200, 200, 200]));
        // 3 * feather = 9 px away from the hole: untouched
        assert_eq!(base.get_pixel(45 - 10, 50), &Rgb([10, 10, 10]));
        assert_eq!(base.get_pixel(0, 0), &Rgb([10, 10, 10]));
        // just outside the hole: blended, strictly between
        let edge = base.get_pixel(44, 50)[0];
        assert!(edge > 10 && edge < 200, "edge {edge}");
    }

    #[test]
    fn zero_feather_is_a_hard_cut() {
        let mut base = RgbImage::from_pixel(20, 20, Rgb([0, 0, 0]));
        let crop = Rect { x: 0, y: 0, w: 20, h: 20 };
        let mask = rect_mask(crop, Rect { x: 5, y: 5, w: 5, h: 5 });
        composite(&mut base, &RgbImage::from_pixel(20, 20, Rgb([255, 0, 0])), crop, &mask, 0);
        assert_eq!(base.get_pixel(4, 7), &Rgb([0, 0, 0]));
        assert_eq!(base.get_pixel(5, 7), &Rgb([255, 0, 0]));
    }
}
