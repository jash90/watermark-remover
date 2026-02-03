import { useState, useCallback, useEffect } from 'react';
import { invoke } from '@tauri-apps/api/core';
import type {
  WatermarkRegion,
  RemovalOptions,
  ProcessResult,
  ImageInfo,
  BatchFile,
} from '../types/watermark';

interface UseWatermarkRemovalReturn {
  // State
  isProcessing: boolean;
  error: string | null;
  processedResult: ProcessResult | null;
  hasApiKey: boolean;

  // Image functions
  removeWatermark: (imagePath: string, region: WatermarkRegion, options?: Partial<RemovalOptions>) => Promise<ProcessResult | null>;
  getImageInfo: (imagePath: string) => Promise<ImageInfo | null>;
  loadImageBase64: (imagePath: string) => Promise<string | null>;
  saveProcessedImage: (sourcePath: string, destinationPath: string) => Promise<boolean>;
  cleanupTempFiles: () => Promise<void>;

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
  lossless: false,
};

export function useWatermarkRemoval(): UseWatermarkRemovalReturn {
  const [isProcessing, setIsProcessing] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [processedResult, setProcessedResult] = useState<ProcessResult | null>(null);
  const [hasApiKey, setHasApiKey] = useState(false);

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
        const result = await invoke<ProcessResult>('remove_watermark', {
          imagePath: file.path,
          region,
          options,
        });

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
    hasApiKey,

    // Image functions
    removeWatermark,
    getImageInfo,
    loadImageBase64,
    saveProcessedImage,
    cleanupTempFiles,

    // Batch processing functions
    processBatch,

    // API Key functions
    checkApiKey,

    // Utility
    clearError,
    clearResult,
  };
}
