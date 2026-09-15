import assert from 'node:assert/strict';
import { loadTsModule } from './_load-ts-module.mjs';

const { formatRenderedTurnsForCopy } = loadTsModule(
  new URL('../../src/lib/rendered-turns.ts', import.meta.url)
);

const turn = (text, speaker_label) => ({ text, speaker_label });

// A meeting that was never diarized, or a Rendering cached before turn markers existed:
// one unlabeled turn, copied out exactly as the flat text always was.
assert.equal(
  formatRenderedTurnsForCopy([turn('是這樣的')], {}),
  '是這樣的',
  'a single unlabeled turn copies out with no header'
);

// Multiple turns with names resolved through the map, blank line between turns.
assert.equal(
  formatRenderedTurnsForCopy(
    [turn('早晨大家', 'speaker_00'), turn('我哋開始啦', 'speaker_01')],
    { speaker_00: 'Alice', speaker_01: 'Bob' }
  ),
  '**Alice:**\n早晨大家\n\n**Bob:**\n我哋開始啦',
  'each turn gets a header from the resolved name, separated by a blank line'
);

// A label with no resolved name (not covered by a rename, or the meeting was never
// diarized for that turn) stays unattributed rather than showing a raw label.
assert.equal(
  formatRenderedTurnsForCopy([turn('uncovered', 'speaker_07')], {}),
  'uncovered',
  'an unresolved label is emitted as a bare turn'
);

// A renamed speaker: the map is the single source of truth, so the new name flows through
// without needing the cached Rendering to change.
assert.equal(
  formatRenderedTurnsForCopy([turn('多謝晒', 'speaker_00')], { speaker_00: 'Dr. Chan' }),
  '**Dr. Chan:**\n多謝晒',
  'a user-assigned name is carried into the copy verbatim'
);

// No turns at all copies out as an empty string rather than throwing.
assert.equal(formatRenderedTurnsForCopy([], {}), '', 'no turns copies out empty');

console.log('rendered-turns.test.mjs: all assertions passed');
