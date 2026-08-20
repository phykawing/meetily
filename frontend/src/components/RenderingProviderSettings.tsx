'use client';

import { useEffect, useState } from 'react';
import { invoke } from '@tauri-apps/api/core';
import { ShieldAlert } from 'lucide-react';
import { toast } from 'sonner';

type RenderingProviderSetting = 'local' | 'summary_provider';

const DEFAULT_PROVIDER: RenderingProviderSetting = 'local';

const PROVIDER_DISPLAY_NAMES: Record<string, string> = {
  'builtin-ai': 'Built-in AI (local)',
  openai: 'OpenAI',
  claude: 'Claude',
  groq: 'Groq',
  ollama: 'Ollama',
  openrouter: 'OpenRouter',
  'custom-openai': 'Custom Server (OpenAI)',
};

function displayNameForProvider(provider: string | null): string {
  if (!provider) return 'no summary provider configured';
  return PROVIDER_DISPLAY_NAMES[provider] || provider;
}

/**
 * Which provider produces a 書面語 Rendering (see phykawing/meetily#8). A Rendering is an
 * LLM pass over the entire transcript, so this is a distinct, explicit privacy setting from
 * the summary provider — see docs/adr/0002 and phykawing/meetily#15.
 */
export function RenderingProviderSettings() {
  const [provider, setProvider] = useState<RenderingProviderSetting>(DEFAULT_PROVIDER);
  const [loading, setLoading] = useState(true);
  const [saving, setSaving] = useState(false);
  const [summaryProvider, setSummaryProvider] = useState<string | null>(null);
  const [showConfirm, setShowConfirm] = useState(false);
  const summaryProviderIsLocal = summaryProvider === 'builtin-ai';

  useEffect(() => {
    let cancelled = false;

    Promise.all([
      invoke<string>('get_rendering_provider'),
      invoke('api_get_model_config') as Promise<any>,
    ])
      .then(([storedProvider, modelConfig]) => {
        if (cancelled) return;
        if (storedProvider === 'summary_provider') setProvider('summary_provider');
        setSummaryProvider(modelConfig?.provider ?? null);
      })
      .catch((error) => {
        console.error('Failed to load rendering provider setting:', error);
      })
      .finally(() => {
        if (!cancelled) setLoading(false);
      });

    return () => {
      cancelled = true;
    };
  }, []);

  useEffect(() => {
    let cancelled = false;
    let unlisten: (() => void) | undefined;

    (async () => {
      const { listen } = await import('@tauri-apps/api/event');
      const fn = await listen<{ provider: string }>('model-config-updated', (event) => {
        setSummaryProvider(event.payload.provider ?? null);
      });
      // The effect may have been cleaned up while `listen` was still resolving — in that
      // case there's nothing left to store the unlisten function in, so tear it down
      // immediately instead of leaking a subscription tied to an unmounted component.
      if (cancelled) {
        fn();
      } else {
        unlisten = fn;
      }
    })();

    return () => {
      cancelled = true;
      unlisten?.();
    };
  }, []);

  const persist = async (next: RenderingProviderSetting) => {
    const previous = provider;
    setProvider(next);
    setSaving(true);
    try {
      await invoke('set_rendering_provider', { provider: next });
      toast.success('Rendering provider saved', {
        description:
          next === 'local'
            ? '書面語 renderings will be produced by the local model.'
            : `書面語 renderings will follow the summary provider (currently ${displayNameForProvider(summaryProvider)}).`,
      });
    } catch (error) {
      console.error('Failed to save rendering provider setting:', error);
      setProvider(previous);
      toast.error('Failed to save rendering provider setting', {
        description: error instanceof Error ? error.message : String(error),
      });
    } finally {
      setSaving(false);
    }
  };

  const handleSelectLocal = () => {
    if (provider === 'local') return;
    persist('local');
  };

  const handleSelectSummaryProvider = () => {
    if (provider === 'summary_provider') return;
    setShowConfirm(true);
  };

  const handleConfirm = () => {
    setShowConfirm(false);
    persist('summary_provider');
  };

  const handleCancel = () => {
    setShowConfirm(false);
  };

  if (loading) {
    return (
      <div className="bg-white rounded-lg border border-gray-200 p-6 shadow-sm">
        <div className="animate-pulse h-16 bg-gray-100 rounded-lg" />
      </div>
    );
  }

  return (
    <div className="bg-white rounded-lg border border-gray-200 p-6 shadow-sm">
      <h3 className="text-lg font-semibold text-gray-900 mb-2">Rendering Provider</h3>
      <p className="text-sm text-gray-600 mb-4">
        書面語 rendering rewrites a meeting&apos;s entire transcript, so which model performs
        it is a separate, explicit privacy decision from the summary provider above. By
        default, no transcript text leaves this machine.
      </p>

      <div className="space-y-2">
        <label className="flex items-start gap-3 p-3 border rounded-lg cursor-pointer hover:bg-gray-50 has-[:checked]:border-blue-500 has-[:checked]:bg-blue-50">
          <input
            type="radio"
            name="rendering-provider"
            className="mt-1"
            checked={provider === 'local'}
            disabled={saving}
            onChange={handleSelectLocal}
          />
          <span>
            <span className="block text-sm font-medium text-gray-900">Local (recommended)</span>
            <span className="block text-xs text-gray-600">
              Uses the app&apos;s Built-in AI model. Nothing about your meeting is sent anywhere.
              If no local model is downloaded, rendering is unavailable rather than falling
              back to a remote provider.
            </span>
          </span>
        </label>

        <label className="flex items-start gap-3 p-3 border rounded-lg cursor-pointer hover:bg-gray-50 has-[:checked]:border-blue-500 has-[:checked]:bg-blue-50">
          <input
            type="radio"
            name="rendering-provider"
            className="mt-1"
            checked={provider === 'summary_provider'}
            disabled={saving}
            onChange={handleSelectSummaryProvider}
          />
          <span>
            <span className="block text-sm font-medium text-gray-900">Same as summary provider</span>
            <span className="block text-xs text-gray-600">
              Uses whichever provider is configured for summaries (currently{' '}
              {displayNameForProvider(summaryProvider)}). If that provider changes later,
              renderings follow it without asking again.
            </span>
          </span>
        </label>
      </div>

      {showConfirm && summaryProviderIsLocal && (
        <div className="fixed inset-0 bg-black bg-opacity-50 flex items-center justify-center z-50">
          <div className="bg-white rounded-lg p-6 max-w-md w-full mx-4">
            <h2 className="text-lg font-semibold mb-3">Follow the summary provider?</h2>
            <p className="text-sm text-gray-600 mb-4">
              Your summary provider is currently <strong>Built-in AI (local)</strong>, so
              nothing about your meeting is sent anywhere yet. But if you switch the summary
              provider to a cloud provider later, 書面語 renderings will follow it
              automatically — sending the whole transcript to that provider — without asking
              again.
            </p>
            <div className="flex justify-end space-x-3">
              <button
                onClick={handleCancel}
                className="px-4 py-2 text-sm text-gray-600 hover:bg-gray-100 rounded-md transition-colors"
              >
                Cancel
              </button>
              <button
                onClick={handleConfirm}
                className="px-4 py-2 text-sm bg-blue-600 text-white hover:bg-blue-700 rounded-md transition-colors"
              >
                Confirm
              </button>
            </div>
          </div>
        </div>
      )}

      {showConfirm && !summaryProviderIsLocal && (
        <div className="fixed inset-0 bg-black bg-opacity-50 flex items-center justify-center z-50">
          <div className="bg-white rounded-lg p-6 max-w-md w-full mx-4">
            <div className="flex items-center gap-2 mb-3">
              <ShieldAlert className="h-5 w-5 text-yellow-600 flex-shrink-0" />
              <h2 className="text-lg font-semibold">Send transcripts to {displayNameForProvider(summaryProvider)}?</h2>
            </div>
            <p className="text-sm text-gray-600 mb-4">
              &quot;Same as summary provider&quot; is currently{' '}
              <strong>{displayNameForProvider(summaryProvider)}</strong>. Every 書面語
              rendering is a full-transcript pass, so choosing this sends the whole
              meeting&apos;s text to that provider whenever a rendering is generated. If you
              change the summary provider later, renderings will follow it too, without
              asking again.
            </p>
            <div className="flex justify-end space-x-3">
              <button
                onClick={handleCancel}
                className="px-4 py-2 text-sm text-gray-600 hover:bg-gray-100 rounded-md transition-colors"
              >
                Cancel
              </button>
              <button
                onClick={handleConfirm}
                className="px-4 py-2 text-sm bg-blue-600 text-white hover:bg-blue-700 rounded-md transition-colors"
              >
                Use {displayNameForProvider(summaryProvider)}
              </button>
            </div>
          </div>
        </div>
      )}
    </div>
  );
}
