# Watermark Removal Engines — Analysis, Options and Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Replace the current "send the whole image to Gemini with coordinates in the prompt" approach with a mask-based, full-resolution pipeline that runs offline by default (no AI) and can plug in better AI backends (local LaMa, Gemini, Stability Erase) as optional engines.

**Architecture:** One Rust pipeline does everything that doesn't depend on the backend: EXIF-aware decoding, building a mask from the region, optionally shrinking that mask to the watermark's actual strokes, cropping a context window around it, calling an `Engine` on that crop only, and compositing the result back onto the untouched full-resolution original with a feathered edge. Each backend (Telea, PatchMatch, translucent "unblend", LaMa ONNX, Gemini, Stability) is one `Engine` variant.

**Tech Stack:** Tauri 2, Rust (`image` 0.25.9, `imageproc` 0.25, `tokio`, `reqwest`), React 19 + TypeScript. Later milestones: `ort` 2.0 (ONNX Runtime) + LaMa ONNX.

**Spec:** This document. Part I is the analysis and design, Part II covers the tasks for Milestone 1, and Part III is the roadmap for the later milestones. Each later milestone gets its own detailed plan once Milestone 1 lands.

---

# Part I — Analysis

## 1. How the app works today

```
React (RegionSelector) ──invoke('remove_watermark', {imagePath, region, options})──▶ Rust
Rust commands.rs: read API key → GeminiClient::remove_watermark(path, region)
gemini_client.rs: base64(whole file) + prompt "watermark is at x=…, y=…, w=…, h=…"
                  → POST gemini-2.5-flash-image:generateContent?key=API_KEY
                  ← whole new image (bytes) → written with the ORIGINAL extension
```

OpenCV (Telea/NS inpainting and video) was removed in `82a6f50` because the native builds were painful (see commits `3e52aea`…`7958c43`: vcpkg, LLVM downgrades, Windows breakage). The OpenCV code worked. It was dropped because the builds were painful, not because the results were poor.

## 2. Why the current approach is structurally wrong

The model choice isn't the main problem. The way the model gets used is. In order of impact:

| # | Problem | Where | Consequence |
|---|---------|-------|-------------|
| 1 | **No mask.** The region goes into the prompt as text coordinates. | `gemini_client.rs:110-116` | Image models don't map text pixel coordinates to locations reliably. The model guesses where the watermark is. Batch mode's "same region for all images" gets no benefit from this. |
| 2 | **The whole image is regenerated.** | `commands.rs:93-104` | Every pixel of the output is new. The whole image is resynthesised (and may be resampled), so fine detail everywhere changes, colours drift, and faces or text outside the watermark can be altered. The original pixels outside the region are never reused. |
| 3 | **Every output gets a SynthID watermark.** | Gemini platform behaviour | The "watermark remover" adds an invisible watermark to the whole image. (All Gemini image outputs carry SynthID, see sources.) |
| 4 | **MIME/extension mismatch.** Response `mimeType` is ignored and the bytes are written with the input extension. | `commands.rs:101-105, 125-131` | A `.jpg` input can produce PNG bytes saved as `processed_….jpg` with an `image/jpeg` data URL. |
| 5 | **"Lossless" does nothing useful.** | `commands.rs:141-170` | Re-encoding an AI-regenerated image as PNG can't bring back quality the model already threw away. The comment "image crate doesn't support lossless WebP" is wrong too: `image` 0.25 **only** has a lossless WebP encoder (`WebPEncoder::new_lossless`). |
| 6 | **Legacy model hard-coded.** | `gemini_client.rs:9` | `gemini-2.5-flash-image` is marked legacy. Google suggests moving to the 3.x image models. |
| 7 | **API key in the URL query string.** | `gemini_client.rs:151, 213, 236` | The key ends up in proxy logs and error strings (`format!("API request failed: {}", e)` includes the URL). Use the `x-goog-api-key` header. |
| 8 | **EXIF orientation ignored.** | `image_processor.rs:31` | The webview shows phone JPEGs rotated (it honours EXIF), but Rust reports unrotated dimensions. With a real mask, the region would land on the wrong pixels. |
| 9 | **Refusals are hidden.** | `gemini_client.rs:199-210` | When Gemini answers with text only (a policy refusal is common for "remove watermark"), the user sees "No image found in API response" and the model's actual explanation is lost. |
| 10 | **Batch loop reports a stale error.** | `App.tsx:141-151` | `error || 'Processing failed'` reads the `error` captured when the callback was created, not the one from the failed call. `processBatch` in the hook is a duplicate and isn't used. |
| 11 | **No tests at all.** | — | Neither `cargo test` nor a JS test runner covers anything. |

**Conclusion:** switching to a different model without fixing #1–#2 keeps most of the damage. Whatever backend is chosen, the fix is the same: **mask → crop → inpaint crop → composite onto the original**. That is Milestone 1, and every option below builds on it.

## 3. Options

### 3a. Keep AI and use it properly

