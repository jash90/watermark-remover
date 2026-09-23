use std::sync::atomic::AtomicBool;
use watermark_remover_lib::upscale::{onnx::OnnxUpscaler, tiling::upscale_tiled};
fn main() {
    let img = image::open("/tmp/wmr-qa/base.jpg").unwrap().resize_exact(1024, 1024, image::imageops::FilterType::Triangle).to_rgb8();
    let t = std::time::Instant::now();
    let mut m = OnnxUpscaler::load(std::path::Path::new(concat!(env!("CARGO_MANIFEST_DIR"), "/resources/models/realesr-general-x4v3.onnx"))).unwrap();
    println!("load {:?}", t.elapsed());
    let t = std::time::Instant::now();
    let out = upscale_tiled(&img, 4, 384, 24, &mut |x| m.run(x), &mut |_, _| {}, &AtomicBool::new(false)).unwrap();
    println!("1024x1024 -> {:?} in {:?}", out.dimensions(), t.elapsed());
    out.save("/tmp/wmr-qa/up4.png").unwrap();
}
