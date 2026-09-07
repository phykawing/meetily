import React, { useState, useEffect } from 'react';
import { Globe } from 'lucide-react';
import { invoke } from '@tauri-apps/api/core';
import Analytics from '@/lib/analytics';
import { toast } from 'sonner';
import { useConfig } from '@/contexts/ConfigContext';
import { LANGUAGES, CANTONESE_LANGUAGE_CODE } from '@/constants/languages';
import { cantoneseUnavailableReason, cantoneseCapabilityKnown } from '@/lib/cantonese-capability';
import type { ModelInfo } from '@/lib/whisper';

interface LanguageSelectionProps {
  selectedLanguage: string;
  onLanguageChange: (language: string) => void;
  disabled?: boolean;
  provider?: 'localWhisper' | 'parakeet' | 'deepgram' | 'elevenLabs' | 'groq' | 'openai';
  /** The currently configured Whisper model name, used to judge Cantonese capability. */
  modelName?: string;
}

export function LanguageSelection({
  selectedLanguage,
  onLanguageChange,
  disabled = false,
  provider = 'localWhisper',
  modelName
}: LanguageSelectionProps) {
  const [saving, setSaving] = useState(false);
  const [whisperModels, setWhisperModels] = useState<ModelInfo[]>([]);
  // Only true once whisper_get_available_models has actually returned. Left false on IPC
  // failure on purpose: with capability unknowable we must not reset a saved selection.
  const [modelsLoaded, setModelsLoaded] = useState(false);
  const { setSelectedLanguage, transcriptConfigLoaded } = useConfig();

  // Parakeet only supports auto-detection (doesn't support manual language selection)
  const isParakeet = provider === 'parakeet';

  useEffect(() => {
    if (provider !== 'localWhisper') return;
    let cancelled = false;
    let retried = false;

    const fetchModels = () => {
      invoke<ModelInfo[]>('whisper_get_available_models')
        .then((models) => {
          if (!cancelled) {
            setWhisperModels(models);
            setModelsLoaded(true);
          }
        })
        .catch((err) => {
          console.error('Failed to fetch Whisper models for language capability check:', err);
          // A single transient IPC failure shouldn't strand Cantonese as permanently
          // "unavailable" for the rest of this mount — retry once before giving up.
          if (!cancelled && !retried) {
            retried = true;
            setTimeout(fetchModels, 1000);
          }
        });
    };
    fetchModels();

    return () => { cancelled = true; };
  }, [provider]);

  // The capability gate is specific to the app's local providers (Whisper's per-model
  // rule, Parakeet's blanket lack of language selection). Cloud providers aren't governed
  // by it, so Cantonese behaves like any other language for them.
  const isLocalProvider = provider === 'localWhisper' || isParakeet;
  const currentModel = whisperModels.find((m) => m.name === modelName);
  const cantoneseReason = isLocalProvider
    ? cantoneseUnavailableReason({
        isParakeet,
        modelName,
        supportsCantonese: currentModel?.supports_cantonese,
      })
    : null;

  // Switching to a model that can't serve Cantonese must not leave a stale selection behind
  // a disabled option — that reaches the backend as an unsupported-language error instead of
  // falling back cleanly. But only once capability is actually known: acting while the
  // provider, model name or model list are still loading would reset the user's saved
  // Cantonese choice on a guess, and this write persists (localStorage + Rust).
  const capabilityKnown = cantoneseCapabilityKnown({
    configLoaded: transcriptConfigLoaded,
    isParakeet,
    modelsLoaded,
    modelName,
  });
  useEffect(() => {
    if (capabilityKnown && cantoneseReason && selectedLanguage === CANTONESE_LANGUAGE_CODE) {
      setSelectedLanguage('auto');
      onLanguageChange('auto');
    }
  }, [capabilityKnown, cantoneseReason, selectedLanguage]);

  const availableLanguages = isParakeet
    ? LANGUAGES.filter(lang => lang.code === 'auto' || lang.code === 'auto-translate' || lang.code === CANTONESE_LANGUAGE_CODE)
    : LANGUAGES;

  const handleLanguageChange = async (languageCode: string) => {
    if (languageCode === CANTONESE_LANGUAGE_CODE && cantoneseReason) return;
    setSaving(true);
    try {
      // Save language preference to localStorage and sync to backend
      setSelectedLanguage(languageCode);
      onLanguageChange(languageCode);
      console.log('Language preference saved:', languageCode);

      // Track language selection analytics
      const selectedLang = LANGUAGES.find(lang => lang.code === languageCode);
      await Analytics.track('language_selected', {
        language_code: languageCode,
        language_name: selectedLang?.name || 'Unknown',
        is_auto_detect: (languageCode === 'auto').toString(),
        is_auto_translate: (languageCode === 'auto-translate').toString()
      });

      // Show success toast
      const languageName = selectedLang?.name || languageCode;
      toast.success("Language preference saved", {
        description: `Transcription language set to ${languageName}`
      });
    } catch (error) {
      console.error('Failed to save language preference:', error);
      toast.error("Failed to save language preference", {
        description: error instanceof Error ? error.message : String(error)
      });
    } finally {
      setSaving(false);
    }
  };

  // Find the selected language name for display
  const selectedLanguageName = LANGUAGES.find(
    lang => lang.code === selectedLanguage
  )?.name || 'Auto Detect (Original Language)';

  return (
    <div className="space-y-4">
      <div className="flex items-center justify-between">
        <div className="flex items-center gap-2">
          <Globe className="h-4 w-4 text-gray-600" />
          <h4 className="text-sm font-medium text-gray-900">Transcription Language</h4>
        </div>
      </div>

      <div className="space-y-2">
        <select
          value={selectedLanguage}
          onChange={(e) => handleLanguageChange(e.target.value)}
          disabled={disabled || saving}
          className="w-full px-3 py-2 text-sm bg-white border border-gray-300 rounded-md shadow-sm focus:outline-none focus:ring-1 focus:ring-blue-500 focus:border-blue-500 disabled:bg-gray-50 disabled:text-gray-500"
        >
          {availableLanguages.map((language) => (
            <option
              key={language.code}
              value={language.code}
              disabled={language.code === CANTONESE_LANGUAGE_CODE && !!cantoneseReason}
              title={language.code === CANTONESE_LANGUAGE_CODE ? cantoneseReason ?? undefined : undefined}
            >
              {language.name}
              {language.code !== 'auto' && language.code !== 'auto-translate' && ` (${language.code})`}
            </option>
          ))}
        </select>

        {/* Parakeet language limitation warning */}
        {isParakeet && (
          <div className="p-2 bg-amber-50 border border-amber-200 rounded text-amber-800">
            <p className="font-medium">ℹ️ Parakeet Language Support</p>
            <p className="mt-1 text-xs">Parakeet currently only supports automatic language detection. Manual language selection is not available. Use Whisper if you need to specify a particular language.</p>
          </div>
        )}

        {/* Cantonese capability note */}
        {cantoneseReason && (
          <div className="p-2 bg-gray-50 border border-gray-200 rounded text-gray-700">
            <p className="text-xs"><strong>Cantonese:</strong> {cantoneseReason}</p>
          </div>
        )}

        {/* Info text */}
        <div className="text-xs space-y-2 pt-2">
          <p className="text-gray-600">
            <strong>Current:</strong> {selectedLanguageName}
          </p>
          {selectedLanguage === 'auto' && (
            <div className="p-2 bg-yellow-50 border border-yellow-200 rounded text-yellow-800">
              <p className="font-medium">⚠️ Auto Detect may produce incorrect results</p>
              <p className="mt-1">For best accuracy, select your specific language (e.g., English, Spanish, etc.)</p>
            </div>
          )}
          {selectedLanguage === 'auto-translate' && (
            <div className="p-2 bg-blue-50 border border-blue-200 rounded text-blue-800">
              <p className="font-medium">🌐 Translation Mode Active</p>
              <p className="mt-1">All audio will be automatically translated to English. Best for multilingual meetings where you need English output.</p>
            </div>
          )}
          {selectedLanguage !== 'auto' && selectedLanguage !== 'auto-translate' && (
            <p className="text-gray-600">
              Transcription will be optimized for <strong>{selectedLanguageName}</strong>
            </p>
          )}
        </div>
      </div>
    </div>
  );
}
