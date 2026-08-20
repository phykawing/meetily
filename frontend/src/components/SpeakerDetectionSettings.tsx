import React, { useEffect, useState, useCallback } from 'react';
import { invoke } from '@tauri-apps/api/core';
import { listen } from '@tauri-apps/api/event';
import { Users, CheckCircle2, AlertCircle } from 'lucide-react';
import { toast } from 'sonner';

type Consent = 'not_asked' | 'granted' | 'declined';

interface DiarizationModelStatus {
  id: string;
  displayName: string;
  sizeBytes: number;
  license: string;
  downloaded: boolean;
}

interface DiarizationStatus {
  consent: Consent;
  models: DiarizationModelStatus[];
  totalSizeBytes: number;
  ready: boolean;
}

function formatSize(bytes: number): string {
  const mb = bytes / (1024 * 1024);
  return mb >= 1000 ? `${(mb / 1000).toFixed(1)} GB` : `${mb.toFixed(0)} MB`;
}

export function SpeakerDetectionSettings() {
  const [status, setStatus] = useState<DiarizationStatus | null>(null);
  const [loading, setLoading] = useState(true);
  const [showConfirm, setShowConfirm] = useState(false);
  const [downloading, setDownloading] = useState(false);
  const [progress, setProgress] = useState<Record<string, number>>({});
  const [downloadError, setDownloadError] = useState<string | null>(null);

  const refreshStatus = useCallback(async () => {
    try {
      const result = await invoke<DiarizationStatus>('diarization_model_status');
      setStatus(result);
    } catch (error) {
      console.error('Failed to load diarization model status:', error);
    } finally {
      setLoading(false);
    }
  }, []);

  useEffect(() => {
    refreshStatus();
  }, [refreshStatus]);

  useEffect(() => {
    const unlistenPromises = [
      listen<{ modelId: string; progress: number }>('diarization-model-download-progress', (event) => {
        setProgress((prev) => ({ ...prev, [event.payload.modelId]: event.payload.progress }));
      }),
      listen<{ modelId: string }>('diarization-model-download-complete', () => {
        refreshStatus();
      }),
      listen<{ modelId: string; error: string }>('diarization-model-download-error', (event) => {
        setDownloadError(event.payload.error);
      }),
    ];

    return () => {
      unlistenPromises.forEach((p) => p.then((unlisten) => unlisten()));
    };
  }, [refreshStatus]);

  const startDownload = async () => {
    setDownloading(true);
    setDownloadError(null);
    setProgress({});
    try {
      await invoke('download_diarization_models');
      toast.success('Speaker detection models downloaded');
    } catch (error) {
      const message = error instanceof Error ? error.message : String(error);
      setDownloadError(message);
      toast.error('Failed to download speaker detection models', { description: message });
    } finally {
      setDownloading(false);
      refreshStatus();
    }
  };

  const handleEnable = () => setShowConfirm(true);

  const handleAccept = async () => {
    setShowConfirm(false);
    try {
      await invoke('set_diarization_consent', { consent: 'granted' });
      await refreshStatus();
      await startDownload();
    } catch (error) {
      console.error('Failed to save diarization consent:', error);
      toast.error('Failed to save your choice', {
        description: error instanceof Error ? error.message : String(error),
      });
    }
  };

  const handleDecline = async () => {
    setShowConfirm(false);
    try {
      await invoke('set_diarization_consent', { consent: 'declined' });
      await refreshStatus();
    } catch (error) {
      console.error('Failed to save diarization consent:', error);
    }
  };

  if (loading || !status) {
    return (
      <div className="border-t pt-6">
        <div className="animate-pulse h-16 bg-gray-100 rounded-lg" />
      </div>
    );
  }

  const totalSize = formatSize(status.totalSizeBytes);

  return (
    <div className="border-t pt-6">
      <div className="flex items-center gap-2 mb-2">
        <Users className="h-5 w-5 text-gray-600" />
        <h4 className="text-base font-medium text-gray-900">Speaker Detection</h4>
      </div>
      <p className="text-sm text-gray-600 mb-4">
        Discovers distinct speakers in a finished meeting's audio and attributes each
        transcript segment to them. Runs entirely on your CPU after a one-time model
        download — nothing is fetched until you agree.
      </p>

      {status.consent === 'granted' && status.ready && (
        <div className="flex items-center gap-2 p-4 border rounded-lg bg-green-50 text-green-800 text-sm">
          <CheckCircle2 className="h-4 w-4 flex-shrink-0" />
          Speaker detection models are installed ({totalSize}).
        </div>
      )}

      {status.consent === 'granted' && !status.ready && !downloading && (
        <div className="p-4 border rounded-lg bg-yellow-50 space-y-3">
          <div className="text-sm text-yellow-800">
            {downloadError
              ? `Download failed: ${downloadError}`
              : `Speaker detection models are not fully downloaded yet (${totalSize} total).`}
          </div>
          <button
            onClick={startDownload}
            className="px-3 py-2 text-sm font-medium text-white bg-blue-600 rounded-md hover:bg-blue-700"
          >
            {downloadError ? 'Retry Download' : 'Download Models'}
          </button>
        </div>
      )}

      {downloading && (
        <div className="p-4 border rounded-lg bg-blue-50 space-y-3">
          {status.models.map((model) => (
            <div key={model.id}>
              <div className="flex justify-between text-xs text-blue-800 mb-1">
                <span>{model.displayName}</span>
                <span>{model.downloaded ? '100%' : `${progress[model.id] ?? 0}%`}</span>
              </div>
              <div className="w-full bg-blue-200 rounded-full h-2">
                <div
                  className="bg-blue-600 h-2 rounded-full transition-all duration-300 ease-out"
                  style={{ width: `${model.downloaded ? 100 : progress[model.id] ?? 0}%` }}
                />
              </div>
            </div>
          ))}
        </div>
      )}

      {(status.consent === 'not_asked' || status.consent === 'declined') && !downloading && (
        <div className="p-4 border rounded-lg bg-gray-50">
          {status.consent === 'declined' && (
            <div className="flex items-center gap-2 text-sm text-gray-600 mb-3">
              <AlertCircle className="h-4 w-4 flex-shrink-0" />
              You previously declined the speaker detection model download.
            </div>
          )}
          <button
            onClick={handleEnable}
            className="px-3 py-2 text-sm font-medium text-white bg-blue-600 rounded-md hover:bg-blue-700"
          >
            Enable Speaker Detection
          </button>
        </div>
      )}

      {showConfirm && (
        <div className="fixed inset-0 bg-black bg-opacity-50 flex items-center justify-center z-50">
          <div className="bg-white rounded-lg p-6 max-w-md w-full mx-4">
            <h2 className="text-lg font-semibold mb-3">Download speaker detection models?</h2>
            <p className="text-sm text-gray-600 mb-4">
              This downloads about <strong>{totalSize}</strong> of open-source models to your
              device. They run locally on your CPU — nothing about your meeting audio is sent
              anywhere. You can decline and keep using recording, transcription and
              summarisation exactly as before.
            </p>
            <ul className="text-xs text-gray-500 mb-4 list-disc list-inside space-y-1">
              {status.models.map((model) => (
                <li key={model.id}>
                  {model.displayName} — {formatSize(model.sizeBytes)} ({model.license})
                </li>
              ))}
            </ul>
            <div className="flex justify-end space-x-3">
              <button
                onClick={handleDecline}
                className="px-4 py-2 text-sm text-gray-600 hover:bg-gray-100 rounded-md transition-colors"
              >
                Not Now
              </button>
              <button
                onClick={handleAccept}
                className="px-4 py-2 text-sm bg-blue-600 text-white hover:bg-blue-700 rounded-md transition-colors"
              >
                Download
              </button>
            </div>
          </div>
        </div>
      )}
    </div>
  );
}
