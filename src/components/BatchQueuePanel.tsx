import type { BatchFile, BatchProgress } from '../types/watermark';

interface BatchQueuePanelProps {
  files: BatchFile[];
  onRemoveFile: (id: string) => void;
  progress: BatchProgress | null;
  isProcessing: boolean;
}

function getStatusIcon(status: BatchFile['status']) {
  switch (status) {
    case 'pending':
      return '⏳';
    case 'processing':
      return '🔄';
    case 'completed':
      return '✅';
    case 'failed':
      return '❌';
  }
}

function getStatusClass(status: BatchFile['status']) {
  switch (status) {
    case 'pending':
      return 'batch-item-pending';
    case 'processing':
      return 'batch-item-processing';
    case 'completed':
      return 'batch-item-completed';
    case 'failed':
      return 'batch-item-failed';
  }
}

export function BatchQueuePanel({ files, onRemoveFile, progress, isProcessing }: BatchQueuePanelProps) {
  const completedCount = files.filter(f => f.status === 'completed').length;
  const failedCount = files.filter(f => f.status === 'failed').length;
  const totalCount = files.length;

  return (
    <div className="batch-queue-panel">
      <div className="batch-queue-header">
        <h4>Batch Queue</h4>
        <span className="batch-count">{totalCount} files</span>
      </div>

      {progress && (
        <div className="batch-progress">
          <div className="batch-progress-bar-container">
            <div
              className="batch-progress-bar"
              style={{ width: `${(progress.currentIndex / progress.totalFiles) * 100}%` }}
            />
          </div>
          <p className="batch-progress-text">
            Processing {progress.currentIndex} of {progress.totalFiles}: {progress.currentFilename}
          </p>
        </div>
      )}

      {!isProcessing && completedCount > 0 && (
        <div className="batch-summary">
          <span className="batch-completed">{completedCount} completed</span>
          {failedCount > 0 && <span className="batch-failed">{failedCount} failed</span>}
        </div>
      )}

      <ul className="batch-file-list">
        {files.map(file => (
          <li key={file.id} className={`batch-item ${getStatusClass(file.status)}`}>
            <span className="batch-item-icon">{getStatusIcon(file.status)}</span>
            <span className="batch-item-name" title={file.path}>
              {file.filename}
            </span>
            {file.status === 'pending' && !isProcessing && (
              <button
                className="batch-item-remove"
                onClick={() => onRemoveFile(file.id)}
                title="Remove from queue"
              >
                ×
              </button>
            )}
            {file.error && (
              <span className="batch-item-error" title={file.error}>
                {file.error.slice(0, 30)}...
              </span>
            )}
          </li>
        ))}
      </ul>
    </div>
  );
}
