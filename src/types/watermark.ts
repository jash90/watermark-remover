export interface WatermarkRegion {
  x: number;
  y: number;
  width: number;
  height: number;
}

export interface RemovalOptions {
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
