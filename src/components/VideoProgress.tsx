import { useEffect, useState } from 'react';
import type { VideoProgress as VideoProgressType } from '../types/watermark';

interface VideoProgressProps {
  progress: VideoProgressType;
  onCancel: () => void;
  startTime: number;
}

function formatTime(seconds: number): string {
  if (seconds < 60) {
    return `${Math.round(seconds)}s`;
  }
  const minutes = Math.floor(seconds / 60);
  const secs = Math.round(seconds % 60);
  return `${minutes}m ${secs}s`;
}

export function VideoProgress({ progress, onCancel, startTime }: VideoProgressProps) {
  const [elapsedTime, setElapsedTime] = useState(0);

  useEffect(() => {
    const interval = setInterval(() => {
      setElapsedTime((Date.now() - startTime) / 1000);
    }, 1000);

    return () => clearInterval(interval);
  }, [startTime]);

  const { current_frame, total_frames, percent } = progress;

  // Estimate remaining time based on elapsed time and progress
  const estimatedTotal = percent > 0 ? (elapsedTime / percent) * 100 : 0;
  const estimatedRemaining = Math.max(0, estimatedTotal - elapsedTime);

  return (
    <div className="video-progress">
      <div className="video-progress-header">
        <h3>Processing Video</h3>
        <p className="video-progress-subtitle">
          Removing watermark frame by frame...
        </p>
      </div>

      <div className="video-progress-bar-container">
        <div
          className="video-progress-bar"
          style={{ width: `${Math.min(100, percent)}%` }}
        />
      </div>

      <div className="video-progress-stats">
        <div className="video-progress-stat">
          <span className="stat-label">Progress</span>
          <span className="stat-value">{percent.toFixed(1)}%</span>
        </div>
        <div className="video-progress-stat">
          <span className="stat-label">Frames</span>
          <span className="stat-value">{current_frame} / {total_frames}</span>
        </div>
        <div className="video-progress-stat">
          <span className="stat-label">Elapsed</span>
          <span className="stat-value">{formatTime(elapsedTime)}</span>
        </div>
        <div className="video-progress-stat">
          <span className="stat-label">Remaining</span>
          <span className="stat-value">
            {percent > 5 ? `~${formatTime(estimatedRemaining)}` : 'Calculating...'}
          </span>
        </div>
      </div>

      <div className="video-progress-info">
        <p>
          <strong>Note:</strong> Video processing can take a while depending on
          the video length and resolution. The watermark will be removed from
          every frame using inpainting.
        </p>
      </div>

      <button
        className="btn btn-secondary btn-cancel"
        onClick={onCancel}
      >
        Cancel Processing
      </button>
    </div>
  );
}
