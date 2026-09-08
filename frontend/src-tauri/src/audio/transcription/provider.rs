// audio/transcription/provider.rs
//
// Defines the unified TranscriptionProvider trait and common types for all
// transcription engines (Whisper, Parakeet, future providers).

use async_trait::async_trait;

// ============================================================================
// TRANSCRIPTION PROVIDER TRAIT & ERROR TYPES
// ============================================================================

/// Granular error types for transcription operations
#[derive(Debug, Clone)]
pub enum TranscriptionError {
    ModelNotLoaded,
    AudioTooShort { samples: usize, minimum: usize },
    EngineFailed(String),
    /// The loaded model cannot decode the selected language. The string is the
    /// ready-to-show, actionable message (it names the model and how to fix it), so
    /// `Display` renders it verbatim rather than re-wrapping it.
    UnsupportedLanguage(String),
}

impl std::fmt::Display for TranscriptionError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::ModelNotLoaded => write!(f, "No transcription model is loaded"),
            Self::AudioTooShort { samples, minimum } => write!(
                f,
                "Audio too short: {} samples (minimum {})",
                samples, minimum
            ),
            Self::EngineFailed(msg) => write!(f, "Transcription engine failed: {}", msg),
            Self::UnsupportedLanguage(message) => write!(f, "{}", message),
        }
    }
}

impl std::error::Error for TranscriptionError {}

impl TranscriptionError {
    /// Classify an `anyhow::Error` coming back from a Whisper engine call. A capability
    /// failure carries an
    /// [`UnsupportedLanguageError`](crate::whisper_engine::language::UnsupportedLanguageError)
    /// — recovered here so it surfaces as `UnsupportedLanguage` with `actionable: true`
    /// instead of a doubly-wrapped `EngineFailed` string.
    pub fn from_engine_error(e: anyhow::Error) -> Self {
        match e.downcast_ref::<crate::whisper_engine::language::UnsupportedLanguageError>() {
            Some(unsupported) => Self::UnsupportedLanguage(unsupported.to_string()),
            None => Self::EngineFailed(e.to_string()),
        }
    }

    /// Whether the user can do something about this (surfaced to the frontend as the
    /// `actionable` flag, which drives a modal + model picker rather than a transient
    /// toast).
    pub fn is_actionable(&self) -> bool {
        matches!(self, Self::UnsupportedLanguage(_))
    }
}

/// Unified transcription result across all providers
#[derive(Debug, Clone)]
pub struct TranscriptResult {
    pub text: String,
    pub confidence: Option<f32>, // None if provider doesn't support confidence scores
    pub is_partial: bool,
}

/// Trait for transcription providers (Whisper, Parakeet, future providers)
#[async_trait]
pub trait TranscriptionProvider: Send + Sync {
    /// Transcribe audio samples to text
    ///
    /// # Arguments
    /// * `audio` - Audio samples (16kHz mono, f32 format)
    /// * `language` - Optional language hint (e.g., "en", "es", "fr")
    ///
    /// # Returns
    /// * `TranscriptResult` with text, optional confidence, and partial flag
    async fn transcribe(
        &self,
        audio: Vec<f32>,
        language: Option<String>,
    ) -> std::result::Result<TranscriptResult, TranscriptionError>;

    /// Check if a model is currently loaded
    async fn is_model_loaded(&self) -> bool;

    /// Get the name of the currently loaded model
    async fn get_current_model(&self) -> Option<String>;

    /// Get the provider name (for logging/debugging)
    fn provider_name(&self) -> &'static str;
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::whisper_engine::language::UnsupportedLanguageError;

    #[test]
    fn capability_failure_becomes_an_actionable_unsupported_language_error() {
        let cause = UnsupportedLanguageError {
            ui_language: "yue".to_string(),
            model_name: "base".to_string(),
        };

        let classified = TranscriptionError::from_engine_error(anyhow::anyhow!(cause.clone()));

        assert!(matches!(
            classified,
            TranscriptionError::UnsupportedLanguage(_)
        ));
        assert!(classified.is_actionable());
        // The cause's actionable message reaches the user verbatim — no
        // "Transcription engine failed:" wrap.
        assert_eq!(classified.to_string(), cause.to_string());
    }

    #[test]
    fn other_engine_errors_stay_non_actionable_engine_failures() {
        let classified =
            TranscriptionError::from_engine_error(anyhow::anyhow!("GPU fell over"));

        assert!(matches!(classified, TranscriptionError::EngineFailed(_)));
        assert!(!classified.is_actionable());
        assert_eq!(
            classified.to_string(),
            "Transcription engine failed: GPU fell over"
        );
    }
}
