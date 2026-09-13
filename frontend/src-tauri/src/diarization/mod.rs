// diarization/
//
// Speaker diarization (issue #3): a consent-gated model download (docs/adr/0005,
// ADR-0001) plus the post-meeting pass itself. `consent`/`manager` answer "do we have
// consent, and are the models on disk" and perform the download once consent is granted;
// `pipeline` runs the actual sherpa-onnx inference and clustering (docs/adr/0007) and
// `alignment` maps the resulting speaker turns onto transcript chunks. `commands` is the
// Tauri surface for all of the above, including per-meeting speaker renaming.

pub mod alignment;
pub mod commands;
pub mod consent;
pub mod manager;
pub mod models;
pub mod pipeline;
