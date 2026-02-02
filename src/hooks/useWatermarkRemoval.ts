import { useState, useCallback, useRef, useEffect } from 'react';
import { invoke } from '@tauri-apps/api/core';
import type {
  WatermarkRegion,
  RemovalOptions,
  ProcessResult,
  ImageInfo,
  VideoInfo,
  VideoProgress,
  VideoProcessResult,
  BatchFile,
} from '../types/watermark';

interface UseWatermarkRemovalReturn {
  // State
  isProcessing: boolean;
  error: string | null;
  processedResult: ProcessResult | null;
  videoProgress: VideoProgress | null;
  hasApiKey: boolean;

  // Image functions
  removeWatermark: (imagePath: string, region: WatermarkRegion, options?: Partial<RemovalOptions>) => Promise<ProcessResult | null>;
  removeWatermarkCloud: (imagePath: string, region: WatermarkRegion) => Promise<ProcessResult | null>;
  getImageInfo: (imagePath: string) => Promise<ImageInfo | null>;
  loadImageBase64: (imagePath: string) => Promise<string | null>;
  saveProcessedImage: (sourcePath: string, destinationPath: string) => Promise<boolean>;
  cleanupTempFiles: () => Promise<void>;

  // Video functions
  getVideoInfo: (videoPath: string) => Promise<VideoInfo | null>;
  extractVideoFrame: (videoPath: string) => Promise<string | null>;
  processVideo: (videoPath: string, region: WatermarkRegion, options?: Partial<RemovalOptions>) => Promise<VideoProcessResult | null>;
  cancelVideoProcessing: () => Promise<void>;

  // Batch processing functions
  processBatch: (
    files: BatchFile[],
    region: WatermarkRegion,
    options: RemovalOptions,
    onFileUpdate: (id: string, update: Partial<BatchFile>) => void
  ) => Promise<void>;

  // API Key functions
  checkApiKey: () => Promise<void>;

  // Utility
  clearError: () => void;
  clearResult: () => void;
}

const DEFAULT_OPTIONS: RemovalOptions = {
  algorithm: 'telea',
  dilate_pixels: 3,
  inpaint_radius: 5,
  processing_method: 'local',
  lossless: false,
};

