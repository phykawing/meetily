/**
 * Pure decision logic pulled out of `useAutoDiarization` (phykawing/meetily#34 item 4) so
 * the hook's branch points can be unit tested without a React/Tauri test harness. Each
 * function here mirrors exactly one decision the hook's async effect makes - see that file
 * for how they compose into the overall state machine.
 */

/**
 * Whether enough of the meeting's metadata has arrived to decide anything yet: `'wait'`
 * keeps blocking (no id, or metadata still loading), `'no-audio'` means metadata is in but
 * there is no folder path to diarize, and `'proceed'` means it's safe to check model/consent
 * status next.
 */
export function autoDiarizationWaitState(
  meetingId: string | null,
  metadataLoaded: boolean,
  meetingFolderPath: string | null | undefined
): 'wait' | 'no-audio' | 'proceed' {
  if (!meetingId || !metadataLoaded) {
    return 'wait';
  }
  if (!meetingFolderPath) {
    return 'no-audio';
  }
  return 'proceed';
}

/** Whether the automatic pass should be started, given the model/consent status. */
export function shouldRunAutoDiarization(status: { ready: boolean; autoRun: boolean }): boolean {
  return status.ready && status.autoRun;
}

/**
 * Classifies a `run_diarization_command` failure: `'lock-held'` when another meeting's pass
 * is already running process-wide (diarization is single-flight - see
 * `diarization::pipeline`), `'failed'` for everything else. The message text is the only
 * signal available - the command rejects with a plain string, not a typed error.
 */
export function classifyAutoDiarizationStartError(message: string): 'lock-held' | 'failed' {
  return /already (running|in progress)/i.test(message) ? 'lock-held' : 'failed';
}
