import assert from 'node:assert/strict';
import { loadTsModule } from './_load-ts-module.mjs';

/**
 * Direct coverage for `speaker-turns.ts` itself (phykawing/meetily#34 item 4) - both
 * `transcript-export.test.mjs` and `summary-transcript-payload.test.mjs` already exercise
 * `formatSpeakerTurns` thoroughly through their two callers, so this focuses on what those
 * don't: `formatTurnTime` in isolation, and `formatSpeakerTurns`'s own edge cases (empty
 * input, a single segment, no `speakerNames` entries at all).
 */
const { formatTurnTime, formatSpeakerTurns } = loadTsModule(
  new URL('../../src/lib/speaker-turns.ts', import.meta.url)
);

// formatTurnTime -------------------------------------------------------------

assert.equal(formatTurnTime(undefined, '3:04 PM'), '3:04 PM', 'no audio_start_time falls back to the wall-clock label');
assert.equal(formatTurnTime(0, 'fallback'), '[00:00]', 'zero seconds is a real timestamp, not treated as missing');
assert.equal(formatTurnTime(65, 'fallback'), '[01:05]', 'seconds beyond one minute roll over into minutes');
assert.equal(formatTurnTime(3661, 'fallback'), '[61:01]', 'minutes are not capped at 59 for long recordings');
assert.equal(formatTurnTime(59.9, 'fallback'), '[00:59]', 'fractional seconds are floored, not rounded');

// formatSpeakerTurns -----------------------------------------------------------
//
// Compared via `.join('\n')` rather than `assert.deepEqual` on the raw array: the array
// `formatSpeakerTurns` builds is constructed fresh inside the VM-compiled module, so it
// belongs to that module's realm - structurally identical but not `deepStrictEqual` to a
// same-shaped array literal from this file's realm. `transcript-export.ts` and
// `summary-transcript-payload.ts` both join before returning to their callers, so this
// mirrors how the function is actually consumed everywhere in the app.

const seg = (text, start, speaker_label) => ({ text, audio_start_time: start, timestamp: 'wall-clock', speaker_label });

assert.equal(formatSpeakerTurns([], {}).join('\n'), '', 'no segments produces no lines');

assert.equal(
  formatSpeakerTurns([seg('solo line', 0)], {}).join('\n'),
  '[00:00] solo line',
  'a single unattributed segment is one bare line with no header'
);

assert.equal(
  formatSpeakerTurns([seg('hi', 0, 'speaker_00')], {}).join('\n'),
  '[00:00] hi',
  'a labeled segment with no matching entry in speakerNames stays unattributed'
);

assert.equal(
  formatSpeakerTurns([seg('hi', 0, 'speaker_00')], { speaker_00: 'Alice' }, '  ').join('\n'),
  '**Alice:**\n[00:00] hi  ',
  'lineSuffix is appended to content lines but never to the header line'
);

console.log('speaker-turns.test.mjs: all assertions passed');
