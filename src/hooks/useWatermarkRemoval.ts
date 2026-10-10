import { useState, useCallback } from 'react';
import { invoke } from '@tauri-apps/api/core';
import type {
  WatermarkRegion,
  RemovalOptions,
  ProcessResult,
  ImageInfo,
} from '../types/watermark';

interface UseWatermarkRemovalReturn {
  // State
  isProcessing: boolean;
  error: string | null;
  processedResult: ProcessResult | null;

  // Image functions
  removeWatermark: (imagePath: string, region: WatermarkRegion, options?: Partial<RemovalOptions>) => Promise<ProcessResult | null>;
  getImageInfo: (imagePath: string) => Promise<ImageInfo | null>;
  loadImageBase64: (imagePath: string) => Promise<string | null>;
  saveProcessedImage: (sourcePath: string, destinationPath: string) => Promise<boolean>;
  cleanupTempFiles: () => Promise<void>;

  // Utility
  clearError: () => void;
  clearResult: () => void;
}

const DEFAULT_OPTIONS: RemovalOptions = {
  lossless: false,
  refine_mask: true,
};

export function useWatermarkRemoval(): UseWatermarkRemovalReturn {
  const [isProcessing, setIsProcessing] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [processedResult, setProcessedResult] = useState<ProcessResult | null>(null);

  const removeWatermark = useCallback(async (
    imagePath: string,
    region: WatermarkRegion,
    options?: Partial<RemovalOptions>
  ): Promise<ProcessResult | null> => {
    setIsProcessing(true);
    setError(null);

    try {
      const mergedOptions = { ...DEFAULT_OPTIONS, ...options };

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

  return {
    // State
    isProcessing,
    error,
    processedResult,

    // Image functions
    removeWatermark,
    getImageInfo,
    loadImageBase64,
    saveProcessedImage,
    cleanupTempFiles,

    // Utility
    clearError,
    clearResult,
  };
}