| Option | How | Pros | Cons |
|---|---|---|---|
| **A. Gemini through the pipeline** | Send only the crop plus a binary mask as a second image, resize the answer back to the crop size, composite. Model id configurable (default to a 3.x image model). | Smallest change. Understands semantics (can rebuild a face or an object behind a logo). | Needs a key and internet, costs money, may refuse, still regenerates the crop and puts SynthID on the patch. |
| **B. Stability AI Erase** (`POST v2beta/stable-image/edit/erase`) | Multipart: image + **real binary mask**. 5 credits per call (≈ $0.05). | Built for exactly this task and takes a real mask. | Paid, another key, resamples above 4 MP (the crop approach makes that irrelevant). |
| **C. Local LaMa (ONNX) via `ort`** | `Carve/LaMa-ONNX` `lama_fp32.onnx` (Apache-2.0, **208 MB**, fixed 512×512 input). Crop a square context around the hole, scale to 512, infer, scale back, composite. | Offline, free, no key, no SynthID. Close to state of the art for object and watermark removal (it's the model behind IOPaint/lama-cleaner). Runs on CPU in about 1 s at 512². | +208 MB download (fetch on first use, not bundled). **`ort` has no prebuilt ONNX Runtime for `x86_64-apple-darwin`** (its `dist.tsv` lists aarch64-apple-darwin+coreml, Windows x64/arm64+directml, and Linux x64/arm64 only), so Intel Macs need `load-dynamic` plus a bundled `libonnxruntime.dylib`, or no LaMa on Intel. |

### 3b. No AI (classical image processing), all pure Rust with no OpenCV

| Option | How | Good at | Weak at |
|---|---|---|---|
| **D. Fast-marching inpainting (Telea 2004)** | Fill the hole from its boundary inwards with weighted averages of known neighbours. About 150 lines of our own code. | Thin text, lines, and logos on smooth or gradient backgrounds. Instant. | Large holes on texture (grass, fabric) come out blurred. |
| **E. Mask refinement** (not an engine, a pre-step) | Predict the background under the rectangle (Telea from the rectangle border), diff it against the real pixels, threshold with Otsu, dilate. You get a mask of just the **watermark strokes** instead of the whole rectangle. | Makes *every* engine better: most pixels in the rectangle keep their true content, and thin masks are easy to fill. | Falls back to the full rectangle on busy textures (guarded by a ratio check). |
| **F. PatchMatch / exemplar inpainting** (Barnes 2009, Wexler 2007) | Multi-scale nearest-neighbour patch search plus voting. This is the algorithm behind Photoshop's Content-Aware Fill. | Textures, repeating structures, large holes. No model files. | Slower (hundreds of ms to seconds). Can't invent semantic content like a face. About 500 lines. |
| **G. Translucent "unblend" (reverse alpha blending)** | Most stock watermarks are `I = α·W + (1−α)·J`. Estimate the colour `W` (usually white or black) and per-pixel `α`, then **recover** `J = (I − α·W)/(1−α)`. Use inpainting only where `α` is close to 1. | **Recovers the real image** under a semi-transparent watermark. This is the best-quality method for translucent watermarks, AI included, because it doesn't invent anything. | Opaque watermarks (fall back to D/F). Heavy JPEG compression adds noise at the edges. |
| **H. Multi-image matting** (Dekel et al., CVPR 2017) | In batch mode (same watermark, many images), the median of the image gradients across images isolates the watermark's own gradient. Poisson-integrate that to get `α·W`, estimate `α` and `W`, then unblend every image. | Exactly the case the batch mode already assumes. Very accurate from about 10+ images. Also works on video frames. | The most complex option. It only works if the watermark is identical and at the same position across images. |

**Licensing checks done:** the pure-Rust `inpaint` crate (Telea port, v0.1.7) is **EUPL-1.2**, a copyleft licence that doesn't fit an MIT app, so we write our own Telea (about 150 lines, below). `imageproc` is MIT. LaMa-ONNX is Apache-2.0. `ort` is MIT/Apache-2.0.

## 4. Recommendation

A tiered product:

1. **Default: "Local (fast)" = Telea + mask refinement.** No key, no network, instant. Fixes problems #1–#8 in one milestone. **(Milestone 1, detailed below.)**
2. **"Local (content-aware)" = PatchMatch** for textured backgrounds. (Milestone 2)
3. **"Recover translucent watermark" = unblend** in single-image and batch (multi-image) modes. This is the differentiator: it restores the real pixels. (Milestones 3 and 5)
4. **"Local AI (LaMa)"** as an optional high-quality mode with the model downloaded on first use. (Milestone 4)
5. **Cloud engines (Gemini fixed, Stability Erase)** kept as opt-in and behind the same pipeline. Gemini moves to the pipeline in Milestone 1 and Stability comes in Milestone 6.

This order ships a clearly better, AI-free app after Milestone 1. Each later milestone is independent and can be shipped on its own.

### Decisions needed from you before Milestone 2+

- Default engine: this plan makes the local engine the default (`telea`), so the app works without a key. Is that the product you want?
- LaMa on Intel Macs: ship a bundled universal `libonnxruntime.dylib` via `load-dynamic`, or hide LaMa on Intel?
- Keep Gemini at all? It's the only engine that puts SynthID on its output and it may refuse. Stability Erase is a better fit for cloud erase.
- Legal positioning: removing watermarks from images you don't hold rights to can breach copyright or DMCA §1202 / EU InfoSoc Art. 7. Removing a watermark from your own photos or licensed assets is fine. Consider a short in-app notice.

---

# Part II — Milestone 1: Mask-based pipeline + offline Telea engine

## Global Constraints

- Rust edition 2021, `image = "0.25"` (lockfile at 0.25.9); add `imageproc = "0.25"` only. No OpenCV, no C/C++ deps in this milestone.
- No EUPL/GPL crates (the app is MIT).
- Pixels farther than `3 × feather` from the mask must be **byte-identical** to the decoded original (after EXIF orientation).
- Output keeps the original's width, height and alpha channel.
- Output bytes always match their extension and MIME type.
- The Gemini API key goes only in the `x-goog-api-key` header, never in the URL.
- Tauri command arg names stay camelCase at the top level. Fields inside `options` are snake_case (`refine_mask`), matching serde.
- All new Rust code lives under `src-tauri/src/removal/`. Tests sit next to the code (`#[cfg(test)] mod tests`). The evaluation tool is `src-tauri/examples/eval.rs`.
- Run Rust commands from `src-tauri/`: `cargo test --lib`, `cargo clippy --all-targets -- -D warnings`.

## Review Focus

1. **Phone JPEG with EXIF orientation 6/8**: the region drawn on the rotated preview must land on the same pixels in the output. (Task 5 test `load_oriented_applies_exif_rotation`.)
2. **Region touching or overhanging the image border** (users drag past the edge): no panic, and the region is clamped. (Task 1 `clamps_region_overhanging_right_edge`, Task 3 `fills_hole_touching_border`.)
3. **Batch images with different sizes than the first image**: the region is clamped per image, and an error only when the region is completely outside. (Task 1 `rejects_region_outside_image`, Task 6 `pipeline_clamps_region_for_smaller_image`.)
4. **PNG with transparency**: alpha is preserved, not flattened to black. (Task 6 `pipeline_preserves_alpha`.)
5. **Gemini text-only reply (refusal)**: the user sees the model's text, not "No image found". (Task 7 `text_only_reply_surfaces_model_text`.)

## File Structure

```
src-tauri/src/
  lib.rs                    MODIFY  pub mod removal; pub mod image_processor; pub mod gemini_client
  image_processor.rs        MODIFY  RemovalOptions gains engine + refine_mask; dims via codec::load_oriented
  commands.rs               MODIFY  remove_watermark → pipeline; delete reencode_lossless
  gemini_client.rs          MODIFY  inpaint(crop, mask); header auth; model const; refusal text
  removal/
    mod.rs                  CREATE  module list
    geometry.rs             CREATE  Rect, clamp_region, expand, context_margin
    mask.rs                 CREATE  rect_mask, dilate, hole_count, HOLE
    telea.rs                CREATE  fast-marching inpainting
    composite.rs            CREATE  feathered composite onto the original
    codec.rs                CREATE  load_oriented, OutputFormat, encode
    refine.rs               CREATE  stroke-level mask refinement
    engine.rs               CREATE  enum Engine { Telea, Gemini, (test) Fill }
    pipeline.rs             CREATE  run(): the backend-independent flow
src-tauri/examples/eval.rs  CREATE  synthetic benchmark (PSNR per engine/config)
src/types/watermark.ts      MODIFY  EngineKind, RemovalOptions
src/components/OptionsPanel.tsx MODIFY engine picker, refine toggle
src/hooks/useWatermarkRemoval.ts MODIFY DEFAULT_OPTIONS
src/App.tsx                 MODIFY  DEFAULT_OPTIONS, API-key gating only for gemini, batch error fix
```

---

### Task 1: Geometry — clamp and expand regions

**Files:**
- Create: `src-tauri/src/removal/mod.rs`, `src-tauri/src/removal/geometry.rs`
- Modify: `src-tauri/src/lib.rs:1-3`, `src-tauri/Cargo.toml` (add `imageproc`)

**Interfaces:**
- Consumes: `crate::image_processor::WatermarkRegion { x: i32, y: i32, width: i32, height: i32 }`
- Produces: `pub struct Rect { pub x: u32, pub y: u32, pub w: u32, pub h: u32 }` (Copy, Eq, Debug); `pub fn clamp_region(r: &WatermarkRegion, img_w: u32, img_h: u32) -> Result<Rect, String>`; `pub fn expand(inner: Rect, margin: u32, img_w: u32, img_h: u32) -> Rect`; `pub fn context_margin(hole: Rect) -> u32`

- [ ] **Step 1: Wire modules and dependency**

`src-tauri/Cargo.toml` `[dependencies]` add:
```toml
imageproc = "0.25"
```
`src-tauri/src/lib.rs` replace the three `mod` lines with:
```rust
mod commands;
pub mod gemini_client;
pub mod image_processor;
pub mod removal;
```
`src-tauri/src/removal/mod.rs`:
```rust
pub mod geometry;
```

- [ ] **Step 2: Write the failing tests** in `src-tauri/src/removal/geometry.rs`

```rust
use crate::image_processor::WatermarkRegion;

#[cfg(test)]
mod tests {
    use super::*;

    fn region(x: i32, y: i32, width: i32, height: i32) -> WatermarkRegion {
        WatermarkRegion { x, y, width, height }
    }

    #[test]
    fn clamps_region_overhanging_right_edge() {
        let r = clamp_region(&region(90, 10, 20, 5), 100, 50).unwrap();
        assert_eq!(r, Rect { x: 90, y: 10, w: 10, h: 5 });
    }

    #[test]
    fn clamps_negative_origin() {
        let r = clamp_region(&region(-5, -5, 20, 20), 100, 50).unwrap();
        assert_eq!(r, Rect { x: 0, y: 0, w: 15, h: 15 });
    }

    #[test]
    fn rejects_region_outside_image() {
        assert!(clamp_region(&region(200, 10, 20, 5), 100, 50).is_err());
    }

    #[test]
    fn rejects_empty_region() {
        assert!(clamp_region(&region(10, 10, 0, 5), 100, 50).is_err());
    }

    #[test]
    fn expand_stops_at_borders() {
        let r = expand(Rect { x: 2, y: 2, w: 10, h: 10 }, 5, 15, 100);
        assert_eq!(r, Rect { x: 0, y: 0, w: 15, h: 17 });
    }

    #[test]
    fn context_margin_has_floor_and_scales() {
        assert_eq!(context_margin(Rect { x: 0, y: 0, w: 10, h: 10 }), 32);
        assert_eq!(context_margin(Rect { x: 0, y: 0, w: 200, h: 40 }), 100);
    }
}
```

- [ ] **Step 3: Run to verify failure**

Run: `cd src-tauri && cargo test --lib removal::geometry`
Expected: compile error `cannot find type Rect` / `cannot find function clamp_region`.

- [ ] **Step 4: Implement** (above the tests module)

```rust
/// Axis-aligned rectangle in image pixel coordinates.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Rect {
    pub x: u32,
    pub y: u32,
    pub w: u32,
    pub h: u32,
}

/// Intersects a user-drawn region with the image bounds.
pub fn clamp_region(r: &WatermarkRegion, img_w: u32, img_h: u32) -> Result<Rect, String> {
    if r.width <= 0 || r.height <= 0 {
        return Err("Region must have a positive size".to_string());
    }
    let x0 = r.x.max(0) as u32;
    let y0 = r.y.max(0) as u32;
    let x1 = ((r.x + r.width).max(0) as u32).min(img_w);
    let y1 = ((r.y + r.height).max(0) as u32).min(img_h);
    if x0 >= x1 || y0 >= y1 {
        return Err(format!("Region lies outside the {}x{} image", img_w, img_h));
    }
    Ok(Rect { x: x0, y: y0, w: x1 - x0, h: y1 - y0 })
}

/// Grows `inner` by `margin` on every side, clamped to the image.
pub fn expand(inner: Rect, margin: u32, img_w: u32, img_h: u32) -> Rect {
    let x0 = inner.x.saturating_sub(margin);
    let y0 = inner.y.saturating_sub(margin);
    let x1 = (inner.x + inner.w + margin).min(img_w);
    let y1 = (inner.y + inner.h + margin).min(img_h);
    Rect { x: x0, y: y0, w: x1 - x0, h: y1 - y0 }
}

/// How much surrounding context an engine gets around the hole.
pub fn context_margin(hole: Rect) -> u32 {
    (hole.w.max(hole.h) / 2).max(32)
}
```

- [ ] **Step 5: Run to verify pass**

Run: `cd src-tauri && cargo test --lib removal::geometry`
Expected: 6 passed.

- [ ] **Step 6: Commit**

```bash
git add src-tauri/Cargo.toml src-tauri/Cargo.lock src-tauri/src/lib.rs src-tauri/src/removal
git commit -m "feat(removal): add region geometry helpers"
```

---

### Task 2: Masks

**Files:**
- Create: `src-tauri/src/removal/mask.rs`
- Modify: `src-tauri/src/removal/mod.rs` (add `pub mod mask;`)

**Interfaces:**
- Consumes: `Rect` (Task 1)
- Produces: `pub const HOLE: u8 = 255;` `pub fn rect_mask(crop: Rect, hole: Rect) -> GrayImage` (crop-sized, 255 inside `hole`); `pub fn dilate(mask: &GrayImage, radius: u8) -> GrayImage`; `pub fn hole_count(mask: &GrayImage) -> usize`

- [ ] **Step 1: Write the failing tests** in `mask.rs`

```rust
use super::geometry::Rect;
use image::{GrayImage, Luma};
use imageproc::distance_transform::Norm;
use imageproc::morphology;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rect_mask_marks_exactly_the_hole_in_crop_coordinates() {
        let crop = Rect { x: 10, y: 10, w: 20, h: 20 };
        let hole = Rect { x: 15, y: 12, w: 4, h: 3 };
        let m = rect_mask(crop, hole);
        assert_eq!(m.dimensions(), (20, 20));
        assert_eq!(hole_count(&m), 12);
        assert_eq!(m.get_pixel(5, 2)[0], HOLE);
        assert_eq!(m.get_pixel(8, 4)[0], HOLE);
        assert_eq!(m.get_pixel(9, 4)[0], 0);
        assert_eq!(m.get_pixel(5, 5)[0], 0);
    }

    #[test]
    fn dilate_grows_square_by_radius() {
        let crop = Rect { x: 0, y: 0, w: 20, h: 20 };
        let m = rect_mask(crop, Rect { x: 10, y: 10, w: 1, h: 1 });
        assert_eq!(hole_count(&dilate(&m, 2)), 25);
        assert_eq!(hole_count(&dilate(&m, 0)), 1);
    }
}
```

- [ ] **Step 2: Run to verify failure**

Run: `cd src-tauri && cargo test --lib removal::mask`
Expected: compile error `cannot find function rect_mask`.

- [ ] **Step 3: Implement** (above the tests module)

```rust
/// Mask value marking pixels an engine must fill.
pub const HOLE: u8 = 255;

/// Crop-sized mask with `hole` (given in image coordinates) set to HOLE.
pub fn rect_mask(crop: Rect, hole: Rect) -> GrayImage {
    GrayImage::from_fn(crop.w, crop.h, |x, y| {
        let (ix, iy) = (crop.x + x, crop.y + y);
        let inside = ix >= hole.x && ix < hole.x + hole.w && iy >= hole.y && iy < hole.y + hole.h;
        Luma([if inside { HOLE } else { 0 }])
    })
}

/// Grows the hole by `radius` pixels (square structuring element).
pub fn dilate(mask: &GrayImage, radius: u8) -> GrayImage {
    if radius == 0 {
        return mask.clone();
    }
    morphology::dilate(mask, Norm::LInf, radius)
}

pub fn hole_count(mask: &GrayImage) -> usize {
    mask.pixels().filter(|p| p[0] == HOLE).count()
}
```

- [ ] **Step 4: Run to verify pass**

Run: `cd src-tauri && cargo test --lib removal::mask`
Expected: 2 passed.

- [ ] **Step 5: Commit**

```bash
git add src-tauri/src/removal
git commit -m "feat(removal): add mask construction and dilation"
```

---

### Task 3: Telea fast-marching inpainting (pure Rust, no AI)

**Files:**
- Create: `src-tauri/src/removal/telea.rs`
- Modify: `src-tauri/src/removal/mod.rs` (add `pub mod telea;`)

**Interfaces:**
- Consumes: `HOLE` (Task 2)
- Produces: `pub fn inpaint(img: &RgbImage, mask: &GrayImage, radius: u32) -> Result<RgbImage, String>`. It returns an image the same size as `img`, where only `HOLE` pixels change.

Algorithm (Telea 2004, as in OpenCV `INPAINT_TELEA`, without the image-gradient correction term): flags KNOWN/BAND/INSIDE, distance `T` solved with the eikonal equation, hole pixels filled in increasing `T` order as a weighted mean of known pixels within `radius`. Weights = direction (alignment with ∇T) × distance (1/|r|³) × level-set (1/(1+|ΔT|)). A min-heap on `f32::to_bits` works because `T ≥ 0`.

- [ ] **Step 1: Write the failing tests** in `telea.rs`

```rust
use super::mask::HOLE;
use image::{GrayImage, Rgb, RgbImage};
use std::cmp::Reverse;
use std::collections::BinaryHeap;

#[cfg(test)]
mod tests {
    use super::*;
    use image::Luma;

    fn hole_mask(w: u32, h: u32, x0: u32, y0: u32, hw: u32, hh: u32) -> GrayImage {
        GrayImage::from_fn(w, h, |x, y| {
            let inside = x >= x0 && x < x0 + hw && y >= y0 && y < y0 + hh;
            Luma([if inside { HOLE } else { 0 }])
        })
    }

    #[test]
    fn reconstructs_flat_colour_exactly() {
        let img = RgbImage::from_pixel(40, 30, Rgb([120, 80, 200]));
        let mut damaged = img.clone();
        for y in 10..20 { for x in 12..28 { damaged.put_pixel(x, y, Rgb([255, 255, 255])); } }
        let out = inpaint(&damaged, &hole_mask(40, 30, 12, 10, 16, 10), 5).unwrap();
        assert_eq!(out, img);
    }

    #[test]
    fn approximates_horizontal_gradient() {
        // Loose bounds on purpose. Quality is measured by examples/eval.rs, not here.
        let img = RgbImage::from_fn(100, 40, |x, _| { let v = (x as f32 * 2.55) as u8; Rgb([v, v, v]) });
        let mut damaged = img.clone();
        for y in 15..25 { for x in 45..55 { damaged.put_pixel(x, y, Rgb([0, 0, 0])); } }
        let out = inpaint(&damaged, &hole_mask(100, 40, 45, 15, 10, 10), 5).unwrap();
        let mut total = 0u32;
        for y in 15..25 { for x in 45..55 {
            let d = (out.get_pixel(x, y)[0] as i32 - img.get_pixel(x, y)[0] as i32).unsigned_abs();
            assert!(d <= 25, "pixel ({x},{y}) off by {d}");
            total += d;
        } }
        assert!(total as f32 / 100.0 < 8.0, "mean error {}", total as f32 / 100.0);
    }

    #[test]
    fn leaves_known_pixels_untouched() {
        let img = RgbImage::from_fn(30, 30, |x, y| Rgb([(x * 8) as u8, (y * 8) as u8, 50]));
        let mask = hole_mask(30, 30, 10, 10, 5, 5);
        let out = inpaint(&img, &mask, 5).unwrap();
        for (x, y, p) in out.enumerate_pixels() {
            if mask.get_pixel(x, y)[0] != HOLE { assert_eq!(p, img.get_pixel(x, y)); }
        }
    }

    #[test]
    fn fills_hole_touching_border() {
        let img = RgbImage::from_pixel(20, 20, Rgb([10, 20, 30]));
        let out = inpaint(&img, &hole_mask(20, 20, 0, 0, 6, 20), 5).unwrap();
        assert_eq!(out.get_pixel(0, 0), &Rgb([10, 20, 30]));
    }

    #[test]
    fn rejects_mask_covering_everything() {
        let img = RgbImage::new(8, 8);
        assert!(inpaint(&img, &hole_mask(8, 8, 0, 0, 8, 8), 5).is_err());
    }

    #[test]
    fn rejects_mismatched_mask() {
        assert!(inpaint(&RgbImage::new(8, 8), &GrayImage::new(4, 4), 5).is_err());
    }
}
```

- [ ] **Step 2: Run to verify failure**

Run: `cd src-tauri && cargo test --lib removal::telea`
Expected: compile error `cannot find function inpaint`.

- [ ] **Step 3: Implement** (above the tests module)

```rust
const KNOWN: u8 = 0;
const BAND: u8 = 1;
const INSIDE: u8 = 2;
const INF: f32 = 1.0e6;
const NEIGHBOURS: [(i64, i64); 4] = [(1, 0), (-1, 0), (0, 1), (0, -1)];

struct Grid {
    w: i64,
    h: i64,
    flags: Vec<u8>,
    t: Vec<f32>,
}

impl Grid {
    fn idx(&self, x: i64, y: i64) -> Option<usize> {
        (x >= 0 && y >= 0 && x < self.w && y < self.h).then(|| (y * self.w + x) as usize)
    }

    /// T at (x, y) if it is not still inside the hole (KNOWN or BAND).
    fn settled_t(&self, x: i64, y: i64) -> Option<f32> {
        self.idx(x, y).filter(|&k| self.flags[k] != INSIDE).map(|k| self.t[k])
    }

    /// Eikonal update from two orthogonal neighbours; mirrors OpenCV's
    /// `FastMarching_solve` (KNOWN and BAND neighbours both contribute).
    fn solve(&self, a: (i64, i64), b: (i64, i64)) -> f32 {
        match (self.settled_t(a.0, a.1), self.settled_t(b.0, b.1)) {
            (Some(t1), Some(t2)) if (t1 - t2).abs() >= 1.0 => 1.0 + t1.min(t2),
            (Some(t1), Some(t2)) => (t1 + t2 + (2.0 - (t1 - t2) * (t1 - t2)).sqrt()) / 2.0,
            (Some(t1), None) => 1.0 + t1,
            (None, Some(t2)) => 1.0 + t2,
            (None, None) => INF,
        }
    }

    fn grad(&self, x: i64, y: i64) -> (f32, f32) {
        let tp = self.t[(y * self.w + x) as usize];
        let axis = |fwd: Option<f32>, back: Option<f32>| match (fwd, back) {
            (Some(a), Some(b)) => (a - b) / 2.0,
            (Some(a), None) => a - tp,
            (None, Some(b)) => tp - b,
            (None, None) => 0.0,
        };
        (
            axis(self.settled_t(x + 1, y), self.settled_t(x - 1, y)),
            axis(self.settled_t(x, y + 1), self.settled_t(x, y - 1)),
        )
    }
}

/// Fast-marching inpainting (Telea 2004) without the image-gradient term.
/// Fills every `HOLE` pixel of `mask` from the known pixels within `radius`.
pub fn inpaint(img: &RgbImage, mask: &GrayImage, radius: u32) -> Result<RgbImage, String> {
    let (w, h) = img.dimensions();
    if mask.dimensions() != (w, h) {
        return Err("Mask size does not match image size".to_string());
    }
    let mut g = Grid {
        w: w as i64,
        h: h as i64,
        flags: vec![KNOWN; (w * h) as usize],
        t: vec![0.0; (w * h) as usize],
    };
    for (x, y, m) in mask.enumerate_pixels() {
        if m[0] == HOLE {
            let i = (y * w + x) as usize;
            g.flags[i] = INSIDE;
            g.t[i] = INF;
        }
    }
    if g.flags.iter().all(|&f| f == INSIDE) {
        return Err("Nothing to inpaint from: the mask covers the whole area".to_string());
    }

    let mut px: Vec<[f32; 3]> = img.pixels().map(|p| [p[0] as f32, p[1] as f32, p[2] as f32]).collect();
    let mut heap = BinaryHeap::new();
    for y in 0..g.h {
        for x in 0..g.w {
            let i = (y * g.w + x) as usize;
            let touches_hole = NEIGHBOURS
                .iter()
                .any(|(dx, dy)| g.idx(x + dx, y + dy).is_some_and(|k| g.flags[k] == INSIDE));
            if g.flags[i] == KNOWN && touches_hole {
                g.flags[i] = BAND;
                heap.push(Reverse((0.0f32.to_bits(), i)));
            }
        }
    }

    let r = radius as i64;
    while let Some(Reverse((_, i))) = heap.pop() {
        g.flags[i] = KNOWN;
        let (x, y) = (i as i64 % g.w, i as i64 / g.w);
        for (dx, dy) in NEIGHBOURS {
            let (nx, ny) = (x + dx, y + dy);
            let Some(j) = g.idx(nx, ny) else { continue };
            if g.flags[j] != INSIDE {
                continue;
            }
            g.t[j] = [(-1, -1), (1, -1), (-1, 1), (1, 1)]
                .iter()
                .map(|(sx, sy)| g.solve((nx + sx, ny), (nx, ny + sy)))
                .fold(INF, f32::min);
            g.flags[j] = BAND;
            px[j] = fill_pixel(&g, &px, nx, ny, r);
            heap.push(Reverse((g.t[j].to_bits(), j)));
        }
    }

    Ok(RgbImage::from_fn(w, h, |x, y| {
        let p = px[(y * w + x) as usize];
        Rgb([p[0].round() as u8, p[1].round() as u8, p[2].round() as u8])
    }))
}

fn fill_pixel(g: &Grid, px: &[[f32; 3]], x: i64, y: i64, r: i64) -> [f32; 3] {
    let i = (y * g.w + x) as usize;
    let (gx, gy) = g.grad(x, y);
    let mut acc = [0.0f32; 3];
    let mut wsum = 0.0f32;
    for qy in (y - r).max(0)..=(y + r).min(g.h - 1) {
        for qx in (x - r).max(0)..=(x + r).min(g.w - 1) {
            let k = (qy * g.w + qx) as usize;
            if k == i || g.flags[k] == INSIDE {
                continue;
            }
            let (rx, ry) = ((x - qx) as f32, (y - qy) as f32);
            let d2 = rx * rx + ry * ry;
            if d2 > (r * r) as f32 {
                continue;
            }
            let mut dir = rx * gx + ry * gy;
            if dir.abs() <= 0.01 {
                dir = 1.0e-6;
            }
            let dst = 1.0 / (d2 * d2.sqrt());
            let lev = 1.0 / (1.0 + (g.t[k] - g.t[i]).abs());
            let wgt = (dir * dst * lev).abs();
            for c in 0..3 {
                acc[c] += wgt * px[k][c];
            }
            wsum += wgt;
        }
    }
    if wsum > 0.0 {
        acc.map(|a| a / wsum)
    } else {
        px[i]
    }
}
```

- [ ] **Step 4: Run to verify pass**

Run: `cd src-tauri && cargo test --lib removal::telea`
Expected: 6 passed. If `approximates_horizontal_gradient` fails, the algorithm is wrong (compare against OpenCV's `cv::inpaint` source). Don't loosen the bounds.

- [ ] **Step 5: Commit**

```bash
git add src-tauri/src/removal
git commit -m "feat(removal): pure-Rust Telea fast-marching inpainting"
```

---

### Task 4: Feathered composite

**Files:**
- Create: `src-tauri/src/removal/composite.rs`
- Modify: `src-tauri/src/removal/mod.rs` (add `pub mod composite;`)

**Interfaces:**
- Consumes: `Rect`, `dilate`, `HOLE`
- Produces: `pub fn composite(base: &mut RgbImage, patch: &RgbImage, crop: Rect, mask: &GrayImage, feather: u8)`. It requires `patch` and `mask` to be `crop`-sized. Hole pixels are fully replaced, a band of about `feather` px around them is blended, and everything else stays byte-identical.

- [ ] **Step 1: Write the failing tests**

```rust
use super::geometry::Rect;
use super::mask::{dilate, HOLE};
use image::{GrayImage, RgbImage};
use imageproc::filter::gaussian_blur_f32;

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
```

- [ ] **Step 2: Run to verify failure**

Run: `cd src-tauri && cargo test --lib removal::composite`
Expected: compile error `cannot find function composite`.

- [ ] **Step 3: Implement**

```rust
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
```

- [ ] **Step 4: Run to verify pass**

Run: `cd src-tauri && cargo test --lib removal::composite`
Expected: 2 passed.

- [ ] **Step 5: Commit**

```bash
git add src-tauri/src/removal
git commit -m "feat(removal): feathered composite onto the original"
```

---

### Task 5: Codec — EXIF-aware decode, format-correct encode

**Files:**
- Create: `src-tauri/src/removal/codec.rs`
- Modify: `src-tauri/src/removal/mod.rs` (add `pub mod codec;`), `src-tauri/src/image_processor.rs:31-35`

**Interfaces:**
- Produces: `pub fn load_oriented(path: &str) -> Result<DynamicImage, String>`; `pub enum OutputFormat { Png, Jpeg, WebpLossless }` with `extension(self) -> &'static str`, `mime(self) -> &'static str`; `pub fn output_format(original_ext: &str, lossless: bool) -> OutputFormat`; `pub fn encode(img: &DynamicImage, format: OutputFormat) -> Result<Vec<u8>, String>`

- [ ] **Step 1: Write the failing tests**

```rust
use image::codecs::jpeg::JpegEncoder;
use image::{DynamicImage, ImageFormat, ImageReader};
use std::io::Cursor;

#[cfg(test)]
mod tests {
    use super::*;
    use image::{GenericImageView, Rgb, RgbImage};

    #[test]
    fn picks_format_from_extension() {
        assert_eq!(output_format("JPG", false), OutputFormat::Jpeg);
        assert_eq!(output_format("jpeg", true), OutputFormat::Png);
        assert_eq!(output_format("webp", false), OutputFormat::WebpLossless);
        assert_eq!(output_format("gif", false), OutputFormat::Png);
        assert_eq!(output_format("png", false), OutputFormat::Png);
    }

    #[test]
    fn encoded_bytes_match_declared_format() {
        let img = DynamicImage::ImageRgb8(RgbImage::from_pixel(8, 8, Rgb([1, 2, 3])));
        for (fmt, expected) in [
            (OutputFormat::Png, ImageFormat::Png),
            (OutputFormat::Jpeg, ImageFormat::Jpeg),
            (OutputFormat::WebpLossless, ImageFormat::WebP),
        ] {
            let bytes = encode(&img, fmt).unwrap();
            assert_eq!(image::guess_format(&bytes).unwrap(), expected);
        }
    }

    #[test]
    fn load_oriented_applies_exif_rotation() {
        // 4x2 JPEG with EXIF Orientation=6 (rotate 90° CW) must load as 2x4.
        let path = std::env::temp_dir().join("wmr_orientation6.jpg");
        std::fs::write(&path, jpeg_with_orientation(4, 2, 6)).unwrap();
        let img = load_oriented(path.to_str().unwrap()).unwrap();
        assert_eq!(img.dimensions(), (2, 4));
    }

    /// Minimal JPEG with an APP1/EXIF segment carrying only the Orientation tag.
    fn jpeg_with_orientation(w: u32, h: u32, orientation: u16) -> Vec<u8> {
        let mut jpeg = Vec::new();
        JpegEncoder::new_with_quality(&mut jpeg, 90)
            .encode_image(&RgbImage::from_pixel(w, h, Rgb([50, 60, 70])))
            .unwrap();
        let mut tiff = vec![b'M', b'M', 0, 42, 0, 0, 0, 8, 0, 1];
        tiff.extend_from_slice(&[0x01, 0x12, 0, 3, 0, 0, 0, 1]);
        tiff.extend_from_slice(&orientation.to_be_bytes());
        tiff.extend_from_slice(&[0, 0, 0, 0, 0, 0]);
        let mut app1 = b"Exif\0\0".to_vec();
        app1.extend_from_slice(&tiff);
        let len = (app1.len() + 2) as u16;
        let mut out = jpeg[..2].to_vec(); // SOI
        out.extend_from_slice(&[0xFF, 0xE1]);
        out.extend_from_slice(&len.to_be_bytes());
        out.extend_from_slice(&app1);
        out.extend_from_slice(&jpeg[2..]);
        out
    }
}
```

- [ ] **Step 2: Run to verify failure**

Run: `cd src-tauri && cargo test --lib removal::codec`
Expected: compile error `cannot find type OutputFormat`.

- [ ] **Step 3: Implement**

```rust
/// Decodes an image and applies its EXIF orientation, so pixel coordinates
/// match what the webview shows.
pub fn load_oriented(path: &str) -> Result<DynamicImage, String> {
    let mut decoder = ImageReader::open(path)
        .and_then(|r| r.with_guessed_format())
        .map_err(|e| format!("Failed to open image: {}", e))?
        .into_decoder()
        .map_err(|e| format!("Failed to decode image: {}", e))?;
    let orientation = image::ImageDecoder::orientation(&mut decoder)
        .map_err(|e| format!("Failed to read orientation: {}", e))?;
    let mut img = DynamicImage::from_decoder(decoder).map_err(|e| format!("Failed to decode image: {}", e))?;
    img.apply_orientation(orientation);
    Ok(img)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OutputFormat {
    Png,
    Jpeg,
    /// The `image` crate's WebP encoder is lossless-only.
    WebpLossless,
}

impl OutputFormat {
    pub fn extension(self) -> &'static str {
        match self {
            OutputFormat::Png => "png",
            OutputFormat::Jpeg => "jpg",
            OutputFormat::WebpLossless => "webp",
        }
    }

    pub fn mime(self) -> &'static str {
        match self {
            OutputFormat::Png => "image/png",
            OutputFormat::Jpeg => "image/jpeg",
            OutputFormat::WebpLossless => "image/webp",
        }
    }
}

/// Keeps the original container where possible; `lossless` turns JPEG into PNG.
pub fn output_format(original_ext: &str, lossless: bool) -> OutputFormat {
    match (original_ext.to_ascii_lowercase().as_str(), lossless) {
        ("jpg" | "jpeg", false) => OutputFormat::Jpeg,
        ("webp", _) => OutputFormat::WebpLossless,
        _ => OutputFormat::Png,
    }
}

pub fn encode(img: &DynamicImage, format: OutputFormat) -> Result<Vec<u8>, String> {
    let mut buf = Cursor::new(Vec::new());
    match format {
        OutputFormat::Jpeg => JpegEncoder::new_with_quality(&mut buf, 95)
            .encode_image(&img.to_rgb8())
            .map_err(|e| format!("Failed to encode JPEG: {}", e))?,
        OutputFormat::Png => img
            .write_to(&mut buf, ImageFormat::Png)
            .map_err(|e| format!("Failed to encode PNG: {}", e))?,
        OutputFormat::WebpLossless => DynamicImage::ImageRgba8(img.to_rgba8())
            .write_to(&mut buf, ImageFormat::WebP)
            .map_err(|e| format!("Failed to encode WebP: {}", e))?,
    }
    Ok(buf.into_inner())
}
```

In `image_processor.rs`, replace `get_image_dimensions` with:
```rust
pub fn get_image_dimensions(image_path: &str) -> Result<(u32, u32), String> {
    Ok(crate::removal::codec::load_oriented(image_path)?.dimensions())
}
```

- [ ] **Step 4: Run to verify pass**

Run: `cd src-tauri && cargo test --lib removal::codec`
Expected: 3 passed.

- [ ] **Step 5: Commit**

```bash
git add src-tauri/src
git commit -m "feat(removal): EXIF-aware decoding and format-correct encoding"
```

---

### Task 6: Mask refinement, engine enum and the pipeline

**Files:**
- Create: `src-tauri/src/removal/refine.rs`, `src-tauri/src/removal/engine.rs`, `src-tauri/src/removal/pipeline.rs`
- Modify: `src-tauri/src/removal/mod.rs` (add `pub mod refine; pub mod engine; pub mod pipeline;`), `src-tauri/src/image_processor.rs:12-28`

**Interfaces:**
- Consumes: Tasks 1–5.
- Produces:
  - `refine::refine_mask(crop: &RgbImage, rect_mask: &GrayImage, grow: u8) -> Result<Refined, String>`, `pub struct Refined { pub mask: GrayImage, pub used_refinement: bool }`
  - `engine::Engine` enum: `Telea { radius: u32 }`, `Gemini(GeminiClient)` (the variant is added in Task 7), `#[cfg(test)] Fill([u8; 3])`; `async fn inpaint(&self, crop: RgbImage, mask: GrayImage) -> Result<RgbImage, String>`
  - `pipeline::PipelineOptions { refine_mask: bool, grow: u8, feather: u8 }`, `pipeline::PipelineOutput { image: DynamicImage, mask_refined: bool }`, `pipeline::run(original: &DynamicImage, region: &WatermarkRegion, opts: &PipelineOptions, engine: &Engine) -> Result<PipelineOutput, String>` (async)
  - `image_processor::EngineKind { Telea, Gemini }` (serde lowercase), and `RemovalOptions` gains `engine: EngineKind` (default Telea) and `refine_mask: bool` (default true)

- [ ] **Step 1: Extend `RemovalOptions`** in `image_processor.rs` (replace the struct, `default_lossless` and `impl Default`):

```rust
#[derive(Debug, Serialize, Deserialize, Clone, Copy, PartialEq, Eq, Default)]
#[serde(rename_all = "lowercase")]
pub enum EngineKind {
    #[default]
    Telea,
    Gemini,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct RemovalOptions {
    #[serde(default)]
    pub lossless: bool,
    #[serde(default)]
    pub engine: EngineKind,
    #[serde(default = "default_refine_mask")]
    pub refine_mask: bool,
}

fn default_refine_mask() -> bool {
    true
}

impl Default for RemovalOptions {
    fn default() -> Self {
        Self { lossless: false, engine: EngineKind::default(), refine_mask: default_refine_mask() }
    }
}
```

- [ ] **Step 2: Write failing tests for refinement** (`refine.rs`)

```rust
use super::mask::{dilate, hole_count, HOLE};
use super::telea;
use image::{GrayImage, Luma, RgbImage};
use imageproc::contrast::otsu_level;

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
        for x in 25..55 { for y in 29..31 { img.put_pixel(x, y, Rgb([250, 250, 250])); } }
        let r = refine_mask(&img, &rect, 1).unwrap();
        assert!(r.used_refinement);
        assert_eq!(r.mask.get_pixel(30, 30)[0], HOLE);
        assert_eq!(r.mask.get_pixel(30, 22)[0], 0, "background inside the rectangle is kept");
        assert!(hole_count(&r.mask) < hole_count(&rect) / 2);
    }

    #[test]
    fn falls_back_to_rectangle_for_opaque_block() {
        let (mut img, rect) = setup();
        for x in 20..60 { for y in 20..40 { img.put_pixel(x, y, Rgb([0, 0, 0])); } }
        let r = refine_mask(&img, &rect, 1).unwrap();
        assert!(!r.used_refinement);
        assert_eq!(r.mask, rect);
    }
}
```

- [ ] **Step 3: Implement refinement**

```rust
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
```

Run: `cd src-tauri && cargo test --lib removal::refine` → 2 passed.

- [ ] **Step 4: Write the engine** (`engine.rs`; the Gemini variant arrives in Task 7)

```rust
use super::telea;
use image::{GrayImage, RgbImage};

pub enum Engine {
    Telea { radius: u32 },
    #[cfg(test)]
    Fill([u8; 3]),
}

impl Engine {
    /// Returns an image the size of `crop` with the HOLE pixels of `mask` filled.
    pub async fn inpaint(&self, crop: RgbImage, mask: GrayImage) -> Result<RgbImage, String> {
        match self {
            Engine::Telea { radius } => {
                let radius = *radius;
                tokio::task::spawn_blocking(move || telea::inpaint(&crop, &mask, radius))
                    .await
                    .map_err(|e| format!("Inpainting task failed: {}", e))?
            }
            #[cfg(test)]
            Engine::Fill(c) => Ok(RgbImage::from_fn(crop.width(), crop.height(), |x, y| {
                if mask.get_pixel(x, y)[0] == super::mask::HOLE { image::Rgb(*c) } else { *crop.get_pixel(x, y) }
            })),
        }
    }
}
```

- [ ] **Step 5: Write failing pipeline tests** (`pipeline.rs`)

```rust
use super::composite::composite;
use super::engine::Engine;
use super::geometry::{clamp_region, context_margin, expand};
use super::mask::{dilate, rect_mask};
use super::refine::refine_mask;
use crate::image_processor::WatermarkRegion;
use image::imageops::{self, FilterType};
use image::{DynamicImage, GenericImageView, Rgba, RgbaImage, RgbImage};

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

    #[tokio::test]
    async fn pipeline_keeps_size_and_untouched_pixels() {
        let src = DynamicImage::ImageRgb8(RgbImage::from_fn(300, 200, |x, y| Rgb([x as u8, y as u8, 7])));
        let out = run(&src, &region(100, 80, 40, 20), &opts(), &Engine::Fill([255, 0, 255])).await.unwrap();
        assert_eq!(out.image.dimensions(), (300, 200));
        let (a, b) = (src.to_rgb8(), out.image.to_rgb8());
        assert_eq!(b.get_pixel(120, 90), &Rgb([255, 0, 255]));
        assert_eq!(b.get_pixel(10, 10), a.get_pixel(10, 10));
        assert_eq!(b.get_pixel(299, 199), a.get_pixel(299, 199));
        assert_eq!(b.get_pixel(100 - 7, 90), a.get_pixel(100 - 7, 90));
    }

    #[tokio::test]
    async fn pipeline_preserves_alpha() {
        let src = DynamicImage::ImageRgba8(RgbaImage::from_pixel(64, 64, Rgba([10, 20, 30, 77])));
        let out = run(&src, &region(20, 20, 10, 10), &opts(), &Engine::Fill([0, 0, 0])).await.unwrap();
        assert!(out.image.color().has_alpha());
        assert_eq!(out.image.to_rgba8().get_pixel(25, 25)[3], 77);
    }

    #[tokio::test]
    async fn pipeline_clamps_region_for_smaller_image() {
        let src = DynamicImage::ImageRgb8(RgbImage::new(50, 50));
        let out = run(&src, &region(40, 40, 30, 30), &opts(), &Engine::Fill([9, 9, 9])).await.unwrap();
        assert_eq!(out.image.to_rgb8().get_pixel(49, 49), &Rgb([9, 9, 9]));
        assert!(run(&src, &region(60, 60, 5, 5), &opts(), &Engine::Fill([9, 9, 9])).await.is_err());
    }

    #[tokio::test]
    async fn pipeline_with_telea_removes_stroke_on_flat_background() {
        let mut img = RgbImage::from_pixel(120, 80, Rgb([70, 140, 200]));
        for x in 40..80 { for y in 39..41 { img.put_pixel(x, y, Rgb([255, 255, 255])); } }
        let src = DynamicImage::ImageRgb8(img);
        let o = PipelineOptions { refine_mask: true, grow: 1, feather: 2 };
        let out = run(&src, &region(35, 30, 50, 20), &o, &Engine::Telea { radius: 5 }).await.unwrap();
        assert!(out.mask_refined);
        assert_eq!(out.image.to_rgb8().get_pixel(60, 40), &Rgb([70, 140, 200]));
    }
}
```

Run: `cd src-tauri && cargo test --lib removal::pipeline`. Expected: compile error `cannot find function run`.

- [ ] **Step 6: Implement the pipeline** (above the tests module)

```rust
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
pub async fn run(
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

    let rect = rect_mask(crop, hole);
    let (mask, mask_refined) = if opts.refine_mask {
        let r = refine_mask(&crop_img, &rect, opts.grow)?;
        (r.mask, r.used_refinement)
    } else {
        (dilate(&rect, opts.grow), false)
    };

    let patch = engine.inpaint(crop_img, mask.clone()).await?;
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
```

- [ ] **Step 7: Run all removal tests**

Run: `cd src-tauri && cargo test --lib removal`
Expected: every test in geometry, mask, telea, composite, codec, refine and pipeline passes.

- [ ] **Step 8: Commit**

```bash
git add src-tauri/src
git commit -m "feat(removal): mask refinement, engine enum and crop/composite pipeline"
```

---

### Task 7: Gemini as a pipeline engine (crop + mask, header auth, refusal text)

**Files:**
- Modify: `src-tauri/src/gemini_client.rs` (whole `remove_watermark` fn, `GEMINI_API_URL`, `test_connection`, `list_models`), `src-tauri/src/removal/engine.rs`

**Interfaces:**
- Produces: `GeminiClient::inpaint(&self, crop: &RgbImage, mask: &GrayImage) -> Result<RgbImage, String>`; `fn extract_image(response_text: &str) -> Result<Vec<u8>, String>` (private, tested); `Engine::Gemini(GeminiClient)`

- [ ] **Step 1: Confirm the model id.** Run the app (`npm run tauri dev`), open Settings, and use the existing `list_gemini_models` button (or `curl -H "x-goog-api-key: $KEY" https://generativelanguage.googleapis.com/v1beta/models | grep image`). Pick the current Flash image model (expected `gemini-3.1-flash-image`) and use that string below.

- [ ] **Step 2: Write the failing parsing tests** (append to `gemini_client.rs`)

```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn text_only_reply_surfaces_model_text() {
        let body = r#"{"candidates":[{"content":{"parts":[{"text":"I can't help remove watermarks."}]}}]}"#;
        let err = extract_image(body).unwrap_err();
        assert!(err.contains("I can't help remove watermarks."), "{err}");
    }

    #[test]
    fn image_reply_is_decoded() {
        let body = format!(
            r#"{{"candidates":[{{"content":{{"parts":[{{"inlineData":{{"mimeType":"image/png","data":"{}"}}}}]}}}}]}}"#,
            BASE64.encode([1u8, 2, 3])
        );
        assert_eq!(extract_image(&body).unwrap(), vec![1, 2, 3]);
    }
}
```

Run: `cd src-tauri && cargo test --lib gemini_client`. Expected: compile error `cannot find function extract_image`.

- [ ] **Step 3: Implement.** Replace the `GEMINI_API_URL` const with:
```rust
const GEMINI_API_BASE: &str = "https://generativelanguage.googleapis.com/v1beta";
const GEMINI_IMAGE_MODEL: &str = "gemini-3.1-flash-image"; // value confirmed in Step 1
```
Replace `pub async fn remove_watermark(...)` (the entire function) with:
```rust
    /// Fills the white pixels of `mask` in `crop`. Only the crop is sent; the
    /// caller composites the answer back onto the full-resolution original.
    pub async fn inpaint(&self, crop: &RgbImage, mask: &GrayImage) -> Result<RgbImage, String> {
        let prompt = "The first image is a crop of a photo. The second image is a mask of the same size: \
            white pixels mark a watermark or overlay. Return the first image with ONLY the white-masked \
            pixels replaced by background that naturally continues the surrounding content. Keep the \
            framing, size, colours and every unmasked pixel unchanged. Do not add any text.";
        let request = GeminiRequest {
            contents: vec![Content {
                parts: vec![
                    Part::InlineData { inline_data: png_part(&DynamicImage::ImageRgb8(crop.clone()))? },
                    Part::InlineData { inline_data: png_part(&DynamicImage::ImageLuma8(mask.clone()))? },
                    Part::Text { text: prompt.to_string() },
                ],
            }],
            generation_config: GenerationConfig {
                response_modalities: vec!["TEXT".to_string(), "IMAGE".to_string()],
            },
        };
        let url = format!("{}/models/{}:generateContent", GEMINI_API_BASE, GEMINI_IMAGE_MODEL);
        let response = self
            .client
            .post(&url)
            .header("x-goog-api-key", &self.api_key)
            .json(&request)
            .send()
            .await
            .map_err(|e| format!("API request failed: {}", e.without_url()))?;
        let status = response.status();
        let text = response.text().await.map_err(|e| format!("Failed to read response: {}", e))?;
        if !status.is_success() {
            return Err(format!("API returned error status {}: {}", status, text));
        }
        let bytes = extract_image(&text)?;
        Ok(image::load_from_memory(&bytes)
            .map_err(|e| format!("Failed to decode response image: {}", e))?
            .to_rgb8())
    }
```
Add these free functions below `impl GeminiClient`:
```rust
fn png_part(img: &DynamicImage) -> Result<InlineData, String> {
    let bytes = crate::removal::codec::encode(img, crate::removal::codec::OutputFormat::Png)?;
    Ok(InlineData { mime_type: "image/png".to_string(), data: BASE64.encode(bytes) })
}

fn extract_image(response_text: &str) -> Result<Vec<u8>, String> {
    let response: GeminiResponse = serde_json::from_str(response_text)
        .map_err(|e| format!("Failed to parse response: {}", e))?;
    if let Some(error) = response.error {
        return Err(format!("Gemini API error: {} (status: {})", error.message, error.status.unwrap_or_default()));
    }
    let parts = response
        .candidates
        .and_then(|c| c.into_iter().next())
        .and_then(|c| c.content)
        .and_then(|c| c.parts)
        .ok_or("Gemini returned no content")?;
    let mut texts = Vec::new();
    for part in parts {
        if let Some(inline) = part.inline_data {
            return BASE64.decode(&inline.data).map_err(|e| format!("Failed to decode response image: {}", e));
        }
        if let Some(t) = part.text {
            texts.push(t);
        }
    }
    Err(if texts.is_empty() {
        "Gemini returned no image".to_string()
    } else {
        format!("Gemini returned no image: {}", texts.join(" "))
    })
}
```
Update the imports at the top of the file: remove `std::fs` and `std::path::Path`, remove `use crate::image_processor::WatermarkRegion;`, and add `use image::{DynamicImage, GrayImage, RgbImage};`. In `test_connection` and `list_models`, change the URL to `format!("{}/models", GEMINI_API_BASE)` and add `.header("x-goog-api-key", &self.api_key)` before `.send()`.

In `engine.rs`, add the variant and match arm:
```rust
use crate::gemini_client::GeminiClient;
// in enum Engine:
    Gemini(GeminiClient),
// in match self:
            Engine::Gemini(client) => client.inpaint(&crop, &mask).await,
```

- [ ] **Step 4: Run tests**

Run: `cd src-tauri && cargo test --lib`
Expected: all pass (`commands.rs` still calls the old `remove_watermark`, so if it fails to compile, carry straight on to Task 8 Step 1 before running).

- [ ] **Step 5: Commit**

```bash
git add src-tauri/src
git commit -m "feat(gemini): send crop+mask, header auth, surface refusals"
```

---

### Task 8: Wire the command and the UI

**Files:**
- Modify: `src-tauri/src/commands.rs:63-170` (`remove_watermark`, delete `reencode_lossless`), `src/types/watermark.ts:8-10`, `src/hooks/useWatermarkRemoval.ts:42-44`, `src/App.tsx:15-17, 141-151, 347, 363, 400, 419`, `src/components/OptionsPanel.tsx`

**Interfaces:**
- Consumes: `pipeline::run`, `Engine`, `codec::*`, `EngineKind`, `RemovalOptions`
- Produces: the Tauri command `remove_watermark(imagePath, region, options)` with the same response shape (`ProcessResult`) as before. TS `EngineKind = 'telea' | 'gemini'`.

- [ ] **Step 1: Rewrite `remove_watermark`** in `commands.rs`:

```rust
#[tauri::command]
pub async fn remove_watermark(
    app: tauri::AppHandle,
    image_path: String,
    region: WatermarkRegion,
    options: Option<RemovalOptions>,
) -> Result<ProcessResult, String> {
    let options = options.unwrap_or_default();
    let original_size = fs::metadata(&image_path)
        .map_err(|e| format!("Failed to get original file size: {}", e))?
        .len();

    let engine = match options.engine {
        EngineKind::Telea => Engine::Telea { radius: 5 },
        EngineKind::Gemini => {
            let api_key = get_stored_api_key(&app)?;
            if api_key.is_empty() {
                return Err("Gemini API key not configured. Please set it in Settings.".to_string());
            }
            Engine::Gemini(GeminiClient::new(api_key))
        }
    };

    let original = codec::load_oriented(&image_path)?;
    let pipeline_options = PipelineOptions { refine_mask: options.refine_mask, grow: 2, feather: 3 };
    let output = pipeline::run(&original, &region, &pipeline_options, &engine).await?;

    let original_extension = PathBuf::from(&image_path)
        .extension()
        .and_then(|e| e.to_str())
        .unwrap_or("png")
        .to_string();
    let format = codec::output_format(&original_extension, options.lossless);
    let final_bytes = codec::encode(&output.image, format)?;
    let processed_size = final_bytes.len() as u64;

    let output_filename = format!(
        "processed_{}_{}.{}",
        Uuid::new_v4().to_string().split('-').next().unwrap_or("img"),
        chrono_lite_timestamp(),
        format.extension()
    );
    let output_path = get_temp_dir()?.join(&output_filename);
    fs::write(&output_path, &final_bytes)
        .map_err(|e| format!("Failed to save processed image: {}", e))?;

    Ok(ProcessResult {
        output_path: output_path.to_string_lossy().to_string(),
        base64_preview: Some(format!("data:{};base64,{}", format.mime(), BASE64.encode(&final_bytes))),
        original_size,
        processed_size,
    })
}
```
Delete `fn reencode_lossless`. Update the imports:
```rust
use crate::image_processor::{self, EngineKind, RemovalOptions, WatermarkRegion};
use crate::removal::codec;
use crate::removal::engine::Engine;
use crate::removal::pipeline::{self, PipelineOptions};
```

- [ ] **Step 2: Verify the Rust side**

Run: `cd src-tauri && cargo test --lib && cargo clippy --all-targets -- -D warnings`
Expected: tests pass, no warnings.

- [ ] **Step 3: Frontend types and defaults**

`src/types/watermark.ts`:
```ts
export type EngineKind = 'telea' | 'gemini';

export interface RemovalOptions {
  lossless: boolean;
  engine: EngineKind;
  refine_mask: boolean;
}
```
In both `src/hooks/useWatermarkRemoval.ts` and `src/App.tsx`, set:
```ts
const DEFAULT_OPTIONS: RemovalOptions = {
  lossless: false,
  engine: 'telea',
  refine_mask: true,
};
```

- [ ] **Step 4: API-key gating only for the cloud engine.** In `App.tsx`, add after the hook destructuring:
```ts
  const needsApiKey = options.engine === 'gemini' && !hasApiKey;
```
and replace both `|| !hasApiKey}` (lines 363 and 419) with `|| needsApiKey}`.

- [ ] **Step 5: Fix the stale batch error** (`App.tsx` batch loop). Replace the `removeWatermark` call and its `if/else` with a direct invoke so the real error string is recorded:
```ts
      try {
        const result = await invoke<ProcessResult>('remove_watermark', {
          imagePath: file.path,
          region: selectedRegion,
          options,
        });
        handleBatchFileUpdate(file.id, { status: 'completed', processedPath: result.output_path });
      } catch (err) {
        handleBatchFileUpdate(file.id, {
          status: 'failed',
          error: err instanceof Error ? err.message : String(err),
        });
      }
```
Add `import { invoke } from '@tauri-apps/api/core';` and `ProcessResult` to the type import. Bypassing the hook here is safe because the `batch-processing` stage passes `isProcessing={true}` to `BatchQueuePanel` itself (`App.tsx:436`) and doesn't read the hook's flag. Remove `error` from that `useCallback`'s dependency list. Delete the unused `processBatch` from the hook (both its implementation and its entry in `UseWatermarkRemovalReturn`).

- [ ] **Step 6: OptionsPanel.** Replace the "Processing Method" group and the `cloud-info` block with:
```tsx
      <div className="option-group">
        <label>Processing Method</label>
        <label className="checkbox-label">
          <input
            type="radio"
            name="engine"
            checked={options.engine === 'telea'}
            onChange={() => onChange({ ...options, engine: 'telea' })}
            disabled={disabled}
          />
          Local (fast, offline)
        </label>
        <label className="checkbox-label">
          <input
            type="radio"
            name="engine"
            checked={options.engine === 'gemini'}
            onChange={() => onChange({ ...options, engine: 'gemini' })}
            disabled={disabled}
          />
          Google Gemini (cloud, needs API key)
        </label>
        {options.engine === 'gemini' && !hasApiKey && (
          <p className="option-warning">API key not configured. Please set it in Settings.</p>
        )}
      </div>

      <div className="option-group">
        <label className="checkbox-label">
          <input
            type="checkbox"
            checked={options.refine_mask}
            onChange={(e) => onChange({ ...options, refine_mask: e.target.checked })}
            disabled={disabled}
          />
          Detect watermark shape inside the selection
        </label>
        <p className="option-description">
          Only the watermark's own pixels are replaced; the rest of the selection keeps its original content.
        </p>
      </div>
```
Change the lossless description to: `JPEG is saved as PNG. PNG and WebP are always lossless.`

- [ ] **Step 7: Build and check it by hand**

Run: `npm run build` → no TS errors. Then `npm run tauri dev` and check:
1. Without an API key, "Local" is selected and "Remove Watermark" is enabled. A white-text watermark on sky or a gradient disappears, the output has the same pixel size as the input, and the rest of the image is identical.
2. A phone JPEG in portrait (EXIF orientation 6): the removed area matches the drawn rectangle.
3. A `.jpg` input saves a real JPEG; with Lossless it saves `.png`.
4. Gemini engine with a key: the output keeps the original resolution. If Gemini refuses, its own text shows up in the error banner.
5. Batch of 3 images where one is smaller: the smaller one is processed with a clamped region, or fails with "Region lies outside…". It never shows "Processing failed".

- [ ] **Step 8: Commit**

```bash
git add src src-tauri/src
git commit -m "feat: route removal through local pipeline, engine picker in UI"
```

---

### Task 9: Evaluation harness (objective quality numbers for every future engine)

**Files:**
- Create: `src-tauri/examples/eval.rs`

**Interfaces:**
- Consumes: `watermark_remover_lib::removal::{engine::Engine, pipeline::{run, PipelineOptions}}`, `watermark_remover_lib::image_processor::WatermarkRegion`
- Produces: `cargo run --release --example eval [-- <folder of clean images>]` prints a table of PSNR (dB) inside the watermark rectangle per background × watermark opacity × config. Milestones 2–6 must beat these numbers.

- [ ] **Step 1: Write the tool**

```rust
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
        ("noise".into(), RgbImage::from_fn(400, 300, |x, y| { let n = hash(x, y); Rgb([100 + n, 110 + n, 90 + n]) })),
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
    if se == 0.0 { f64::INFINITY } else { 10.0 * (255.0 * 255.0 / (se / n)).log10() }
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
                    name, opacity, refine, psnr(clean, &marked, &region), psnr(clean, &out.image.to_rgb8(), &region)
                );
            }
        }
    }
}
```

- [ ] **Step 2: Run it**

Run: `cd src-tauri && cargo run --release --example eval`
Expected: a 24-row table. Output dB beats input dB on `gradient` and `sky`, and `refine=true` is at least as good as `false` on `sky`. Save the output as the baseline in the PR description.

- [ ] **Step 3: Commit**

```bash
git add src-tauri/examples/eval.rs
git commit -m "chore: synthetic watermark-removal benchmark"
```

---

# Part III — Roadmap (each milestone gets its own detailed plan when started)

Every milestone adds **one `Engine` variant (or a pipeline pre-step), one `EngineKind` value and one radio option**, and must post its numbers from `examples/eval.rs` next to the Telea baseline.

### Milestone 2 — PatchMatch "content-aware" engine (no AI) · ~3–4 days
- **File:** `src-tauri/src/removal/patchmatch.rs` → `pub fn inpaint(img: &RgbImage, mask: &GrayImage, patch: u32 /*7*/, iterations: u32 /*5*/) -> Result<RgbImage, String>`.
- **Algorithm (Wexler/Barnes):** a Gaussian pyramid down to about 32 px on the short side. At the coarsest level, initialise the hole with Telea. At every level, repeat: nearest-neighbour field (for each patch touching the hole, the best source patch fully outside the hole) using random init, propagation (scanline, alternating direction) and random search (radius halves each step); then **voting** (each hole pixel = weighted mean of the overlapping patches' source pixels). Upsample the NNF to the next level.
- **Tests:** stripes (period 12 px, hole 20×20) must be rebuilt with the stripes in phase (column mean error < 10). Known pixels stay untouched. It must run deterministically from a fixed seed (`rand_pcg` or a hand-written xorshift, no new big deps).
- **Acceptance:** on `stripes` and `noise`, eval PSNR beats Telea by ≥ 3 dB. Runs under 2 s for a 400×300 crop in release on an M1.

### Milestone 3 — Translucent watermark recovery (single image, no AI) · ~3 days
- **File:** `src-tauri/src/removal/unblend.rs` → `pub fn unblend(crop: &RgbImage, mask: &GrayImage, background_guess: &RgbImage) -> Result<(RgbImage, GrayImage /*opaque residue mask*/), String>`.
- **Model:** `I = α·W + (1−α)·J`. Estimate `W` as the dominant extreme colour of the stroke pixels (for white watermarks, the per-channel 95th percentile). Estimate per-pixel `α` by least squares on `I − B = α(W − B)`, where `B` is the background guess from Telea/PatchMatch. Smooth `α` with a 3×3 median. Recover `J = (I − αW)/(1−α)` where `α < 0.85`. Pixels with `α ≥ 0.85` go into the residue mask, which is then inpainted by the chosen engine.
- **Pipeline:** new `PipelineOptions.recover_translucent: bool`. Run it after refinement and before the engine.
- **Acceptance:** on eval opacity 0.35 and 0.6, PSNR is ≥ 6 dB better than Telea alone on `stripes`/`noise` (textures Telea can't invent). At opacity 1.0 it's no worse than Telea.

### Milestone 4 — Local AI: LaMa via ONNX Runtime · ~4–5 days
- **Deps:** `ort = "2.0.0-rc.x"` (check the current rc on crates.io). Features: `download-binaries` for aarch64-apple-darwin (+`coreml`), x86_64/aarch64 Windows (+`directml`) and Linux. **Intel Mac:** use `load-dynamic` with Microsoft's `onnxruntime-osx-universal2` dylib bundled as a Tauri resource, or hide the engine on that target (decision needed, see Part I §4). Update `.github/workflows/release.yml` to match.
- **Model:** `Carve/LaMa-ONNX/lama_fp32.onnx` (Apache-2.0, 208,044,816 bytes). Downloaded on first use into `app_data_dir()/models/` with a SHA-256 check and a progress event. Never bundled.
- **File:** `src-tauri/src/removal/lama.rs` → `pub struct Lama { session: ort::session::Session }`, `Lama::load(path) -> Result<Self, String>`, `fn inpaint(&self, crop: &RgbImage, mask: &GrayImage) -> Result<RgbImage, String>`. Crop a square around the hole of side `max(512, 2·max(hole.w, hole.h))`, resize to 512, and build NCHW tensors (image `[1,3,512,512]`, mask `[1,1,512,512]`). **First step of that plan: inspect the model's input names, value ranges (0–1 vs 0–255) and output range with a one-off test before writing the wrapper.**
- **Acceptance:** beats PatchMatch on `noise`/real photos in the eval (run with a folder of real photos). CPU inference < 3 s at 512² on the slowest CI-supported target. The app still starts and works offline when the model isn't downloaded yet.

### Milestone 5 — Batch: multi-image watermark estimation (Dekel et al. 2017, no AI) · ~5 days
- **File:** `src-tauri/src/removal/multi_image.rs` → `pub fn estimate(crops: &[RgbImage]) -> Result<WatermarkModel { w: [f32;3], alpha: GrayImage /*f32 scaled*/ }, String>`. Steps: per-pixel **median of Sobel gradients** across N crops, which isolates the watermark's gradient. Threshold it to get a mask, **Poisson-reconstruct** `α·W` (Jacobi/Gauss–Seidel, about 200 iterations on the crop), pick `W` by 1-D search minimising the recovered images' gradient energy, and get `α = (α·W)/W`. Then unblend every image with Milestone 3's `J = (I − αW)/(1−α)`.
- **UI:** in batch mode with ≥ 8 images, offer "Estimate watermark from all images". New command `estimate_watermark(paths, region)` returns a preview of the estimated watermark, and `process_batch` reuses the model.
- **Acceptance:** 20 synthetic images (different `synthetic()` backgrounds, same stamp, opacity 0.5) reach ≥ 35 dB mean PSNR in the region. This also unlocks bringing video back later (frames = images).

### Milestone 6 — Cloud cleanup · ~1–2 days
- `Engine::Stability` → `POST https://api.stability.ai/v2beta/stable-image/edit/erase` (multipart: `image`, `mask`, `output_format=png`, header `authorization: Bearer <key>`), sending the crop only.
- Settings stores one key per provider. Replace plaintext `tauri-plugin-store` key storage with the OS keychain (`keyring` crate).
- Drop Gemini if Milestone 4 makes it redundant (it's the only engine that adds SynthID).

## Risks

| Risk | Mitigation |
|---|---|
| Telea blurs large or textured regions | Mask refinement keeps holes thin. PatchMatch (M2) and LaMa (M4) cover textures. |
| `ort` build friction repeats the OpenCV story | Prebuilt binaries for 3 of 4 targets. Intel Mac gets `load-dynamic` or no LaMa, decided up front. LaMa sits behind a Cargo feature so a broken target can ship without it. |
| 208 MB model | Download on demand with a checksum. The app is fully usable without it. |
| Gemini refusals or policy | Its text is now surfaced. The local engine is the default. |
| Legal use | Short notice in the app and README: for images you own or have rights to. |
| `imageproc` pulls in nalgebra/rand/rayon for three functions (dilate, Otsu, Gaussian blur) | Fine for M1. If build times or binary size hurt, write those three by hand (about 80 lines). |

## Sources

- Gemini image models, resolutions and SynthID: https://ai.google.dev/gemini-api/docs/image-generation · https://developers.googleblog.com/introducing-gemini-2-5-flash-image/
- Stability Erase API: https://platform.stability.ai/docs/api-reference · https://docs.aws.amazon.com/bedrock/latest/userguide/model-card-stability-ai-stable-image-erase-object.html
- LaMa ONNX: https://huggingface.co/Carve/LaMa-ONNX · https://huggingface.co/opencv/inpainting_lama
- `ort` prebuilt binaries / execution providers: https://github.com/pykeio/ort (ort-sys/build/download/dist.tsv, docs/perf/execution-providers)
- `inpaint` crate (EUPL-1.2, not used): https://crates.io/crates/inpaint
- Dekel, Rubinstein, Liu, Freeman, "On the Effectiveness of Visible Watermarks", CVPR 2017: https://openaccess.thecvf.com/content_cvpr_2017/papers/Dekel_On_the_Effectiveness_CVPR_2017_paper.pdf
- Telea, "An Image Inpainting Technique Based on the Fast Marching Method", 2004
