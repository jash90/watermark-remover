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
        <p className="option-description">
          Uses Google Gemini AI for intelligent watermark removal. Requires internet and API key.
        </p>
        {!hasApiKey && (
          <p className="option-warning">
            API key not configured. Please set it in Settings.
          </p>
        )}
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
          Preserves full image quality. JPEG files will be saved as PNG.
        </p>
      </div>

      <div className="cloud-info">
        <p>Cloud processing uses AI to intelligently fill the watermark area.</p>
      </div>
    </div>
  );
}
