import { useState, useCallback, useEffect } from 'react';
import { save, open } from '@tauri-apps/plugin-dialog';
import { ImageDropZone } from './components/ImageDropZone';
import { RegionSelector } from './components/RegionSelector';
import { PreviewPanel } from './components/PreviewPanel';
import { OptionsPanel } from './components/OptionsPanel';
import { VideoProgress } from './components/VideoProgress';
import { SettingsModal } from './components/SettingsModal';
import { BatchQueuePanel } from './components/BatchQueuePanel';
import { useWatermarkRemoval } from './hooks/useWatermarkRemoval';
import type { WatermarkRegion, RemovalOptions, ImageInfo, VideoInfo, MediaType, BatchFile, BatchProgress } from './types/watermark';
import './App.css';

type AppStage = 'upload' | 'select' | 'video-processing' | 'preview' | 'batch-select' | 'batch-processing' | 'batch-results';

const DEFAULT_OPTIONS: RemovalOptions = {
  algorithm: 'telea',
  dilate_pixels: 3,
  inpaint_radius: 5,
  processing_method: 'local',
};

function App() {
  const [stage, setStage] = useState<AppStage>('upload');
  const [mediaType, setMediaType] = useState<MediaType>('image');
  const [imagePath, setImagePath] = useState<string | null>(null);
  const [imageBase64, setImageBase64] = useState<string | null>(null);
  const [imageInfo, setImageInfo] = useState<ImageInfo | null>(null);
  const [videoInfo, setVideoInfo] = useState<VideoInfo | null>(null);
  const [selectedRegion, setSelectedRegion] = useState<WatermarkRegion | null>(null);
  const [options, setOptions] = useState<RemovalOptions>(DEFAULT_OPTIONS);
  const [processedImage, setProcessedImage] = useState<string | null>(null);
  const [processedPath, setProcessedPath] = useState<string | null>(null);
  const [originalSize, setOriginalSize] = useState<number | null>(null);
  const [processedSize, setProcessedSize] = useState<number | null>(null);
  const [videoProcessingStartTime, setVideoProcessingStartTime] = useState<number>(0);
  const [isSettingsOpen, setIsSettingsOpen] = useState(false);

  // Batch processing state
  const [batchFiles, setBatchFiles] = useState<BatchFile[]>([]);
  const [batchProgress, setBatchProgress] = useState<BatchProgress | null>(null);

  const {
    isProcessing,
    error,
    videoProgress,
    hasApiKey,
    removeWatermark,
    getImageInfo,
    loadImageBase64,
    saveProcessedImage,
    cleanupTempFiles,
    getVideoInfo,
    extractVideoFrame,
    processVideo,
    cancelVideoProcessing,
    processBatch,
    checkApiKey,
    clearError,
  } = useWatermarkRemoval();

  // Refresh API key status when settings modal closes
  const handleSettingsClose = useCallback(() => {
    setIsSettingsOpen(false);
    checkApiKey();
  }, [checkApiKey]);

  // Cleanup temp files on unmount
  useEffect(() => {
    return () => {
      cleanupTempFiles();
    };
  }, [cleanupTempFiles]);

  const handleMediaSelect = useCallback(async (path: string, type: MediaType) => {
    clearError();
    setImagePath(path);
    setMediaType(type);

    if (type === 'video') {
      // Handle video file
      const info = await getVideoInfo(path);
      if (!info) return;
      setVideoInfo(info);

      // Extract first frame for region selection
      const frameBase64 = await extractVideoFrame(path);
      if (!frameBase64) return;
      setImageBase64(frameBase64);

      // Set imageInfo for RegionSelector compatibility
      setImageInfo({
        width: info.width,
        height: info.height,
        path: path,
      });
    } else {
      // Handle image file
      const info = await getImageInfo(path);
      if (!info) return;
      setImageInfo(info);
      setVideoInfo(null);

      const base64 = await loadImageBase64(path);
      if (!base64) return;
      setImageBase64(base64);
    }

    setStage('select');
    setSelectedRegion(null);
    setProcessedImage(null);
    setProcessedPath(null);
  }, [getImageInfo, getVideoInfo, loadImageBase64, extractVideoFrame, clearError]);

  // Legacy handler for backwards compatibility
  const handleImageSelect = useCallback(async (path: string) => {
    await handleMediaSelect(path, 'image');
  }, [handleMediaSelect]);

  // Handler for batch file selection
  const handleBatchSelect = useCallback(async (paths: string[]) => {
    clearError();

    // Create batch file entries
    const files: BatchFile[] = paths.map((path, index) => ({
      id: `file-${index}-${Date.now()}`,
      path,
      filename: path.split('/').pop() || `file-${index}`,
      status: 'pending' as const,
    }));

    setBatchFiles(files);

    // Load first file for region selection
    const firstPath = paths[0];
    const info = await getImageInfo(firstPath);
    if (!info) return;
    setImageInfo(info);

    const base64 = await loadImageBase64(firstPath);
    if (!base64) return;
    setImageBase64(base64);
    setImagePath(firstPath);
    setMediaType('image');

    setStage('batch-select');
    setSelectedRegion(null);
    setProcessedImage(null);
    setProcessedPath(null);
  }, [getImageInfo, loadImageBase64, clearError]);

  // Handler for removing a file from batch queue
  const handleRemoveBatchFile = useCallback((id: string) => {
    setBatchFiles(prev => prev.filter(f => f.id !== id));
  }, []);

  // Handler for updating a batch file's status
  const handleBatchFileUpdate = useCallback((id: string, update: Partial<BatchFile>) => {
    setBatchFiles(prev => prev.map(f => f.id === id ? { ...f, ...update } : f));
  }, []);

  // Handler for processing batch files
  const handleProcessBatch = useCallback(async () => {
    if (!selectedRegion || batchFiles.length === 0) return;

    setStage('batch-processing');

    // Process files sequentially
    for (let i = 0; i < batchFiles.length; i++) {
      const file = batchFiles[i];
      if (file.status === 'completed' || file.status === 'failed') continue;

      setBatchProgress({
        currentIndex: i + 1,
        totalFiles: batchFiles.length,
        currentFilename: file.filename,
      });

      handleBatchFileUpdate(file.id, { status: 'processing' });

      try {
        const result = await removeWatermark(file.path, selectedRegion, options);
        if (result) {
          handleBatchFileUpdate(file.id, {
            status: 'completed',
            processedPath: result.output_path,
          });
        } else {
          handleBatchFileUpdate(file.id, {
            status: 'failed',
            error: error || 'Processing failed',
          });
        }
      } catch (err) {
        handleBatchFileUpdate(file.id, {
          status: 'failed',
          error: err instanceof Error ? err.message : String(err),
        });
      }
    }

    setBatchProgress(null);
    setStage('batch-results');
  }, [selectedRegion, batchFiles, options, removeWatermark, handleBatchFileUpdate, error]);

  // Handler for saving all batch results
  const handleSaveBatch = useCallback(async () => {
    // Open folder selection dialog
    const destination = await open({
      directory: true,
      title: 'Select folder to save processed images',
    });

    if (!destination || typeof destination !== 'string') return;

    const completedFiles = batchFiles.filter(f => f.status === 'completed' && f.processedPath);
    let savedCount = 0;

    for (const file of completedFiles) {
      if (!file.processedPath) continue;

      // Generate output filename
      const nameWithoutExt = file.filename.replace(/\.[^/.]+$/, '');
      // Get extension from processedPath (may differ from original if lossless converted JPEG→PNG)
      const extension = file.processedPath.split('.').pop() || 'png';
      const outputPath = `${destination}/${nameWithoutExt}_watermark.${extension}`;

      const success = await saveProcessedImage(file.processedPath, outputPath);
      if (success) {
        savedCount++;
      }
    }

    if (savedCount === completedFiles.length) {
      // All files saved successfully
      alert(`Successfully saved ${savedCount} files to ${destination}`);
    } else {
      alert(`Saved ${savedCount} of ${completedFiles.length} files. Some files may have failed.`);
    }
  }, [batchFiles, saveProcessedImage]);

  const handleRegionSelect = useCallback((region: WatermarkRegion) => {
    setSelectedRegion(region);
  }, []);

  const handleRemoveWatermark = useCallback(async () => {
    if (!imagePath || !selectedRegion) return;

    if (mediaType === 'video') {
      // Start video processing
      setVideoProcessingStartTime(Date.now());
      setStage('video-processing');

      const result = await processVideo(imagePath, selectedRegion, options);
      if (result) {
        setProcessedPath(result.output_path);
        // For videos, we don't have a base64 preview - just show success
        setStage('preview');
      } else {
        // Processing failed or was cancelled
        setStage('select');
      }
    } else {
      // Image processing (existing logic)
      const result = await removeWatermark(imagePath, selectedRegion, options);
      if (result && result.base64_preview) {
        setProcessedImage(result.base64_preview);
        setProcessedPath(result.output_path);
        setOriginalSize(result.original_size);
        setProcessedSize(result.processed_size);
        setStage('preview');
      }
    }
  }, [imagePath, selectedRegion, options, mediaType, removeWatermark, processVideo]);

  const handleSave = useCallback(async () => {
    if (!processedPath || !imagePath) return;

    const isVideo = mediaType === 'video';
    // Get extension from processedPath (may differ from original if lossless converted JPEG→PNG)
    const extension = isVideo ? 'mp4' : (processedPath.split('.').pop() || 'png');

    // Extract original filename without extension
    const pathParts = imagePath.split('/');
    const originalFilename = pathParts[pathParts.length - 1];
    const nameWithoutExt = originalFilename.replace(/\.[^/.]+$/, '');

    const filterName = isVideo ? 'Videos' : 'Images';
    const defaultName = `${nameWithoutExt}_watermark.${extension}`;

    const destination = await save({
      filters: [
        {
          name: filterName,
          extensions: [extension],
        },
      ],
      defaultPath: defaultName,
    });

    if (destination) {
      const success = await saveProcessedImage(processedPath, destination);
      if (success) {
        // Optionally show success message
      }
    }
  }, [processedPath, imagePath, mediaType, saveProcessedImage]);

  const handleReset = useCallback(() => {
    setStage('upload');
    setMediaType('image');
    setImagePath(null);
    setImageBase64(null);
    setImageInfo(null);
    setVideoInfo(null);
    setSelectedRegion(null);
    setProcessedImage(null);
    setProcessedPath(null);
    setOriginalSize(null);
    setProcessedSize(null);
    setOptions(DEFAULT_OPTIONS);
    setBatchFiles([]);
    setBatchProgress(null);
    clearError();
    cleanupTempFiles();
  }, [clearError, cleanupTempFiles]);

  const handleCancelVideoProcessing = useCallback(async () => {
    await cancelVideoProcessing();
    setStage('select');
  }, [cancelVideoProcessing]);

  const handleBack = useCallback(() => {
    if (stage === 'preview') {
      setStage('select');
      setProcessedImage(null);
      setProcessedPath(null);
    } else if (stage === 'video-processing' || stage === 'batch-processing') {
      // Cannot go back during processing - user must cancel
      return;
    } else if (stage === 'batch-results') {
      setStage('batch-select');
    } else if (stage === 'batch-select') {
      handleReset();
    } else if (stage === 'select') {
      handleReset();
    }
  }, [stage, handleReset]);

  return (
    <div className="app">
      <header className="app-header">
        <h1>Watermark Remover</h1>
        <div className="header-actions">
          {stage !== 'upload' && (
            <>
              <button className="btn btn-secondary" onClick={handleBack}>
                Back
              </button>
              <button className="btn btn-secondary" onClick={handleReset}>
                New Image
              </button>
            </>
          )}
          <button
            className="btn btn-icon"
            onClick={() => setIsSettingsOpen(true)}
            title="Settings"
          >
            <svg
              xmlns="http://www.w3.org/2000/svg"
              width="20"
              height="20"
              viewBox="0 0 24 24"
              fill="none"
              stroke="currentColor"
              strokeWidth="2"
              strokeLinecap="round"
              strokeLinejoin="round"
            >
              <circle cx="12" cy="12" r="3" />
              <path d="M19.4 15a1.65 1.65 0 0 0 .33 1.82l.06.06a2 2 0 0 1 0 2.83 2 2 0 0 1-2.83 0l-.06-.06a1.65 1.65 0 0 0-1.82-.33 1.65 1.65 0 0 0-1 1.51V21a2 2 0 0 1-2 2 2 2 0 0 1-2-2v-.09A1.65 1.65 0 0 0 9 19.4a1.65 1.65 0 0 0-1.82.33l-.06.06a2 2 0 0 1-2.83 0 2 2 0 0 1 0-2.83l.06-.06a1.65 1.65 0 0 0 .33-1.82 1.65 1.65 0 0 0-1.51-1H3a2 2 0 0 1-2-2 2 2 0 0 1 2-2h.09A1.65 1.65 0 0 0 4.6 9a1.65 1.65 0 0 0-.33-1.82l-.06-.06a2 2 0 0 1 0-2.83 2 2 0 0 1 2.83 0l.06.06a1.65 1.65 0 0 0 1.82.33H9a1.65 1.65 0 0 0 1-1.51V3a2 2 0 0 1 2-2 2 2 0 0 1 2 2v.09a1.65 1.65 0 0 0 1 1.51 1.65 1.65 0 0 0 1.82-.33l.06-.06a2 2 0 0 1 2.83 0 2 2 0 0 1 0 2.83l-.06.06a1.65 1.65 0 0 0-.33 1.82V9a1.65 1.65 0 0 0 1.51 1H21a2 2 0 0 1 2 2 2 2 0 0 1-2 2h-.09a1.65 1.65 0 0 0-1.51 1z" />
            </svg>
          </button>
        </div>
      </header>

      <main className="app-main">
        {error && (
          <div className="error-banner">
            <span>{error}</span>
            <button onClick={clearError}>&times;</button>
          </div>
        )}

        {stage === 'upload' && (
          <div className="upload-stage">
            <ImageDropZone onMediaSelect={handleMediaSelect} onBatchSelect={handleBatchSelect} />
          </div>
        )}

        {stage === 'select' && imageBase64 && imageInfo && (
          <div className="select-stage">
            <div className="select-content">
              <RegionSelector
                imageSrc={imageBase64}
                imageInfo={imageInfo}
                onRegionSelect={handleRegionSelect}
                selectedRegion={selectedRegion}
                disabled={isProcessing}
              />
              <div className="select-sidebar">
                {mediaType === 'video' && videoInfo && (
                  <div className="video-info-panel">
                    <h4>Video Info</h4>
                    <p>Resolution: {videoInfo.width} x {videoInfo.height}</p>
                    <p>Duration: {videoInfo.duration_secs.toFixed(1)}s</p>
                    <p>Frames: {videoInfo.frame_count}</p>
                    <p>FPS: {videoInfo.fps.toFixed(2)}</p>
                    <p className="video-warning">
                      Processing {videoInfo.frame_count} frames may take a while.
                    </p>
                  </div>
                )}
                <OptionsPanel
                  options={options}
                  onChange={setOptions}
                  disabled={isProcessing}
                  hasApiKey={hasApiKey}
                />
                {selectedRegion && (
                  <div className="region-info">
                    <h4>Selected Region</h4>
                    <p>
                      Position: ({selectedRegion.x}, {selectedRegion.y})
                    </p>
                    <p>
                      Size: {selectedRegion.width} x {selectedRegion.height}
                    </p>
                  </div>
                )}
                <button
                  className="btn btn-primary btn-large"
                  onClick={handleRemoveWatermark}
                  disabled={!selectedRegion || isProcessing}
                >
                  {isProcessing ? (
                    <>
                      <span className="spinner" />
                      Processing...
                    </>
                  ) : mediaType === 'video' ? (
                    'Process Video'
                  ) : (
                    'Remove Watermark'
                  )}
                </button>
              </div>
            </div>
          </div>
        )}

        {stage === 'video-processing' && videoProgress && (
          <div className="video-processing-stage">
            <VideoProgress
              progress={videoProgress}
              onCancel={handleCancelVideoProcessing}
              startTime={videoProcessingStartTime}
            />
          </div>
        )}

        {stage === 'batch-select' && imageBase64 && imageInfo && (
          <div className="batch-select-stage">
            <div className="batch-select-content">
              <RegionSelector
                imageSrc={imageBase64}
                imageInfo={imageInfo}
                onRegionSelect={handleRegionSelect}
                selectedRegion={selectedRegion}
                disabled={isProcessing}
              />
              <div className="batch-select-sidebar">
                <BatchQueuePanel
                  files={batchFiles}
                  onRemoveFile={handleRemoveBatchFile}
                  progress={null}
                  isProcessing={false}
                />
                <OptionsPanel
                  options={options}
                  onChange={setOptions}
                  disabled={isProcessing}
                  hasApiKey={hasApiKey}
                />
                {selectedRegion && (
                  <div className="region-info">
                    <h4>Selected Region</h4>
                    <p>
                      Position: ({selectedRegion.x}, {selectedRegion.y})
                    </p>
                    <p>
                      Size: {selectedRegion.width} x {selectedRegion.height}
                    </p>
                    <p className="region-note">
                      This region will be applied to all {batchFiles.length} files
                    </p>
                  </div>
                )}
                <button
                  className="btn btn-primary btn-large"
                  onClick={handleProcessBatch}
                  disabled={!selectedRegion || isProcessing || batchFiles.length === 0}
                >
                  Process All ({batchFiles.length} files)
                </button>
              </div>
            </div>
          </div>
        )}

        {stage === 'batch-processing' && (
          <div className="batch-processing-stage">
            <div className="batch-processing-panel">
              <h2>Processing Batch</h2>
              <BatchQueuePanel
                files={batchFiles}
                onRemoveFile={handleRemoveBatchFile}
                progress={batchProgress}
                isProcessing={true}
              />
            </div>
          </div>
        )}

        {stage === 'batch-results' && (
          <div className="batch-results-stage">
            <div className="batch-results-panel">
              <div className="batch-results-icon">
                <svg
                  xmlns="http://www.w3.org/2000/svg"
                  width="80"
                  height="80"
                  viewBox="0 0 24 24"
                  fill="none"
                  stroke="currentColor"
                  strokeWidth="1.5"
                  strokeLinecap="round"
                  strokeLinejoin="round"
                >
                  <path d="M22 11.08V12a10 10 0 1 1-5.93-9.14" />
                  <polyline points="22 4 12 14.01 9 11.01" />
                </svg>
              </div>
              <h2>Batch Processing Complete</h2>
              <p>
                {batchFiles.filter(f => f.status === 'completed').length} of {batchFiles.length} files processed successfully
              </p>
              <BatchQueuePanel
                files={batchFiles}
                onRemoveFile={handleRemoveBatchFile}
                progress={null}
                isProcessing={false}
              />
              <div className="batch-results-actions">
                <button className="btn btn-secondary" onClick={handleBack}>
                  Adjust Selection
                </button>
                <button
                  className="btn btn-primary btn-large"
                  onClick={handleSaveBatch}
                  disabled={batchFiles.filter(f => f.status === 'completed').length === 0}
                >
                  Save All ({batchFiles.filter(f => f.status === 'completed').length} files)
                </button>
              </div>
            </div>
          </div>
        )}

        {stage === 'preview' && imageInfo && (mediaType === 'image' ? processedImage : processedPath) && (
          <div className="preview-stage">
            {mediaType === 'image' && imageBase64 && processedImage ? (
              <PreviewPanel
                originalImage={imageBase64}
                processedImage={processedImage}
                imageWidth={imageInfo.width}
                imageHeight={imageInfo.height}
                originalSize={originalSize ?? undefined}
                processedSize={processedSize ?? undefined}
              />
            ) : (
              <div className="video-complete-panel">
                <div className="video-complete-icon">
                  <svg
                    xmlns="http://www.w3.org/2000/svg"
                    width="80"
                    height="80"
                    viewBox="0 0 24 24"
                    fill="none"
                    stroke="currentColor"
                    strokeWidth="1.5"
                    strokeLinecap="round"
                    strokeLinejoin="round"
                  >
                    <path d="M22 11.08V12a10 10 0 1 1-5.93-9.14" />
                    <polyline points="22 4 12 14.01 9 11.01" />
                  </svg>
                </div>
                <h2>Video Processing Complete</h2>
                <p>The watermark has been successfully removed from all frames.</p>
                {videoInfo && (
                  <p className="video-stats">
                    Processed {videoInfo.frame_count} frames ({videoInfo.duration_secs.toFixed(1)} seconds)
                  </p>
                )}
              </div>
            )}
            <div className="preview-actions">
              <button className="btn btn-secondary" onClick={handleBack}>
                Adjust Selection
              </button>
              <button className="btn btn-primary btn-large" onClick={handleSave}>
                Save {mediaType === 'video' ? 'Video' : 'Result'}
              </button>
            </div>
          </div>
        )}
      </main>

      <footer className="app-footer">
        <p>
          Powered by {options.processing_method === 'cloud' ? 'Google Gemini AI' : 'OpenCV Inpainting'}
        </p>
      </footer>

      <SettingsModal isOpen={isSettingsOpen} onClose={handleSettingsClose} />
    </div>
  );
}

export default App;
