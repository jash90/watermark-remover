import { useState, useCallback, DragEvent } from 'react';
import { open } from '@tauri-apps/plugin-dialog';

import type { MediaType } from '../types/watermark';

interface MediaDropZoneProps {
  onMediaSelect: (path: string, type: MediaType) => void;
  onBatchSelect?: (paths: string[]) => void;
  disabled?: boolean;
}

// Backwards compatibility alias
interface ImageDropZoneProps extends MediaDropZoneProps {
  onImageSelect?: (path: string) => void;
}

const IMAGE_EXTENSIONS = ['png', 'jpg', 'jpeg', 'webp', 'gif'];
const VIDEO_EXTENSIONS = ['mp4', 'avi', 'mov', 'mkv', 'webm'];
const ACCEPTED_EXTENSIONS = [...IMAGE_EXTENSIONS, ...VIDEO_EXTENSIONS];

function getMediaType(extension: string): MediaType {
  return VIDEO_EXTENSIONS.includes(extension.toLowerCase()) ? 'video' : 'image';
}

export function ImageDropZone({ onMediaSelect, onBatchSelect, onImageSelect, disabled = false }: ImageDropZoneProps) {
  const [isDragging, setIsDragging] = useState(false);

  // Handle both new and legacy callbacks
  const handleSelect = (path: string, type: MediaType) => {
    if (onMediaSelect) {
      onMediaSelect(path, type);
    } else if (onImageSelect && type === 'image') {
      onImageSelect(path);
    }
  };

  // Handle multiple file selection for batch processing
  const handleBatchSelect = (paths: string[]) => {
    if (onBatchSelect) {
      // Filter to only include images for batch processing
      const imagePaths = paths.filter(path => {
        const ext = path.split('.').pop()?.toLowerCase() || '';
        return IMAGE_EXTENSIONS.includes(ext);
      });
      if (imagePaths.length > 0) {
        onBatchSelect(imagePaths);
      }
    }
  };

  const handleDragEnter = useCallback((e: DragEvent<HTMLDivElement>) => {
    e.preventDefault();
    e.stopPropagation();
    if (!disabled) {
      setIsDragging(true);
    }
  }, [disabled]);

  const handleDragLeave = useCallback((e: DragEvent<HTMLDivElement>) => {
    e.preventDefault();
    e.stopPropagation();
    setIsDragging(false);
  }, []);

  const handleDragOver = useCallback((e: DragEvent<HTMLDivElement>) => {
    e.preventDefault();
    e.stopPropagation();
  }, []);

  const handleDrop = useCallback((e: DragEvent<HTMLDivElement>) => {
    e.preventDefault();
    e.stopPropagation();
    setIsDragging(false);

    if (disabled) return;

    const files = e.dataTransfer.files;
    if (files.length > 0) {
      const file = files[0];
      const extension = file.name.split('.').pop()?.toLowerCase();
      if (extension && ACCEPTED_EXTENSIONS.includes(extension)) {
        // Note: In Tauri, we need to use the dialog for proper file access
        // Drag and drop gives us the file name but we need to use dialog for path
        handleOpenDialog();
      }
    }
  }, [disabled]);

  const handleOpenDialog = useCallback(async () => {
    if (disabled) return;

    try {
      const selected = await open({
        multiple: true,
        filters: [
          {
            name: 'Media Files',
            extensions: ACCEPTED_EXTENSIONS,
          },
          {
            name: 'Images',
            extensions: IMAGE_EXTENSIONS,
          },
          {
            name: 'Videos',
            extensions: VIDEO_EXTENSIONS,
          },
        ],
      });

      if (selected) {
        if (Array.isArray(selected) && selected.length > 1) {
          // Multiple files selected - batch mode (images only)
          handleBatchSelect(selected);
        } else {
          // Single file selected
          const path = Array.isArray(selected) ? selected[0] : selected;
          const extension = path.split('.').pop()?.toLowerCase() || '';
          const type = getMediaType(extension);
          handleSelect(path, type);
        }
      }
    } catch (err) {
      console.error('Failed to open file dialog:', err);
    }
  }, [handleSelect, handleBatchSelect, disabled]);

  return (
    <div
      className={`drop-zone ${isDragging ? 'dragging' : ''} ${disabled ? 'disabled' : ''}`}
      onDragEnter={handleDragEnter}
      onDragLeave={handleDragLeave}
      onDragOver={handleDragOver}
      onDrop={handleDrop}
      onClick={handleOpenDialog}
    >
      <div className="drop-zone-content">
        <svg
          className="drop-zone-icon"
          xmlns="http://www.w3.org/2000/svg"
          width="64"
          height="64"
          viewBox="0 0 24 24"
          fill="none"
          stroke="currentColor"
          strokeWidth="1.5"
          strokeLinecap="round"
          strokeLinejoin="round"
        >
          <rect x="3" y="3" width="18" height="18" rx="2" ry="2" />
          <circle cx="8.5" cy="8.5" r="1.5" />
          <polyline points="21 15 16 10 5 21" />
        </svg>
        <p className="drop-zone-text">
          {isDragging ? 'Drop files here' : 'Click or drag files to start'}
        </p>
        <p className="drop-zone-hint">
          Images: PNG, JPG, JPEG, WebP, GIF
        </p>
        <p className="drop-zone-hint">
          Videos: MP4, AVI, MOV, MKV, WebM
        </p>
        <p className="drop-zone-hint batch-hint">
          Select multiple images for batch processing
        </p>
      </div>
    </div>
  );
}
