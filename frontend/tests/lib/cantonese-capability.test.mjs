import assert from 'node:assert/strict';
import { loadTsModule } from './_load-ts-module.mjs';

const { cantoneseUnavailableReason, cantoneseCapabilityKnown } = loadTsModule(
  new URL('../../src/lib/cantonese-capability.ts', import.meta.url)
);

// --- cantoneseCapabilityKnown -------------------------------------------------
// This gate exists so a saved 'yue' selection is never reset on a guess while the
// provider, model name and Whisper model list are still loading. Each of those
// inputs arrives asynchronously; capability is only "known" once they have.

// Config not loaded: nothing is known yet, even if the (default) provider looks
// like Parakeet — the real saved provider may still be localWhisper.
assert.equal(
  cantoneseCapabilityKnown({ configLoaded: false, isParakeet: true, modelsLoaded: true, modelName: 'x' }),
  false,
  'config not loaded is never known, even when isParakeet is true'
);
assert.equal(
  cantoneseCapabilityKnown({ configLoaded: false, isParakeet: false, modelsLoaded: true, modelName: 'large-v3' }),
  false
);

// Parakeet, once the config has landed: its answer does not depend on the Whisper
// model list, so it is known immediately.
assert.equal(
  cantoneseCapabilityKnown({ configLoaded: true, isParakeet: true, modelsLoaded: false, modelName: undefined }),
  true,
  'Parakeet after config load is known without the Whisper model list'
);

// Whisper, model list not back yet: not known.
assert.equal(
  cantoneseCapabilityKnown({ configLoaded: true, isParakeet: false, modelsLoaded: false, modelName: 'large-v3' }),
  false,
  'Whisper with the model list still loading is not known'
);

// Whisper, list back but no model configured: nothing to judge, and the backend
// reports "No model loaded" regardless — treat as known so we do not spin, but
// resetting is pointless (caller also guards on this).
assert.equal(
  cantoneseCapabilityKnown({ configLoaded: true, isParakeet: false, modelsLoaded: true, modelName: undefined }),
  false,
  'Whisper with no model name is not known'
);
assert.equal(
  cantoneseCapabilityKnown({ configLoaded: true, isParakeet: false, modelsLoaded: true, modelName: '' }),
  false,
  'an empty model name is not known'
);

// Whisper, fully loaded: known.
assert.equal(
  cantoneseCapabilityKnown({ configLoaded: true, isParakeet: false, modelsLoaded: true, modelName: 'large-v3-turbo' }),
  true,
  'fully-loaded Whisper is known'
);

// --- cantoneseUnavailableReason ---------------------------------------------
// Unchanged behaviour: it still returns a reason string while capability is
// unknown (a disabled option shown early is fine; only the *reset* is gated).

assert.equal(
  cantoneseUnavailableReason({ isParakeet: true }),
  "Parakeet doesn't support manual language selection, so Cantonese isn't available."
);
assert.equal(
  cantoneseUnavailableReason({ isParakeet: false, supportsCantonese: true }),
  null,
  'a Cantonese-capable model has no reason'
);
assert.equal(
  cantoneseUnavailableReason({ isParakeet: false, modelName: 'base', supportsCantonese: false }),
  "'base' isn't Cantonese-capable. Load a large-v3 model, or a registered Cantonese-capable model."
);
assert.match(
  cantoneseUnavailableReason({ isParakeet: false, supportsCantonese: false }) ?? '',
  /Load a large-v3 model/,
  'unknown model still yields a generic reason'
);

console.log('cantonese-capability.test.mjs: all assertions passed');