export function useWatermarkRemoval(): UseWatermarkRemovalReturn {
  const [isProcessing, setIsProcessing] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [processedResult, setProcessedResult] = useState<ProcessResult | null>(null);
  const [videoProgress, setVideoProgress] = useState<VideoProgress | null>(null);
  const [hasApiKey, setHasApiKey] = useState(false);
  const progressIntervalRef = useRef<NodeJS.Timeout | null>(null);

  // Check for API key on mount
  const checkApiKey = useCallback(async () => {
    try {
      const key = await invoke<string>('get_gemini_api_key');
      setHasApiKey(key.length > 0);
    } catch {
      setHasApiKey(false);
    }
  }, []);

  useEffect(() => {
    checkApiKey();
  }, [checkApiKey]);

  // Cleanup progress polling on unmount
  useEffect(() => {
    return () => {
      if (progressIntervalRef.current) {
        clearInterval(progressIntervalRef.current);
      }
    };
  }, []);

  const removeWatermark = useCallback(async (
    imagePath: string,
    region: WatermarkRegion,
    options?: Partial<RemovalOptions>
  ): Promise<ProcessResult | null> => {
    setIsProcessing(true);
    setError(null);

    try {
      const mergedOptions = { ...DEFAULT_OPTIONS, ...options };

      // Use cloud processing if selected
      if (mergedOptions.processing_method === 'cloud') {
        const result = await invoke<ProcessResult>('remove_watermark_cloud', {
          imagePath,
          region,
          options: mergedOptions,
        });
        setProcessedResult(result);
        return result;
      }

      // Use local processing
      const result = await invoke<ProcessResult>('remove_watermark', {
        imagePath,
        region,
        options: mergedOptions,
      });
      setProcessedResult(result);
      return result;
    } catch (err) {
      const errorMessage = err instanceof Error ? err.message : String(err);
      setError(errorMessage);
      return null;
    } finally {
      setIsProcessing(false);
    }
  }, []);

  const removeWatermarkCloud = useCallback(async (
    imagePath: string,
    region: WatermarkRegion
  ): Promise<ProcessResult | null> => {
    setIsProcessing(true);
    setError(null);

    try {
      const result = await invoke<ProcessResult>('remove_watermark_cloud', {
        imagePath,
        region,
      });
      setProcessedResult(result);
      return result;
    } catch (err) {
      const errorMessage = err instanceof Error ? err.message : String(err);
      setError(errorMessage);
      return null;
    } finally {
      setIsProcessing(false);
    }
  }, []);

  const getImageInfo = useCallback(async (imagePath: string): Promise<ImageInfo | null> => {
    try {
      return await invoke<ImageInfo>('get_image_info', { imagePath });
    } catch (err) {
      const errorMessage = err instanceof Error ? err.message : String(err);
      setError(errorMessage);
      return null;
    }
  }, []);

  const loadImageBase64 = useCallback(async (imagePath: string): Promise<string | null> => {
    try {
      return await invoke<string>('load_image_base64', { imagePath });
    } catch (err) {
      const errorMessage = err instanceof Error ? err.message : String(err);
      setError(errorMessage);
      return null;
    }
  }, []);

  const saveProcessedImage = useCallback(async (
    sourcePath: string,
    destinationPath: string
  ): Promise<boolean> => {
    try {
      await invoke('save_processed_image', { sourcePath, destinationPath });
      return true;
    } catch (err) {
      const errorMessage = err instanceof Error ? err.message : String(err);
      setError(errorMessage);
      return false;
    }
  }, []);

  const cleanupTempFiles = useCallback(async (): Promise<void> => {
    try {
      await invoke('cleanup_temp_files');
    } catch (err) {
      console.error('Failed to cleanup temp files:', err);
    }
  }, []);

  const clearError = useCallback(() => {
    setError(null);
  }, []);

  const clearResult = useCallback(() => {
    setProcessedResult(null);
  }, []);

  // ================================
  // Video-specific functions
  // ================================

  const getVideoInfo = useCallback(async (videoPath: string): Promise<VideoInfo | null> => {
    try {
      return await invoke<VideoInfo>('get_video_info', { videoPath });
    } catch (err) {
      const errorMessage = err instanceof Error ? err.message : String(err);
      setError(errorMessage);
      return null;
    }
  }, []);

  const extractVideoFrame = useCallback(async (videoPath: string): Promise<string | null> => {
    try {
      return await invoke<string>('extract_video_frame', { videoPath, outputPath: '' });
    } catch (err) {
      const errorMessage = err instanceof Error ? err.message : String(err);
      setError(errorMessage);
      return null;
    }
  }, []);

  const startProgressPolling = useCallback(() => {
    if (progressIntervalRef.current) {
      clearInterval(progressIntervalRef.current);
    }

    progressIntervalRef.current = setInterval(async () => {
      try {
        const progress = await invoke<VideoProgress>('get_video_progress');
        setVideoProgress(progress);
      } catch (err) {
        console.error('Failed to get video progress:', err);
      }
    }, 500); // Poll every 500ms
  }, []);

  const stopProgressPolling = useCallback(() => {
    if (progressIntervalRef.current) {
      clearInterval(progressIntervalRef.current);
      progressIntervalRef.current = null;
    }
  }, []);

  const processVideo = useCallback(async (
    videoPath: string,
    region: WatermarkRegion,
    options?: Partial<RemovalOptions>
  ): Promise<VideoProcessResult | null> => {
    setIsProcessing(true);
    setError(null);
    setVideoProgress({ current_frame: 0, total_frames: 0, percent: 0, estimated_remaining_secs: null });

    // Start polling for progress
    startProgressPolling();

    try {
      const mergedOptions = { ...DEFAULT_OPTIONS, ...options };
      const result = await invoke<VideoProcessResult>('process_video', {
        videoPath,
        region,
        options: mergedOptions,
      });
      return result;
    } catch (err) {
      const errorMessage = err instanceof Error ? err.message : String(err);
      setError(errorMessage);
      return null;
    } finally {
      stopProgressPolling();
      setIsProcessing(false);
      setVideoProgress(null);
    }
  }, [startProgressPolling, stopProgressPolling]);

  const cancelVideoProcessing = useCallback(async (): Promise<void> => {
    try {
      await invoke('cancel_video_processing');
      stopProgressPolling();
    } catch (err) {
      console.error('Failed to cancel video processing:', err);
    }
  }, [stopProgressPolling]);

  // ================================
  // Batch processing functions
  // ================================

  const processBatch = useCallback(async (
    files: BatchFile[],
    region: WatermarkRegion,
    options: RemovalOptions,
    onFileUpdate: (id: string, update: Partial<BatchFile>) => void
  ): Promise<void> => {
    setIsProcessing(true);
    setError(null);

    for (const file of files) {
      if (file.status === 'completed' || file.status === 'failed') {
        continue; // Skip already processed files
      }

      onFileUpdate(file.id, { status: 'processing' });

      try {
        let result: ProcessResult | null = null;

        if (options.processing_method === 'cloud') {
          result = await invoke<ProcessResult>('remove_watermark_cloud', {
            imagePath: file.path,
            region,
            options,
          });
        } else {
          result = await invoke<ProcessResult>('remove_watermark', {
            imagePath: file.path,
            region,
            options,
          });
        }

        if (result) {
          onFileUpdate(file.id, {
            status: 'completed',
            processedPath: result.output_path,
          });
        } else {
          onFileUpdate(file.id, {
            status: 'failed',
            error: 'Processing returned no result',
          });
        }
      } catch (err) {
        const errorMessage = err instanceof Error ? err.message : String(err);
        onFileUpdate(file.id, {
          status: 'failed',
          error: errorMessage,
        });
      }
    }

    setIsProcessing(false);
  }, []);

  return {
    // State
    isProcessing,
    error,
    processedResult,
    videoProgress,
    hasApiKey,

    // Image functions
    removeWatermark,
    removeWatermarkCloud,
    getImageInfo,
    loadImageBase64,
    saveProcessedImage,
    cleanupTempFiles,

    // Video functions
    getVideoInfo,
    extractVideoFrame,
    processVideo,
    cancelVideoProcessing,

    // Batch processing functions
    processBatch,

    // API Key functions
    checkApiKey,

    // Utility
    clearError,
    clearResult,
  };
}
