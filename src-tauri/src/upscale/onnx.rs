use image::{Rgb, RgbImage};
use ndarray::Array4;
use std::path::Path;

/// Real-ESRGAN ×4 model exported by scripts/export_upscalers.py:
/// input "input" [1,3,H,W] RGB in [0,1] → output "output" [1,3,4H,4W].
pub struct OnnxUpscaler {
    session: ort::session::Session,
}

impl OnnxUpscaler {
    pub fn load(path: &Path) -> Result<Self, String> {
        let builder = ort::session::Session::builder().map_err(|e| format!("ONNX Runtime init failed: {e}"))?;
        // MLProgram on CPU+GPU with a compiled-model cache (the Neural Engine rejects the
        // dynamic input shape and breaks fp32 parity):
        // 1024² → 4096² in ~4.3 s on Apple Silicon vs ~14 s on CPU (default CoreML was ~28 s).
        #[cfg(all(target_os = "macos", target_arch = "aarch64"))]
        let builder = {
            use ort::ep::coreml::{ComputeUnits, ModelFormat};
            let cache = std::env::temp_dir().join("watermark-remover").join("coreml-cache");
            let _ = std::fs::create_dir_all(&cache);
            builder
                .with_execution_providers([ort::ep::CoreML::default()
                    .with_model_format(ModelFormat::MLProgram)
                    .with_compute_units(ComputeUnits::CPUAndGPU)
                    .with_model_cache_dir(cache.to_string_lossy())
                    .build()])
                .map_err(|e| format!("CoreML setup failed: {e}"))?
        };
        #[cfg(windows)]
        let builder = builder
            .with_execution_providers([ort::ep::DirectML::default().build()])
            .map_err(|e| format!("DirectML setup failed: {e}"))?;
        let mut builder = builder;
        let session = builder
            .commit_from_file(path)
            .map_err(|e| format!("Failed to load upscaling model: {e}"))?;
        Ok(Self { session })
    }

    pub fn run(&mut self, img: &RgbImage) -> Result<RgbImage, String> {
        let (w, h) = img.dimensions();
        let mut input = Array4::<f32>::zeros((1, 3, h as usize, w as usize));
        for (x, y, p) in img.enumerate_pixels() {
            for c in 0..3 {
                input[[0, c, y as usize, x as usize]] = p[c] as f32 / 255.0;
            }
        }
        let tensor = ort::value::TensorRef::from_array_view(input.view()).map_err(|e| e.to_string())?;
        let outputs = self
            .session
            .run(ort::inputs!["input" => tensor])
            .map_err(|e| format!("Upscaling failed: {e}"))?;
        let out = outputs["output"].try_extract_array::<f32>().map_err(|e| e.to_string())?;
        let (ow, oh) = (w * 4, h * 4);
        if out.shape() != [1, 3, oh as usize, ow as usize] {
            return Err(format!("Unexpected model output shape {:?}", out.shape()));
        }
        Ok(RgbImage::from_fn(ow, oh, |x, y| {
            let v = |c: usize| (out[[0, c, y as usize, x as usize]].clamp(0.0, 1.0) * 255.0).round() as u8;
            Rgb([v(0), v(1), v(2)])
        }))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::upscale::tiling::upscale_tiled;
    use std::sync::atomic::AtomicBool;

    fn root() -> std::path::PathBuf {
        std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
    }

    fn fast_model() -> OnnxUpscaler {
        OnnxUpscaler::load(&root().join("resources/models/realesr-general-x4v3.onnx")).unwrap()
    }

    #[test]
    fn matches_pytorch_reference() {
        let input = image::open(root().join("tests/fixtures/upscale/in_32.png")).unwrap().to_rgb8();
        let reference = image::open(root().join("tests/fixtures/upscale/x4v3_128.png")).unwrap().to_rgb8();
        let out = fast_model().run(&input).unwrap();
        assert_eq!(out.dimensions(), (128, 128));
        let worst = out
            .pixels()
            .zip(reference.pixels())
            .flat_map(|(a, b)| (0..3).map(move |c| (a[c] as i32 - b[c] as i32).abs()))
            .max()
            .unwrap();
        assert!(worst <= 2, "max difference vs PyTorch: {worst}");
    }

    #[test]
    fn tiled_matches_untiled() {
        let img = RgbImage::from_fn(96, 80, |x, y| Rgb([(x * 5) as u8, (y * 3) as u8, ((x + y) * 2) as u8]));
        let mut model = fast_model();
        let whole = model.run(&img).unwrap();
        let never = AtomicBool::new(false);
        let tiled = upscale_tiled(&img, 4, 48, 24, &mut |t| model.run(t), &mut |_, _| {}, &never).unwrap();
        let worst = whole
            .pixels()
            .zip(tiled.pixels())
            .flat_map(|(a, b)| (0..3).map(move |c| (a[c] as i32 - b[c] as i32).abs()))
            .max()
            .unwrap();
        assert!(worst <= 3, "seam difference {worst}");
    }
}
