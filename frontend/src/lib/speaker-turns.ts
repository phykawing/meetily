/**
 * Shared Speaker Turn grouping for a meeting's transcript.
 *
 * Both the clipboard export (`transcript-export.ts`, phykawing/meetily#17) and the summary
 * prompt (`summary-transcript-payload.ts`, phykawing/meetily#18) render the same
 * conversation the same way: consecutive segments from one Speaker are collected under a
 * single `**Name:**` header, with a blank line between turns. This is the one place that
 * rule lives, so the two views cannot drift apart.
 *
 * Attribution is resolved only through `speakerNames` (the meeting's `speaker_label` ->
 * display name map from `get_meeting_speakers`), never from anything on the segment. A
 * meeting that was never diarized - or a segment a pass did not cover - has no entry and
 * is emitted as a bare timestamped line, exactly as before diarization existed.
 */

export interface SpeakerTurnSegment {
  /** Recording-relative start, in seconds. Absent on transcripts predating audio timing. */
  audio_start_time?: number;
  /** Wall-clock fallback label, used when `audio_start_time` is absent. */
  timestamp: string;
  text: string;
  /** The diarized Speaker's per-meeting cluster id, e.g. "speaker_00". */
  speaker_label?: string;
}

export function formatTurnTime(seconds: number | undefined, fallback: string): string {
  if (seconds === undefined) {
    return fallback;
  }
  const totalSecs = Math.floor(seconds);
  const mins = Math.floor(totalSecs / 60);
  const secs = totalSecs % 60;
  return `[${mins.toString().padStart(2, '0')}:${secs.toString().padStart(2, '0')}]`;
}

/**
 * Renders `segments` to lines grouped into Speaker Turns. `lineSuffix` is appended to each
 * content line (the export uses `"  "` for a Markdown hard break; the summary prompt uses
 * `""`). Header lines never carry the suffix. Returns the lines; the caller joins them.
 */
export function formatSpeakerTurns(
  segments: SpeakerTurnSegment[],
  speakerNames: Record<string, string>,
  lineSuffix = '',
): string[] {
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

    lines.push(`${formatTurnTime(segment.audio_start_time, segment.timestamp)} ${segment.text}${lineSuffix}`);
  });

  return lines;
}
