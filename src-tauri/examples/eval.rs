//! Synthetic watermark benchmark.
//! Usage: cargo run --release --example eval [-- <folder with clean .png/.jpg>]
use image::{DynamicImage, Rgb, RgbImage};
use watermark_remover_lib::image_processor::WatermarkRegion;
use watermark_remover_lib::removal::engine::Engine;
use watermark_remover_lib::removal::pipeline::{run, PipelineOptions};

fn synthetic() -> Vec<(String, RgbImage)> {
    let hash = |x: u32, y: u32| ((x.wrapping_mul(73856093) ^ y.wrapping_mul(19349663)) % 64) as u8;
    vec![
        ("gradient".into(), RgbImage::from_fn(400, 300, |x, y| Rgb([(x / 2) as u8, (y / 2) as u8, 128]))),
        ("sky".into(), RgbImage::from_fn(400, 300, |_, y| Rgb([90 + (y / 6) as u8, 150 + (y / 6) as u8, 230]))),
        ("stripes".into(), RgbImage::from_fn(400, 300, |x, _| if (x / 6) % 2 == 0 { Rgb([40, 40, 40]) } else { Rgb([200, 180, 160]) })),
        ("noise".into(), RgbImage::from_fn(400, 300, |x, y| {
            let n = hash(x, y);
            Rgb([100 + n, 110 + n, 90 + n])
        })),
    ]
}

fn from_folder(dir: &str) -> Vec<(String, RgbImage)> {
    std::fs::read_dir(dir)
        .expect("readable folder")
        .filter_map(|e| e.ok())
        .filter_map(|e| {
            let img = image::open(e.path()).ok()?.to_rgb8();
            Some((e.file_name().to_string_lossy().to_string(), img))
        })
        .collect()
}

/// White "X" + frame, 3 px strokes, blended at `opacity`, centred in the image.
fn stamp(img: &RgbImage, opacity: f32) -> (RgbImage, WatermarkRegion) {
    let (w, h) = img.dimensions();
    let (rw, rh) = (w / 4, h / 6);
    let (rx, ry) = ((w - rw) / 2, (h - rh) / 2);
    let mut out = img.clone();
    for y in ry..ry + rh {
        for x in rx..rx + rw {
            let (u, v) = ((x - rx) as f32 / rw as f32, (y - ry) as f32 / rh as f32);
            let on_diag = (u - v).abs() * (rh as f32) < 1.5 || (u + v - 1.0).abs() * (rh as f32) < 1.5;
            let on_frame = x < rx + 3 || x >= rx + rw - 3 || y < ry + 3 || y >= ry + rh - 3;
            if on_diag || on_frame {
                let p = out.get_pixel_mut(x, y);
                for c in 0..3 {
                    p[c] = (255.0 * opacity + p[c] as f32 * (1.0 - opacity)).round() as u8;
                }
            }
        }
    }
    let region = WatermarkRegion { x: rx as i32 - 4, y: ry as i32 - 4, width: rw as i32 + 8, height: rh as i32 + 8 };
    (out, region)
}

fn psnr(a: &RgbImage, b: &RgbImage, r: &WatermarkRegion) -> f64 {
    let (mut se, mut n) = (0f64, 0f64);
    for y in r.y.max(0) as u32..((r.y + r.height) as u32).min(a.height()) {
        for x in r.x.max(0) as u32..((r.x + r.width) as u32).min(a.width()) {
            for c in 0..3 {
                let d = a.get_pixel(x, y)[c] as f64 - b.get_pixel(x, y)[c] as f64;
                se += d * d;
                n += 1.0;
            }
        }
    }
    if se == 0.0 {
        f64::INFINITY
    } else {
        10.0 * (255.0 * 255.0 / (se / n)).log10()
    }
}

#[tokio::main]
async fn main() {
    let images = match std::env::args().nth(1) {
        Some(dir) => from_folder(&dir),
        None => synthetic(),
    };
    println!("{:<20} {:>7} {:>8} {:>9} {:>9}", "image", "opacity", "refine", "input dB", "output dB");
    for (name, clean) in &images {
        for opacity in [0.35f32, 0.6, 1.0] {
            let (marked, region) = stamp(clean, opacity);
            for refine in [false, true] {
                let opts = PipelineOptions { refine_mask: refine, grow: 2, feather: 3 };
                let src = DynamicImage::ImageRgb8(marked.clone());
                let out = run(&src, &region, &opts, &Engine::Telea { radius: 5 }).await.expect("pipeline");
                println!(
                    "{:<20} {:>7.2} {:>8} {:>9.2} {:>9.2}",
                    name,
                    opacity,
                    refine,
                    psnr(clean, &marked, &region),
                    psnr(clean, &out.image.to_rgb8(), &region)
                );
            }
        }
    }
}
