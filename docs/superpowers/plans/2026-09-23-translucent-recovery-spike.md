# Plan B — Translucent Watermark Recovery (Gated Spike) Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Remove semi-transparent watermarks by *recovering* the real pixels under them (reverse alpha blending) instead of guessing them. Ship it only if it measurably and visibly beats the current Telea fill.

**Architecture:** Model: `I = α·W + (1−α)·J` per pixel. The steps:
1. Estimate the watermark colour `W` (white or black) and one global opacity `α₀` from the refined stroke mask.
2. Recover `J = (I − α₀·W)/(1 − α₀)` in the stroke interiors.
3. Handle the anti-aliased edge band separately. This is the unsolved part.

The whole thing sits behind a go/no-go gate measured on a ground-truth benchmark.

**Tech Stack:** Rust, the existing `removal` modules. No new dependencies.

**Spec:** `docs/superpowers/plans/2026-09-23-roadmap-v0.3.md` (section "Plan B"). The prototype results that justify this plan's structure are below.

## Prototype results (2026-09-23, throwaway code, not committed)

Setup: PSNR in the watermark rectangle against the clean original. "telea" is the current shipping path.

| Case | Input | Telea | Recovery variant: global α mode | Recovery variant: + seam-refined α | Recovery variant: + Telea edge band |
|---|---|---|---|---|---|
| noise bg, white α 0.60 | 15.7 | 24.6 | **32.3** | 31.2 | 27.9 |
| noise bg, white α 0.35 | 20.3 | 24.4 | 24.6 | 26.4 | 24.2 |
| stripes bg, white α 0.60 | 14.1 | 11.6 | 13.4 | 13.3 | 12.9 |
| smooth sky, white α 0.60 | 18.1 | **58.6** | 37.9 | 57.3 | 36.6 |
| red watermark (any bg) | — | = | skipped by colour guard (correct) | same | same |
| real photo "© STOCKPHOTO", α 0.55, JPEG | 20.8 | 26.8 | 27.4 (α̂ = 0.56) | 28.0 (α̂ = 0.48) | 25.9 |

**Visual check (what the numbers hide):**
- With the global-mode α (0.56, almost exact), the **letter interiors are recovered almost perfectly**: the real texture comes back at the right brightness. Thin dark outlines remain along the anti-aliased edges.
- The seam-refined α scores better in PSNR, but it under-removes: the letters stay clearly visible.
- On smooth backgrounds Telea is already near-perfect (58 dB). Recovery can only lose there, unless α is refined to within about ±0.01.

