import assert from 'node:assert/strict';
import { loadTsModule } from './_load-ts-module.mjs';

const { autoDiarizationWaitState, shouldRunAutoDiarization, classifyAutoDiarizationStartError } =
  loadTsModule(new URL('../../src/lib/auto-diarization.ts', import.meta.url));

// autoDiarizationWaitState ----------------------------------------------------

assert.equal(autoDiarizationWaitState(null, false, null), 'wait', 'no meeting id yet: keep waiting');
assert.equal(autoDiarizationWaitState(null, true, '/path'), 'wait', 'no meeting id even if metadata says loaded');
assert.equal(autoDiarizationWaitState('m1', false, '/path'), 'wait', 'metadata not loaded yet: keep waiting');
assert.equal(
  autoDiarizationWaitState('m1', true, null),
  'no-audio',
  'metadata loaded but no folder path: nothing to diarize'
);
assert.equal(
  autoDiarizationWaitState('m1', true, undefined),
  'no-audio',
  'undefined folder path is treated the same as null'
);
assert.equal(
  autoDiarizationWaitState('m1', true, '/path/to/meeting'),
  'proceed',
  'id, loaded metadata, and a folder path: safe to proceed'
);

// shouldRunAutoDiarization -----------------------------------------------------

assert.equal(shouldRunAutoDiarization({ ready: true, autoRun: true }), true, 'ready and auto-run enabled: run');
assert.equal(shouldRunAutoDiarization({ ready: false, autoRun: true }), false, 'not ready: never run');
assert.equal(
  shouldRunAutoDiarization({ ready: true, autoRun: false }),
  false,
  'ready but the user turned off the automatic pass: do not run'
);
assert.equal(shouldRunAutoDiarization({ ready: false, autoRun: false }), false, 'neither ready nor auto-run: do not run');

// classifyAutoDiarizationStartError --------------------------------------------

assert.equal(
  classifyAutoDiarizationStartError('Speaker detection is already running'),
  'lock-held',
  'the single-flight lock message is recognized regardless of exact wording after "already"'
);
assert.equal(
  classifyAutoDiarizationStartError('Diarization already in progress'),
  'lock-held',
  '"already in progress" phrasing is also recognized as a held lock'
);
assert.equal(
  classifyAutoDiarizationStartError('Retranscription can\'t run while speaker detection is in progress'),
  'failed',
  '"in progress" without "already" immediately before it does not match - this message is a real failure to surface, not a lock to silently skip'
);
assert.equal(
  classifyAutoDiarizationStartError('SPEAKER DETECTION IS ALREADY RUNNING'),
  'lock-held',
  'matching is case-insensitive'
);
assert.equal(
  classifyAutoDiarizationStartError('Speaker detection isn\'t enabled yet. Enable it under Settings > Preferences first.'),
  'failed',
  'a consent/setup error is a real failure, not a held lock'
);
assert.equal(
  classifyAutoDiarizationStartError('Network error'),
  'failed',
  'an unrelated error message is a real failure'
);

console.log('auto-diarization.test.mjs: all assertions passed');
