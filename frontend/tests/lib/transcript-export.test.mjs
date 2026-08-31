import assert from 'node:assert/strict';
import { loadTsModule } from './_load-ts-module.mjs';

const { formatTranscriptForExport } = loadTsModule(
  new URL('../../src/lib/transcript-export.ts', import.meta.url)
);

const seg = (text, start, speaker_label) => ({
  text,
  audio_start_time: start,
  timestamp: 'wall-clock',
  speaker_label,
});

// A meeting that was never diarized: no map entries, output is bare timestamped lines.
assert.equal(
  formatTranscriptForExport([seg('hello', 0), seg('there', 3)], {}),
  '[00:00] hello  \n[00:03] there  ',
  'undiarized transcript exports exactly as before diarization existed'
);

// Names are resolved through the map and prefix each turn once.
assert.equal(
  formatTranscriptForExport(
    [
      seg('morning all', 0, 'speaker_00'),
      seg('lets start', 4, 'speaker_00'),
      seg('one sec', 9, 'speaker_01'),
    ],
    { speaker_00: 'Alice', speaker_01: 'Bob' }
  ),
  '**Alice:**\n[00:00] morning all  \n[00:04] lets start  \n\n**Bob:**\n[00:09] one sec  ',
  'each turn is prefixed with the assigned name once, with a blank line between turns'
);

// The same speaker returning later opens a new turn header.
assert.equal(
  formatTranscriptForExport(
    [
      seg('a', 0, 'speaker_00'),
      seg('b', 2, 'speaker_01'),
      seg('c', 4, 'speaker_00'),
    ],
    { speaker_00: 'Alice', speaker_01: 'Bob' }
  ),
  '**Alice:**\n[00:00] a  \n\n**Bob:**\n[00:02] b  \n\n**Alice:**\n[00:04] c  ',
  'a speaker returning after another speaker gets a fresh header'
);

// A renamed speaker: the map is the single source of truth, so the new name flows through.
assert.equal(
  formatTranscriptForExport([seg('hi', 0, 'speaker_00')], { speaker_00: 'Dr. Chan' }),
  '**Dr. Chan:**\n[00:00] hi  ',
  'a user-assigned name is carried into the export verbatim'
);

// A segment whose label has no name (e.g. not covered by the pass) stays unattributed and
// does not swallow the following attributed turn.
assert.equal(
  formatTranscriptForExport(
    [seg('uncovered', 0, 'speaker_07'), seg('covered', 3, 'speaker_00')],
    { speaker_00: 'Alice' }
  ),
  '[00:00] uncovered  \n\n**Alice:**\n[00:03] covered  ',
  'a label with no resolved name is emitted as a bare line'
);

// Missing audio timing falls back to the wall-clock timestamp string.
assert.equal(
  formatTranscriptForExport([{ text: 'legacy', timestamp: '3:04 PM' }], {}),
  '3:04 PM legacy  ',
  'segments without audio timing fall back to the wall-clock label'
);

console.log('transcript-export.test.mjs: all assertions passed');
