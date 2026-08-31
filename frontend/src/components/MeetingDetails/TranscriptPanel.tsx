"use client";

import { Transcript, TranscriptSegmentData } from '@/types';
import { TranscriptView } from '@/components/TranscriptView';
import { VirtualizedTranscriptView } from '@/components/VirtualizedTranscriptView';
import { TranscriptButtonGroup } from './TranscriptButtonGroup';
import { useCallback, useEffect, useMemo, useRef, useState } from 'react';
import { invoke } from '@tauri-apps/api/core';
import { listen } from '@tauri-apps/api/event';
import { toast } from 'sonner';
import { Button } from '@/components/ui/button';
import { Loader2 } from 'lucide-react';
import { fetchSpeakerNames } from '@/lib/meeting-speakers';

type WrittenForm = 'colloquial' | 'written';

interface TranscriptPanelProps {
  transcripts: Transcript[];
  customPrompt: string;
  onPromptChange: (value: string) => void;
  onCopyTranscript: () => void;
  onOpenMeetingFolder: () => Promise<void>;
  isRecording: boolean;
  disableAutoScroll?: boolean;

  // Optional pagination props (when using virtualization)
  usePagination?: boolean;
  segments?: TranscriptSegmentData[];
  hasMore?: boolean;
  isLoadingMore?: boolean;
  totalCount?: number;
  loadedCount?: number;
  onLoadMore?: () => void;

  // Retranscription props
  meetingId?: string;
  meetingFolderPath?: string | null;
  onRefetchTranscripts?: () => Promise<void>;
}

