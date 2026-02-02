import { useState, useEffect, useCallback } from 'react';
import { invoke } from '@tauri-apps/api/core';

interface SettingsModalProps {
  isOpen: boolean;
  onClose: () => void;
}

export function SettingsModal({ isOpen, onClose }: SettingsModalProps) {
  const [apiKey, setApiKey] = useState('');
  const [savedApiKey, setSavedApiKey] = useState('');
  const [isLoading, setIsLoading] = useState(false);
  const [isTesting, setIsTesting] = useState(false);
  const [isListingModels, setIsListingModels] = useState(false);
  const [models, setModels] = useState<string[]>([]);
  const [message, setMessage] = useState<{ type: 'success' | 'error'; text: string } | null>(null);

  const loadApiKey = useCallback(async () => {
    try {
      const key = await invoke<string>('get_gemini_api_key');
      setApiKey(key);
      setSavedApiKey(key);
    } catch (err) {
      console.error('Failed to load API key:', err);
    }
  }, []);

  useEffect(() => {
    if (isOpen) {
      loadApiKey();
      setMessage(null);
    }
  }, [isOpen, loadApiKey]);

  const handleSave = async () => {
    setIsLoading(true);
    setMessage(null);

    try {
      await invoke('set_gemini_api_key', { apiKey });
      setSavedApiKey(apiKey);
      setMessage({ type: 'success', text: 'API key saved successfully!' });
    } catch (err) {
      const errorMessage = err instanceof Error ? err.message : String(err);
      setMessage({ type: 'error', text: `Failed to save: ${errorMessage}` });
    } finally {
      setIsLoading(false);
    }
  };

  const handleTestConnection = async () => {
    if (!apiKey) {
      setMessage({ type: 'error', text: 'Please enter an API key first' });
      return;
    }

    // Save first if changed
    if (apiKey !== savedApiKey) {
      await handleSave();
    }

    setIsTesting(true);
    setMessage(null);

    try {
      const success = await invoke<boolean>('test_gemini_connection');
      if (success) {
        setMessage({ type: 'success', text: 'Connection successful! API key is valid.' });
      }
    } catch (err) {
      const errorMessage = err instanceof Error ? err.message : String(err);
      setMessage({ type: 'error', text: `Connection failed: ${errorMessage}` });
    } finally {
      setIsTesting(false);
    }
  };

  const handleClear = async () => {
    setApiKey('');
    setIsLoading(true);
    try {
      await invoke('set_gemini_api_key', { apiKey: '' });
      setSavedApiKey('');
      setModels([]);
      setMessage({ type: 'success', text: 'API key cleared' });
    } catch (err) {
      const errorMessage = err instanceof Error ? err.message : String(err);
      setMessage({ type: 'error', text: `Failed to clear: ${errorMessage}` });
    } finally {
      setIsLoading(false);
    }
  };

  const handleListModels = async () => {
    if (!apiKey) {
      setMessage({ type: 'error', text: 'Please enter an API key first' });
      return;
    }

    // Save first if changed
    if (apiKey !== savedApiKey) {
      await handleSave();
    }

    setIsListingModels(true);
    setMessage(null);

    try {
      const availableModels = await invoke<string[]>('list_gemini_models');
      setModels(availableModels);
      if (availableModels.length === 0) {
        setMessage({ type: 'error', text: 'No models found' });
      } else {
        setMessage({ type: 'success', text: `Found ${availableModels.length} models` });
      }
    } catch (err) {
      const errorMessage = err instanceof Error ? err.message : String(err);
      setMessage({ type: 'error', text: `Failed to list models: ${errorMessage}` });
    } finally {
      setIsListingModels(false);
    }
  };

  if (!isOpen) return null;

  const hasChanges = apiKey !== savedApiKey;
  const hasKey = apiKey.length > 0;

  return (
    <div className="modal-overlay" onClick={onClose}>
      <div className="modal-content" onClick={(e) => e.stopPropagation()}>
        <div className="modal-header">
          <h2>Settings</h2>
          <button className="modal-close" onClick={onClose}>
            &times;
          </button>
        </div>

        <div className="modal-body">
          <div className="settings-section">
            <h3>Google Gemini API</h3>
            <p className="settings-description">
              Configure your Gemini API key to enable cloud-based watermark removal.
              Cloud processing typically provides better results for complex watermarks.
            </p>

            <div className="form-group">
              <label htmlFor="apiKey">API Key</label>
              <input
                type="password"
                id="apiKey"
                value={apiKey}
                onChange={(e) => setApiKey(e.target.value)}
                placeholder="Enter your Gemini API key"
                disabled={isLoading || isTesting}
              />
            </div>

            <div className="settings-actions">
              <button
                className="btn btn-secondary"
                onClick={handleTestConnection}
                disabled={!hasKey || isLoading || isTesting || isListingModels}
              >
                {isTesting ? (
                  <>
                    <span className="spinner" />
                    Testing...
                  </>
                ) : (
                  'Test Connection'
                )}
              </button>
              <button
                className="btn btn-secondary"
                onClick={handleListModels}
                disabled={!hasKey || isLoading || isTesting || isListingModels}
              >
                {isListingModels ? (
                  <>
                    <span className="spinner" />
                    Loading...
                  </>
                ) : (
                  'List Models'
                )}
              </button>
              <button
                className="btn btn-primary"
                onClick={handleSave}
                disabled={!hasChanges || isLoading || isTesting || isListingModels}
              >
                {isLoading ? (
                  <>
                    <span className="spinner" />
                    Saving...
                  </>
                ) : (
                  'Save'
                )}
              </button>
            </div>

            {models.length > 0 && (
              <div className="models-list">
                <h4>Available Models:</h4>
                <ul>
                  {models.map((model, index) => (
                    <li key={index}>{model.replace('models/', '')}</li>
                  ))}
                </ul>
              </div>
            )}

            {message && (
              <div className={`settings-message ${message.type}`}>
                {message.text}
              </div>
            )}

            <div className="settings-info">
              <h4>How to get an API key:</h4>
              <ol>
                <li>
                  Visit{' '}
                  <a
                    href="https://aistudio.google.com/apikey"
                    target="_blank"
                    rel="noopener noreferrer"
                  >
                    Google AI Studio
                  </a>
                </li>
                <li>Sign in with your Google account</li>
                <li>Click "Create API Key"</li>
                <li>Copy and paste the key here</li>
              </ol>

              <h4>Free tier limits:</h4>
              <ul>
                <li>15 requests per minute</li>
                <li>1,500 requests per day</li>
                <li>Max image size: 20MB</li>
              </ul>
            </div>
          </div>
        </div>
      </div>
    </div>
  );
}
