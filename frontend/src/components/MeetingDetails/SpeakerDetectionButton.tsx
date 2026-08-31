'use client';

import { useCallback, useEffect, useRef, useState } from 'react';
import { invoke } from '@tauri-apps/api/core';
import { listen } from '@tauri-apps/api/event';
import { Button } from '@/components/ui/button';
import { Users, Loader2 } from 'lucide-react';
import { toast } from 'sonner';
import Analytics from '@/lib/analytics';
import { SpeakerDetectionDialog } from './SpeakerDetectionDialog';

interface SpeakerDetectionButtonProps {
  meetingId: string;
  meetingFolderPath: string;
  onComplete?: () => Promise<void> | void;
}

interface DiarizationStatus {
  consent: 'not_asked' | 'granted' | 'declined';
  ready: boolean;
}

interface DiarizationProgress {
  meeting_id: string;
  stage: string;
  progress_percentage: number;
  message: string;
}

interface DiarizationResult {
  meeting_id: string;
  num_speakers: number;
  num_segments_flagged: number;
}

interface DiarizationError {
  meeting_id: string;
  error: string;
}

/**
 * Triggers a post-meeting diarization pass on demand (phykawing/meetily#16). The models
 * themselves are downloaded behind a separate consent prompt (Settings > Speaker
 * Detection, #10) - this button only runs the pass once that's ready, and otherwise
 * points the user there rather than duplicating the download flow inline.
 */
export function SpeakerDetectionButton({ meetingId, meetingFolderPath, onComplete }: SpeakerDetectionButtonProps) {
  const [ready, setReady] = useState(false);
  const [isRunning, setIsRunning] = useState(false);
  const [progressMessage, setProgressMessage] = useState<string | null>(null);
  const [dialogOpen, setDialogOpen] = useState(false);

  useEffect(() => {
    let cancelled = false;
    invoke<DiarizationStatus>('diarization_model_status')
      .then((status) => {
        if (!cancelled) setReady(status.ready);
      })
      .catch((error) => {
        console.error('Failed to load diarization model status:', error);
      });
    // A pass may already be running for this meeting when the button mounts - most often
    // the automatic post-recording one (phykawing/meetily#18), which this button did not
    // start. Reflect that so it shows as busy rather than offering a click that just
    // errors. A pass running for a *different* meeting is ignored - that meeting's own
    // button reflects it, and the progress listener below still filters by meeting_id.
    invoke<string | null>('diarization_running_meeting')
      .then((runningMeetingId) => {
        if (!cancelled && runningMeetingId === meetingId) {
          setIsRunning(true);
          setProgressMessage('Detecting...');
        }
      })
      .catch((error) => {
        console.error('Failed to check diarization running state:', error);
      });
    return () => {
      cancelled = true;
    };
  }, [meetingId]);

  const onCompleteRef = useRef(onComplete);
  useEffect(() => {
    onCompleteRef.current = onComplete;
  }, [onComplete]);

  useEffect(() => {
    const unlistenPromises = [
      listen<DiarizationProgress>('diarization-progress', (event) => {
        if (event.payload.meeting_id === meetingId) {
          // Covers a pass this button did not start (the automatic post-recording one):
          // the first progress event is enough to switch it into the running state.
          setIsRunning(true);
          setProgressMessage(event.payload.message);
        }
      }),
      listen<DiarizationResult>('diarization-complete', async (event) => {
        if (event.payload.meeting_id !== meetingId) return;
        setIsRunning(false);
        setProgressMessage(null);
        toast.success(
          event.payload.num_speakers === 0
            ? 'Speaker detection complete - no distinct speakers found'
            : event.payload.num_speakers === 1
            ? 'Speaker detection complete - one speaker found'
            : `Speaker detection complete - ${event.payload.num_speakers} speakers found`
        );
        if (onCompleteRef.current) {
          await onCompleteRef.current();
        }
      }),
      listen<DiarizationError>('diarization-error', (event) => {
        if (event.payload.meeting_id !== meetingId) return;
        setIsRunning(false);
        setProgressMessage(null);
        toast.error('Speaker detection failed', { description: event.payload.error });
      }),
    ];

    return () => {
      unlistenPromises.forEach((p) => p.then((unlisten) => unlisten()));
    };
  }, [meetingId]);

  const handleClick = useCallback(() => {
    Analytics.trackButtonClick('detect_speakers', 'meeting_details');
    // Always confirm through the dialog: it carries the optional expected-count hint and,
    // when this meeting already has speakers, the warning that a re-run discards their
    // assigned names (phykawing/meetily#19).
    setDialogOpen(true);
  }, []);

  const handleConfirm = useCallback(
    async (expectedSpeakers: number | null) => {
      setIsRunning(true);
      setProgressMessage('Starting...');
      try {
        // The command itself is the authoritative gate (re-checks consent + model
        // readiness), so this always attempts the call rather than trusting the `ready`
        // state fetched at mount, which could be stale if the user granted consent in
        // Settings without reloading this page.
        await invoke('run_diarization_command', { meetingId, meetingFolderPath, expectedSpeakers });
      } catch (error) {
        setIsRunning(false);
        setProgressMessage(null);
        toast.error('Could not start speaker detection', {
          description: error instanceof Error ? error.message : String(error),
        });
      }
    },
    [meetingId, meetingFolderPath]
  );

  return (
    <>
      <Button
        size="sm"
        variant="outline"
        className="xl:px-4"
        onClick={handleClick}
        disabled={isRunning}
        title={ready ? 'Detect speakers in this recording' : 'Enable Speaker Detection under Settings > Preferences first'}
      >
        {isRunning ? <Loader2 className="xl:mr-2 animate-spin" size={18} /> : <Users className="xl:mr-2" size={18} />}
        <span className="hidden lg:inline">{isRunning ? (progressMessage ?? 'Detecting...') : 'Speakers'}</span>
      </Button>
      <SpeakerDetectionDialog
        open={dialogOpen}
        onOpenChange={setDialogOpen}
        meetingId={meetingId}
        onConfirm={handleConfirm}
      />
    </>
  );
}
