import { useCallback, useEffect, useState } from 'react';
import { invoke } from '@tauri-apps/api/core';
import { listen } from '@tauri-apps/api/event';
import type { ProcessResult, UpscaleModel, UpscaleProgress, UpscaleScale } from '../types/watermark';

export function useUpscale() {
  const [ai, setAi] = useState(false);
  const [busy, setBusy] = useState(false);
  const [progress, setProgress] = useState<UpscaleProgress | null>(null);
  const [download, setDownload] = useState<{ done: number; total: number } | null>(null);
  const [error, setError] = useState<string | null>(null);

  useEffect(() => {
    invoke<{ ai: boolean }>('upscale_capabilities').then((c) => setAi(c.ai)).catch(() => setAi(false));
    const offProgress = listen<UpscaleProgress>('upscale-progress', (e) => setProgress(e.payload));
    const offDownload = listen<{ done: number; total: number }>('model-download', (e) => setDownload(e.payload));
    return () => {
      offProgress.then((f) => f());
      offDownload.then((f) => f());
    };
  }, []);

  const upscale = useCallback(
    async (imagePath: string, scale: UpscaleScale, model: UpscaleModel, lossless: boolean): Promise<ProcessResult | null> => {
      setBusy(true);
      setError(null);
      setProgress(null);
      setDownload(null);
      try {
        return await invoke<ProcessResult>('upscale_image', { imagePath, scale, model, lossless });
      } catch (err) {
        const message = err instanceof Error ? err.message : String(err);
        if (message !== 'Cancelled') setError(message);
        return null;
      } finally {
        setBusy(false);
      }
    },
    []
  );

  const cancel = useCallback(() => {
    invoke('cancel_upscale');
  }, []);

  const clearError = useCallback(() => setError(null), []);

  return { ai, busy, progress, download, error, upscale, cancel, clearError };
}
