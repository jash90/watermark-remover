import type { RemovalOptions } from '../types/watermark';

interface OptionsPanelProps {
  options: RemovalOptions;
  onChange: (options: RemovalOptions) => void;
  disabled?: boolean;
  hasApiKey?: boolean;
}

export function OptionsPanel({ options, onChange, disabled = false, hasApiKey = false }: OptionsPanelProps) {
  return (
    <div className={`options-panel ${disabled ? 'disabled' : ''}`}>
      <h3>Options</h3>

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

      <div className="option-group">
        <label className="checkbox-label">
          <input
            type="checkbox"
            checked={options.lossless}
            onChange={(e) => onChange({ ...options, lossless: e.target.checked })}
            disabled={disabled}
          />
          Lossless compression
        </label>
        <p className="option-description">
          JPEG is saved as PNG. PNG and WebP are always lossless.
        </p>
      </div>
    </div>
  );
}
