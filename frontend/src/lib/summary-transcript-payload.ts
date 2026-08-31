/**
 * Builds the transcript payload handed to summary generation
 * (`useSummaryGeneration.processSummary`).
 *
 * Two outputs, deliberately different:
 *
 * - `transcriptText` is the prompt the LLM summarises. Each Speaker Turn is prefixed once
 *   with the assigned display name so the summary can attribute action items to a person
 *   instead of being speaker-blind (phykawing/meetily#18, docs/adr/0001). The turn-grouping
 *   is shared with the transcript export (`speaker-turns.ts`), so a summary prompt and an
 *   exported transcript attribute the same conversation the same way. A meeting that was
 *   never diarized - or a segment a pass did not cover - has no entry in `speakerNames` and
 *   is emitted as a bare timestamped line, byte-for-byte as before diarization existed.
 *
 * - `transcriptTexts` is the raw per-segment text, unprefixed, used for summary-language
 *   detection. Speaker names must not leak into language detection, so this is never
 *   decorated.
 */

import { formatSpeakerTurns, SpeakerTurnSegment } from '@/lib/speaker-turns';

export type SummaryPayloadSegment = SpeakerTurnSegment;

export interface SummaryTranscriptPayload {
  transcriptText: string;
  transcriptTexts: string[];
}

export function buildSummaryTranscriptPayload(
  segments: SummaryPayloadSegment[],
  speakerNames: Record<string, string>,
): SummaryTranscriptPayload {
  return {
    transcriptText: formatSpeakerTurns(segments, speakerNames).join('\n'),
    transcriptTexts: segments.map((s) => s.text),
  };
}
