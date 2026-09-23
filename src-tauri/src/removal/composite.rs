use super::geometry::Rect;
use super::mask::{dilate, HOLE};
use image::{DynamicImage, GrayImage, ImageBuffer, Pixel, Primitive, RgbImage};
use imageproc::filter::gaussian_blur_f32;
use num_traits::{NumCast, ToPrimitive};

/// Blend weight per crop pixel: 1.0 inside the hole, a feathered ramp just
/// outside it, 0.0 elsewhere (those pixels are never written).
fn weights(mask: &GrayImage, feather: u8) -> Vec<f32> {
    let soft = (feather > 0).then(|| gaussian_blur_f32(&dilate(mask, feather), feather as f32 / 2.0));
    mask.enumerate_pixels()
        .map(|(x, y, m)| match (&soft, m[0] == HOLE) {
            (_, true) => 1.0,
            (Some(s), false) => s.get_pixel(x, y)[0] as f32 / 255.0,
            (None, false) => 0.0,
        })
        .collect()
}

/// Writes `patch` into the colour channels of `img` (alpha untouched), converting
/// each 8-bit RGB patch pixel with `colour` into the buffer's native scale.
fn blend<P>(
    img: &mut ImageBuffer<P, Vec<P::Subpixel>>,
    patch: &RgbImage,
    crop: Rect,
    weights: &[f32],
    colour: impl Fn([u8; 3]) -> [f32; 3],
) where
    P: Pixel,
    P::Subpixel: Primitive,
{
    let colour_channels = (P::CHANNEL_COUNT - P::HAS_ALPHA as u8) as usize;
    for y in 0..crop.h {
        for x in 0..crop.w {
            let a = weights[(y * crop.w + x) as usize];
            if a <= 0.0 {
                continue;
            }
            let src = colour(patch.get_pixel(x, y).0);
            let dst = img.get_pixel_mut(crop.x + x, crop.y + y);
            for (c, channel) in dst.channels_mut().iter_mut().take(colour_channels).enumerate() {
                let old = channel.to_f32().unwrap_or(0.0);
                *channel = NumCast::from((src[c] * a + old * (1.0 - a)).round()).unwrap_or(*channel);
            }
        }
    }
}

fn rgb(p: [u8; 3]) -> [f32; 3] {
    p.map(|c| c as f32)
}

fn luma(p: [u8; 3]) -> [f32; 3] {
    let l = 0.299 * p[0] as f32 + 0.587 * p[1] as f32 + 0.114 * p[2] as f32;
    [l, l, l]
}

/// Blends `patch` into `base` at `crop`. HOLE pixels are fully replaced,
/// a ~`feather` px band around them is blended, the rest stays byte-identical.
pub fn composite(base: &mut RgbImage, patch: &RgbImage, crop: Rect, mask: &GrayImage, feather: u8) {
    assert_eq!(patch.dimensions(), (crop.w, crop.h), "patch must be crop-sized");
    assert_eq!(mask.dimensions(), (crop.w, crop.h), "mask must be crop-sized");
    blend(base, patch, crop, &weights(mask, feather), rgb);
}

/// Same as `composite`, but writes into the original image in its own colour
/// type and bit depth (greyscale, 16-bit, with or without alpha).
pub fn composite_into(base: &mut DynamicImage, patch: &RgbImage, crop: Rect, mask: &GrayImage, feather: u8) {
    assert_eq!(patch.dimensions(), (crop.w, crop.h), "patch must be crop-sized");
    assert_eq!(mask.dimensions(), (crop.w, crop.h), "mask must be crop-sized");
    let w = weights(mask, feather);
    let wide = |p: [u8; 3]| rgb(p).map(|c| c * 257.0);
    let wide_luma = |p: [u8; 3]| luma(p).map(|c| c * 257.0);
    match base {
        DynamicImage::ImageLuma8(b) => blend(b, patch, crop, &w, luma),
        DynamicImage::ImageLumaA8(b) => blend(b, patch, crop, &w, luma),
        DynamicImage::ImageRgb8(b) => blend(b, patch, crop, &w, rgb),
        DynamicImage::ImageRgba8(b) => blend(b, patch, crop, &w, rgb),
        DynamicImage::ImageLuma16(b) => blend(b, patch, crop, &w, wide_luma),
        DynamicImage::ImageLumaA16(b) => blend(b, patch, crop, &w, wide_luma),
        DynamicImage::ImageRgb16(b) => blend(b, patch, crop, &w, wide),
        DynamicImage::ImageRgba16(b) => blend(b, patch, crop, &w, wide),
        other => {
            // 32-bit float (HDR) inputs aren't accepted by the UI; fall back to 8-bit RGBA.
            let mut rgba = other.to_rgba8();
            blend(&mut rgba, patch, crop, &w, rgb);
            *other = DynamicImage::ImageRgba8(rgba);
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