**Conclusions this plan is built on:**
1. Estimating `W ∈ {white, black}` plus the histogram mode of α is reliable. It got 0.54–0.56 against a true 0.55, and 0.60 against 0.60.
2. The colour guard (a large residual means the model doesn't fit) safely skips coloured watermarks.
3. **The edges are the open problem.** Per-pixel α from a Telea background guess is too noisy there.
4. A benchmark needs a **visual ghost metric**, not only PSNR.

## Global Constraints

- The removal result must never be *worse* than today's Telea path on the benchmark: per case, PSNR ≥ Telea − 0.5 dB.
- The feature is off in the UI until the gate in Task 3 passes. If the gate fails, the spike ends with a written report, and the code stays behind `#[cfg(test)]`/examples or is deleted.
- Pure Rust, no new dependencies. All Rust commands run from `src-tauri/`.

## Review Focus

1. **Smooth backgrounds (sky, studio walls):** recovery must not replace an already-perfect Telea fill with a noisier one. (The Task 3 gate row "sky".)
2. **Coloured or gradient watermarks:** the colour guard must return `None`, not a wrong recovery. (Task 2 test `coloured_watermark_is_not_unblended`.)
3. **Opaque watermarks (α ≥ 0.85):** they fall back to plain inpainting. (Task 2 test `opaque_watermark_is_not_unblended`.)
4. **JPEG-compressed sources:** recovery amplifies noise by `1/(1−α)`, so the benchmark must include q85 JPEG. (Task 1 fixture set.)
5. **Tiny marks (under 16 stroke pixels):** return `None`. (Task 2 test `tiny_mask_is_skipped`.)

## File Structure

```
src-tauri/examples/bench_translucent.rs   CREATE  ground-truth benchmark + ghost metric
src-tauri/src/removal/unblend.rs          CREATE  estimate_watermark(), recover_interior()
src-tauri/src/removal/mod.rs              MODIFY  pub mod unblend;
src-tauri/src/removal/pipeline.rs         MODIFY  (Task 4, only after the gate) recover_translucent option
src-tauri/src/image_processor.rs          MODIFY  (Task 4) RemovalOptions.recover_translucent
src/types/watermark.ts, src/components/OptionsPanel.tsx  MODIFY (Task 4) checkbox
docs/superpowers/reports/2026-xx-translucent-spike.md    CREATE  go/no-go report (Task 3)
```

---

### Task 1: Ground-truth benchmark with a ghost metric

**Files:**
- Create: `src-tauri/examples/bench_translucent.rs`

**Interfaces:**
- Consumes: `watermark_remover_lib::removal::{geometry::*, mask::*, refine::refine_mask, telea}` (existing public API; after Plan A, `refine_mask` and `telea::inpaint_with_ignore` take an extra `ignore: Option<&GrayImage>`. Pass `None`).
- Produces: `cargo run --release --example bench_translucent [-- <folder of clean photos>]` prints one row per case: `case, psnr_input, psnr_telea, psnr_candidate, ghost_telea, ghost_candidate`. Task 3 reads these rows.

**Ghost metric definition:** for every stroke pixel `p` whose true α is ≥ 0.5·α₀, and every 4-neighbour `q` whose true α is 0, accumulate `(result(p) − clean(p)) − (result(q) − clean(q))` per channel. Report the mean absolute value of the per-channel sums divided by the number of pairs. It is 0 when the border is invisible. A visible outline shows up as |ghost| ≳ 3.

- [ ] **Step 1: Write the benchmark**

```rust
//! Translucent-watermark benchmark with ground truth.
//! Usage: cargo run --release --example bench_translucent [-- <folder with clean photos>]
use image::{imageops, DynamicImage, GrayImage, Luma, Rgb, RgbImage};
use watermark_remover_lib::image_processor::WatermarkRegion;
use watermark_remover_lib::removal::geometry::{clamp_region, context_margin, expand};
use watermark_remover_lib::removal::mask::rect_mask;
use watermark_remover_lib::removal::refine::refine_mask;
use watermark_remover_lib::removal::telea;

/// Soft-edged "© MARK"-like stroke pattern as a coverage map in [0, 1].
fn coverage(w: u32, h: u32) -> Vec<f32> {
    let mut cov = vec![0.0f32; (w * h) as usize];
    let (cx, cy, r) = (w as f32 * 0.3, h as f32 * 0.5, h as f32 * 0.3);
    for y in 0..h {
        for x in 0..w {
            let (fx, fy) = (x as f32 + 0.5, y as f32 + 0.5);
            // ring
            let d_ring = ((fx - cx).hypot(fy - cy) - r).abs() - 3.0;
            // bar
            let d_bar = if fx > w as f32 * 0.5 && fx < w as f32 * 0.9 { (fy - cy).abs() - 4.0 } else { f32::MAX };
            // 1.5 px anti-aliasing ramp, like rendered text
            let d = d_ring.min(d_bar);
            cov[(y * w + x) as usize] = (0.5 - d / 1.5).clamp(0.0, 1.0);
        }
    }
    cov
}

struct Case {
    name: String,
    clean: RgbImage,
    marked: RgbImage,
    region: WatermarkRegion,
    alpha: Vec<f32>, // true per-pixel alpha inside the region (region-sized)
}

fn make_case(name: &str, clean: &RgbImage, opacity: f32, white: bool, jpeg_quality: Option<u8>) -> Case {
    let (w, h) = clean.dimensions();
    let (rw, rh) = (w / 3, h / 6);
    let (rx, ry) = ((w - rw) / 2, (h - rh) / 2);
    let cov = coverage(rw, rh);
    let wm = if white { 255.0 } else { 0.0 };
    let mut marked = clean.clone();
    for y in 0..rh {
        for x in 0..rw {
            let a = opacity * cov[(y * rw + x) as usize];
            let p = marked.get_pixel_mut(rx + x, ry + y);
            for c in 0..3 {
                p[c] = (wm * a + p[c] as f32 * (1.0 - a)).round() as u8;
            }
        }
    }
    if let Some(q) = jpeg_quality {
        let mut bytes = Vec::new();
        image::codecs::jpeg::JpegEncoder::new_with_quality(&mut bytes, q).encode_image(&marked).unwrap();
        marked = image::load_from_memory(&bytes).unwrap().to_rgb8();
    }
    let alpha = cov.iter().map(|c| c * opacity).collect();
    let region = WatermarkRegion { x: rx as i32 - 6, y: ry as i32 - 6, width: rw as i32 + 12, height: rh as i32 + 12 };
    Case {
        name: format!("{name} a={opacity:.2} {} q={}", if white { "white" } else { "black" }, jpeg_quality.map_or("-".into(), |q| q.to_string())),
        clean: clean.clone(),
        marked,
        region,
        alpha,
    }
}

fn psnr(a: &RgbImage, b: &RgbImage, r: &WatermarkRegion) -> f64 {
    let (mut se, mut n) = (0f64, 0f64);
    for y in r.y as u32..(r.y + r.height) as u32 {
        for x in r.x as u32..(r.x + r.width) as u32 {
            for c in 0..3 {
                let d = a.get_pixel(x, y)[c] as f64 - b.get_pixel(x, y)[c] as f64;
                se += d * d;
                n += 1.0;
            }
        }
    }
    if se == 0.0 { f64::INFINITY } else { 10.0 * (255.0f64 * 255.0 / (se / n)).log10() }
}

/// Mean |signed error step| across stroke borders; ~0 when the border is invisible.
fn ghost(case: &Case, result: &RgbImage) -> f64 {
    let (rx, ry) = ((case.region.x + 6) as u32, (case.region.y + 6) as u32);
    let (rw, rh) = ((case.region.width - 12) as u32, (case.region.height - 12) as u32);
    let peak = case.alpha.iter().cloned().fold(0.0f32, f32::max);
    let a = |x: u32, y: u32| case.alpha[(y * rw + x) as usize];
    let err = |x: u32, y: u32, c: usize| result.get_pixel(rx + x, ry + y)[c] as f64 - case.clean.get_pixel(rx + x, ry + y)[c] as f64;
    let (mut acc, mut pairs) = ([0f64; 3], 0usize);
    for y in 1..rh - 1 {
        for x in 1..rw - 1 {
            if a(x, y) < 0.5 * peak {
                continue;
            }
            for (nx, ny) in [(x + 1, y), (x - 1, y), (x, y + 1), (x, y - 1)] {
                if a(nx, ny) == 0.0 {
                    for c in 0..3 {
                        acc[c] += err(x, y, c) - err(nx, ny, c);
                    }
                    pairs += 1;
                }
            }
        }
    }
    if pairs == 0 { 0.0 } else { acc.iter().map(|v| v.abs()).sum::<f64>() / (3.0 * pairs as f64) }
}

/// Today's shipping path: refined mask + Telea.
fn telea_result(case: &Case) -> RgbImage {
    let (w, h) = case.marked.dimensions();
    let hole = clamp_region(&case.region, w, h).unwrap();
    let crop = expand(hole, context_margin(hole), w, h);
    let c = imageops::crop_imm(&case.marked, crop.x, crop.y, crop.w, crop.h).to_image();
    let m = refine_mask(&c, &rect_mask(crop, hole), 2, None).unwrap().mask;
    let patch = telea::inpaint(&c, &m, 5).unwrap();
    let mut out = case.marked.clone();
    imageops::replace(&mut out, &patch, crop.x as i64, crop.y as i64);
    out
}

/// Candidate under test. Task 2 replaces this body with the unblend path.
fn candidate_result(case: &Case) -> RgbImage {
    telea_result(case)
}

fn backgrounds(folder: Option<String>) -> Vec<(String, RgbImage)> {
    let hash = |x: u32, y: u32| ((x.wrapping_mul(73856093) ^ y.wrapping_mul(19349663)) % 64) as u8;
    let mut v: Vec<(String, RgbImage)> = vec![
        ("sky".into(), RgbImage::from_fn(480, 320, |_, y| Rgb([90 + (y / 6) as u8, 150 + (y / 6) as u8, 230]))),
        ("noise".into(), RgbImage::from_fn(480, 320, |x, y| { let n = hash(x, y); Rgb([100 + n, 110 + n, 90 + n]) })),
        ("stripes".into(), RgbImage::from_fn(480, 320, |x, _| if (x / 6) % 2 == 0 { Rgb([40, 40, 40]) } else { Rgb([200, 180, 160]) })),
    ];
    if let Some(dir) = folder {
        for e in std::fs::read_dir(dir).expect("readable folder").flatten() {
            if let Ok(img) = image::open(e.path()) {
                v.push((e.file_name().to_string_lossy().to_string(), img.to_rgb8()));
            }
        }
    }
    v
}

fn main() {
    let folder = std::env::args().nth(1);
    println!("{:<42} {:>7} {:>7} {:>7} {:>8} {:>8}", "case", "input", "telea", "cand", "ghostT", "ghostC");
    for (name, clean) in backgrounds(folder) {
        for (opacity, white, q) in [(0.35, true, None), (0.55, true, None), (0.55, true, Some(85)), (0.45, false, None)] {
            let case = make_case(&name, &clean, opacity, white, q);
            let t = telea_result(&case);
            let c = candidate_result(&case);
            println!(
                "{:<42} {:>7.2} {:>7.2} {:>7.2} {:>8.2} {:>8.2}",
                case.name,
                psnr(&case.clean, &case.marked, &case.region),
                psnr(&case.clean, &t, &case.region),
                psnr(&case.clean, &c, &case.region),
                ghost(&case, &t),
                ghost(&case, &c)
            );
        }
    }
    let _ = (GrayImage::new(1, 1), Luma([0u8]), DynamicImage::new_rgb8(1, 1)); // keep imports stable for Task 2
}
```

- [ ] **Step 2: Run the baseline**

Run: `cd src-tauri && cargo run --release --example bench_translucent`
Expected: 12 rows. `cand` equals `telea` on every row, because the candidate is still Telea. Save the output to the report draft as "baseline". Also run with a folder of 5–10 real photos (`-- ~/Pictures/wmr-bench`) and save that too.

- [ ] **Step 3: Commit**

```bash
git add src-tauri/examples/bench_translucent.rs
git commit -m "chore: ground-truth benchmark for translucent watermark recovery"
```

---

### Task 2: `unblend` — the validated core (W, α₀, colour guard, interior recovery)

**Files:**
- Create: `src-tauri/src/removal/unblend.rs`
- Modify: `src-tauri/src/removal/mod.rs` (add `pub mod unblend;`), `src-tauri/examples/bench_translucent.rs` (`candidate_result`)

**Interfaces:**
- Consumes: `HOLE` (`removal::mask`), the refined crop mask, and a background guess `bg: &RgbImage` (the Telea patch of the same crop).
- Produces:
  - `pub struct WatermarkEstimate { pub colour: [f32; 3], pub alpha: f32 }`
  - `pub fn estimate_watermark(crop: &RgbImage, mask: &GrayImage, bg: &RgbImage) -> Option<WatermarkEstimate>`. `None` means the region isn't a translucent white or black overlay (coloured, opaque, too small, or no signal).
  - `pub struct Recovered { pub image: RgbImage, pub edge_band: GrayImage }`
  - `pub fn recover_interior(crop: &RgbImage, mask: &GrayImage, bg: &RgbImage, est: &WatermarkEstimate) -> Recovered`. Interior pixels (per-pixel α within [0.6, 1.4]·α₀) are unblended. Every other mask pixel is marked HOLE in `edge_band` for a second pass.

- [ ] **Step 1: Write the failing tests** (`unblend.rs`)

```rust
use super::mask::HOLE;
use image::{GrayImage, Luma, Rgb, RgbImage};

#[cfg(test)]
mod tests {
    use super::*;

    const W: u32 = 60;
    const H: u32 = 40;

    fn texture() -> RgbImage {
        RgbImage::from_fn(W, H, |x, y| {
            let n = ((x.wrapping_mul(73856093) ^ y.wrapping_mul(19349663)) % 64) as u8;
            Rgb([100 + n, 110 + n, 90 + n])
        })
    }

    /// Hard-edged 20x10 block blended with `colour` at `alpha`; returns (marked, mask).
    fn stamp(clean: &RgbImage, colour: [u8; 3], alpha: f32) -> (RgbImage, GrayImage) {
        let mut marked = clean.clone();
        let mask = GrayImage::from_fn(W, H, |x, y| Luma([if (20..40).contains(&x) && (15..25).contains(&y) { HOLE } else { 0 }]));
        for (x, y, m) in mask.enumerate_pixels() {
            if m[0] == HOLE {
                let p = marked.get_pixel_mut(x, y);
                for c in 0..3 {
                    p[c] = (colour[c] as f32 * alpha + p[c] as f32 * (1.0 - alpha)).round() as u8;
                }
            }
        }
        (marked, mask)
    }

    /// Stand-in for the Telea guess: the clean image's mean colour in the hole.
    fn mean_bg(clean: &RgbImage) -> RgbImage {
        let mut s = [0f32; 3];
        for p in clean.pixels() {
            for c in 0..3 {
                s[c] += p[c] as f32;
            }
        }
        let n = (W * H) as f32;
        RgbImage::from_pixel(W, H, Rgb([(s[0] / n) as u8, (s[1] / n) as u8, (s[2] / n) as u8]))
    }

    #[test]
    fn estimates_white_watermark_and_opacity() {
        let clean = texture();
        let (marked, mask) = stamp(&clean, [255, 255, 255], 0.5);
        let est = estimate_watermark(&marked, &mask, &mean_bg(&clean)).expect("estimate");
        assert_eq!(est.colour, [255.0, 255.0, 255.0]);
        assert!((est.alpha - 0.5).abs() <= 0.04, "alpha {}", est.alpha);
    }

    #[test]
    fn recovers_texture_under_the_watermark() {
        let clean = texture();
        let (marked, mask) = stamp(&clean, [255, 255, 255], 0.5);
        let bg = mean_bg(&clean);
        let est = estimate_watermark(&marked, &mask, &bg).unwrap();
        let rec = recover_interior(&marked, &mask, &bg, &est);
        let mut worst = 0i32;
        for (x, y, m) in mask.enumerate_pixels() {
            if m[0] == HOLE && rec.edge_band.get_pixel(x, y)[0] != HOLE {
                for c in 0..3 {
                    worst = worst.max((rec.image.get_pixel(x, y)[c] as i32 - clean.get_pixel(x, y)[c] as i32).abs());
                }
            }
        }
        assert!(worst <= 6, "worst recovered error {worst}");
    }

    #[test]
    fn coloured_watermark_is_not_unblended() {
        let clean = texture();
        let (marked, mask) = stamp(&clean, [220, 30, 30], 0.5);
        assert!(estimate_watermark(&marked, &mask, &mean_bg(&clean)).is_none());
    }

    #[test]
    fn opaque_watermark_is_not_unblended() {
        let clean = texture();
        let (marked, mask) = stamp(&clean, [255, 255, 255], 1.0);
        assert!(estimate_watermark(&marked, &mask, &mean_bg(&clean)).is_none());
    }

    #[test]
    fn tiny_mask_is_skipped() {
        let clean = texture();
        let mask = GrayImage::from_fn(W, H, |x, y| Luma([if x < 3 && y < 3 { HOLE } else { 0 }]));
        assert!(estimate_watermark(&clean, &mask, &clean).is_none());
    }
}
```

- [ ] **Step 2: Run to verify failure**

Run: `cd src-tauri && cargo test --lib removal::unblend`
Expected: compile error `cannot find function estimate_watermark`.

- [ ] **Step 3: Implement** (above `#[cfg(test)]`; this is the prototype's validated core)

```rust
/// Above this opacity the watermark is treated as opaque (nothing to recover).
const ALPHA_MAX: f32 = 0.85;
/// Below this there is no watermark signal worth unblending.
const ALPHA_MIN: f32 = 0.05;
/// RMS residual (in 0..255 units) above which the white/black overlay model doesn't fit.
const MAX_RMS_RESIDUAL: f32 = 40.0;
const MIN_PIXELS: usize = 16;

pub struct WatermarkEstimate {
    pub colour: [f32; 3],
    pub alpha: f32,
}

pub struct Recovered {
    pub image: RgbImage,
    pub edge_band: GrayImage,
}

fn rgb(p: &Rgb<u8>) -> [f32; 3] {
    [p[0] as f32, p[1] as f32, p[2] as f32]
}

/// Per-pixel opacity along W−B, and the squared residual off that line.
fn alpha_at(i: [f32; 3], b: [f32; 3], w: [f32; 3]) -> (f32, f32) {
    let d = [i[0] - b[0], i[1] - b[1], i[2] - b[2]];
    let e = [w[0] - b[0], w[1] - b[1], w[2] - b[2]];
    let ee = e[0] * e[0] + e[1] * e[1] + e[2] * e[2];
    if ee < 1.0 {
        return (0.0, d[0] * d[0] + d[1] * d[1] + d[2] * d[2]);
    }
    let a = ((d[0] * e[0] + d[1] * e[1] + d[2] * e[2]) / ee).clamp(0.0, 1.0);
    let r = [d[0] - a * e[0], d[1] - a * e[1], d[2] - a * e[2]];
    (a, r[0] * r[0] + r[1] * r[1] + r[2] * r[2])
}

fn hole_pixels(mask: &GrayImage) -> Vec<(u32, u32)> {
    mask.enumerate_pixels().filter(|(_, _, m)| m[0] == HOLE).map(|(x, y, _)| (x, y)).collect()
}

/// Estimates a white or black translucent overlay. `bg` is a background guess
/// for the hole (e.g. the Telea fill of the same crop).
pub fn estimate_watermark(crop: &RgbImage, mask: &GrayImage, bg: &RgbImage) -> Option<WatermarkEstimate> {
    let hole = hole_pixels(mask);
    if hole.len() < MIN_PIXELS {
        return None;
    }
    let residual = |w: [f32; 3]| -> f32 {
        hole.iter().map(|&(x, y)| alpha_at(rgb(crop.get_pixel(x, y)), rgb(bg.get_pixel(x, y)), w).1).sum()
    };
    let colour = [[255.0f32; 3], [0.0f32; 3]]
        .into_iter()
        .min_by(|a, b| residual(*a).total_cmp(&residual(*b)))?;
    if (residual(colour) / hole.len() as f32).sqrt() > MAX_RMS_RESIDUAL {
        return None;
    }
    // Opacity = mode of the per-pixel alpha histogram (stroke interiors share one opacity).
    let mut hist = [0usize; 50];
    for &(x, y) in &hole {
        let a = alpha_at(rgb(crop.get_pixel(x, y)), rgb(bg.get_pixel(x, y)), colour).0;
        if a > 0.15 {
            hist[((a * 49.0) as usize).min(49)] += 1;
        }
    }
    let smoothed: Vec<usize> = (0usize..50).map(|i| (i.saturating_sub(1)..=(i + 1).min(49)).map(|k| hist[k]).sum()).collect();
    let (bin, &count) = smoothed.iter().enumerate().max_by_key(|(_, c)| **c)?;
    if count < MIN_PIXELS / 2 {
        return None;
    }
    let alpha = (bin as f32 + 0.5) / 49.0;
    (ALPHA_MIN..=ALPHA_MAX).contains(&alpha).then_some(WatermarkEstimate { colour, alpha })
}

/// Unblends interior stroke pixels with the global opacity; everything else in
/// the mask (anti-aliased edges, outliers) is returned in `edge_band`.
pub fn recover_interior(crop: &RgbImage, mask: &GrayImage, bg: &RgbImage, est: &WatermarkEstimate) -> Recovered {
    let mut image = crop.clone();
    let mut edge_band = GrayImage::new(crop.width(), crop.height());
    for (x, y) in hole_pixels(mask) {
        let i = rgb(crop.get_pixel(x, y));
        let a = alpha_at(i, rgb(bg.get_pixel(x, y)), est.colour).0;
        if a < 0.6 * est.alpha || a > 1.4 * est.alpha {
            edge_band.put_pixel(x, y, Luma([HOLE]));
            continue;
        }
        let j = |c: usize| ((i[c] - est.alpha * est.colour[c]) / (1.0 - est.alpha)).round().clamp(0.0, 255.0) as u8;
        image.put_pixel(x, y, Rgb([j(0), j(1), j(2)]));
    }
    Recovered { image, edge_band }
}
```

- [ ] **Step 4: Run to verify pass**

Run: `cd src-tauri && cargo test --lib removal::unblend`
Expected: 5 passed. If `recovers_texture_under_the_watermark` exceeds 6, print the α estimate. The ±0.04 opacity tolerance, amplified by 1/(1−α) = 2, bounds the error at about 6 levels.

- [ ] **Step 5: Wire it into the benchmark.** In `bench_translucent.rs`, replace `candidate_result` with:

```rust
use watermark_remover_lib::removal::unblend::{estimate_watermark, recover_interior};

fn candidate_result(case: &Case) -> RgbImage {
    let (w, h) = case.marked.dimensions();
    let hole = clamp_region(&case.region, w, h).unwrap();
    let crop = expand(hole, context_margin(hole), w, h);
    let c = imageops::crop_imm(&case.marked, crop.x, crop.y, crop.w, crop.h).to_image();
    let m = refine_mask(&c, &rect_mask(crop, hole), 2, None).unwrap().mask;
    let bg = telea::inpaint(&c, &m, 5).unwrap();
    let patch = match estimate_watermark(&c, &m, &bg) {
        Some(est) => {
            let rec = recover_interior(&c, &m, &bg, &est);
            // Edge strategy under test (Task 3 iterates on this line only).
            if rec.edge_band.pixels().any(|p| p[0] == 255) {
                telea::inpaint(&rec.image, &rec.edge_band, 3).unwrap()
            } else {
                rec.image
            }
        }
        None => bg,
    };
    let mut out = case.marked.clone();
    imageops::replace(&mut out, &patch, crop.x as i64, crop.y as i64);
    out
}
```

Run: `cd src-tauri && cargo run --release --example bench_translucent`, then again with the real-photo folder. Paste both tables into the report draft.

- [ ] **Step 6: Commit**

```bash
git add src-tauri/src/removal/unblend.rs src-tauri/src/removal/mod.rs src-tauri/examples/bench_translucent.rs
git commit -m "feat(removal): translucent watermark estimate and interior recovery (not wired)"
```

---

### Task 3: Edge strategy iteration and the go/no-go gate (time-boxed: 2 days)

**Files:**
- Modify: `src-tauri/src/removal/unblend.rs` (only an edge-handling function added here), `src-tauri/examples/bench_translucent.rs` (the "edge strategy" line)
- Create: `docs/superpowers/reports/2026-xx-translucent-spike.md`

**Interfaces:**
- Produces: either `pub fn recover(crop, mask, bg) -> Option<RgbImage>` in `unblend.rs` (gate passed), or a report recommending no-go (gate failed).

Try these edge strategies in order. Each one is a single change to the "edge strategy" line or a helper in `unblend.rs`. Record every result in the report.
1. **Telea on the edge band from recovered interiors** (already wired in Task 2).
2. **Shape-based edge α:** α(x) = α₀ · coverage(x). Coverage comes from the distance to the interior set: 1 at interior pixels, a linear ramp to 0 over 1.5 px. Then unblend the edge band with that α instead of inpainting it.
3. **Guided α:** the per-pixel α is smoothed with a 5×5 box filter weighted by colour similarity in `I` (a cheap guided filter). Clamp to [0, α₀], then unblend.

**Gate (all must hold on the benchmark, synthetic rows and at least 5 real photos):**
- PSNR(candidate) ≥ PSNR(telea) − 0.5 dB on **every** row.
- Mean PSNR gain over all textured rows (noise, stripes, real photos) ≥ **+2 dB**.
- |ghost(candidate)| ≤ **2.0** on every row where α ≥ 0.35.
- A visual check by a human on 3 real photos (before/after crops in the report) confirms no visible outline.

- [ ] **Step 1:** Run the benchmark for strategy 1 and record the table.
- [ ] **Step 2:** Implement strategy 2 behind a function `fn unblend_edges_by_shape(rec: &Recovered, mask: &GrayImage, est: &WatermarkEstimate, crop: &RgbImage) -> RgbImage`, with a unit test on the hard-edged stamp from Task 2 asserting max error ≤ 8 at edge pixels. Record the table.
- [ ] **Step 3:** Implement strategy 3 the same way, with a unit test. Record the table.
- [ ] **Step 4: Decide.** If a strategy passes the gate, promote it into `pub fn recover(crop: &RgbImage, mask: &GrayImage, bg: &RgbImage) -> Option<RgbImage>` (estimate, then interior, then edges) with a test that it returns `None` for the coloured and opaque cases. **If none passes, stop.** Write the no-go report, keep `unblend.rs` (it's tested and harmless, just unwired), and move to Plan D (LaMa on the ONNX runtime from Plan C).
- [ ] **Step 5: Commit** the report (and code if go):

```bash
git add docs/superpowers/reports src-tauri/src/removal/unblend.rs src-tauri/examples/bench_translucent.rs
git commit -m "docs: translucent recovery spike report (<go|no-go>)"
```

---

### Task 4 (only if the gate passed): Pipeline + UI

**Files:**
- Modify: `src-tauri/src/image_processor.rs` (`RemovalOptions`), `src-tauri/src/removal/pipeline.rs` (`PipelineOptions`, `run`), `src-tauri/src/commands.rs` (pass the option), `src/types/watermark.ts`, `src/App.tsx` + `src/hooks/useWatermarkRemoval.ts` (`DEFAULT_OPTIONS`), `src/components/OptionsPanel.tsx`

**Interfaces:**
- Consumes: `unblend::recover` (Task 3).
- Produces: `RemovalOptions.recover_translucent: bool` (serde default `true`), `PipelineOptions.recover_translucent: bool`, `PipelineOutput.recovered: bool`.

- [ ] **Step 1: Failing pipeline test** (`pipeline.rs`):

```rust
    #[test]
    fn pipeline_recovers_texture_under_white_overlay() {
        let clean = RgbImage::from_fn(160, 100, |x, y| {
            let n = ((x.wrapping_mul(73856093) ^ y.wrapping_mul(19349663)) % 64) as u8;
            Rgb([100 + n, 110 + n, 90 + n])
        });
        let mut marked = clean.clone();
        for y in 40..60 { for x in 50..110 {
            let p = marked.get_pixel_mut(x, y);
            for c in 0..3 { p[c] = (255.0 * 0.5 + p[c] as f32 * 0.5).round() as u8; }
        } }
        let o = PipelineOptions { refine_mask: true, grow: 2, feather: 3, recover_translucent: true };
        let out = run(&DynamicImage::ImageRgb8(marked), &region(45, 35, 70, 30), &o, &Engine::Telea { radius: 5 }).unwrap();
        assert!(out.recovered);
        let got = out.image.to_rgb8();
        let err = (got.get_pixel(80, 50)[0] as i32 - clean.get_pixel(80, 50)[0] as i32).abs();
        assert!(err <= 8, "interior error {err}");
    }
```

Run: `cd src-tauri && cargo test --lib removal::pipeline`. Expected: compile error (no field `recover_translucent`).

- [ ] **Step 2: Implement.** Add `pub recover_translucent: bool` to `PipelineOptions` and `pub recovered: bool` to `PipelineOutput`. Update every existing `PipelineOptions { .. }` literal (tests, `commands.rs`, `examples/eval.rs`) to include `recover_translucent: false`, except `commands.rs`, which passes `options.recover_translucent`. In `run`, after computing `patch`:

```rust
    let mut recovered = false;
    let patch = if opts.recover_translucent {
        match super::unblend::recover(&crop_img, &mask, &patch) {
            Some(better) => {
                recovered = true;
                better
            }
            None => patch,
        }
    } else {
        patch
    };
```

Return `PipelineOutput { image, mask_refined, recovered }`. In `image_processor.rs` add:

```rust
    #[serde(default = "default_recover_translucent")]
    pub recover_translucent: bool,
```

with `fn default_recover_translucent() -> bool { true }`, and include it in `Default`.

- [ ] **Step 3: UI.** In `src/types/watermark.ts` add `recover_translucent: boolean;` to `RemovalOptions`. Add `recover_translucent: true,` to both `DEFAULT_OPTIONS`. In `OptionsPanel.tsx`, after the "Detect watermark shape" group:

```tsx
      <div className="option-group">
        <label className="checkbox-label">
          <input
            type="checkbox"
            checked={options.recover_translucent}
            onChange={(e) => onChange({ ...options, recover_translucent: e.target.checked })}
            disabled={disabled}
          />
          Recover the image under see-through watermarks
        </label>
        <p className="option-description">
          For semi-transparent white or black marks: restores the real pixels instead of painting over them.
        </p>
      </div>
```

- [ ] **Step 4: Verify**

Run: `cd src-tauri && cargo test --lib && cargo clippy --all-targets -- -D warnings && cd .. && npm run build`
Expected: all green. Then run the app (`npm run tauri dev`) on the three benchmark photos and your Gemini sample. Confirm that the result is the one from the report.

- [ ] **Step 5: Commit**

```bash
git add src src-tauri
git commit -m "feat: recover the image under semi-transparent watermarks"
```
