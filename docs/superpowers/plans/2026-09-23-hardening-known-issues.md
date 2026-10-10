# Plan A — Hardening: Known Issues Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Fix the five known issues left after the local-pipeline work, and add a CI gate so they stay fixed.

**Architecture:** Every fix stays inside the existing `src-tauri/src/removal/` modules:
- geometry (overflow)
- codec (orientation, cheap dimension reads, greyscale JPEG)
- telea, refine, engine and pipeline (transparent pixels are not fill sources)
- composite (writes into the original's own colour type and bit depth instead of flattening to 8-bit RGB)

A new GitHub Actions workflow runs the test, clippy and build gates on every PR.

**Tech Stack:** Rust (`image` 0.25.9, `imageproc` 0.25, `jpeg-encoder` 0.7, `png` 0.18, `num-traits` 0.2), GitHub Actions.

**Spec:** `docs/superpowers/plans/2026-09-23-roadmap-v0.3.md` (section "Plan A") and the "Known issues" list from the Milestone 1 final review (`docs/superpowers/plans/2026-09-23-watermark-removal-engines.md`).

## Global Constraints

- Pixels the pipeline doesn't touch must stay **byte-identical** to the decoded, EXIF-oriented original, in the original's colour type and bit depth.
- Supported inputs: PNG (8/16-bit, greyscale, greyscale+alpha, RGB, RGBA, indexed), JPEG (RGB, greyscale), WebP, GIF (first frame).
- No new native (C/C++) dependencies. Pure-Rust crates only, MIT/Apache/BSD licences.
- Run all Rust commands from `src-tauri/`. Gates: `cargo test --lib`, `cargo clippy --all-targets -- -D warnings`, and `npm run build` from the repo root.
- Commit messages end with `Co-Authored-By: Claude Opus 5.5 (1M context) <noreply@anthropic.com>`.

## Review Focus

1. **16-bit PNG with alpha (`Rgba16`):** the output must stay `Rgba16` and alpha must be untouched. (Task 6 test `pipeline_keeps_rgba16`.)
2. **Greyscale JPEG:** the output must be a greyscale JPEG, not a 3-channel file three times the size. (Task 6 test `greyscale_jpeg_stays_greyscale`.)
3. **A hole entirely surrounded by transparent pixels:** no panic, and the pixels are left as they were. (Task 5 test `hole_inside_transparent_area_is_left_alone`.)
4. **EXIF orientation 5–8 on the dimension read:** it must match `load_oriented` exactly. (Task 4 test `read_dimensions_matches_decoded_orientation`.)
5. **A region with `i32::MAX` width from a malformed IPC call:** clamped, no panic. (Task 2 test `clamps_region_with_huge_width`.)

## File Structure

```
.github/workflows/ci.yml             CREATE  test + clippy + frontend build on PR / push to main
src-tauri/Cargo.toml                 MODIFY  + num-traits = "0.2"
src-tauri/src/removal/geometry.rs    MODIFY  i64 arithmetic in clamp_region
src-tauri/src/removal/codec.rs       MODIFY  read_dimensions(); greyscale JPEG encode; orientation tests
src-tauri/src/image_processor.rs     MODIFY  get_image_dimensions → codec::read_dimensions
src-tauri/src/removal/telea.rs       MODIFY  IGNORED flag + inpaint_with_ignore()
src-tauri/src/removal/refine.rs      MODIFY  refine_mask(.., ignore)
src-tauri/src/removal/engine.rs      MODIFY  Engine::inpaint(.., ignore)
src-tauri/src/removal/composite.rs   MODIFY  weights(), generic blend(), composite_into(&mut DynamicImage)
src-tauri/src/removal/pipeline.rs    MODIFY  ignore mask from alpha; composite_into; drop restore_alpha
```

---

### Task 1: CI gate for pull requests

**Files:**
- Create: `.github/workflows/ci.yml`

**Interfaces:** none (infrastructure).

- [ ] **Step 1: Write the workflow**

```yaml
name: CI

on:
  pull_request:
  push:
    branches: [main]

jobs:
  check:
    runs-on: macos-latest
    steps:
      - uses: actions/checkout@v4

      - uses: actions/setup-node@v4
        with:
          node-version: 'lts/*'
          cache: npm

      - uses: dtolnay/rust-toolchain@stable
        with:
          components: clippy

      - uses: actions/cache@v4
        with:
          path: |
            ~/.cargo/registry/index/
            ~/.cargo/registry/cache/
            ~/.cargo/git/db/
            src-tauri/target/
          key: ci-${{ runner.os }}-${{ hashFiles('src-tauri/Cargo.lock') }}
          restore-keys: ci-${{ runner.os }}-

      - name: Install frontend dependencies
        run: npm ci

      - name: Frontend type-check and build
        run: npm run build

      - name: Rust tests
        working-directory: src-tauri
        run: cargo test --lib

      - name: Clippy
        working-directory: src-tauri
        run: cargo clippy --all-targets -- -D warnings
```

- [ ] **Step 2: Validate the YAML locally**

Run: `ruby -e 'require "yaml"; YAML.load_file(".github/workflows/ci.yml"); puts "ok"'`
Expected: `ok`

- [ ] **Step 3: Commit**

```bash
git add .github/workflows/ci.yml
git commit -m "ci: run tests, clippy and frontend build on PRs"
```

(The first PR that includes this file proves it works. The run must be green before the merge.)

---

### Task 2: Overflow-safe region clamping

**Files:**
- Modify: `src-tauri/src/removal/geometry.rs:13-25`

**Interfaces:**
- Produces: `clamp_region(r: &WatermarkRegion, img_w: u32, img_h: u32) -> Result<Rect, String>`. The signature is unchanged; the arithmetic is now i64.

- [ ] **Step 1: Write the failing tests** (add to `mod tests` in `geometry.rs`)

```rust
    #[test]
    fn clamps_region_with_huge_width() {
        let r = clamp_region(&region(10, 10, i32::MAX, i32::MAX), 100, 50).unwrap();
        assert_eq!(r, Rect { x: 10, y: 10, w: 90, h: 40 });
    }

    #[test]
    fn rejects_region_starting_at_i32_max() {
        assert!(clamp_region(&region(i32::MAX, 0, i32::MAX, 5), 100, 50).is_err());
    }
```

- [ ] **Step 2: Run to verify failure**

Run: `cd src-tauri && cargo test --lib removal::geometry`
Expected: FAIL, panicked with `attempt to add with overflow`.

- [ ] **Step 3: Implement.** Replace the body of `clamp_region` with:

```rust
pub fn clamp_region(r: &WatermarkRegion, img_w: u32, img_h: u32) -> Result<Rect, String> {
    if r.width <= 0 || r.height <= 0 {
        return Err("Region must have a positive size".to_string());
    }
    // i64 so that x + width can't overflow for any i32 input.
    let clamp_x = |v: i64| v.clamp(0, img_w as i64) as u32;
    let clamp_y = |v: i64| v.clamp(0, img_h as i64) as u32;
    let (x0, y0) = (clamp_x(r.x as i64), clamp_y(r.y as i64));
    let x1 = clamp_x(r.x as i64 + r.width as i64);
    let y1 = clamp_y(r.y as i64 + r.height as i64);
    if x0 >= x1 || y0 >= y1 {
        return Err(format!("Region lies outside the {}x{} image", img_w, img_h));
    }
    Ok(Rect { x: x0, y: y0, w: x1 - x0, h: y1 - y0 })
}
```

- [ ] **Step 4: Run to verify pass**

Run: `cd src-tauri && cargo test --lib removal::geometry`
Expected: 8 passed.

- [ ] **Step 5: Commit**

```bash
git add src-tauri/src/removal/geometry.rs
git commit -m "fix(geometry): clamp regions with i64 arithmetic (no overflow)"
```

---

### Task 3: EXIF orientation 3, 6 and 8, with pixel-placement tests

**Files:**
- Modify: `src-tauri/src/removal/codec.rs` (tests module only)

**Interfaces:**
- Consumes: `load_oriented(path: &str) -> Result<DynamicImage, String>` (existing).

These are characterisation tests for a coverage gap. They are expected to **pass** against the current code. If one fails, `load_oriented` has a real bug: fix it in this task using superpowers:systematic-debugging, and don't change the test.

- [ ] **Step 1: Add the tests** (inside `mod tests` in `codec.rs`, next to `jpeg_with_orientation`)

```rust
    /// 16x8 JPEG: left half red, right half blue, with the given EXIF orientation.
    fn two_tone_jpeg(orientation: u16) -> Vec<u8> {
        let img = RgbImage::from_fn(16, 8, |x, _| if x < 8 { Rgb([230, 10, 10]) } else { Rgb([10, 10, 230]) });
        with_orientation(&img, orientation)
    }

    /// Encodes `img` and inserts an APP1/EXIF segment carrying only the Orientation tag.
    fn with_orientation(img: &RgbImage, orientation: u16) -> Vec<u8> {
        let mut jpeg = Vec::new();
        JpegEncoder::new_with_quality(&mut jpeg, 95).encode_image(img).unwrap();
        let mut tiff = vec![b'M', b'M', 0, 42, 0, 0, 0, 8, 0, 1];
        tiff.extend_from_slice(&[0x01, 0x12, 0, 3, 0, 0, 0, 1]);
        tiff.extend_from_slice(&orientation.to_be_bytes());
        tiff.extend_from_slice(&[0, 0, 0, 0, 0, 0]);
        let mut app1 = b"Exif\0\0".to_vec();
        app1.extend_from_slice(&tiff);
        let len = (app1.len() + 2) as u16;
        let mut out = jpeg[..2].to_vec();
        out.extend_from_slice(&[0xFF, 0xE1]);
        out.extend_from_slice(&len.to_be_bytes());
        out.extend_from_slice(&app1);
        out.extend_from_slice(&jpeg[2..]);
        out
    }

    fn load_bytes(name: &str, bytes: &[u8]) -> DynamicImage {
        let path = std::env::temp_dir().join(name);
        std::fs::write(&path, bytes).unwrap();
        load_oriented(path.to_str().unwrap()).unwrap()
    }

    fn is_red(p: image::Rgb<u8>) -> bool {
        p[0] > 180 && p[2] < 70
    }

    #[test]
    fn orientation_6_puts_left_half_on_top() {
        let img = load_bytes("wmr_o6.jpg", &two_tone_jpeg(6)).to_rgb8();
        assert_eq!(img.dimensions(), (8, 16));
        assert!(is_red(*img.get_pixel(4, 3)), "top should be red");
        assert!(!is_red(*img.get_pixel(4, 12)), "bottom should be blue");
    }

    #[test]
    fn orientation_8_puts_left_half_at_bottom() {
        let img = load_bytes("wmr_o8.jpg", &two_tone_jpeg(8)).to_rgb8();
        assert_eq!(img.dimensions(), (8, 16));
        assert!(!is_red(*img.get_pixel(4, 3)), "top should be blue");
        assert!(is_red(*img.get_pixel(4, 12)), "bottom should be red");
    }

    #[test]
    fn orientation_3_mirrors_halves() {
        let img = load_bytes("wmr_o3.jpg", &two_tone_jpeg(3)).to_rgb8();
        assert_eq!(img.dimensions(), (16, 8));
        assert!(!is_red(*img.get_pixel(3, 4)), "left should be blue");
        assert!(is_red(*img.get_pixel(12, 4)), "right should be red");
    }
```

- [ ] **Step 2: Run**

Run: `cd src-tauri && cargo test --lib removal::codec::tests::orientation`
Expected: 3 passed (characterisation).

- [ ] **Step 3: Commit**

```bash
git add src-tauri/src/removal/codec.rs
git commit -m "test(codec): pin EXIF orientation 3/6/8 pixel placement"
```

---

### Task 4: Read dimensions without decoding pixels

**Files:**
- Modify: `src-tauri/src/removal/codec.rs` (new fn after `load_oriented`), `src-tauri/src/image_processor.rs:32-34`

**Interfaces:**
- Consumes: `two_tone_jpeg`, `load_bytes` (Task 3 test helpers).
- Produces: `pub fn read_dimensions(path: &str) -> Result<(u32, u32), String>`, giving the displayed size after EXIF orientation.

- [ ] **Step 1: Write the failing test** (in `codec.rs` tests)

```rust
    #[test]
    fn read_dimensions_matches_decoded_orientation() {
        for (orientation, name) in (1u16..=8).map(|o| (o, format!("wmr_d{o}.jpg"))) {
            let bytes = two_tone_jpeg(orientation);
            let decoded = load_bytes(&name, &bytes);
            let path = std::env::temp_dir().join(&name);
            assert_eq!(read_dimensions(path.to_str().unwrap()).unwrap(), decoded.dimensions(), "orientation {orientation}");
        }
    }
```

- [ ] **Step 2: Run to verify failure**

Run: `cd src-tauri && cargo test --lib removal::codec::tests::read_dimensions`
Expected: compile error `cannot find function read_dimensions`.

- [ ] **Step 3: Implement** (after `load_oriented` in `codec.rs`; add `use image::metadata::Orientation;` to the imports)

```rust
/// Displayed width and height (after EXIF orientation), read from the header only.
pub fn read_dimensions(path: &str) -> Result<(u32, u32), String> {
    let mut decoder = ImageReader::open(path)
        .and_then(|r| r.with_guessed_format())
        .map_err(|e| format!("Failed to open image: {}", e))?
        .into_decoder()
        .map_err(|e| format!("Failed to read image header: {}", e))?;
    let (w, h) = image::ImageDecoder::dimensions(&decoder);
    let orientation = image::ImageDecoder::orientation(&mut decoder).unwrap_or(Orientation::NoTransforms);
    Ok(match orientation {
        Orientation::Rotate90 | Orientation::Rotate270 | Orientation::Rotate90FlipH | Orientation::Rotate270FlipH => (h, w),
        _ => (w, h),
    })
}
```

In `image_processor.rs`, replace `get_image_dimensions` with the version below and remove the now-unused `use image::GenericImageView;`:

```rust
pub fn get_image_dimensions(image_path: &str) -> Result<(u32, u32), String> {
    crate::removal::codec::read_dimensions(image_path)
}
```

- [ ] **Step 4: Run to verify pass**

Run: `cd src-tauri && cargo test --lib && cargo clippy --all-targets -- -D warnings`
Expected: all pass, no warnings.

- [ ] **Step 5: Measure the effect.** Create a 6000×4000 JPEG with `sips` from any photo (`sips -Z 6000 in.jpg --out /tmp/big.jpg`). Time `get_image_info` via a scratch example, or in the app, before and after. Expected: under 5 ms after (header only), against hundreds of ms before. Record the numbers in the commit message.

- [ ] **Step 6: Commit**

```bash
git add src-tauri/src/removal/codec.rs src-tauri/src/image_processor.rs
git commit -m "perf(codec): read image dimensions from the header only"
```

---

### Task 5: Transparent pixels are never fill sources

**Files:**
- Modify: `src-tauri/src/removal/telea.rs:6-120`, `src-tauri/src/removal/refine.rs:18-50`, `src-tauri/src/removal/engine.rs`, `src-tauri/src/removal/pipeline.rs:25-54`

**Interfaces:**
- Produces:
  - `telea::inpaint_with_ignore(img: &RgbImage, mask: &GrayImage, ignore: Option<&GrayImage>, radius: u32) -> Result<RgbImage, String>`. Pixels where `ignore == HOLE` are neither sources nor filled. `telea::inpaint(img, mask, radius)` stays as a wrapper that passes `None`.
  - `refine::refine_mask(crop: &RgbImage, rect_mask: &GrayImage, grow: u8, ignore: Option<&GrayImage>) -> Result<Refined, String>`
  - `Engine::inpaint(&self, crop: &RgbImage, mask: &GrayImage, ignore: Option<&GrayImage>) -> Result<RgbImage, String>`

- [ ] **Step 1: Write the failing Telea tests** (in `telea.rs` tests)

```rust
    fn transparent_left(w: u32, h: u32, cols: u32) -> GrayImage {
        GrayImage::from_fn(w, h, |x, _| Luma([if x < cols { HOLE } else { 0 }]))
    }

    #[test]
    fn ignored_pixels_are_not_fill_sources() {
        // Left 8 columns: transparent (RGB black underneath). Right: opaque red.
        let img = RgbImage::from_fn(24, 12, |x, _| if x < 8 { Rgb([0, 0, 0]) } else { Rgb([200, 0, 0]) });
        let mask = hole_mask(24, 12, 8, 3, 4, 6);
        let out = inpaint_with_ignore(&img, &mask, Some(&transparent_left(24, 12, 8)), 5).unwrap();
        for y in 3..9 {
            for x in 8..12 {
                assert_eq!(out.get_pixel(x, y), &Rgb([200, 0, 0]), "({x},{y}) picked up transparent black");
            }
        }
    }

    #[test]
    fn hole_inside_transparent_area_is_left_alone() {
        let img = RgbImage::from_pixel(12, 12, Rgb([7, 8, 9]));
        let mask = hole_mask(12, 12, 2, 2, 3, 3);
        let ignore = GrayImage::from_pixel(12, 12, Luma([HOLE]));
        // Nothing known anywhere → error, not a panic.
        assert!(inpaint_with_ignore(&img, &mask, Some(&ignore), 5).is_err());
    }
```

- [ ] **Step 2: Run to verify failure**

Run: `cd src-tauri && cargo test --lib removal::telea`
Expected: compile error `cannot find function inpaint_with_ignore`.

- [ ] **Step 3: Implement in `telea.rs`**
  - Add `const IGNORED: u8 = 3;` next to the other flags.
  - Change `settled_t` so only KNOWN/BAND count:
    ```rust
    /// T at (x, y) if it is final or on the front (KNOWN or BAND).
    fn settled_t(&self, x: i64, y: i64) -> Option<f32> {
        self.idx(x, y).filter(|&k| self.flags[k] == KNOWN || self.flags[k] == BAND).map(|k| self.t[k])
    }
    ```
  - Replace the signature and start of `inpaint`:
    ```rust
    /// Fast-marching inpainting (Telea 2004) without the image-gradient term.
    /// Fills every `HOLE` pixel of `mask` from the known pixels within `radius`.
    pub fn inpaint(img: &RgbImage, mask: &GrayImage, radius: u32) -> Result<RgbImage, String> {
        inpaint_with_ignore(img, mask, None, radius)
    }

    /// Like `inpaint`, but pixels marked HOLE in `ignore` (e.g. fully transparent)
    /// are neither used as sources nor filled.
    pub fn inpaint_with_ignore(
        img: &RgbImage,
        mask: &GrayImage,
        ignore: Option<&GrayImage>,
        radius: u32,
    ) -> Result<RgbImage, String> {
        let (w, h) = img.dimensions();
        if mask.dimensions() != (w, h) || ignore.is_some_and(|i| i.dimensions() != (w, h)) {
            return Err("Mask size does not match image size".to_string());
        }
    ```
  - After the loop that marks INSIDE pixels, add:
    ```rust
        if let Some(ignore) = ignore {
            for (x, y, m) in ignore.enumerate_pixels() {
                if m[0] == HOLE {
                    let i = (y * w + x) as usize;
                    g.flags[i] = IGNORED;
                    g.t[i] = INF;
                }
            }
        }
    ```
  - Replace the "nothing to inpaint from" check:
    ```rust
        if !g.flags.iter().any(|&f| f == KNOWN) {
            return Err("Nothing to inpaint from: the mask covers the whole area".to_string());
        }
    ```
  - In `fill_pixel`, change `if k == i || g.flags[k] == INSIDE {` to:
    ```rust
            if k == i || g.flags[k] == INSIDE || g.flags[k] == IGNORED {
    ```

- [ ] **Step 4: Run to verify pass**

Run: `cd src-tauri && cargo test --lib removal::telea`
Expected: 8 passed.

- [ ] **Step 5: Thread `ignore` through refine, engine and pipeline. Write the failing pipeline test first** (in `pipeline.rs` tests)

```rust
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
```

Run: `cd src-tauri && cargo test --lib removal::pipeline::tests::transparent`
Expected: FAIL (the fill at (44,20) is darkened by the transparent black).

- [ ] **Step 6: Implement the threading**

`engine.rs`:
```rust
    /// Returns an image the size of `crop` with the HOLE pixels of `mask` filled.
    /// Pixels marked HOLE in `ignore` are not used as sources.
    /// CPU-bound; callers on an async runtime should run it via `spawn_blocking`.
    pub fn inpaint(&self, crop: &RgbImage, mask: &GrayImage, ignore: Option<&GrayImage>) -> Result<RgbImage, String> {
        match self {
            Engine::Telea { radius } => telea::inpaint_with_ignore(crop, mask, ignore, *radius),
            #[cfg(test)]
            Engine::Fill(c) => Ok(RgbImage::from_fn(crop.width(), crop.height(), |x, y| {
                if mask.get_pixel(x, y)[0] == super::mask::HOLE { image::Rgb(*c) } else { *crop.get_pixel(x, y) }
            })),
        }
    }
```

`refine.rs`: change the signature and the prediction call, and exclude ignored pixels from the samples:
```rust
pub fn refine_mask(crop: &RgbImage, rect_mask: &GrayImage, grow: u8, ignore: Option<&GrayImage>) -> Result<Refined, String> {
    let predicted = telea::inpaint_with_ignore(crop, rect_mask, ignore, 5)?;
    let ignored = |x: u32, y: u32| ignore.is_some_and(|i| i.get_pixel(x, y)[0] == HOLE);
    let (w, h) = crop.dimensions();
    let diff = GrayImage::from_fn(w, h, |x, y| {
        if rect_mask.get_pixel(x, y)[0] != HOLE || ignored(x, y) {
            return Luma([0]);
        }
```
(The rest of the function is unchanged.) Update the three existing refine tests to pass `None` as the new last argument.

`pipeline.rs` inside `run`, after `let crop_img = ...`:
```rust
    // Fully transparent pixels hide arbitrary RGB (often black); never copy them into the fill.
    let ignore = original.color().has_alpha().then(|| {
        let rgba = original.to_rgba8();
        GrayImage::from_fn(crop.w, crop.h, |x, y| {
            Luma([if rgba.get_pixel(crop.x + x, crop.y + y)[3] == 0 { HOLE } else { 0 }])
        })
    });
```
Then pass `ignore.as_ref()` to `refine_mask(&crop_img, &rect, opts.grow, ignore.as_ref())` and to `engine.inpaint(&crop_img, &mask, ignore.as_ref())`. Add `use super::mask::HOLE;` and `use image::{GrayImage, Luma};` to the imports.

- [ ] **Step 7: Run everything**

Run: `cd src-tauri && cargo test --lib && cargo clippy --all-targets -- -D warnings`
Expected: all pass, including `transparent_neighbours_do_not_darken_the_fill`.

- [ ] **Step 8: Commit**

```bash
git add src-tauri/src/removal
git commit -m "fix(removal): transparent pixels are never used as fill sources"
```

---

### Task 6: Keep the original's colour type and bit depth

**Files:**
- Modify: `src-tauri/Cargo.toml` (+ `num-traits = "0.2"`), `src-tauri/src/removal/composite.rs` (whole file), `src-tauri/src/removal/pipeline.rs:25-66`, `src-tauri/src/removal/codec.rs` (`encode_jpeg`)

**Interfaces:**
- Consumes: `Engine::inpaint(.., ignore)` (Task 5).
- Produces: `pub fn composite_into(base: &mut DynamicImage, patch: &RgbImage, crop: Rect, mask: &GrayImage, feather: u8)`. `composite(base: &mut RgbImage, ..)` is kept with the same behaviour. `pipeline::run` returns an image with `original.color()`.

- [ ] **Step 1: Write the failing tests** (`pipeline.rs` tests)

```rust
    #[test]
    fn pipeline_keeps_rgba16() {
        let src = DynamicImage::ImageRgba16(image::ImageBuffer::from_pixel(64, 64, Rgba([1000u16, 20000, 60000, 12345])));
        let out = run(&src, &region(20, 20, 10, 10), &opts(), &Engine::Fill([255, 0, 0])).unwrap();
        assert_eq!(out.image.color(), image::ColorType::Rgba16);
        let px = out.image.as_rgba16().unwrap();
        assert_eq!(px.get_pixel(25, 25), &Rgba([65535, 0, 0, 12345]));
        assert_eq!(px.get_pixel(2, 2), &Rgba([1000, 20000, 60000, 12345]), "untouched pixel must be identical");
    }

    #[test]
    fn pipeline_keeps_greyscale() {
        let src = DynamicImage::ImageLuma8(image::GrayImage::from_pixel(64, 64, image::Luma([90])));
        let out = run(&src, &region(20, 20, 10, 10), &opts(), &Engine::Fill([255, 255, 255])).unwrap();
        assert_eq!(out.image.color(), image::ColorType::L8);
        assert_eq!(out.image.as_luma8().unwrap().get_pixel(25, 25)[0], 255);
        assert_eq!(out.image.as_luma8().unwrap().get_pixel(1, 1)[0], 90);
    }
```

and in `codec.rs` tests:
```rust
    #[test]
    fn greyscale_jpeg_stays_greyscale() {
        let img = DynamicImage::ImageLuma8(image::GrayImage::from_fn(64, 48, |x, y| image::Luma([(x * 3 + y) as u8])));
        let out = encode(&img, OutputFormat::Jpeg, &SourceHints::default()).unwrap();
        assert_eq!(image::load_from_memory(&out).unwrap().color(), image::ColorType::L8);
    }
```

- [ ] **Step 2: Run to verify failure**

Run: `cd src-tauri && cargo test --lib keeps_ greyscale_jpeg`
Expected: FAIL. The colour type is `Rgba8`/`Rgb8` instead of `Rgba16`/`L8`, and the JPEG decodes as `Rgb8`.

- [ ] **Step 3: Implement `composite.rs`** (replace the file's code above `#[cfg(test)]`)

```rust
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
    let colour_channels = (P::CHANNEL_COUNT - u8::from(P::HAS_ALPHA)) as usize;
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
    p.map(f32::from)
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
```

Add `num-traits = "0.2"` under `[dependencies]` in `src-tauri/Cargo.toml`.

- [ ] **Step 4: Use it in the pipeline.** In `pipeline.rs` `run`, replace `let mut rgb = original.to_rgb8();` with `let rgb = original.to_rgb8();`. Replace the final two statements with:

```rust
    let mut image = original.clone();
    composite_into(&mut image, &patch, crop, &mask, opts.feather);
    Ok(PipelineOutput { image, mask_refined })
```

Then delete `fn restore_alpha` and update the imports (`use super::composite::composite_into;`, and drop `Rgba, RgbaImage` from the non-test imports if unused).

- [ ] **Step 5: Greyscale JPEG in `codec.rs` `encode_jpeg`.** Replace the lines from `let rgb = img.to_rgb8();` through `.encode(rgb.as_raw(), w, h, ColorType::Rgb)` so that greyscale sources are encoded as single-channel JPEGs:

```rust
    let greyscale = matches!(img.color(), image::ColorType::L8 | image::ColorType::La8 | image::ColorType::L16 | image::ColorType::La16);
    let (data, color_type, width, height) = if greyscale {
        let l = img.to_luma8();
        let (w, h) = l.dimensions();
        (l.into_raw(), ColorType::Luma, w, h)
    } else {
        let c = img.to_rgb8();
        let (w, h) = c.dimensions();
        (c.into_raw(), ColorType::Rgb, w, h)
    };
    let (w, h) = (
        u16::try_from(width).map_err(|_| "Image is too wide for JPEG".to_string())?,
        u16::try_from(height).map_err(|_| "Image is too tall for JPEG".to_string())?,
    );
```

and make the final encode call `.encode(&data, w, h, color_type)`. The encoder setup (sampling, tables, ICC) in between stays unchanged.

- [ ] **Step 6: Run everything**

Run: `cd src-tauri && cargo test --lib && cargo clippy --all-targets -- -D warnings && cargo run --release --example eval`
Expected: all tests pass (including the three new ones), no warnings, and the eval table matches the current baseline within ±0.1 dB on every row. The only change on RGB inputs is where the blend is written.

- [ ] **Step 7: Commit**

```bash
git add src-tauri/Cargo.toml src-tauri/Cargo.lock src-tauri/src/removal
git commit -m "fix(removal): keep the source colour type and bit depth (16-bit, greyscale)"
```

---

## Self-review notes

- The five known issues map to Tasks 2 (overflow), 3 (orientation 8 tests), 4 (header-only dimensions), 5 (transparent bleed) and 6 (16-bit/greyscale). Task 1 adds the CI gate the roadmap asks for.
- Task 5 changes `refine_mask` and `Engine::inpaint` signatures. Task 6 consumes the Task-5 versions, so do them in order.
- Precision note, accepted by design: filled pixels in a 16-bit image come from an 8-bit patch scaled by 257. Only the synthesised pixels are affected; untouched pixels stay exact.
