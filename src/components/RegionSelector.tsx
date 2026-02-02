import { useRef, useState, useCallback, useEffect, MouseEvent } from 'react';
import type { WatermarkRegion, ImageInfo } from '../types/watermark';

interface RegionSelectorProps {
  imageSrc: string;
  imageInfo: ImageInfo;
  onRegionSelect: (region: WatermarkRegion) => void;
  selectedRegion: WatermarkRegion | null;
  disabled?: boolean;
}

interface Point {
  x: number;
  y: number;
}

export function RegionSelector({
  imageSrc,
  imageInfo,
  onRegionSelect,
  selectedRegion,
  disabled = false,
}: RegionSelectorProps) {
  const containerRef = useRef<HTMLDivElement>(null);
  const [isSelecting, setIsSelecting] = useState(false);
  const [startPoint, setStartPoint] = useState<Point | null>(null);
  const [currentPoint, setCurrentPoint] = useState<Point | null>(null);
  const [scale, setScale] = useState(1);

  // Calculate scale to fit image in container
  useEffect(() => {
    const updateScale = () => {
      if (containerRef.current && imageInfo) {
        const containerWidth = containerRef.current.clientWidth - 40; // padding
        const containerHeight = containerRef.current.clientHeight - 40;
        const scaleX = containerWidth / imageInfo.width;
        const scaleY = containerHeight / imageInfo.height;
        setScale(Math.min(scaleX, scaleY, 1)); // Don't scale up
      }
    };

    updateScale();
    window.addEventListener('resize', updateScale);
    return () => window.removeEventListener('resize', updateScale);
  }, [imageInfo]);

  const getImageCoordinates = useCallback((e: MouseEvent<HTMLDivElement>): Point | null => {
    if (!containerRef.current) return null;

    const rect = containerRef.current.getBoundingClientRect();
    const imgElement = containerRef.current.querySelector('img');
    if (!imgElement) return null;

    const imgRect = imgElement.getBoundingClientRect();
    const x = (e.clientX - imgRect.left) / scale;
    const y = (e.clientY - imgRect.top) / scale;

    // Clamp to image bounds
    return {
      x: Math.max(0, Math.min(imageInfo.width, Math.round(x))),
      y: Math.max(0, Math.min(imageInfo.height, Math.round(y))),
    };
  }, [scale, imageInfo]);

  const handleMouseDown = useCallback((e: MouseEvent<HTMLDivElement>) => {
    if (disabled) return;
    const point = getImageCoordinates(e);
    if (point) {
      setIsSelecting(true);
      setStartPoint(point);
      setCurrentPoint(point);
    }
  }, [disabled, getImageCoordinates]);

  const handleMouseMove = useCallback((e: MouseEvent<HTMLDivElement>) => {
    if (!isSelecting || disabled) return;
    const point = getImageCoordinates(e);
    if (point) {
      setCurrentPoint(point);
    }
  }, [isSelecting, disabled, getImageCoordinates]);

  const handleMouseUp = useCallback(() => {
    if (!isSelecting || !startPoint || !currentPoint || disabled) {
      setIsSelecting(false);
      return;
    }

    const x = Math.min(startPoint.x, currentPoint.x);
    const y = Math.min(startPoint.y, currentPoint.y);
    const width = Math.abs(currentPoint.x - startPoint.x);
    const height = Math.abs(currentPoint.y - startPoint.y);

    // Only register selection if it has meaningful size
    if (width > 5 && height > 5) {
      onRegionSelect({ x, y, width, height });
    }

    setIsSelecting(false);
    setStartPoint(null);
    setCurrentPoint(null);
  }, [isSelecting, startPoint, currentPoint, disabled, onRegionSelect]);

  // Get selection rectangle for display
  const getSelectionStyle = useCallback(() => {
    if (isSelecting && startPoint && currentPoint) {
      const x = Math.min(startPoint.x, currentPoint.x);
      const y = Math.min(startPoint.y, currentPoint.y);
      const width = Math.abs(currentPoint.x - startPoint.x);
      const height = Math.abs(currentPoint.y - startPoint.y);
      return {
        left: x * scale,
        top: y * scale,
        width: width * scale,
        height: height * scale,
      };
    }
    if (selectedRegion) {
      return {
        left: selectedRegion.x * scale,
        top: selectedRegion.y * scale,
        width: selectedRegion.width * scale,
        height: selectedRegion.height * scale,
      };
    }
    return null;
  }, [isSelecting, startPoint, currentPoint, selectedRegion, scale]);

  const selectionStyle = getSelectionStyle();

  return (
    <div
      ref={containerRef}
      className={`region-selector ${disabled ? 'disabled' : ''}`}
      onMouseDown={handleMouseDown}
      onMouseMove={handleMouseMove}
      onMouseUp={handleMouseUp}
      onMouseLeave={handleMouseUp}
    >
      <div className="image-container" style={{ width: imageInfo.width * scale, height: imageInfo.height * scale }}>
        <img
          src={imageSrc}
          alt="Source"
          style={{
            width: imageInfo.width * scale,
            height: imageInfo.height * scale,
          }}
          draggable={false}
        />
        {selectionStyle && (
          <div
            className="selection-overlay"
            style={selectionStyle}
          >
            <div className="selection-border" />
          </div>
        )}
      </div>
      {!disabled && (
        <p className="selector-hint">
          Click and drag to select the watermark region
        </p>
      )}
    </div>
  );
}
