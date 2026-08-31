// Plain (non-strict) assert: the module under test builds its result objects in a
// separate VM realm (see _load-ts-module.mjs), so `deepStrictEqual` rejects them for a
// cross-realm prototype mismatch even when the structure is identical. `deepEqual` compares
// by structure only, which is what we want here.
import assert from 'node:assert';
import { loadTsModule } from './_load-ts-module.mjs';

const { parseExpectedSpeakerCount } = loadTsModule(
  new URL('../../src/lib/expected-speaker-count.ts', import.meta.url)
);

// The hint is optional: an empty (or whitespace-only) field means "detect automatically",
// which is a valid state, not an error.
assert.deepEqual(parseExpectedSpeakerCount(''), { kind: 'empty' });
assert.deepEqual(parseExpectedSpeakerCount('   '), { kind: 'empty' });

// A plausible count is accepted and surfaced as a number.
assert.deepEqual(parseExpectedSpeakerCount('1'), { kind: 'valid', count: 1 });
assert.deepEqual(parseExpectedSpeakerCount('3'), { kind: 'valid', count: 3 });
assert.deepEqual(parseExpectedSpeakerCount('100'), { kind: 'valid', count: 100 });
assert.deepEqual(parseExpectedSpeakerCount('  4 '), { kind: 'valid', count: 4 }, 'surrounding whitespace is trimmed');
assert.deepEqual(parseExpectedSpeakerCount('02'), { kind: 'valid', count: 2 }, 'a leading zero is still a plain integer');

// Zero and out-of-range values are invalid - they match the backend guard, which would
// discard them anyway.
assert.deepEqual(parseExpectedSpeakerCount('0'), { kind: 'invalid' });
assert.deepEqual(parseExpectedSpeakerCount('101'), { kind: 'invalid' });
assert.deepEqual(parseExpectedSpeakerCount('999'), { kind: 'invalid' });

// Non-integers and non-numbers are invalid.
assert.deepEqual(parseExpectedSpeakerCount('2.5'), { kind: 'invalid' });
assert.deepEqual(parseExpectedSpeakerCount('-2'), { kind: 'invalid' });
assert.deepEqual(parseExpectedSpeakerCount('two'), { kind: 'invalid' });
assert.deepEqual(parseExpectedSpeakerCount('1e2'), { kind: 'invalid' });
assert.deepEqual(parseExpectedSpeakerCount('3 people'), { kind: 'invalid' });

console.log('expected-speaker-count.test.mjs: all assertions passed');
