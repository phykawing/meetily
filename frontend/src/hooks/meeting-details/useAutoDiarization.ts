import { useEffect, useState } from 'react';
import { invoke } from '@tauri-apps/api/core';
import { listen } from '@tauri-apps/api/event';
import { toast } from 'sonner';

/**
 * Runs the post-meeting speaker-detection pass automatically when the user lands on a
 * meeting straight from recording (phykawing/meetily#18, docs/adr/0001).
 *
 * The pass is only started when it can actually run - consent granted and models present
 * (`diarization_model_status().ready`). For everyone else this is a no-op and auto-summary
 * behaves exactly as it did before this feature.
 *
 * This hook shows no toasts for pass *progress or completion*: `SpeakerDetectionButton`
 * already listens for `diarization-progress` / `diarization-complete` / `diarization-error`
 * on this meeting regardless of who started the pass, and reports the outcome (and
 * refreshes the transcript) there. It does surface a failure to *start* the pass, which
 * emits no event and so is reported nowhere else (issue #18: "reports the failure").
 *
 * `blocksAutoSummary` is the whole point: while it is true the caller must hold off
 * generating the automatic summary so the summary prompt can carry speaker labels instead
 * of being speaker-blind. It is released on every terminal outcome - not ready, failed to
 * start, pass error, pass complete - and, as a backstop, after `MAX_BLOCK_MS`, so the
 * automatic summary is never wedged waiting forever. A failed pass still lets the summary
 * through (speaker-blind, as today); the failure itself is surfaced by the button.
 */

export type AutoDiarizationPhase =
  | 'inactive' // not started: not from recording, or diarization not enabled
  | 'checking' // deciding whether the pass can run (also: waiting for the meeting to load)
  | 'running' // pass in progress
  | 'complete' // pass finished
  | 'error'; // pass could not start, or failed

interface DiarizationEventPayload {
  meeting_id: string;
}

// Backstop so a summary is never blocked forever if, say, the meeting's folder path never
// materialises or an expected completion event is missed. Comfortably longer than a
// realistic CPU diarization pass on a normal-length meeting.
const MAX_BLOCK_MS = 10 * 60 * 1000;

// Meetings a pass has already been kicked off for in this app session, so navigating back
// to a just-recorded meeting does not launch a second pass on top of the first.
const started = new Set<string>();

export function useAutoDiarization({
  meetingId,
  meetingFolderPath,
  enabled,
}: {
  meetingId: string | null;
  meetingFolderPath: string | null | undefined;
  enabled: boolean;
}): { phase: AutoDiarizationPhase; blocksAutoSummary: boolean } {
  // Start already blocking when this arrived from recording, so the automatic summary
  // cannot slip through in the render or two before the async check below resolves.
  const [phase, setPhase] = useState<AutoDiarizationPhase>(enabled ? 'checking' : 'inactive');

  useEffect(() => {
    if (!enabled) {
      setPhase('inactive');
      return;
    }

    let cancelled = false;
    const unlisteners: Array<() => void> = [];
    const set = (next: AutoDiarizationPhase) => {
      if (!cancelled) setPhase(next);
    };

    // Backstop timer - releases the gate even if nothing else does.
    const backstop = setTimeout(() => set('error'), MAX_BLOCK_MS);

    (async () => {
      // Listeners first: a pass that ends quickly (or one already running, started by the
      // Speakers button) must still move the gate off 'running'.
      unlisteners.push(
        await listen<DiarizationEventPayload>('diarization-complete', (event) => {
          if (event.payload.meeting_id === meetingId) set('complete');
        }),
      );
      unlisteners.push(
        await listen<DiarizationEventPayload>('diarization-error', (event) => {
          if (event.payload.meeting_id === meetingId) set('error');
        }),
      );
      if (cancelled) return;

      // The meeting is still loading (no id or no saved-audio folder yet). Stay in
      // 'checking' - which keeps blocking - and let the effect re-run when they arrive,
      // rather than briefly unblocking and letting the summary slip through.
      if (!meetingId || !meetingFolderPath) {
        return;
      }

      // Already handled this meeting this session - reflect whether our pass is still going
      // rather than starting another one.
      if (started.has(meetingId)) {
        try {
          const runningMeetingId = await invoke<string | null>('diarization_running_meeting');
          set(runningMeetingId === meetingId ? 'running' : 'complete');
        } catch {
          set('complete');
        }
        return;
      }

      set('checking');

      let ready = false;
      try {
        const status = await invoke<{ ready: boolean }>('diarization_model_status');
        ready = status.ready;
      } catch (error) {
        console.warn('auto-diarization: could not read model status', error);
        set('inactive');
        return;
      }
      if (cancelled) return;
      if (!ready) {
        // The common case: consent not granted / models not downloaded. Auto-summary must
        // behave exactly as it did before this feature.
        set('inactive');
        return;
      }

      set('running');
      try {
        await invoke('run_diarization_command', { meetingId, meetingFolderPath });
        started.add(meetingId);
      } catch (error) {
        const message = error instanceof Error ? error.message : String(error);
        if (/already (running|in progress)/i.test(message)) {
          // The pass is single-flight process-wide (diarization::pipeline), and this
          // rejection does not say whose pass holds the lock. It is almost certainly
          // *another* meeting's - a freshly-recorded meeting's own pass has not started
          // yet - so do not wait on a `diarization-complete` that will carry a different
          // meeting_id and never release the gate. Skip, let the summary through, and
          // point the user at the manual button.
          console.warn('auto-diarization: another pass holds the lock; skipping', error);
          toast.info('Speaker detection skipped', {
            description:
              'Another recording is still being processed. Run it from the transcript panel when that finishes.',
          });
          set('inactive');
        } else {
          console.warn('auto-diarization: could not start pass', error);
          toast.error('Speaker detection could not start', { description: message });
          set('error');
        }
      }
    })();

    return () => {
      cancelled = true;
      clearTimeout(backstop);
      unlisteners.forEach((unlisten) => unlisten());
    };
  }, [enabled, meetingId, meetingFolderPath]);

  return {
    phase,
    blocksAutoSummary: phase === 'checking' || phase === 'running',
  };
}
