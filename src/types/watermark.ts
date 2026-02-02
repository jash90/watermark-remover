export interface WatermarkRegion {
  x: number;
  y: number;
  width: number;
  height: number;
}

export interface RemovalOptions {
  algorithm: 'telea' | 'navier_stokes';
  dilate_pixels: number;
  inpaint_radius: number;
  processing_method: 'local' | 'cloud';
  lossless: boolean;
}

export interface ProcessResult {
  output_path: string;
  base64_preview: string | null;
  original_size: number;
  processed_size: number;
}

export interface ImageInfo {
  width: number;
  height: number;
  path: string;
}

export interface SelectionState {
  isSelecting: boolean;
  startX: number;
  startY: number;
  endX: number;
  endY: number;
}

export interface AppState {
  imagePath: string | null;
  imageBase64: string | null;
  imageInfo: ImageInfo | null;
  region: WatermarkRegion | null;
  processedImage: string | null;
  processedPath: string | null;
  isProcessing: boolean;
  error: string | null;
}

// Video-specific types
export interface VideoInfo {
  width: number;
  height: number;
  fps: number;
  frame_count: number;
  duration_secs: number;
  codec: string;
  path: string;
}

export interface VideoProgress {
  current_frame: number;
  total_frames: number;
  percent: number;
  estimated_remaining_secs: number | null;
}

export interface VideoProcessResult {
  output_path: string;
  frames_processed: number;
  duration_secs: number;
}

export type MediaType = 'image' | 'video';

export interface MediaFile {
  path: string;
  type: MediaType;
  extension: string;
}

// Batch processing types
export interface BatchFile {
  id: string;
  path: string;
  filename: string;
  status: 'pending' | 'processing' | 'completed' | 'failed';
  error?: string;
  processedPath?: string;
}

export interface BatchProgress {
  currentIndex: number;
  totalFiles: number;
  currentFilename: string;
}
