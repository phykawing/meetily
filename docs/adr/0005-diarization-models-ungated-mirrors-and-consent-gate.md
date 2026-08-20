# Diarization models come from ungated mirrors, behind an enforced consent gate

Speaker diarization (#3, ADR-0001) needs a segmentation model and a speaker-embedding
model. The natural choices are pyannote.audio's `segmentation-3.0` and
`wespeaker-voxceleb-resnet34-LM` — but both canonical Hugging Face repositories are
gated (`"gated": "auto"`): fetching them requires a logged-in account that has clicked
through pyannote's terms, which an unattended in-app download cannot do. `k2-fsa/sherpa-onnx`
(maintainer `csukuangfj`) publishes ungated ONNX conversions of the same checkpoints —
one on Hugging Face, one as a GitHub release asset — used here instead:

- **Segmentation**: `https://huggingface.co/csukuangfj/sherpa-onnx-pyannote-segmentation-3-0/resolve/main/model.onnx`
  (~5.7 MB, MIT — confirmed from the mirror's own `LICENSE` file).
- **Embedding**: `https://github.com/k2-fsa/sherpa-onnx/releases/download/speaker-recongition-models/wespeaker_en_voxceleb_resnet34_LM.onnx`
  (~25.3 MB, CC-BY-4.0 — the ONNX port of `pyannote/wespeaker-voxceleb-resnet34-LM`,
  whose Hugging Face listing carries `license:cc-by-4.0`). Attribution recorded in
  `THIRD_PARTY_NOTICES.md`.

Both URLs were verified to return `200` with a real `Content-Length` before being
hardcoded (see `frontend/src-tauri/src/diarization/models.rs`); those sizes are also
what the consent prompt shows as the approximate download size.

## Consent is an enforced gate, not just a UI convention

`diarization::commands::download_diarization_models` checks the persisted consent state
itself and refuses to run unless it is `Granted` — the frontend cannot bypass this by
skipping a confirmation dialog. Consent is tri-state (`not_asked` / `granted` /
`declined`), stored in the existing `settings` table (`diarizationConsent`, nullable —
`NULL` means not asked). `declined` is recorded explicitly rather than left as
`not_asked`: #18 (automatic detection on recording stop) will run this pass after every
meeting, so an unremembered "no" would re-prompt after each one.

This issue (#10) only builds the consent gate and the download itself — no code path yet
calls `download_diarization_models` or checks `diarization_model_status().ready`. #16 (the
actual diarization pass) is blocked on this issue specifically so it can consume that
`ready` flag as its single readiness check instead of reassembling it.

## Consequences

- Declining, or never being asked, cannot affect recording, transcription or
  summarisation: this module has no call sites anywhere else in the app yet, and the
  download command's own consent check is the only thing that can start a fetch.
- A model file already valid on disk (passes `min_valid_bytes`) is skipped on the next
  download call, so retrying after a failed/partial download only re-fetches what did not
  complete — no separate "retry" code path is needed.
- Pinning literal byte sizes in the manifest means a future upstream re-upload that
  changes file size would need the manifest updated too; this was accepted as simpler
  than fetching size dynamically for a prompt that only needs to be approximately right.
