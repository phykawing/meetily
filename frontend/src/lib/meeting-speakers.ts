import { invoke } from '@tauri-apps/api/core';

/**
 * Fetches a meeting's discovered speakers as a `speaker_label` -> display name map
 * (phykawing/meetily#16, #17). Empty for a meeting that has not been diarized yet.
 *
 * Shared by every place that needs to resolve a transcript row's `speaker_label` into a
 * human-readable name - the transcript panel and the clipboard export - so the shape of
 * the `get_meeting_speakers` result is transformed in exactly one place.
 */
export async function fetchSpeakerNames(meetingId: string): Promise<Record<string, string>> {
  const speakers = await invoke<{ label: string; name: string }[]>('get_meeting_speakers', { meetingId });
  return Object.fromEntries(speakers.map((s) => [s.label, s.name]));
}
