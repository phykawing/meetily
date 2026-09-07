// Explains why Cantonese is unavailable for the currently selected transcription model, so
// every language picker (settings, import, re-transcribe) states the same reason instead of
// each inventing its own wording. Mirrors the capability rule in
// frontend/src-tauri/src/whisper_engine/language.rs::is_cantonese_capable_builtin.
export function cantoneseUnavailableReason(params: {
  isParakeet: boolean;
  modelName?: string;
  supportsCantonese?: boolean;
}): string | null {
  const { isParakeet, modelName, supportsCantonese } = params;

  if (isParakeet) {
    return "Parakeet doesn't support manual language selection, so Cantonese isn't available.";
  }
  if (supportsCantonese) {
    return null;
  }
  return modelName
    ? `'${modelName}' isn't Cantonese-capable. Load a large-v3 model, or a registered Cantonese-capable model.`
    : 'Load a large-v3 model, or a registered Cantonese-capable model, to use Cantonese.';
}

// Whether we know enough yet to act on Cantonese capability — specifically, whether it is
// safe to reset a saved 'yue' selection because the model can't serve it. The provider, the
// model name and the Whisper model list all arrive asynchronously (the provider even
// defaults to Parakeet before the saved config lands), so acting before they settle
// discards the user's choice on a guess. cantoneseUnavailableReason() stays free to return
// a reason while this is false — showing a disabled option early is fine; resetting is not.
export function cantoneseCapabilityKnown(params: {
  configLoaded: boolean;
  isParakeet: boolean;
  modelsLoaded: boolean;
  modelName?: string;
}): boolean {
  const { configLoaded, isParakeet, modelsLoaded, modelName } = params;

  // The provider itself arrives asynchronously and defaults to Parakeet, so nothing is
  // known until the saved transcript config has landed.
  if (!configLoaded) return false;
  // Parakeet's answer doesn't depend on the Whisper model list.
  if (isParakeet) return true;
  // With no model configured there is nothing to judge: the backend reports "No model
  // loaded" regardless, so resetting the language achieves nothing and only discards the
  // user's choice.
  return modelsLoaded && Boolean(modelName);
}
