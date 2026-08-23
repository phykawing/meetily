// diarization/
//
// The consent-gated model download for speaker diarization (docs/adr/0005, ADR-0001).
// The diarization pass itself — alignment, clustering, the post-meeting UI — is separate
// future work; this module only answers "do we have consent, and are the models on
// disk," and performs the download once consent is granted.

pub mod alignment;
pub mod commands;
pub mod consent;
pub mod manager;
pub mod models;
pub mod pipeline;
