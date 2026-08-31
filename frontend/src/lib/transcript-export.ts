/**
 * Formats a meeting's transcript for export (clipboard copy today), prefixing each
 * Speaker Turn with the assigned display name so a shared transcript carries attribution
 * (phykawing/meetily#17, docs/adr/0001).
 *
 * The turn-grouping itself lives in `speaker-turns.ts` and is shared with the summary
 * prompt builder so the two views attribute a conversation identically. This module only
 * pins the export-specific formatting: a Markdown hard line break (two trailing spaces) on
 * every content line, lines joined by newlines.
 */

import { formatSpeakerTurns, SpeakerTurnSegment } from '@/lib/speaker-turns';

export type ExportSegment = SpeakerTurnSegment;

export function formatTranscriptForExport(
  segments: ExportSegment[],
  speakerNames: Record<string, string>,
): string {
  return formatSpeakerTurns(segments, speakerNames, '  ').join('\n');
}
