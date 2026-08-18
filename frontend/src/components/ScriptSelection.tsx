import React, { useEffect, useState } from 'react';
import { Languages } from 'lucide-react';
import { invoke } from '@tauri-apps/api/core';
import { toast } from 'sonner';

interface ScriptSelectionProps {
  disabled?: boolean;
}

const SCRIPT_OPTIONS: { code: string; label: string }[] = [
  { code: 'traditional-hk', label: 'Traditional (Hong Kong)' },
  { code: 'simplified', label: 'Simplified' },
  { code: 'as-recognized', label: 'Leave as recognized' },
];

const DEFAULT_SCRIPT = 'traditional-hk';

export function ScriptSelection({ disabled = false }: ScriptSelectionProps) {
  const [script, setScript] = useState(DEFAULT_SCRIPT);
  const [loading, setLoading] = useState(true);
  const [saving, setSaving] = useState(false);

  useEffect(() => {
    let cancelled = false;

    invoke<string>('get_script_setting')
      .then((saved) => {
        if (!cancelled) setScript(saved || DEFAULT_SCRIPT);
      })
      .catch((error) => {
        console.error('Failed to load script setting:', error);
      })
      .finally(() => {
        if (!cancelled) setLoading(false);
      });

    return () => {
      cancelled = true;
    };
  }, []);

  const handleChange = async (scriptSetting: string) => {
    const previous = script;
    setScript(scriptSetting);
    setSaving(true);
    try {
      await invoke('save_script_setting', { scriptSetting });
      const label = SCRIPT_OPTIONS.find((o) => o.code === scriptSetting)?.label || scriptSetting;
      toast.success('Script setting saved', {
        description: `Chinese transcripts will be converted to ${label} going forward.`,
      });
    } catch (error) {
      console.error('Failed to save script setting:', error);
      setScript(previous);
      toast.error('Failed to save script setting', {
        description: error instanceof Error ? error.message : String(error),
      });
    } finally {
      setSaving(false);
    }
  };

  return (
    <div className="space-y-2 pt-4 mt-4 border-t border-gray-200">
      <div className="flex items-center gap-2">
        <Languages className="h-4 w-4 text-gray-600" />
        <h4 className="text-sm font-medium text-gray-900">Script</h4>
      </div>

      <select
        value={script}
        onChange={(e) => handleChange(e.target.value)}
        disabled={disabled || loading || saving}
        className="w-full px-3 py-2 text-sm bg-white border border-gray-300 rounded-md shadow-sm focus:outline-none focus:ring-1 focus:ring-blue-500 focus:border-blue-500 disabled:bg-gray-50 disabled:text-gray-500"
      >
        {SCRIPT_OPTIONS.map((option) => (
          <option key={option.code} value={option.code}>
            {option.label}
          </option>
        ))}
      </select>

      <p className="text-xs text-gray-600">
        Converts Chinese transcripts to this character set once, as they are stored — applies
        to Mandarin and Cantonese meetings alike. Choose &quot;Leave as recognized&quot; to keep
        exactly what the recognizer produced.
      </p>
    </div>
  );
}
