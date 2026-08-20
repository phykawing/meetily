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
