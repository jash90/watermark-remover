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
