import assert from 'node:assert/strict';
import { loadTsModule } from './_load-ts-module.mjs';

const { buildSummaryTranscriptPayload } = loadTsModule(
  new URL('../../src/lib/summary-transcript-payload.ts', import.meta.url)
);

const seg = (text, start, speaker_label) => ({
  text,
  audio_start_time: start,
  timestamp: 'wall-clock',
  speaker_label,
});

// A meeting that was never diarized: the prompt is exactly the pre-diarization format,
// so summaries for undiarized meetings do not change at all.
{
  const { transcriptText, transcriptTexts } = buildSummaryTranscriptPayload(
    [seg('hello', 0), seg('there', 3)],
    {}
  );
  assert.equal(
    transcriptText,
    '[00:00] hello\n[00:03] there',
    'undiarized transcript builds exactly as before diarization existed'
  );
  assert.deepEqual(transcriptTexts, ['hello', 'there'], 'raw texts are the bare segment text');
}

// An empty speakerNames map means no attribution at all.
assert.equal(
  buildSummaryTranscriptPayload([seg('solo', 0, 'speaker_00')], {}).transcriptText,
  '[00:00] solo',
  'an empty speakerNames map is treated as no diarization'
);

// Names resolve through the map and prefix each turn once.
assert.equal(
  buildSummaryTranscriptPayload(
    [
      seg('morning all', 0, 'speaker_00'),
      seg('lets start', 4, 'speaker_00'),
      seg('one sec', 9, 'speaker_01'),
    ],
    { speaker_00: 'Alice', speaker_01: 'Bob' }
  ).transcriptText,
  '**Alice:**\n[00:00] morning all\n[00:04] lets start\n\n**Bob:**\n[00:09] one sec',
  'each turn is prefixed once, blank line between turns'
);

// The same speaker returning later opens a new turn header.
assert.equal(
  buildSummaryTranscriptPayload(
    [seg('a', 0, 'speaker_00'), seg('b', 2, 'speaker_01'), seg('c', 4, 'speaker_00')],
    { speaker_00: 'Alice', speaker_01: 'Bob' }
  ).transcriptText,
  '**Alice:**\n[00:00] a\n\n**Bob:**\n[00:02] b\n\n**Alice:**\n[00:04] c',
  'a speaker returning after another speaker gets a fresh header'
);

// A renamed speaker: the map is the single source of truth.
assert.equal(
  buildSummaryTranscriptPayload([seg('hi', 0, 'speaker_00')], { speaker_00: 'Dr. Chan' })
    .transcriptText,
  '**Dr. Chan:**\n[00:00] hi',
  'a user-assigned name is carried into the prompt verbatim'
);

// A label with no resolved name (segment not covered by the pass) stays unattributed and
// does not swallow the following attributed turn.
assert.equal(
  buildSummaryTranscriptPayload(
    [seg('uncovered', 0, 'speaker_07'), seg('covered', 3, 'speaker_00')],
    { speaker_00: 'Alice' }
  ).transcriptText,
  '[00:00] uncovered\n\n**Alice:**\n[00:03] covered',
  'a label with no resolved name is emitted as a bare line'
);

// transcriptTexts is never decorated with names - language detection must not see them.
assert.deepEqual(
  buildSummaryTranscriptPayload(
    [seg('bonjour', 0, 'speaker_00'), seg('hello', 2, 'speaker_01')],
    { speaker_00: 'Alice', speaker_01: 'Bob' }
  ).transcriptTexts,
  ['bonjour', 'hello'],
  'raw texts stay clean for language detection even when diarized'
);

// Missing audio timing falls back to the wall-clock timestamp string.
assert.equal(
  buildSummaryTranscriptPayload([{ text: 'legacy', timestamp: '3:04 PM' }], {}).transcriptText,
  '3:04 PM legacy',
  'segments without audio timing fall back to the wall-clock label'
);

console.log('summary-transcript-payload.test.mjs: all assertions passed');
