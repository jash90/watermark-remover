import type { RemovalOptions } from '../types/watermark';

interface OptionsPanelProps {
  options: RemovalOptions;
  onChange: (options: RemovalOptions) => void;
  disabled?: boolean;
  hasApiKey?: boolean;
}

export function OptionsPanel({ options, onChange, disabled = false, hasApiKey = false }: OptionsPanelProps) {
  const isCloudEnabled = options.processing_method === 'cloud';

  return (
    <div className={`options-panel ${disabled ? 'disabled' : ''}`}>
      <h3>Options</h3>

      <div className="option-group">
        <label htmlFor="processing_method">Processing Method</label>
        <select
          id="processing_method"
          value={options.processing_method}
          onChange={(e) => onChange({ ...options, processing_method: e.target.value as 'local' | 'cloud' })}
          disabled={disabled}
        >
          <option value="local">Local (OpenCV) - Free, Offline</option>
          <option value="cloud" disabled={!hasApiKey}>
            Cloud (Gemini AI) - Better Quality {!hasApiKey && '(API key required)'}
          </option>
        </select>
        <p className="option-description">
          {isCloudEnabled
            ? 'Uses Google Gemini AI for intelligent watermark removal. Requires internet.'
            : 'Uses local OpenCV inpainting. Works offline, no external dependencies.'}
        </p>
      </div>

      {!isCloudEnabled && (
        <>
          <div className="option-group">
            <label htmlFor="algorithm">Algorithm</label>
            <select
              id="algorithm"
              value={options.algorithm}
              onChange={(e) => onChange({ ...options, algorithm: e.target.value as 'telea' | 'navier_stokes' })}
              disabled={disabled}
            >
              <option value="telea">Telea (Recommended)</option>
              <option value="navier_stokes">Navier-Stokes</option>
            </select>
            <p className="option-description">
              Telea: Fast, good for most cases. Navier-Stokes: Better for complex textures.
            </p>
          </div>

          <div className="option-group">
            <label htmlFor="dilate">Edge Expansion: {options.dilate_pixels}px</label>
            <input
              type="range"
              id="dilate"
              min="0"
              max="10"
              value={options.dilate_pixels}
              onChange={(e) => onChange({ ...options, dilate_pixels: parseInt(e.target.value) })}
              disabled={disabled}
            />
            <p className="option-description">
              Expands the selection to include edge artifacts. Higher = more expansion.
            </p>
          </div>

          <div className="option-group">
            <label htmlFor="radius">Inpaint Radius: {options.inpaint_radius}</label>
            <input
              type="range"
              id="radius"
              min="1"
              max="15"
              step="0.5"
              value={options.inpaint_radius}
              onChange={(e) => onChange({ ...options, inpaint_radius: parseFloat(e.target.value) })}
              disabled={disabled}
            />
            <p className="option-description">
              How far to look for pixel data when filling. Higher = smoother but slower.
            </p>
          </div>
        </>
      )}

      {/* Lossless option - available for both local and cloud methods */}
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

      {isCloudEnabled && (
        <div className="cloud-info">
          <p>Cloud processing uses AI to intelligently fill the watermark area.</p>
          <p className="cloud-note">Note: Video processing is not supported with cloud method.</p>
        </div>
      )}
    </div>
  );
}