export function TranscriptPanel({
  transcripts,
  customPrompt,
  onPromptChange,
  onCopyTranscript,
  onOpenMeetingFolder,
  isRecording,
  disableAutoScroll = false,
  usePagination = false,
  segments,
  hasMore,
  isLoadingMore,
  totalCount,
  loadedCount,
  onLoadMore,
  meetingId,
  meetingFolderPath,
  onRefetchTranscripts,
}: TranscriptPanelProps) {
  // Convert transcripts to segments if pagination is not used but we want virtualization
  const convertedSegments = useMemo(() => {
    if (usePagination && segments) {
      return segments;
    }
    // Convert transcripts to segments for virtualization
    return transcripts.map(t => ({
      id: t.id,
      timestamp: t.audio_start_time ?? 0,
      endTime: t.audio_end_time,
      text: t.text,
      confidence: t.confidence,
      audioSource: t.audio_source,
      speakerLabel: t.speaker_label,
      speakerUncertain: t.speaker_uncertain,
    }));
  }, [transcripts, usePagination, segments]);

  // Discovered speaker names for this meeting (speaker_label -> display name), from the
  // most recent diarization pass (phykawing/meetily#16). Empty for a meeting that hasn't
  // been diarized yet - segments then render exactly as before diarization existed.
  const [speakerNames, setSpeakerNames] = useState<Record<string, string>>({});

  const refreshSpeakerNames = useCallback(async () => {
    if (!meetingId) {
      setSpeakerNames({});
      return;
    }
    try {
      setSpeakerNames(await fetchSpeakerNames(meetingId));
    } catch (error) {
      console.error('Failed to load meeting speakers:', error);
    }
  }, [meetingId]);

  useEffect(() => {
    refreshSpeakerNames();
  }, [refreshSpeakerNames]);

  // Rename a discovered speaker (phykawing/meetily#17). Optimistically updates the local
  // label -> name map so every segment of that speaker relabels at once, then persists;
  // on failure it reloads the authoritative names from the database. The rename is scoped
  // to this meeting - the backend command keys on (meeting_id, speaker_label).
  const handleRenameSpeaker = useCallback(
    async (label: string, name: string) => {
      if (!meetingId) return;
      setSpeakerNames((current) => ({ ...current, [label]: name }));
      try {
        await invoke('rename_meeting_speaker', { meetingId, speakerLabel: label, name });
        toast.success(`Speaker renamed to "${name}"`);
      } catch (error) {
        toast.error('Could not rename speaker', {
          description: error instanceof Error ? error.message : String(error),
        });
        await refreshSpeakerNames();
      }
    },
    [meetingId, refreshSpeakerNames]
  );

  useEffect(() => {
    const unlisten = listen<{ meeting_id: string }>('diarization-complete', (event) => {
      if (event.payload.meeting_id === meetingId) {
        refreshSpeakerNames();
      }
    });
    return () => {
      unlisten.then((fn) => fn());
    };
  }, [meetingId, refreshSpeakerNames]);

  // Written Form: 口語 (verbatim, the Canonical Transcript) is the default view. 書面語 is a
  // cached Rendering produced by the local model on demand — see phykawing/meetily#8 and
  // docs/adr/0002-canonical-transcript-is-verbatim-colloquial.md.
  const [writtenForm, setWrittenForm] = useState<WrittenForm>('colloquial');
  const [renderedText, setRenderedText] = useState<string | null>(null);
  const [isLoadingRendering, setIsLoadingRendering] = useState(false);
  const [renderingError, setRenderingError] = useState<string | null>(null);

  // Tracks which meeting is currently being viewed, so a rendering fetch that resolves
  // after the user has already switched to a different meeting doesn't clobber that
  // meeting's state (the fetch itself has no built-in cancellation).
  const currentMeetingIdRef = useRef(meetingId);
  useEffect(() => {
    currentMeetingIdRef.current = meetingId;
  }, [meetingId]);

  // Load the persisted per-meeting preference whenever the viewed meeting changes.
  useEffect(() => {
    setRenderedText(null);
    setRenderingError(null);

    if (!meetingId) {
      setWrittenForm('colloquial');
      return;
    }

    let cancelled = false;
    invoke<string>('get_written_form', { meetingId })
      .then((form) => {
        if (!cancelled) setWrittenForm(form === 'written' ? 'written' : 'colloquial');
      })
      .catch((error) => {
        console.error('Failed to load written form preference:', error);
        if (!cancelled) setWrittenForm('colloquial');
      });

    return () => {
      cancelled = true;
    };
  }, [meetingId]);

  const fetchRendering = useCallback(async () => {
    if (!meetingId) return;
    const requestedFor = meetingId;
    setIsLoadingRendering(true);
    setRenderingError(null);
    try {
      const text = await invoke<string>('get_transcript_rendering', { meetingId: requestedFor });
      if (currentMeetingIdRef.current === requestedFor) {
        setRenderedText(text);
      }
    } catch (error) {
      if (currentMeetingIdRef.current === requestedFor) {
        setRenderingError(
          typeof error === 'string' ? error : '書面語 rendering is currently unavailable.'
        );
      }
    } finally {
      if (currentMeetingIdRef.current === requestedFor) {
        setIsLoadingRendering(false);
      }
    }
  }, [meetingId]);

  // Generate (or fetch the cached) rendering the first time the view switches to 書面語 for
  // this meeting. Once loaded, `renderedText` is kept across toggling back to 口語 and
  // forth again — per the "toggling back and forth does not regenerate it" requirement —
  // and is only cleared by a meeting change (above) or a transcript refetch (below).
  useEffect(() => {
    if (
      writtenForm === 'written' &&
      meetingId &&
      renderedText === null &&
      !isLoadingRendering &&
      !renderingError
    ) {
      fetchRendering();
    }
  }, [writtenForm, meetingId, renderedText, isLoadingRendering, renderingError, fetchRendering]);

  const handleSelectWrittenForm = useCallback(
    async (next: WrittenForm) => {
      if (next === writtenForm || !meetingId) return;

      const previous = writtenForm;
      setWrittenForm(next);

      try {
        await invoke('set_written_form', { meetingId, writtenForm: next });
      } catch (error) {
        console.error('Failed to save written form preference:', error);
        toast.error('Failed to save Written Form preference');
        setWrittenForm(previous);
      }
    },
    [writtenForm, meetingId]
  );

  // Retranscription rewrites the Canonical Transcript in place (the panel does not remount
  // for it, unlike a meeting switch), so any cached rendering it produced is stale. This
  // wraps the caller-supplied refetch to drop the local rendering cache before reloading —
  // the next switch to 書面語 will regenerate against the new transcript.
  const handleRefetchTranscripts = useCallback(async () => {
    setRenderedText(null);
    setRenderingError(null);
    if (onRefetchTranscripts) {
      await onRefetchTranscripts();
    }
  }, [onRefetchTranscripts]);

  // The copy button copies whichever view is on screen. When 書面語 is selected but not yet
  // ready (still generating, or failed), copying the colloquial text instead would silently
  // hand back something other than what's displayed, so this refuses rather than falling back.
  const handleCopy = useCallback(() => {
    if (writtenForm === 'written') {
      if (!renderedText) {
        toast.error('書面語 rendering is not ready to copy yet.');
        return;
      }
      navigator.clipboard.writeText(renderedText);
      toast.success('Transcript copied to clipboard');
      return;
    }
    onCopyTranscript();
  }, [writtenForm, renderedText, onCopyTranscript]);

  const canToggleWrittenForm = !isRecording && !!meetingId && convertedSegments.length > 0;

  return (
    <div className="hidden md:flex md:w-1/4 lg:w-1/3 min-w-0 border-r border-gray-200 bg-white flex-col relative shrink-0">
      {/* Title area */}
      <div className="p-4 border-b border-gray-200 space-y-2">
        <TranscriptButtonGroup
          transcriptCount={usePagination ? (totalCount ?? convertedSegments.length) : (transcripts?.length || 0)}
          onCopyTranscript={handleCopy}
          onOpenMeetingFolder={onOpenMeetingFolder}
          meetingId={meetingId}
          meetingFolderPath={meetingFolderPath}
          onRefetchTranscripts={handleRefetchTranscripts}
        />

        {meetingId && (
          <div className="flex items-center justify-center gap-1" role="group" aria-label="Written Form">
            <Button
              size="sm"
              variant={writtenForm === 'colloquial' ? 'default' : 'outline'}
              className="flex-1 text-xs"
              disabled={!canToggleWrittenForm}
              onClick={() => handleSelectWrittenForm('colloquial')}
              title="口語 — the verbatim transcript of what was actually said"
            >
              口語
            </Button>
            <Button
              size="sm"
              variant={writtenForm === 'written' ? 'default' : 'outline'}
              className="flex-1 text-xs"
              disabled={!canToggleWrittenForm}
              onClick={() => handleSelectWrittenForm('written')}
              title="書面語 — rewritten for reading and sharing, cached and regenerated from the transcript"
            >
              書面語
            </Button>
          </div>
        )}
      </div>

      {/* Transcript content - use virtualized view for better performance */}
      <div className="flex-1 overflow-hidden pb-4">
        {writtenForm === 'written' ? (
          <div className="h-full overflow-y-auto px-4 py-3">
            {isLoadingRendering && (
              <div className="flex items-center gap-2 text-sm text-gray-500">
                <Loader2 className="h-4 w-4 animate-spin" />
                Generating 書面語 rendering with the local model…
              </div>
            )}
            {!isLoadingRendering && renderingError && (
              <div className="space-y-2">
                <p className="text-sm text-red-600">{renderingError}</p>
                <Button size="sm" variant="outline" onClick={fetchRendering}>
                  Try again
                </Button>
              </div>
            )}
            {!isLoadingRendering && !renderingError && renderedText && (
              <p className="whitespace-pre-wrap text-sm text-gray-800">{renderedText}</p>
            )}
          </div>
        ) : (
          <VirtualizedTranscriptView
            segments={convertedSegments}
            isRecording={isRecording}
            isPaused={false}
            isProcessing={false}
            isStopping={false}
            enableStreaming={false}
            showConfidence={true}
            disableAutoScroll={disableAutoScroll}
            hasMore={hasMore}
            isLoadingMore={isLoadingMore}
            totalCount={totalCount}
            loadedCount={loadedCount}
            onLoadMore={onLoadMore}
            speakerNames={speakerNames}
            onRenameSpeaker={handleRenameSpeaker}
          />
        )}
      </div>

      {/* Custom prompt input at bottom of transcript section */}
      {!isRecording && convertedSegments.length > 0 && (
        <div className="p-1 border-t border-gray-200">
          <textarea
            placeholder="Add context for AI summary. For example people involved, meeting overview, objective etc..."
            className="w-full px-3 py-2 border border-gray-200 rounded-md text-sm focus:outline-none focus:ring-1 focus:ring-blue-500 focus:border-blue-500 bg-white shadow-sm min-h-[80px] resize-y"
            value={customPrompt}
            onChange={(e) => onPromptChange(e.target.value)}
          />
        </div>
      )}
    </div>
  );
}
