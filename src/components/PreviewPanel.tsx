import { useState, useRef, useCallback, MouseEvent } from 'react';

interface PreviewPanelProps {
  originalImage: string;
  processedImage: string;
  imageWidth: number;
  imageHeight: number;
  originalSize?: number;
  processedSize?: number;
}

function formatFileSize(bytes: number): string {
  if (bytes === 0) return '0 B';
  const k = 1024;
  const sizes = ['B', 'KB', 'MB', 'GB'];
  const i = Math.floor(Math.log(bytes) / Math.log(k));
  return parseFloat((bytes / Math.pow(k, i)).toFixed(2)) + ' ' + sizes[i];
}

export function PreviewPanel({
  originalImage,
  processedImage,
  imageWidth,
  imageHeight,
  originalSize,
  processedSize,
}: PreviewPanelProps) {
  const containerRef = useRef<HTMLDivElement>(null);
  const [sliderPosition, setSliderPosition] = useState(50);
  const [isDragging, setIsDragging] = useState(false);

  // Calculate display size
  const maxWidth = 600;
  const maxHeight = 500;
  const scaleX = maxWidth / imageWidth;
  const scaleY = maxHeight / imageHeight;
  const scale = Math.min(scaleX, scaleY, 1);
  const displayWidth = imageWidth * scale;
  const displayHeight = imageHeight * scale;

  const updateSliderPosition = useCallback((e: MouseEvent<HTMLDivElement>) => {
    if (!containerRef.current) return;
    const rect = containerRef.current.getBoundingClientRect();
    const x = e.clientX - rect.left;
    const percentage = Math.max(0, Math.min(100, (x / rect.width) * 100));
    setSliderPosition(percentage);
  }, []);

  const handleMouseDown = useCallback((e: MouseEvent<HTMLDivElement>) => {
    setIsDragging(true);
    updateSliderPosition(e);
  }, [updateSliderPosition]);

  const handleMouseMove = useCallback((e: MouseEvent<HTMLDivElement>) => {
    if (isDragging) {
      updateSliderPosition(e);
    }
  }, [isDragging, updateSliderPosition]);

  const handleMouseUp = useCallback(() => {
    setIsDragging(false);
  }, []);

  return (
    <div className="preview-panel">
      <div className="preview-labels">
        <span>Before</span>
        <span>After</span>
      </div>
      <div
        ref={containerRef}
        className="preview-container"
        style={{ width: displayWidth, height: displayHeight }}
        onMouseDown={handleMouseDown}
        onMouseMove={handleMouseMove}
        onMouseUp={handleMouseUp}
        onMouseLeave={handleMouseUp}
      >
        {/* Processed (After) image - full width background */}
        <img
          src={processedImage}
          alt="Processed"
          className="preview-image preview-after"
          style={{ width: displayWidth, height: displayHeight }}
          draggable={false}
        />

        {/* Original (Before) image - clipped by slider */}
        <div
          className="preview-original-wrapper"
          style={{
            width: `${sliderPosition}%`,
            height: displayHeight,
          }}
        >
          <img
            src={originalImage}
            alt="Original"
            className="preview-image preview-before"
            style={{ width: displayWidth, height: displayHeight }}
            draggable={false}
          />
        </div>

        {/* Slider handle */}
        <div
          className="preview-slider"
          style={{ left: `${sliderPosition}%` }}
        >
          <div className="slider-line" />
          <div className="slider-handle">
            <svg width="24" height="24" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2">
              <polyline points="15 18 9 12 15 6" />
              <polyline points="9 18 15 12 9 6" transform="translate(6, 0)" />
            </svg>
          </div>
        </div>
      </div>
      <p className="preview-hint">Drag the slider to compare</p>
      {originalSize !== undefined && processedSize !== undefined && (
        <div className="size-comparison">
          <span className="size-original">{formatFileSize(originalSize)}</span>
          <span className="size-arrow">→</span>
          <span className="size-processed">{formatFileSize(processedSize)}</span>
          {originalSize > processedSize ? (
            <span className="size-saved size-saved-positive">
              -{Math.round((1 - processedSize / originalSize) * 100)}%
            </span>
          ) : originalSize < processedSize ? (
            <span className="size-saved size-saved-negative">
              +{Math.round((processedSize / originalSize - 1) * 100)}%
            </span>
          ) : (
            <span className="size-saved">0%</span>
          )}
        </div>
      )}
    </div>
  );
}
