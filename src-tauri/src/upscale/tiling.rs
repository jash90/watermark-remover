use image::{imageops, RgbImage};
use std::sync::atomic::{AtomicBool, Ordering};

/// A rectangle of the input image processed in one model call (without padding).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Tile {
    pub x: u32,
    pub y: u32,
    pub w: u32,
    pub h: u32,
}

/// Row-major grid of tiles of at most `tile`×`tile` covering `w`×`h` exactly once.
pub fn tiles(w: u32, h: u32, tile: u32) -> Vec<Tile> {
    let mut out = Vec::new();
    for y in (0..h).step_by(tile as usize) {
        for x in (0..w).step_by(tile as usize) {
            out.push(Tile { x, y, w: tile.min(w - x), h: tile.min(h - y) });
        }
    }
    out
}

/// Runs `run` on overlapping, equally sized windows (each tile plus at least `pad` px
/// of real context where the image allows) and stitches only every tile's centre.
pub fn upscale_tiled(
    img: &RgbImage,
    factor: u32,
    tile: u32,
    pad: u32,
    run: &mut dyn FnMut(&RgbImage) -> Result<RgbImage, String>,
    on_tile: &mut dyn FnMut(usize, usize),
    cancel: &AtomicBool,
) -> Result<RgbImage, String> {
    let (w, h) = img.dimensions();
    let grid = tiles(w, h, tile);
    let mut out = RgbImage::new(w * factor, h * factor);
    for (n, t) in grid.iter().enumerate() {
        if cancel.load(Ordering::SeqCst) {
            return Err("Cancelled".to_string());
        }
        // Every window has the same size (tile + 2·pad, or the whole image if smaller):
        // border windows slide inwards instead of shrinking. Accelerators (CoreML)
        // compile once per input shape, and edge tiles get real context.
        let window = |start: u32, full: u32| {
            let size = (tile + 2 * pad).min(full);
            (start.saturating_sub(pad).min(full - size), size)
        };
        let ((px, pw), (py, ph)) = (window(t.x, w), window(t.y, h));
        let input = imageops::crop_imm(img, px, py, pw, ph).to_image();
        let result = run(&input)?;
        if result.dimensions() != (pw * factor, ph * factor) {
            return Err(format!("Model returned {:?}, expected {}x{}", result.dimensions(), pw * factor, ph * factor));
        }
        let centre = imageops::crop_imm(&result, (t.x - px) * factor, (t.y - py) * factor, t.w * factor, t.h * factor).to_image();
        imageops::replace(&mut out, &centre, (t.x * factor) as i64, (t.y * factor) as i64);
        on_tile(n + 1, grid.len());
        if cancel.load(Ordering::SeqCst) {
            return Err("Cancelled".to_string());
        }
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;
    use image::Rgb;

    fn sample(w: u32, h: u32) -> RgbImage {
        RgbImage::from_fn(w, h, |x, y| Rgb([(x * 7 % 251) as u8, (y * 11 % 251) as u8, ((x ^ y) % 251) as u8]))
    }

    /// Stand-in "model": exact nearest-neighbour ×4, so any seam bug shows as a mismatch.
    fn nearest4(t: &RgbImage) -> Result<RgbImage, String> {
        Ok(imageops::resize(t, t.width() * 4, t.height() * 4, imageops::FilterType::Nearest))
    }

    #[test]
    fn tiles_cover_the_image_exactly_once() {
        let t = tiles(100, 70, 32);
        let area: u32 = t.iter().map(|t| t.w * t.h).sum();
        assert_eq!(area, 100 * 70);
        assert!(t.iter().all(|t| t.x + t.w <= 100 && t.y + t.h <= 70));
    }

    #[test]
    fn tiled_nearest_matches_whole_image() {
        let img = sample(70, 45);
        let never = AtomicBool::new(false);
        let out = upscale_tiled(&img, 4, 16, 4, &mut nearest4, &mut |_, _| {}, &never).unwrap();
        assert_eq!(out, nearest4(&img).unwrap());
    }

    #[test]
    fn reports_progress_per_tile() {
        let img = sample(40, 40);
        let never = AtomicBool::new(false);
        let mut seen = Vec::new();
        upscale_tiled(&img, 4, 16, 4, &mut nearest4, &mut |d, t| seen.push((d, t)), &never).unwrap();
        assert_eq!(seen, vec![(1, 9), (2, 9), (3, 9), (4, 9), (5, 9), (6, 9), (7, 9), (8, 9), (9, 9)]);
    }

    #[test]
    fn cancel_stops_between_tiles() {
        let img = sample(64, 64);
        let cancel = AtomicBool::new(false);
        let mut calls = 0;
        let mut model = |t: &RgbImage| {
            calls += 1;
            if calls == 2 {
                cancel.store(true, Ordering::SeqCst);
            }
            nearest4(t)
        };
        let err = upscale_tiled(&img, 4, 16, 4, &mut model, &mut |_, _| {}, &cancel).unwrap_err();
        assert_eq!(err, "Cancelled");
        assert_eq!(calls, 2);
    }
}
