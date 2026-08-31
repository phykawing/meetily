/**
 * Parses the free-text "expected number of speakers" field from the speaker-detection
 * dialog (phykawing/meetily#19). The hint is always optional, so an empty field is a
 * first-class valid state ("detect automatically") and is kept distinct from a non-empty
 * field that does not name a plausible count.
 *
 * The field is a plain text input (not `<input type="number">`), so every keystroke -
 * including `'2.5'`, `'-1'`, `'1e2'` - reaches this function verbatim rather than being
 * silently coerced by the browser. A value the parser rejects lets the dialog block the
 * run and point at the field, rather than sending a number the backend would only discard.
 */

/** Smallest hint the field accepts. The backend treats `1` as "auto-detect" all the same. */
export const MIN_EXPECTED_SPEAKERS = 1;
/** Largest hint the field accepts, matching the backend's `num_clusters_for` guard. */
export const MAX_EXPECTED_SPEAKERS = 100;

export type ExpectedSpeakerCount =
  | { kind: 'empty' }
  | { kind: 'valid'; count: number }
  | { kind: 'invalid' };

export function parseExpectedSpeakerCount(raw: string): ExpectedSpeakerCount {
  const trimmed = raw.trim();
  if (trimmed === '') {
    return { kind: 'empty' };
  }
  // Digits only: rejects a leading `-`, a decimal point, exponent notation, thousands
  // separators and stray inner whitespace before `Number` ever sees it, so the result is
  // always a non-negative integer.
  if (!/^\d+$/.test(trimmed)) {
    return { kind: 'invalid' };
  }
  const count = Number(trimmed);
  if (count < MIN_EXPECTED_SPEAKERS || count > MAX_EXPECTED_SPEAKERS) {
    return { kind: 'invalid' };
  }
  return { kind: 'valid', count };
}
