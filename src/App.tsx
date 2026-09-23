import { useState, useCallback, useEffect } from 'react';
import { invoke } from '@tauri-apps/api/core';
import { save, open } from '@tauri-apps/plugin-dialog';
import { ImageDropZone } from './components/ImageDropZone';
import { RegionSelector } from './components/RegionSelector';
import { PreviewPanel } from './components/PreviewPanel';
import { OptionsPanel } from './components/OptionsPanel';
import { BatchQueuePanel } from './components/BatchQueuePanel';
import { useWatermarkRemoval } from './hooks/useWatermarkRemoval';
import type { WatermarkRegion, RemovalOptions, ImageInfo, BatchFile, BatchProgress, ProcessResult } from './types/watermark';
import './App.css';

type AppStage = 'upload' | 'select' | 'preview' | 'batch-select' | 'batch-processing' | 'batch-results';

const DEFAULT_OPTIONS: RemovalOptions = {
  lossless: false,
  refine_mask: true,
};

function App() {
  const [stage, setStage] = useState<AppStage>('upload');
  const [imagePath, setImagePath] = useState<string | null>(null);
  const [imageBase64, setImageBase64] = useState<string | null>(null);
  const [imageInfo, setImageInfo] = useState<ImageInfo | null>(null);
  const [selectedRegion, setSelectedRegion] = useState<WatermarkRegion | null>(null);
  const [options, setOptions] = useState<RemovalOptions>(DEFAULT_OPTIONS);
  const [processedImage, setProcessedImage] = useState<string | null>(null);
  const [processedPath, setProcessedPath] = useState<string | null>(null);
  const [originalSize, setOriginalSize] = useState<number | null>(null);
  const [processedSize, setProcessedSize] = useState<number | null>(null);

  // Batch processing state
  const [batchFiles, setBatchFiles] = useState<BatchFile[]>([]);
  const [batchProgress, setBatchProgress] = useState<BatchProgress | null>(null);

  const {
    isProcessing,
    error,
    removeWatermark,
    getImageInfo,
    loadImageBase64,
    saveProcessedImage,
    cleanupTempFiles,
    clearError,
  } = useWatermarkRemoval();

  // Cleanup temp files on unmount
  useEffect(() => {
    return () => {
      cleanupTempFiles();
    };
  }, [cleanupTempFiles]);

  const handleMediaSelect = useCallback(async (path: string) => {
    clearError();
    setImagePath(path);

    const info = await getImageInfo(path);
    if (!info) return;
    setImageInfo(info);

    const base64 = await loadImageBase64(path);
    if (!base64) return;
    setImageBase64(base64);

    setStage('select');
    setSelectedRegion(null);
    setProcessedImage(null);
    setProcessedPath(null);
  }, [getImageInfo, loadImageBase64, clearError]);

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
        const result = await invoke<ProcessResult>('remove_watermark', {
          imagePath: file.path,
          region: selectedRegion,
          options,
        });
        handleBatchFileUpdate(file.id, { status: 'completed', processedPath: result.output_path });
      } catch (err) {
        handleBatchFileUpdate(file.id, {
          status: 'failed',
          error: err instanceof Error ? err.message : String(err),
        });
      }
    }

    setBatchProgress(null);
    setStage('batch-results');
  }, [selectedRegion, batchFiles, options, handleBatchFileUpdate]);

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

    const result = await removeWatermark(imagePath, selectedRegion, options);
    if (result && result.base64_preview) {
      setProcessedImage(result.base64_preview);
      setProcessedPath(result.output_path);
      setOriginalSize(result.original_size);
      setProcessedSize(result.processed_size);
      setStage('preview');
    }
  }, [imagePath, selectedRegion, options, removeWatermark]);

  const handleSave = useCallback(async () => {
    if (!processedPath || !imagePath) return;

    // Get extension from processedPath (may differ from original if lossless converted JPEG→PNG)
    const extension = processedPath.split('.').pop() || 'png';

    // Extract original filename without extension
    const pathParts = imagePath.split('/');
    const originalFilename = pathParts[pathParts.length - 1];
    const nameWithoutExt = originalFilename.replace(/\.[^/.]+$/, '');

    const defaultName = `${nameWithoutExt}_watermark.${extension}`;

    const destination = await save({
      filters: [
        {
          name: 'Images',
          extensions: [extension],
        },
      ],
      defaultPath: defaultName,
    });

    if (destination) {
      await saveProcessedImage(processedPath, destination);
    }
  }, [processedPath, imagePath, saveProcessedImage]);

  const handleReset = useCallback(() => {
    setStage('upload');
    setImagePath(null);
    setImageBase64(null);
    setImageInfo(null);
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

  const handleBack = useCallback(() => {
    if (stage === 'preview') {
      setStage('select');
      setProcessedImage(null);
      setProcessedPath(null);
    } else if (stage === 'batch-processing') {
      // Cannot go back during processing
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
                <OptionsPanel
                  options={options}
                  onChange={setOptions}
                  disabled={isProcessing}
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
                  ) : (
                    'Remove Watermark'
                  )}
                </button>
              </div>
            </div>
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

        {stage === 'preview' && imageInfo && processedImage && (
          <div className="preview-stage">
            {imageBase64 && processedImage && (
              <PreviewPanel
                originalImage={imageBase64}
                processedImage={processedImage}
                imageWidth={imageInfo.width}
                imageHeight={imageInfo.height}
                originalSize={originalSize ?? undefined}
                processedSize={processedSize ?? undefined}
              />
            )}
            <div className="preview-actions">
              <button className="btn btn-secondary" onClick={handleBack}>
                Adjust Selection
              </button>
              <button className="btn btn-primary btn-large" onClick={handleSave}>
                Save Result
              </button>
            </div>
          </div>
        )}
      </main>

      <footer className="app-footer">
        <p>Runs locally — images never leave your computer</p>
      </footer>

    </div>
  );
}

export default App;
