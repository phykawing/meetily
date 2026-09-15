/**
 * A 書面語 Rendering, returned by `get_transcript_rendering` as Speaker Turns instead of a
 * flat string (phykawing/meetily#37) - each turn carries the diarized Speaker's per-meeting
 * `speaker_label`, never a display name, so a rename never invalidates the cached Rendering.
 * Resolved into a name only at render/copy time via `speakerNames`, exactly like the 口語
 * view's `formatSpeakerTurns` (`speaker-turns.ts`).
 */
export interface RenderedTurn {
  speaker_label?: string;
  text: string;
}

/**
 * Renders `turns` to clipboard text: a `**Name:**` header before each turn whose
 * `speaker_label` resolves to a name, blank lines between turns, and the turn's text as-is
 * otherwise. A meeting that was never diarized (every turn has no `speaker_label`) or whose
 * Rendering predates this feature (a single unlabeled turn) copies out exactly as before -
 * one turn, no header.
 */
export function formatRenderedTurnsForCopy(
  turns: RenderedTurn[],
  speakerNames: Record<string, string>
): string {
  const blocks = turns.map((turn) => {
    const name = turn.speaker_label ? speakerNames[turn.speaker_label] : undefined;
    return name ? `**${name}:**\n${turn.text}` : turn.text;
  });
  return blocks.join('\n\n');
}
