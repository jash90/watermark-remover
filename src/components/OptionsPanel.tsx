import type { RemovalOptions } from '../types/watermark';

interface OptionsPanelProps {
  options: RemovalOptions;
  onChange: (options: RemovalOptions) => void;
  disabled?: boolean;
}

export function OptionsPanel({ options, onChange, disabled = false }: OptionsPanelProps) {
  return (
    <div className={`options-panel ${disabled ? 'disabled' : ''}`}>
      <h3>Options</h3>

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

      <div className="option-group">
        <label className="checkbox-label">
          <input
            type="checkbox"
            checked={options.lossless}
            onChange={(e) => onChange({ ...options, lossless: e.target.checked })}
            disabled={disabled}
          />
          Save JPEG as PNG (lossless)
        </label>
        <p className="option-description">
          Avoids re-compressing the photo, but a PNG is usually 4–5× larger than the JPEG. Leave off to keep the original file size. PNG and WebP inputs are always saved losslessly.
        </p>
      </div>
    </div>
  );
}
