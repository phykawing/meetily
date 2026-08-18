import React, { useEffect, useState } from 'react';
import { BookOpen } from 'lucide-react';
import { invoke } from '@tauri-apps/api/core';
import { toast } from 'sonner';

interface MeetingVocabularyProps {
  disabled?: boolean;
}

export function MeetingVocabulary({ disabled = false }: MeetingVocabularyProps) {
  const [vocabulary, setVocabulary] = useState('');
  const [loading, setLoading] = useState(true);
  const [saving, setSaving] = useState(false);
  const [dirty, setDirty] = useState(false);

  useEffect(() => {
    let cancelled = false;

    invoke<string | null>('get_meeting_vocabulary')
      .then((saved) => {
        if (!cancelled) setVocabulary(saved ?? '');
      })
      .catch((error) => {
        console.error('Failed to load meeting vocabulary:', error);
      })
      .finally(() => {
        if (!cancelled) setLoading(false);
      });

    return () => {
      cancelled = true;
    };
  }, []);

  const handleSave = async () => {
    setSaving(true);
    try {
      const trimmed = vocabulary.trim();
      await invoke('save_meeting_vocabulary', {
        vocabulary: trimmed.length > 0 ? trimmed : null,
      });
      setDirty(false);
      toast.success('Meeting vocabulary saved', {
        description: 'Takes effect on the next transcription.',
      });
    } catch (error) {
      console.error('Failed to save meeting vocabulary:', error);
      toast.error('Failed to save meeting vocabulary', {
        description: error instanceof Error ? error.message : String(error),
      });
    } finally {
      setSaving(false);
    }
  };

  return (
    <div className="space-y-2 pt-4 mt-4 border-t border-gray-200">
      <div className="flex items-center gap-2">
        <BookOpen className="h-4 w-4 text-gray-600" />
        <h4 className="text-sm font-medium text-gray-900">Meeting Vocabulary</h4>
      </div>

      <textarea
        value={vocabulary}
        onChange={(e) => {
          setVocabulary(e.target.value);
          setDirty(true);
        }}
        disabled={disabled || loading || saving}
        rows={3}
        placeholder="Names, jargon, product terms — e.g. 陳大文, Zackriya, Meetily"
        className="w-full px-3 py-2 text-sm bg-white border border-gray-300 rounded-md shadow-sm focus:outline-none focus:ring-1 focus:ring-blue-500 focus:border-blue-500 disabled:bg-gray-50 disabled:text-gray-500 resize-none"
      />

      <p className="text-xs text-gray-600">
        Biases Whisper transcription toward these terms in every Transcription Language. For
        Cantonese, this is applied on top of the built-in seed. An over-long list is
        truncated automatically.
      </p>

      <div className="flex justify-end">
        <button
          onClick={handleSave}
          disabled={disabled || loading || saving || !dirty}
          className="px-3 py-1.5 text-sm font-medium text-white bg-blue-600 rounded-md hover:bg-blue-700 focus:outline-none focus:ring-2 focus:ring-offset-2 focus:ring-blue-500 disabled:opacity-50 disabled:cursor-not-allowed"
        >
          {saving ? 'Saving…' : 'Save'}
        </button>
      </div>
    </div>
  );
}
