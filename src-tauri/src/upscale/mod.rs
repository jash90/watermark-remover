pub mod models;
pub mod tiling;
#[cfg(ai_upscale)]
pub mod onnx;

use image::imageops::{self, FilterType};
use image::{DynamicImage, GenericImageView, RgbImage};
use serde::{Deserialize, Serialize};
use std::path::Path;
use std::sync::atomic::{AtomicBool, Ordering};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Scale {
    X2,
    X4,
    X8,
}

impl Scale {
    pub fn factor(self) -> u32 {
        match self {
            Scale::X2 => 2,
            Scale::X4 => 4,
            Scale::X8 => 8,
        }
    }
}

/// One step of an upscale: the native ×4 model, or a Lanczos ×0.5 downscale.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Pass {
    Model,
    Half,
}

/// ×2 = ×4 then ×0.5 (like Real-ESRGAN's --outscale); ×8 = ×4, ×0.5, ×4, so the
/// second model pass runs on 4× the input pixels instead of 16×.
pub fn passes(scale: Scale) -> Vec<Pass> {
    match scale {
        Scale::X2 => vec![Pass::Model, Pass::Half],
        Scale::X4 => vec![Pass::Model],
        Scale::X8 => vec![Pass::Model, Pass::Half, Pass::Model],
    }
}

pub const MAX_OUTPUT_PIXELS: u64 = 16384 * 16384;

pub fn output_size(w: u32, h: u32, scale: Scale) -> Result<(u32, u32), String> {
    let (ow, oh) = (w as u64 * scale.factor() as u64, h as u64 * scale.factor() as u64);
    if ow * oh > MAX_OUTPUT_PIXELS || ow > u32::MAX as u64 || oh > u32::MAX as u64 {
        return Err(format!(
            "The result would be {}x{} ({} MP). The limit is 256 MP. Choose a smaller scale.",
            ow,
            oh,
            ow * oh / 1_000_000
        ));
    }
    Ok((ow as u32, oh as u32))
}

pub const TILE: u32 = 256;
pub const TILE_PAD: u32 = 24;

#[derive(Debug, Clone, Copy, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct UpscaleProgress {
    pub pass: usize,
    pub passes: usize,
    pub done: usize,
    pub total: usize,
}

#[cfg(ai_upscale)]
type Model = onnx::OnnxUpscaler;

/// Stand-in on targets without ONNX Runtime; never constructed.
#[cfg(not(ai_upscale))]
struct Model;

#[cfg(not(ai_upscale))]
impl Model {
    fn run(&mut self, _: &RgbImage) -> Result<RgbImage, String> {
        unreachable!("no AI model on this target")
    }
}

#[cfg(ai_upscale)]
fn load_model(path: Option<&Path>) -> Result<Option<Model>, String> {
    path.map(onnx::OnnxUpscaler::load).transpose()
}

#[cfg(not(ai_upscale))]
fn load_model(_path: Option<&Path>) -> Result<Option<Model>, String> {
    Ok(None)
}

/// Upscales RGB with the ×4 model (or Lanczos when `model_path` is None or AI is
/// unavailable) following `passes(scale)`; alpha is upscaled with Lanczos3.
pub fn run(
    img: &DynamicImage,
    scale: Scale,
    model_path: Option<&Path>,
    cancel: &AtomicBool,
    progress: &mut dyn FnMut(UpscaleProgress),
) -> Result<DynamicImage, String> {
    let (w, h) = img.dimensions();
    let (ow, oh) = output_size(w, h, scale)?;
    let plan = passes(scale);
    let mut model = load_model(model_path)?;

    let mut rgb: RgbImage = img.to_rgb8();
    for (i, pass) in plan.iter().enumerate() {
        if cancel.load(Ordering::SeqCst) {
            return Err("Cancelled".to_string());
        }
        rgb = match (pass, model.as_mut()) {
            (Pass::Half, _) => imageops::resize(&rgb, rgb.width() / 2, rgb.height() / 2, FilterType::Lanczos3),
            (Pass::Model, Some(m)) => {
                let (pass_no, total_passes) = (i + 1, plan.len());
                tiling::upscale_tiled(
                    &rgb,
                    4,
                    TILE,
                    TILE_PAD,
                    &mut |t| m.run(t),
                    &mut |done, total| progress(UpscaleProgress { pass: pass_no, passes: total_passes, done, total }),
                    cancel,
                )?
            }
            (Pass::Model, None) => imageops::resize(&rgb, rgb.width() * 4, rgb.height() * 4, FilterType::Lanczos3),
        };
    }
    debug_assert_eq!(rgb.dimensions(), (ow, oh));

    if !img.color().has_alpha() {
        return Ok(DynamicImage::ImageRgb8(rgb));
    }
    let alpha = imageops::resize(&img.to_rgba8(), ow, oh, FilterType::Lanczos3);
    Ok(DynamicImage::ImageRgba8(image::RgbaImage::from_fn(ow, oh, |x, y| {
        let p = rgb.get_pixel(x, y);
        image::Rgba([p[0], p[1], p[2], alpha.get_pixel(x, y)[3]])
    })))
}

#[cfg(test)]
mod tests {
    use super::*;
    use image::{DynamicImage, Rgba, RgbaImage};
    use std::sync::atomic::AtomicBool;

    #[test]
    fn classic_x2_doubles_size() {
        let img = DynamicImage::ImageRgb8(image::RgbImage::new(30, 20));
        let out = run(&img, Scale::X2, None, &AtomicBool::new(false), &mut |_| {}).unwrap();
        assert_eq!((out.width(), out.height()), (60, 40));
    }

    #[test]
    fn upscale_keeps_alpha() {
        let img = DynamicImage::ImageRgba8(RgbaImage::from_fn(16, 16, |x, _| Rgba([200, 10, 10, if x < 8 { 0 } else { 255 }])));
        let out = run(&img, Scale::X4, None, &AtomicBool::new(false), &mut |_| {}).unwrap().to_rgba8();
        assert_eq!(out.get_pixel(2, 30)[3], 0);
        assert_eq!(out.get_pixel(60, 30)[3], 255);
    }

    #[cfg(ai_upscale)]
    #[test]
    fn ai_x8_reports_three_passes_and_right_size() {
        let path = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("resources/models/realesr-general-x4v3.onnx");
        let img = DynamicImage::ImageRgb8(image::RgbImage::from_pixel(24, 16, image::Rgb([90, 120, 150])));
        let mut passes_seen = std::collections::BTreeSet::new();
        let out = run(&img, Scale::X8, Some(&path), &AtomicBool::new(false), &mut |p| {
            passes_seen.insert(p.pass);
        })
        .unwrap();
        assert_eq!((out.width(), out.height()), (192, 128));
        assert_eq!(passes_seen.into_iter().collect::<Vec<_>>(), vec![1, 3]);
    }

    #[test]
    fn pass_plans() {
        assert_eq!(passes(Scale::X2), vec![Pass::Model, Pass::Half]);
        assert_eq!(passes(Scale::X4), vec![Pass::Model]);
        assert_eq!(passes(Scale::X8), vec![Pass::Model, Pass::Half, Pass::Model]);
    }

    #[test]
    fn output_sizes() {
        assert_eq!(output_size(1600, 900, Scale::X2).unwrap(), (3200, 1800));
        assert_eq!(output_size(1600, 900, Scale::X8).unwrap(), (12800, 7200));
    }

    #[test]
    fn refuses_outputs_over_256_mp() {
        let err = output_size(4000, 3000, Scale::X8).unwrap_err();
        assert!(err.contains("32000x24000"), "{err}");
    }
}
