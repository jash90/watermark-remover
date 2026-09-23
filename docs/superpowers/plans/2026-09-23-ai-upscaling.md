# Plan C — AI Upscaling ×2 / ×4 / ×8 Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Add local, offline AI upscaling at ×2, ×4 and ×8. It works on an original image or on a watermark-removal result, with progress and cancel, and gives the same file-format guarantees as removal (original format, colour profile, sensible size).

**Architecture:**
- **Models:** Real-ESRGAN (BSD-3-Clause), exported by us to ONNX from the **official** `.pth` weights with a reproducible script. The small "Fast" model (`realesr-general-x4v3`, about 4.9 MB) ships inside the app. The "Quality" model (`RealESRGAN_x4plus`, about 67 MB) downloads on first use and is SHA-256 verified.
- **Inference:** ONNX Runtime via the `ort` crate. It runs tile by tile with overlap, so memory stays bounded and progress can be reported.
- **Scale factors:** ×4 is the native model factor. ×2 is ×4 followed by a high-quality ×0.5 Lanczos downscale (what Real-ESRGAN's own `--outscale` does). ×8 is ×4, then ×0.5, then ×4 again, so the second pass runs on 4× the pixels instead of 16×.
- **Intel Macs:** ONNX Runtime has no x86_64-macOS build (verified: ORT 1.30 ships only `osx-arm64`, and `ort`'s prebuilt list has no `x86_64-apple-darwin`). There, AI upscaling is compiled out and the UI offers a classic Lanczos upscale with an explanation.

**Tech Stack:** Rust: `ort` 2.0.0-rc.13 (`ndarray` feature), `ndarray`, `sha2`, `ureq`. Python 3 + PyTorch for the one-off model export. React + `@tauri-apps/api/event` for progress.

**Spec:** `docs/superpowers/plans/2026-09-23-roadmap-v0.3.md` (section "Plan C").

## Research notes (verified 2026-09-23)

| Fact | Source |
|---|---|
| Real-ESRGAN code and weights are licensed BSD-3-Clause | github.com/xinntao/Real-ESRGAN |
| Official weights: `realesr-general-x4v3.pth` 4,885,111 B (v0.2.5.0), `RealESRGAN_x4plus.pth` 67,040,989 B (v0.1.0), `RealESRGAN_x2plus.pth` 67,061,725 B (v0.2.1) | GitHub release assets (HTTP 200, checked) |
| `realesr-general-x4v3` = SRVGGNetCompact, 1.21 M params, about 4.65 MB fp32 | huggingface.co/qualcomm/Real-ESRGAN-General-x4v3 |
| Third-party ONNX exports on HF are of unknown provenance, and Qualcomm's are fixed 128×128 | HF model pages. **We export ourselves.** |
| `ort` newest is 2.0.0-rc.13 (2026-07-28); prebuilt binaries for aarch64-apple-darwin (+CoreML), Windows x64/arm64 (+DirectML), Linux x64/arm64; **none** for x86_64-apple-darwin | crates.io, `ort-sys/build/download/dist.tsv` |
| Microsoft ONNX Runtime 1.30 ships only `onnxruntime-osx-arm64` for macOS | GitHub releases API |
| Upscayl (a popular alternative) is AGPL-3.0, so we can't reuse it in this MIT app | github.com/upscayl/upscayl |

## Global Constraints

- **Offline first:** the Fast model is bundled. The Quality model is the only network access, and the user starts it explicitly.
- **Supply chain:** every downloaded model is verified against a SHA-256 pinned in the source. On a mismatch, delete the file and show an error.
- **Output limits:** refuse with a clear message when the result would exceed **256 MP** (`16384 × 16384`), or **65 535 px** on either side for JPEG.
- **Output file:** same rules as removal (`codec::output_format`, JPEG tables, ICC). **But** an indexed-PNG source is written as truecolour, because AI detail doesn't fit a palette.
- **Alpha:** RGB goes through the model. Alpha is upscaled separately with Lanczos3.
- `ort` stays pinned with `=` because it's a release candidate. Upgrades happen deliberately.
- All Rust commands run from `src-tauri/`. Gates: `cargo test --lib`, `cargo clippy --all-targets -- -D warnings`, `npm run build`.

## Review Focus

1. **Tile seams:** a tiled result must match a single-pass result within 3 levels. (Task 4 test `tiled_matches_untiled`.)
2. **Huge requests:** ×8 on a 4000×3000 photo is 32000×24000 = 768 MP and must be refused before any work starts. (Task 2 test `refuses_outputs_over_256_mp`.)
3. **Cancel mid-run:** processing stops within one tile, and no partial file is written. (Task 2 test `cancel_stops_between_tiles`.)
4. **Transparent PNG:** alpha is preserved and upscaled, not flattened. (Task 6 test `upscale_keeps_alpha`.)
5. **Tampered or truncated model download:** checksum error, the file is removed, and a retry is possible. (Task 5 test `rejects_model_with_wrong_checksum`.)

## File Structure

```
scripts/export_upscalers.py                          CREATE  .pth → ONNX export + parity fixtures (dev-only)
src-tauri/resources/models/realesr-general-x4v3.onnx CREATE  bundled Fast model (output of the script)
src-tauri/tests/fixtures/upscale/in_32.png           CREATE  parity input
src-tauri/tests/fixtures/upscale/x4v3_128.png        CREATE  PyTorch reference output
src-tauri/build.rs                                   MODIFY  cfg(ai_upscale) when the target has ONNX Runtime
src-tauri/Cargo.toml                                 MODIFY  ort/ndarray (target-gated), sha2, ureq
src-tauri/tauri.conf.json                            MODIFY  bundle.resources
src-tauri/src/upscale/mod.rs                         CREATE  Scale, UpscaleModel, pass plan, size guard, run()
src-tauri/src/upscale/tiling.rs                      CREATE  tiles(), upscale_tiled() with progress + cancel
src-tauri/src/upscale/onnx.rs                        CREATE  OnnxUpscaler (cfg(ai_upscale))
src-tauri/src/upscale/models.rs                      CREATE  registry, resolve, download + verify
src-tauri/src/removal/codec.rs                       MODIFY  preview_data_url()
src-tauri/src/commands.rs                            MODIFY  upscale_image, cancel_upscale, upscale_capabilities
src-tauri/src/lib.rs                                 MODIFY  pub mod upscale; manage state; register commands
src/types/watermark.ts                               MODIFY  UpscaleScale, UpscaleModel, UpscaleProgress
src/hooks/useUpscale.ts                              CREATE  invoke + progress events + cancel
src/components/UpscalePanel.tsx                      CREATE  scale/model pickers, size preview, progress, cancel
src/App.tsx                                          MODIFY  panel in 'select' and 'preview' stages
.github/workflows/release.yml                        MODIFY  nothing required; note that x86_64-apple-darwin builds without AI
```

---

### Task 1: Reproducible ONNX export from the official weights

**Files:**
- Create: `scripts/export_upscalers.py`, `src-tauri/resources/models/realesr-general-x4v3.onnx`, `src-tauri/tests/fixtures/upscale/in_32.png`, `src-tauri/tests/fixtures/upscale/x4v3_128.png`

**Interfaces:**
- Produces:
  - An ONNX graph with input `input` `[1, 3, H, W]` float32 RGB in [0, 1] and output `output` `[1, 3, 4H, 4W]` float32, with dynamic H/W.
  - Fixture PNGs.
  - Printed SHA-256 values for Task 5's registry.

- [ ] **Step 1: Write the script**

```python
#!/usr/bin/env python3
"""Export official Real-ESRGAN weights (BSD-3-Clause) to ONNX with dynamic H/W.

Usage:
  python3 -m venv .venv && .venv/bin/pip install torch onnx onnxruntime pillow numpy
  .venv/bin/python scripts/export_upscalers.py --out src-tauri/resources/models --fixtures src-tauri/tests/fixtures/upscale
The architectures are defined inline (copied from Real-ESRGAN/BasicSR, BSD-3) to avoid basicsr's torchvision pin.
"""
import argparse, hashlib, pathlib, urllib.request
import numpy as np, torch, torch.nn as nn, torch.nn.functional as F
from PIL import Image

WEIGHTS = {
    "realesr-general-x4v3": ("https://github.com/xinntao/Real-ESRGAN/releases/download/v0.2.5.0/realesr-general-x4v3.pth", 4885111),
    "RealESRGAN_x4plus": ("https://github.com/xinntao/Real-ESRGAN/releases/download/v0.1.0/RealESRGAN_x4plus.pth", 67040989),
}


class SRVGGNetCompact(nn.Module):
    def __init__(self, num_in_ch=3, num_out_ch=3, num_feat=64, num_conv=32, upscale=4):
        super().__init__()
        self.upscale = upscale
        self.body = nn.ModuleList([nn.Conv2d(num_in_ch, num_feat, 3, 1, 1), nn.PReLU(num_parameters=num_feat)])
        for _ in range(num_conv):
            self.body.append(nn.Conv2d(num_feat, num_feat, 3, 1, 1))
            self.body.append(nn.PReLU(num_parameters=num_feat))
        self.body.append(nn.Conv2d(num_feat, num_out_ch * upscale * upscale, 3, 1, 1))
        self.upsampler = nn.PixelShuffle(upscale)

    def forward(self, x):
        out = x
        for layer in self.body:
            out = layer(out)
        return self.upsampler(out) + F.interpolate(x, scale_factor=self.upscale, mode="nearest")


class ResidualDenseBlock(nn.Module):
    def __init__(self, num_feat=64, num_grow_ch=32):
        super().__init__()
        self.conv1 = nn.Conv2d(num_feat, num_grow_ch, 3, 1, 1)
        self.conv2 = nn.Conv2d(num_feat + num_grow_ch, num_grow_ch, 3, 1, 1)
        self.conv3 = nn.Conv2d(num_feat + 2 * num_grow_ch, num_grow_ch, 3, 1, 1)
        self.conv4 = nn.Conv2d(num_feat + 3 * num_grow_ch, num_grow_ch, 3, 1, 1)
        self.conv5 = nn.Conv2d(num_feat + 4 * num_grow_ch, num_feat, 3, 1, 1)
        self.lrelu = nn.LeakyReLU(negative_slope=0.2, inplace=True)

    def forward(self, x):
        x1 = self.lrelu(self.conv1(x))
        x2 = self.lrelu(self.conv2(torch.cat((x, x1), 1)))
        x3 = self.lrelu(self.conv3(torch.cat((x, x1, x2), 1)))
        x4 = self.lrelu(self.conv4(torch.cat((x, x1, x2, x3), 1)))
        x5 = self.conv5(torch.cat((x, x1, x2, x3, x4), 1))
        return x5 * 0.2 + x


class RRDB(nn.Module):
    def __init__(self, num_feat, num_grow_ch=32):
        super().__init__()
        self.rdb1 = ResidualDenseBlock(num_feat, num_grow_ch)
        self.rdb2 = ResidualDenseBlock(num_feat, num_grow_ch)
        self.rdb3 = ResidualDenseBlock(num_feat, num_grow_ch)

    def forward(self, x):
        return self.rdb3(self.rdb2(self.rdb1(x))) * 0.2 + x


class RRDBNet(nn.Module):
    def __init__(self, num_in_ch=3, num_out_ch=3, num_feat=64, num_block=23, num_grow_ch=32):
        super().__init__()
        self.conv_first = nn.Conv2d(num_in_ch, num_feat, 3, 1, 1)
        self.body = nn.Sequential(*[RRDB(num_feat, num_grow_ch) for _ in range(num_block)])
        self.conv_body = nn.Conv2d(num_feat, num_feat, 3, 1, 1)
        self.conv_up1 = nn.Conv2d(num_feat, num_feat, 3, 1, 1)
        self.conv_up2 = nn.Conv2d(num_feat, num_feat, 3, 1, 1)
        self.conv_hr = nn.Conv2d(num_feat, num_feat, 3, 1, 1)
        self.conv_last = nn.Conv2d(num_feat, num_out_ch, 3, 1, 1)
        self.lrelu = nn.LeakyReLU(negative_slope=0.2, inplace=True)

    def forward(self, x):
        feat = self.conv_first(x)
        feat = feat + self.conv_body(self.body(feat))
        feat = self.lrelu(self.conv_up1(F.interpolate(feat, scale_factor=2, mode="nearest")))
        feat = self.lrelu(self.conv_up2(F.interpolate(feat, scale_factor=2, mode="nearest")))
        return self.conv_last(self.lrelu(self.conv_hr(feat)))


def fetch(name, cache):
    url, size = WEIGHTS[name]
    path = cache / f"{name}.pth"
    if not path.exists():
        urllib.request.urlretrieve(url, path)
    assert path.stat().st_size == size, f"{name}: unexpected size {path.stat().st_size}"
    return path


def load(name, cache):
    model = SRVGGNetCompact() if name == "realesr-general-x4v3" else RRDBNet()
    state = torch.load(fetch(name, cache), map_location="cpu")
    state = state.get("params_ema", state.get("params", state))
    model.load_state_dict(state, strict=True)
    return model.eval()


def fixture_input():
    y, x = np.mgrid[0:32, 0:32]
    rng = np.random.default_rng(7)
    img = np.stack([x * 8, y * 8, (x + y) * 4], -1) + rng.integers(0, 24, (32, 32, 3))
    return np.clip(img, 0, 255).astype(np.uint8)


def export(name, out_dir, cache):
    model = load(name, cache)
    path = out_dir / f"{name}.onnx"
    torch.onnx.export(
        model, torch.rand(1, 3, 64, 64), path, opset_version=17,
        input_names=["input"], output_names=["output"],
        dynamic_axes={"input": {2: "h", 3: "w"}, "output": {2: "h4", 3: "w4"}},
    )
    import onnxruntime as ort
    sess = ort.InferenceSession(str(path), providers=["CPUExecutionProvider"])
    probe = torch.rand(1, 3, 48, 40)
    with torch.no_grad():
        ref = model(probe).numpy()
    got = sess.run(None, {"input": probe.numpy()})[0]
    diff = float(np.abs(ref - got).max())
    assert diff < 1e-3, f"{name}: ONNX/PyTorch mismatch {diff}"
    digest = hashlib.sha256(path.read_bytes()).hexdigest()
    print(f"{name}.onnx  {path.stat().st_size} bytes  sha256={digest}  max|diff|={diff:.2e}")
    return model


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("--out", required=True, type=pathlib.Path)
    ap.add_argument("--fixtures", required=True, type=pathlib.Path)
    ap.add_argument("--cache", default=pathlib.Path(".cache/weights"), type=pathlib.Path)
    ap.add_argument("--quality", action="store_true", help="also export RealESRGAN_x4plus (67 MB)")
    a = ap.parse_args()
    for d in (a.out, a.fixtures, a.cache):
        d.mkdir(parents=True, exist_ok=True)
    fast = export("realesr-general-x4v3", a.out, a.cache)
    if a.quality:
        export("RealESRGAN_x4plus", a.out, a.cache)
    src = fixture_input()
    Image.fromarray(src).save(a.fixtures / "in_32.png")
    with torch.no_grad():
        t = torch.from_numpy(src).permute(2, 0, 1)[None].float() / 255.0
        y = fast(t).clamp(0, 1)[0].permute(1, 2, 0).numpy()
    Image.fromarray((y * 255.0).round().astype(np.uint8)).save(a.fixtures / "x4v3_128.png")
    print("fixtures written")


if __name__ == "__main__":
    main()
```

- [ ] **Step 2: Run it**

Run:
```bash
python3 -m venv .venv && .venv/bin/pip install torch onnx onnxruntime pillow numpy
.venv/bin/python scripts/export_upscalers.py --out src-tauri/resources/models --fixtures src-tauri/tests/fixtures/upscale --quality
```
Expected output:
- `realesr-general-x4v3.onnx  ~4.9 MB  sha256=…  max|diff|=` below `1e-3`
- the same line for `RealESRGAN_x4plus.onnx`, at about 67 MB
- `fixtures written`

**Write both SHA-256 values down.** Task 5 pins them.

- [ ] **Step 3: Keep only the Fast model in git.** The Quality model is attached to a GitHub release in Task 5. Publishing is an outward action, so ask the repository owner first.

```bash
echo ".venv/\n.cache/\nsrc-tauri/resources/models/RealESRGAN_x4plus.onnx" >> .gitignore
git add scripts/export_upscalers.py src-tauri/resources/models/realesr-general-x4v3.onnx src-tauri/tests/fixtures/upscale .gitignore
git commit -m "chore: export Real-ESRGAN (BSD-3) to ONNX; bundle the fast x4 model"
```

---

### Task 2: Scale planning, size guard, tiling with progress and cancel (no AI yet)

**Files:**
- Create: `src-tauri/src/upscale/mod.rs`, `src-tauri/src/upscale/tiling.rs`
- Modify: `src-tauri/src/lib.rs` (add `pub mod upscale;`)

**Interfaces:**
- Produces:
  - `pub enum Scale { X2, X4, X8 }` (serde `"x2"|"x4"|"x8"`), `Scale::factor(self) -> u32`
  - `pub enum Pass { Model, Half }`, where `Model` runs the ×4 model and `Half` is a Lanczos ×0.5
  - `pub fn passes(scale: Scale) -> Vec<Pass>`: X2 → `[Model, Half]`, X4 → `[Model]`, X8 → `[Model, Half, Model]`
  - `pub const MAX_OUTPUT_PIXELS: u64 = 16384 * 16384;`
  - `pub fn output_size(w: u32, h: u32, scale: Scale) -> Result<(u32, u32), String>`
  - `tiling::Tile { x, y, w, h }`, `tiling::tiles(w: u32, h: u32, tile: u32) -> Vec<Tile>`
  - `tiling::upscale_tiled(img: &RgbImage, factor: u32, tile: u32, pad: u32, run: &mut dyn FnMut(&RgbImage) -> Result<RgbImage, String>, on_tile: &mut dyn FnMut(usize, usize), cancel: &AtomicBool) -> Result<RgbImage, String>`

- [ ] **Step 1: Write the failing tests** (`tiling.rs`)

```rust
use image::{imageops, RgbImage};
use std::sync::atomic::{AtomicBool, Ordering};

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
```

and in `mod.rs`:

```rust
#[cfg(test)]
mod tests {
    use super::*;

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
```

- [ ] **Step 2: Run to verify failure**

Run: `cd src-tauri && cargo test --lib upscale`
Expected: compile errors (missing items).

- [ ] **Step 3: Implement `tiling.rs`** (above the tests)

```rust
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

/// Runs `run` on overlapping tiles (each grown by `pad` px of real context, clamped
/// to the image) and stitches only the un-padded centre of every result.
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
        let (px, py) = (t.x.saturating_sub(pad), t.y.saturating_sub(pad));
        let (pw, ph) = ((t.x + t.w + pad).min(w) - px, (t.y + t.h + pad).min(h) - py);
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
```

- [ ] **Step 4: Implement `mod.rs`** (above the tests)

```rust
pub mod tiling;

use serde::{Deserialize, Serialize};

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
```

Add `pub mod upscale;` to `lib.rs`.

- [ ] **Step 5: Run to verify pass**

Run: `cd src-tauri && cargo test --lib upscale && cargo clippy --all-targets -- -D warnings`
Expected: 7 passed, no warnings.

- [ ] **Step 6: Commit**

```bash
git add src-tauri/src/upscale src-tauri/src/lib.rs
git commit -m "feat(upscale): scale plans, size guard, tiled runner with progress and cancel"
```

---

### Task 3: ONNX Runtime dependency, target-gated (Intel Mac excluded)

**Files:**
- Modify: `src-tauri/Cargo.toml`, `src-tauri/build.rs`

**Interfaces:**
- Produces: `cfg(ai_upscale)`, set by `build.rs` for every target except `x86_64-apple-darwin`. The `ort` + `ndarray` dependencies exist only on those targets.

- [ ] **Step 1: Cargo.toml**

```toml
# ONNX Runtime everywhere it ships binaries (not Intel macOS).
[target.'cfg(not(all(target_os = "macos", target_arch = "x86_64")))'.dependencies]
ort = { version = "=2.0.0-rc.13", features = ["ndarray"] }
ndarray = "0.16"

# GPU/NPU acceleration where available; Cargo unions these features with the table above.
[target.'cfg(all(target_os = "macos", target_arch = "aarch64"))'.dependencies]
ort = { version = "=2.0.0-rc.13", features = ["ndarray", "coreml"] }

[target.'cfg(windows)'.dependencies]
ort = { version = "=2.0.0-rc.13", features = ["ndarray", "directml"] }
```

Then run `cargo tree -i ndarray -e normal` and **set the `ndarray` version to the one `ort` uses** (0.16 or 0.17). Two different `ndarray` versions will not type-check against `ort`'s API.

- [ ] **Step 2: build.rs.** Replace the file with:

```rust
fn main() {
    println!("cargo::rustc-check-cfg=cfg(ai_upscale)");
    let os = std::env::var("CARGO_CFG_TARGET_OS").unwrap_or_default();
    let arch = std::env::var("CARGO_CFG_TARGET_ARCH").unwrap_or_default();
    // ONNX Runtime ships no x86_64 macOS binaries (ORT 1.30, ort 2.0.0-rc.13).
    if !(os == "macos" && arch == "x86_64") {
        println!("cargo:rustc-cfg=ai_upscale");
    }
    tauri_build::build()
}
```

- [ ] **Step 3: Verify both configurations compile**

Run:
```bash
cd src-tauri && cargo build && cargo build --target x86_64-apple-darwin
```
Expected: both succeed. Add the target first with `rustup target add x86_64-apple-darwin` if needed. The first build downloads ONNX Runtime; the second never mentions `ort`.

- [ ] **Step 4: Commit**

```bash
git add src-tauri/Cargo.toml src-tauri/Cargo.lock src-tauri/build.rs
git commit -m "build: add ONNX Runtime on targets that have it (not Intel macOS)"
```

---

### Task 4: `OnnxUpscaler` with parity and seam tests

**Files:**
- Create: `src-tauri/src/upscale/onnx.rs`
- Modify: `src-tauri/src/upscale/mod.rs` (add `#[cfg(ai_upscale)] pub mod onnx;`)

**Interfaces:**
- Consumes: the `ort` API (verify the names against the pinned rc before writing: `Session::builder()`, `with_execution_providers`, `commit_from_file`, `run(ort::inputs![...])`, `TensorRef::from_array_view`, `try_extract_array::<f32>()`). Also `tiling::upscale_tiled` (Task 2) and the Task 1 fixtures.
- Produces:
  - `pub struct OnnxUpscaler`
  - `OnnxUpscaler::load(path: &Path) -> Result<Self, String>`
  - `OnnxUpscaler::run(&mut self, img: &RgbImage) -> Result<RgbImage, String>`, which is exactly ×4

- [ ] **Step 1: Write the failing tests** (`onnx.rs`)

```rust
use image::{Rgb, RgbImage};
use ndarray::Array4;
use std::path::Path;

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
        let worst = out.pixels().zip(reference.pixels())
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
        let worst = whole.pixels().zip(tiled.pixels())
            .flat_map(|(a, b)| (0..3).map(move |c| (a[c] as i32 - b[c] as i32).abs()))
            .max()
            .unwrap();
        assert!(worst <= 3, "seam difference {worst}");
    }
}
```

- [ ] **Step 2: Run to verify failure**

Run: `cd src-tauri && cargo test --lib upscale::onnx`
Expected: compile error (`OnnxUpscaler` not found).

- [ ] **Step 3: Implement** (above the tests)

```rust
/// Real-ESRGAN ×4 model exported by scripts/export_upscalers.py:
/// input "input" [1,3,H,W] RGB in [0,1] → output "output" [1,3,4H,4W].
pub struct OnnxUpscaler {
    session: ort::session::Session,
}

impl OnnxUpscaler {
    pub fn load(path: &Path) -> Result<Self, String> {
        let builder = ort::session::Session::builder().map_err(|e| format!("ONNX Runtime init failed: {e}"))?;
        #[cfg(all(target_os = "macos", target_arch = "aarch64"))]
        let builder = builder
            .with_execution_providers([ort::execution_providers::CoreMLExecutionProvider::default().build()])
            .map_err(|e| format!("CoreML setup failed: {e}"))?;
        #[cfg(windows)]
        let builder = builder
            .with_execution_providers([ort::execution_providers::DirectMLExecutionProvider::default().build()])
            .map_err(|e| format!("DirectML setup failed: {e}"))?;
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
        let tensor = ort::value::TensorRef::from_array_view(&input).map_err(|e| e.to_string())?;
        let outputs = self.session.run(ort::inputs!["input" => tensor]).map_err(|e| format!("Upscaling failed: {e}"))?;
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
```

If the pinned rc names any of these differently, look the API up with context7 (`/pykeio/ort`) and adapt. Don't change the tests.

- [ ] **Step 4: Run to verify pass, and measure**

Run: `cd src-tauri && cargo test --release --lib upscale::onnx -- --nocapture`
Expected: 2 passed. Then time a 1024×1024 → 4096×4096 run through `upscale_tiled` (tile 256, pad 24) in a scratch example, and record it in the commit message. The target on Apple Silicon is under 15 s.

- [ ] **Step 5: Commit**

```bash
git add src-tauri/src/upscale
git commit -m "feat(upscale): ONNX Runtime Real-ESRGAN runner with parity and seam tests"
```

---

### Task 5: Model registry, bundled resolution and verified download

**Files:**
- Create: `src-tauri/src/upscale/models.rs`
- Modify: `src-tauri/Cargo.toml` (+ `sha2 = "0.10"`, `ureq = "3"`), `src-tauri/tauri.conf.json` (`bundle.resources`), `src-tauri/src/upscale/mod.rs` (`pub mod models;`)

**Interfaces:**
- Produces:
  - `pub enum UpscaleModel { Fast, Quality }` (serde `"fast"|"quality"`)
  - `pub struct ModelInfo { pub file: &'static str, pub sha256: &'static str, pub url: Option<&'static str>, pub bytes: u64 }`
  - `pub fn info(m: UpscaleModel) -> ModelInfo`
  - `pub fn verify(path: &Path, sha256: &str) -> Result<(), String>`
  - `pub fn download(url: &str, dest: &Path, sha256: &str, on_bytes: &mut dyn FnMut(u64, u64)) -> Result<(), String>`

- [ ] **Step 1: Failing tests** (`models.rs`)

```rust
use sha2::{Digest, Sha256};
use std::io::{Read, Write};
use std::path::Path;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bundled_fast_model_matches_pinned_checksum() {
        let path = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("resources/models")
            .join(info(UpscaleModel::Fast).file);
        verify(&path, info(UpscaleModel::Fast).sha256).unwrap();
    }

    #[test]
    fn rejects_model_with_wrong_checksum() {
        let path = std::env::temp_dir().join("wmr_bad_model.onnx");
        std::fs::write(&path, b"not a model").unwrap();
        let err = verify(&path, info(UpscaleModel::Fast).sha256).unwrap_err();
        assert!(err.contains("checksum"), "{err}");
    }
}
```

- [ ] **Step 2:** Run `cd src-tauri && cargo test --lib upscale::models`. Expected: compile error.

- [ ] **Step 3: Implement.** Paste the two SHA-256 values printed in Task 1.

```rust
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum UpscaleModel {
    Fast,
    Quality,
}

pub struct ModelInfo {
    pub file: &'static str,
    pub sha256: &'static str,
    /// None = bundled with the app.
    pub url: Option<&'static str>,
    pub bytes: u64,
}

pub fn info(m: UpscaleModel) -> ModelInfo {
    match m {
        UpscaleModel::Fast => ModelInfo {
            file: "realesr-general-x4v3.onnx",
            sha256: "<sha256 printed by scripts/export_upscalers.py>",
            url: None,
            bytes: 0,
        },
        UpscaleModel::Quality => ModelInfo {
            file: "RealESRGAN_x4plus.onnx",
            sha256: "<sha256 printed by scripts/export_upscalers.py>",
            url: Some("https://github.com/jash90/watermark-remover/releases/download/models-v1/RealESRGAN_x4plus.onnx"),
            bytes: 67_000_000,
        },
    }
}

fn sha256_of(path: &Path) -> Result<String, String> {
    let mut file = std::fs::File::open(path).map_err(|e| format!("Cannot read model: {e}"))?;
    let mut hasher = Sha256::new();
    let mut buf = vec![0u8; 1 << 20];
    loop {
        let n = file.read(&mut buf).map_err(|e| format!("Cannot read model: {e}"))?;
        if n == 0 {
            break;
        }
        hasher.update(&buf[..n]);
    }
    Ok(hasher.finalize().iter().map(|b| format!("{b:02x}")).collect())
}

pub fn verify(path: &Path, sha256: &str) -> Result<(), String> {
    let actual = sha256_of(path)?;
    if actual != sha256 {
        return Err(format!("Model checksum mismatch (expected {sha256}, got {actual})"));
    }
    Ok(())
}

/// Downloads to `<dest>.part`, verifies, then renames; a bad file never reaches `dest`.
pub fn download(url: &str, dest: &Path, sha256: &str, on_bytes: &mut dyn FnMut(u64, u64)) -> Result<(), String> {
    let part = dest.with_extension("part");
    let mut response = ureq::get(url).call().map_err(|e| format!("Download failed: {e}"))?;
    let total = response.body().content_length().unwrap_or(0);
    let mut reader = response.body_mut().as_reader();
    let mut file = std::fs::File::create(&part).map_err(|e| format!("Cannot write model: {e}"))?;
    let (mut buf, mut done) = (vec![0u8; 1 << 16], 0u64);
    loop {
        let n = reader.read(&mut buf).map_err(|e| format!("Download failed: {e}"))?;
        if n == 0 {
            break;
        }
        file.write_all(&buf[..n]).map_err(|e| format!("Cannot write model: {e}"))?;
        done += n as u64;
        on_bytes(done, total);
    }
    drop(file);
    if let Err(e) = verify(&part, sha256) {
        let _ = std::fs::remove_file(&part);
        return Err(e);
    }
    std::fs::rename(&part, dest).map_err(|e| format!("Cannot store model: {e}"))
}
```

(Verify the `ureq` 3 API — `call()`, `body()`, `body_mut().as_reader()`, `content_length()` — with context7 before compiling. Adjust the calls, not the behaviour.)

`tauri.conf.json` → inside `"bundle"` add:
```json
    "resources": {
      "resources/models/realesr-general-x4v3.onnx": "models/realesr-general-x4v3.onnx"
    },
```

- [ ] **Step 4:** Run `cd src-tauri && cargo test --lib upscale::models && cargo clippy --all-targets -- -D warnings`. Expected: 2 passed.

- [ ] **Step 5: Publish the Quality model.** This is an **outward action, so ask the repo owner first.** Create a GitHub release `models-v1` on `jash90/watermark-remover` with `RealESRGAN_x4plus.onnx` attached, and the BSD-3 licence text of Real-ESRGAN in the release notes:
```bash
gh release create models-v1 src-tauri/resources/models/RealESRGAN_x4plus.onnx --title "Upscaling models v1" --notes "Real-ESRGAN (BSD-3-Clause, © Xintao Wang) exported to ONNX by scripts/export_upscalers.py"
```
Then run `curl -sIL <url> | grep -i content-length`. Expected: about 67 MB.

- [ ] **Step 6: Commit**

```bash
git add src-tauri/Cargo.toml src-tauri/Cargo.lock src-tauri/tauri.conf.json src-tauri/src/upscale
git commit -m "feat(upscale): model registry, bundled fast model, verified download"
```

---

### Task 6: `upscale::run` + Tauri commands (progress, cancel, capabilities)

**Files:**
- Modify: `src-tauri/src/upscale/mod.rs` (add `run`), `src-tauri/src/removal/codec.rs` (add `preview_data_url`), `src-tauri/src/commands.rs`, `src-tauri/src/lib.rs`

**Interfaces:**
- Consumes: Tasks 2, 4 and 5.
- Produces:
  - `pub struct UpscaleProgress { pub pass: usize, pub passes: usize, pub done: usize, pub total: usize }` (Serialize, camelCase)
  - `pub fn run(img: &DynamicImage, scale: Scale, model_path: Option<&Path>, cancel: &AtomicBool, progress: &mut dyn FnMut(UpscaleProgress)) -> Result<DynamicImage, String>`. With `model_path = None` it runs classic Lanczos.
  - `codec::preview_data_url(img: &DynamicImage) -> Result<String, String>`: a JPEG no larger than 2048 px on the long side.
  - Commands:
    - `upscale_image(app, state, imagePath, scale, model, lossless) -> ProcessResult`
    - `cancel_upscale(state)`
    - `upscale_capabilities() -> { ai: bool }`
    - Event `"upscale-progress"` carrying `UpscaleProgress`, plus `"model-download"` carrying `{ done, total }`.

- [ ] **Step 1: Failing tests** (`upscale/mod.rs` tests)

```rust
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
```

(Progress is reported only for model passes, numbered 1-based by position in `passes()`: X8 → passes 1 and 3.)

- [ ] **Step 2:** Run `cd src-tauri && cargo test --lib upscale::tests`. Expected: compile error (`run` missing).

- [ ] **Step 3: Implement `run`** in `upscale/mod.rs`

```rust
use image::imageops::{self, FilterType};
use image::{DynamicImage, GenericImageView, RgbImage};
use std::path::Path;
use std::sync::atomic::AtomicBool;

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

/// Upscales RGB with the ×4 model (or Lanczos when `model_path` is None) following
/// `passes(scale)`; alpha is upscaled with Lanczos3 and re-attached.
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
    #[cfg(ai_upscale)]
    let mut model = match model_path {
        Some(p) => Some(onnx::OnnxUpscaler::load(p)?),
        None => None,
    };
    #[cfg(not(ai_upscale))]
    let _ = model_path;

    let mut rgb: RgbImage = img.to_rgb8();
    for (i, pass) in plan.iter().enumerate() {
        rgb = match pass {
            Pass::Half => imageops::resize(&rgb, rgb.width() / 2, rgb.height() / 2, FilterType::Lanczos3),
            Pass::Model => {
                #[cfg(ai_upscale)]
                if let Some(m) = model.as_mut() {
                    let (pass_no, total_passes) = (i + 1, plan.len());
                    tiling::upscale_tiled(&rgb, 4, TILE, TILE_PAD, &mut |t| m.run(t), &mut |done, total| {
                        progress(UpscaleProgress { pass: pass_no, passes: total_passes, done, total })
                    }, cancel)?
                } else {
                    imageops::resize(&rgb, rgb.width() * 4, rgb.height() * 4, FilterType::Lanczos3)
                }
                #[cfg(not(ai_upscale))]
                {
                    let _ = (i, &cancel, &progress);
                    imageops::resize(&rgb, rgb.width() * 4, rgb.height() * 4, FilterType::Lanczos3)
                }
            }
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
```

- [ ] **Step 4: `preview_data_url`** in `codec.rs` (with a test that a 5000×100 image yields a data URL whose decoded width is 2048):

```rust
/// Small JPEG preview for the webview, so huge results aren't shipped over IPC.
pub fn preview_data_url(img: &DynamicImage) -> Result<String, String> {
    use base64::{engine::general_purpose::STANDARD, Engine as _};
    let preview = if img.width().max(img.height()) > 2048 { img.resize(2048, 2048, image::imageops::FilterType::Triangle) } else { img.clone() };
    let bytes = encode(&preview, OutputFormat::Jpeg, &SourceHints::default())?;
    Ok(format!("data:image/jpeg;base64,{}", STANDARD.encode(bytes)))
}
```

- [ ] **Step 5: Commands** in `commands.rs`

```rust
use crate::upscale::{self, models::{self, UpscaleModel}, Scale};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use tauri::{Emitter, Manager};

#[derive(Default)]
pub struct UpscaleState {
    pub cancel: Arc<AtomicBool>,
}

#[derive(Serialize)]
pub struct UpscaleCapabilities {
    pub ai: bool,
}

#[tauri::command]
pub fn upscale_capabilities() -> UpscaleCapabilities {
    UpscaleCapabilities { ai: cfg!(ai_upscale) }
}

#[tauri::command]
pub fn cancel_upscale(state: tauri::State<'_, UpscaleState>) {
    state.cancel.store(true, Ordering::SeqCst);
}

fn model_path(app: &tauri::AppHandle, model: UpscaleModel) -> Result<Option<PathBuf>, String> {
    if !cfg!(ai_upscale) {
        return Ok(None);
    }
    let info = models::info(model);
    let path = match info.url {
        None => app
            .path()
            .resolve(format!("models/{}", info.file), tauri::path::BaseDirectory::Resource)
            .map_err(|e| e.to_string())?,
        Some(url) => {
            let dir = app.path().app_data_dir().map_err(|e| e.to_string())?.join("models");
            fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
            let path = dir.join(info.file);
            if models::verify(&path, info.sha256).is_err() {
                let handle = app.clone();
                models::download(url, &path, info.sha256, &mut |done, total| {
                    let _ = handle.emit("model-download", serde_json::json!({ "done": done, "total": total }));
                })?;
            }
            path
        }
    };
    models::verify(&path, info.sha256)?;
    Ok(Some(path))
}

#[tauri::command]
pub async fn upscale_image(
    app: tauri::AppHandle,
    state: tauri::State<'_, UpscaleState>,
    image_path: String,
    scale: Scale,
    model: UpscaleModel,
    lossless: bool,
) -> Result<ProcessResult, String> {
    let original_size = fs::metadata(&image_path).map_err(|e| format!("Failed to read file: {}", e))?.len();
    let cancel = state.cancel.clone();
    cancel.store(false, Ordering::SeqCst);
    let handle = app.clone();
    let (bytes, format, preview) = tauri::async_runtime::spawn_blocking(move || {
        let path = model_path(&handle, model)?;
        let original = codec::load_oriented(&image_path)?;
        let out = upscale::run(&original, scale, path.as_deref(), &cancel, &mut |p| {
            let _ = handle.emit("upscale-progress", p);
        })?;
        let ext = PathBuf::from(&image_path).extension().and_then(|e| e.to_str()).unwrap_or("png").to_string();
        let format = codec::output_format(&ext, lossless);
        let mut hints = fs::read(&image_path).map(|b| codec::read_source_hints(&b)).unwrap_or_default();
        hints.png_palette = None; // AI detail doesn't fit a palette
        let bytes = codec::encode(&out, format, &hints)?;
        Ok::<_, String>((bytes, format, codec::preview_data_url(&out)?))
    })
    .await
    .map_err(|e| format!("Upscaling task failed: {}", e))??;

    let output_path = get_temp_dir()?.join(format!(
        "upscaled_{}_{}.{}",
        Uuid::new_v4().to_string().split('-').next().unwrap_or("img"),
        chrono_lite_timestamp(),
        format.extension()
    ));
    fs::write(&output_path, &bytes).map_err(|e| format!("Failed to save upscaled image: {}", e))?;
    Ok(ProcessResult {
        output_path: output_path.to_string_lossy().to_string(),
        base64_preview: Some(preview),
        original_size,
        processed_size: bytes.len() as u64,
    })
}
```

In `lib.rs`: add `.manage(commands::UpscaleState::default())` to the builder, and register `upscale_image`, `cancel_upscale` and `upscale_capabilities` in `generate_handler!`.

- [ ] **Step 6: Verify**

Run: `cd src-tauri && cargo test --lib && cargo clippy --all-targets -- -D warnings && cargo build --target x86_64-apple-darwin`
Expected: all green. On the Intel target, `ai_x8_reports_three_passes_and_right_size` is compiled out and the command falls back to Lanczos.

- [ ] **Step 7: Commit**

```bash
git add src-tauri/src
git commit -m "feat(upscale): upscale_image command with progress, cancel and previews"
```

---

### Task 7: UI — Upscale panel

**Files:**
- Create: `src/hooks/useUpscale.ts`, `src/components/UpscalePanel.tsx`
- Modify: `src/types/watermark.ts`, `src/App.tsx` (`select` sidebar and `preview` actions), `src/App.css` (progress bar)

**Interfaces:**
- Consumes: the `upscale_image`, `cancel_upscale` and `upscale_capabilities` commands, and the `upscale-progress` / `model-download` events (Task 6).
- Produces: `useUpscale()` returns `{ ai, busy, progress, download, error, upscale(path, scale, model, lossless), cancel() }`.

- [ ] **Step 1: Types** (`src/types/watermark.ts`)

```ts
export type UpscaleScale = 'x2' | 'x4' | 'x8';
export type UpscaleModel = 'fast' | 'quality';

export interface UpscaleProgress {
  pass: number;
  passes: number;
  done: number;
  total: number;
}
```

- [ ] **Step 2: Hook** (`src/hooks/useUpscale.ts`)

```ts
import { useCallback, useEffect, useState } from 'react';
import { invoke } from '@tauri-apps/api/core';
import { listen } from '@tauri-apps/api/event';
import type { ProcessResult, UpscaleModel, UpscaleProgress, UpscaleScale } from '../types/watermark';

export function useUpscale() {
  const [ai, setAi] = useState(false);
  const [busy, setBusy] = useState(false);
  const [progress, setProgress] = useState<UpscaleProgress | null>(null);
  const [download, setDownload] = useState<{ done: number; total: number } | null>(null);
  const [error, setError] = useState<string | null>(null);

  useEffect(() => {
    invoke<{ ai: boolean }>('upscale_capabilities').then((c) => setAi(c.ai)).catch(() => setAi(false));
    const offProgress = listen<UpscaleProgress>('upscale-progress', (e) => setProgress(e.payload));
    const offDownload = listen<{ done: number; total: number }>('model-download', (e) => setDownload(e.payload));
    return () => {
      offProgress.then((f) => f());
      offDownload.then((f) => f());
    };
  }, []);

  const upscale = useCallback(
    async (imagePath: string, scale: UpscaleScale, model: UpscaleModel, lossless: boolean): Promise<ProcessResult | null> => {
      setBusy(true);
      setError(null);
      setProgress(null);
      setDownload(null);
      try {
        return await invoke<ProcessResult>('upscale_image', { imagePath, scale, model, lossless });
      } catch (err) {
        const message = err instanceof Error ? err.message : String(err);
        if (message !== 'Cancelled') setError(message);
        return null;
      } finally {
        setBusy(false);
      }
    },
    []
  );

  const cancel = useCallback(() => {
    invoke('cancel_upscale');
  }, []);

  return { ai, busy, progress, download, error, upscale, cancel };
}
```

- [ ] **Step 3: Panel** (`src/components/UpscalePanel.tsx`)

```tsx
import { useState } from 'react';
import type { UpscaleModel, UpscaleProgress, UpscaleScale } from '../types/watermark';

interface UpscalePanelProps {
  width: number;
  height: number;
  ai: boolean;
  busy: boolean;
  progress: UpscaleProgress | null;
  download: { done: number; total: number } | null;
  onUpscale: (scale: UpscaleScale, model: UpscaleModel) => void;
  onCancel: () => void;
}

const FACTORS: Record<UpscaleScale, number> = { x2: 2, x4: 4, x8: 8 };
const MAX_PIXELS = 16384 * 16384;

export function UpscalePanel({ width, height, ai, busy, progress, download, onUpscale, onCancel }: UpscalePanelProps) {
  const [scale, setScale] = useState<UpscaleScale>('x2');
  const [model, setModel] = useState<UpscaleModel>('fast');
  const f = FACTORS[scale];
  const tooBig = width * f * height * f > MAX_PIXELS;
  const percent = progress ? Math.round(((progress.pass - 1 + progress.done / progress.total) / progress.passes) * 100) : 0;

  return (
    <div className="options-panel">
      <h3>Upscale</h3>
      <div className="option-group">
        {(Object.keys(FACTORS) as UpscaleScale[]).map((s) => (
          <label key={s} className="checkbox-label">
            <input type="radio" name="scale" checked={scale === s} onChange={() => setScale(s)} disabled={busy} />
            {FACTORS[s]}×
          </label>
        ))}
        <p className="option-description">
          {width}×{height} → {width * f}×{height * f}
        </p>
        {tooBig && <p className="option-warning">Too large (over 256 MP). Choose a smaller scale.</p>}
      </div>

      {ai ? (
        <div className="option-group">
          <label className="checkbox-label">
            <input type="radio" name="model" checked={model === 'fast'} onChange={() => setModel('fast')} disabled={busy} />
            AI — Fast (built in)
          </label>
          <label className="checkbox-label">
            <input type="radio" name="model" checked={model === 'quality'} onChange={() => setModel('quality')} disabled={busy} />
            AI — Best quality (downloads 67 MB once)
          </label>
        </div>
      ) : (
        <p className="option-description">
          AI upscaling isn't available on Intel Macs. A classic high-quality (Lanczos) upscale is used instead.
        </p>
      )}

      {busy ? (
        <>
          {download && download.done < download.total && (
            <p className="option-description">
              Downloading model… {Math.round((download.done / Math.max(download.total, 1)) * 100)}%
            </p>
          )}
          <progress className="upscale-progress" value={percent} max={100} />
          <button className="btn btn-secondary" onClick={onCancel}>
            Cancel
          </button>
        </>
      ) : (
        <button className="btn btn-primary" onClick={() => onUpscale(scale, model)} disabled={tooBig}>
          Upscale {FACTORS[scale]}×
        </button>
      )}
    </div>
  );
}
```

Add to `App.css`:
```css
.upscale-progress {
  width: 100%;
  height: 8px;
  margin: 8px 0;
}
```

- [ ] **Step 4: Wire into `App.tsx`**
  - `const upscaler = useUpscale();`
  - A handler: `handleUpscale(scale, model)`. It uses `processedPath ?? imagePath` as the source, calls `upscaler.upscale(source, scale, model, options.lossless)`, and on success sets `processedImage`, `processedPath`, `originalSize` and `processedSize` from the result, then calls `setStage('preview')`.
  - Render `<UpscalePanel …>` in the `select` sidebar under `OptionsPanel` (upscales the original) and in `preview-actions` (upscales the result). Pass `width`/`height` from `imageInfo`.
  - Show `upscaler.error` in the existing error banner: `{(error || upscaler.error) && …}`.
  - `handleSave` already uses the extension of `processedPath`. Change the default filename suffix to `_upscaled` when the last action was an upscale. Track that with `const [lastAction, setLastAction] = useState<'remove' | 'upscale'>('remove')`.

- [ ] **Step 5: Verify**

Run: `npm run build`, then `npm run tauri dev`. Check by hand:
1. ×2 Fast on the 2048² Gemini sample gives a 4096² result, the preview shows, and the saved file is a truecolour PNG.
2. ×8 on a 4000×3000 JPEG: the button is disabled with the 256 MP warning.
3. Cancel during ×4: the UI returns to idle with no error banner and no file.
4. Quality model the first time: the download percentage shows and the second run starts immediately.
5. Remove a watermark, then upscale the result ×4: both steps chain correctly.

- [ ] **Step 6: Commit**

```bash
git add src
git commit -m "feat(ui): upscale panel (2x/4x/8x, fast/quality, progress, cancel)"
```

---

### Task 8: Docs and release notes

**Files:**
- Modify: `README.md` (feature list, "Upscaling" section, third-party licences), `.github/workflows/release.yml` (release body)

- [ ] **Step 1:** Add to README:
  - **Features:** "**AI Upscaling** - 2×, 4× or 8× with Real-ESRGAN, fully offline (Intel Macs use high-quality Lanczos)."
  - A "Third-party models" section: "Real-ESRGAN © 2021 Xintao Wang, BSD-3-Clause. Models exported to ONNX with `scripts/export_upscalers.py`."
- [ ] **Step 2:** Add the same feature line to the `releaseBody` in `release.yml`.
- [ ] **Step 3: Commit**

```bash
git add README.md .github/workflows/release.yml
git commit -m "docs: document AI upscaling and model licences"
```

---

## Self-review notes

- The scale set ×2/×4/×8 maps to Tasks 2 and 6. Fast and Quality models map to Tasks 1 and 5. Intel Mac maps to Task 3 (cfg) plus Task 7 (copy). Progress and cancel map to Tasks 2, 6 and 7. The format and ICC rules come from Task 6, which reuses `codec`.
- Two `ort`/`ureq` API spots are explicitly marked "verify with context7 against the pinned version". Those are the places a release candidate can drift. The tests pin the behaviour.
- The only outward action is Task 5 Step 5 (publishing a release asset), which requires the owner's approval.
