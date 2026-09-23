use super::composite::composite;
use super::engine::Engine;
use super::geometry::{clamp_region, context_margin, expand};
use super::mask::{dilate, rect_mask, HOLE};
use super::refine::refine_mask;
use crate::image_processor::WatermarkRegion;
use image::imageops::{self, FilterType};
use image::{DynamicImage, GenericImageView, GrayImage, Luma, Rgba, RgbaImage, RgbImage};

pub struct PipelineOptions {
    pub refine_mask: bool,
    /// Pixels added around the mask to catch anti-aliased watermark edges.
    pub grow: u8,
    /// Width of the blended seam between the patch and the original.
    pub feather: u8,
}

pub struct PipelineOutput {
    pub image: DynamicImage,
    pub mask_refined: bool,
}

/// Backend-independent removal: mask → crop → engine → composite onto the original.
/// CPU-bound; run it via `spawn_blocking` from async code.
pub fn run(
    original: &DynamicImage,
    region: &WatermarkRegion,
    opts: &PipelineOptions,
    engine: &Engine,
) -> Result<PipelineOutput, String> {
    let (w, h) = original.dimensions();
    let hole = clamp_region(region, w, h)?;
    let crop = expand(hole, context_margin(hole), w, h);
    let mut rgb = original.to_rgb8();
    let crop_img = imageops::crop_imm(&rgb, crop.x, crop.y, crop.w, crop.h).to_image();
    // Fully transparent pixels hide arbitrary RGB (often black); never copy them into the fill.
    let ignore = original.color().has_alpha().then(|| {
        let rgba = original.to_rgba8();
        GrayImage::from_fn(crop.w, crop.h, |x, y| {
            Luma([if rgba.get_pixel(crop.x + x, crop.y + y)[3] == 0 { HOLE } else { 0 }])
        })
    });

    let rect = rect_mask(crop, hole);
    let (mask, mask_refined) = if opts.refine_mask {
        let r = refine_mask(&crop_img, &rect, opts.grow, ignore.as_ref())?;
        (r.mask, r.used_refinement)
    } else {
        (dilate(&rect, opts.grow), false)
    };

    let patch = engine.inpaint(&crop_img, &mask, ignore.as_ref())?;
    let patch = if patch.dimensions() == (crop.w, crop.h) {
        patch
    } else {
        imageops::resize(&patch, crop.w, crop.h, FilterType::Lanczos3)
    };
    composite(&mut rgb, &patch, crop, &mask, opts.feather);

    Ok(PipelineOutput { image: restore_alpha(original, rgb), mask_refined })
}

fn restore_alpha(original: &DynamicImage, rgb: RgbImage) -> DynamicImage {
    if !original.color().has_alpha() {
        return DynamicImage::ImageRgb8(rgb);
    }
    let alpha = original.to_rgba8();
    DynamicImage::ImageRgba8(RgbaImage::from_fn(rgb.width(), rgb.height(), |x, y| {
        let p = rgb.get_pixel(x, y);
        Rgba([p[0], p[1], p[2], alpha.get_pixel(x, y)[3]])
    }))
}

#[cfg(test)]
mod tests {
    use super::*;
    use image::Rgb;

    fn opts() -> PipelineOptions {
        PipelineOptions { refine_mask: false, grow: 0, feather: 2 }
    }

    fn region(x: i32, y: i32, width: i32, height: i32) -> WatermarkRegion {
        WatermarkRegion { x, y, width, height }
    }

    #[test]
    fn transparent_neighbours_do_not_darken_the_fill() {
        // Left half fully transparent (black RGB), right half opaque green, hole straddling the edge.
        let src = DynamicImage::ImageRgba8(RgbaImage::from_fn(80, 40, |x, _| {
            if x < 40 { Rgba([0, 0, 0, 0]) } else { Rgba([20, 180, 40, 255]) }
        }));
        let out = run(&src, &region(36, 15, 12, 10), &opts(), &Engine::Telea { radius: 5 }).unwrap();
        let p = out.image.to_rgba8();
        assert_eq!(p.get_pixel(44, 20), &Rgba([20, 180, 40, 255]));
        assert_eq!(p.get_pixel(38, 20)[3], 0, "transparent pixels stay transparent");
    }

    #[test]
    fn pipeline_keeps_size_and_untouched_pixels() {
        let src = DynamicImage::ImageRgb8(RgbImage::from_fn(300, 200, |x, y| Rgb([x as u8, y as u8, 7])));
        let out = run(&src, &region(100, 80, 40, 20), &opts(), &Engine::Fill([255, 0, 255])).unwrap();
        assert_eq!(out.image.dimensions(), (300, 200));
        let (a, b) = (src.to_rgb8(), out.image.to_rgb8());
        assert_eq!(b.get_pixel(120, 90), &Rgb([255, 0, 255]));
        assert_eq!(b.get_pixel(10, 10), a.get_pixel(10, 10));
        assert_eq!(b.get_pixel(299, 199), a.get_pixel(299, 199));
        assert_eq!(b.get_pixel(100 - 7, 90), a.get_pixel(100 - 7, 90));
    }

    #[test]
    fn pipeline_preserves_alpha() {
        let src = DynamicImage::ImageRgba8(RgbaImage::from_pixel(64, 64, Rgba([10, 20, 30, 77])));
        let out = run(&src, &region(20, 20, 10, 10), &opts(), &Engine::Fill([0, 0, 0])).unwrap();
        assert!(out.image.color().has_alpha());
        assert_eq!(out.image.to_rgba8().get_pixel(25, 25)[3], 77);
    }

    #[test]
    fn pipeline_clamps_region_for_smaller_image() {
        let src = DynamicImage::ImageRgb8(RgbImage::new(50, 50));
        let out = run(&src, &region(40, 40, 30, 30), &opts(), &Engine::Fill([9, 9, 9])).unwrap();
        assert_eq!(out.image.to_rgb8().get_pixel(49, 49), &Rgb([9, 9, 9]));
        assert!(run(&src, &region(60, 60, 5, 5), &opts(), &Engine::Fill([9, 9, 9])).is_err());
    }

    #[test]
    fn pipeline_with_telea_removes_stroke_on_flat_background() {
        let mut img = RgbImage::from_pixel(120, 80, Rgb([70, 140, 200]));
        for x in 40..80 {
            for y in 39..41 {
                img.put_pixel(x, y, Rgb([255, 255, 255]));
            }
        }
        let src = DynamicImage::ImageRgb8(img);
        let o = PipelineOptions { refine_mask: true, grow: 1, feather: 2 };
        let out = run(&src, &region(35, 30, 50, 20), &o, &Engine::Telea { radius: 5 }).unwrap();
        assert!(out.mask_refined);
        assert_eq!(out.image.to_rgb8().get_pixel(60, 40), &Rgb([70, 140, 200]));
    }
}
