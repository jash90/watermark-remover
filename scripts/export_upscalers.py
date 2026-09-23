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
        external_data=False,  # one self-contained file (the dynamo exporter defaults to external weights)
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
