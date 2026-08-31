/**
 * Formats a meeting's transcript for export (clipboard copy today), prefixing each
 * Speaker Turn with the assigned display name so a shared transcript carries attribution
 * (phykawing/meetily#17, docs/adr/0001).
 *
 * Attribution is resolved through `speakerNames` (the meeting's `speaker_label` -> display
 * name map from `get_meeting_speakers`), never from anything on the segment itself. A
 * meeting that has not been diarized - or a segment a pass did not cover - has no entry in
 * the map and is emitted exactly as before diarization existed: a bare timestamped line.
 */

export interface ExportSegment {
  /** Recording-relative start, in seconds. Absent on transcripts predating audio timing. */
  audio_start_time?: number;
  /** Wall-clock fallback label, used when `audio_start_time` is absent. */
  timestamp: string;
  text: string;
  /** The diarized Speaker's per-meeting cluster id, e.g. "speaker_00". */
  speaker_label?: string;
}

function formatTime(seconds: number | undefined, fallback: string): string {
  if (seconds === undefined) {
    return fallback;
  }
  const totalSecs = Math.floor(seconds);
  const mins = Math.floor(totalSecs / 60);
  const secs = totalSecs % 60;
  return `[${mins.toString().padStart(2, '0')}:${secs.toString().padStart(2, '0')}]`;
}

export function formatTranscriptForExport(
  segments: ExportSegment[],
  speakerNames: Record<string, string>,
): string {
  const lines: string[] = [];
  // The label of the turn currently being emitted. `undefined` means the previous line
  // was unattributed, so the next attributed segment - whatever its label - opens a fresh
  // turn header.
  let currentLabel: string | undefined;

  segments.forEach((segment, index) => {
    const name = segment.speaker_label ? speakerNames[segment.speaker_label] : undefined;

    if (name) {
      if (segment.speaker_label !== currentLabel) {
        if (index > 0) {
          lines.push('');
        }
        lines.push(`**${name}:**`);
        currentLabel = segment.speaker_label;
      }
    } else {
      currentLabel = undefined;
    }

    // Two trailing spaces: a Markdown hard line break, matching the existing copy format.
    lines.push(`${formatTime(segment.audio_start_time, segment.timestamp)} ${segment.text}  `);
  });

  return lines.join('\n');
}
