// consent.rs
//
// Speaker-diarization models are downloaded on demand, not bundled and not fetched
// automatically. The user must explicitly answer a prompt stating the approximate
// download size before anything is fetched — see docs/adr/0005 and ADR-0001.

use crate::database::repositories::setting_store::SettingToken;

/// Whether the user has answered the diarization model-download prompt.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DiarizationConsent {
    /// The prompt has not been shown/answered yet. The default — downloads must not
    /// start in this state.
    NotAsked,
    /// The user agreed to download the models.
    Granted,
    /// The user declined. Recording, transcription and summarisation are unaffected;
    /// only the diarization pass stays unavailable.
    Declined,
}

impl SettingToken for DiarizationConsent {
    const TOKENS: &'static [(&'static str, Self)] = &[
        ("not_asked", DiarizationConsent::NotAsked),
        ("granted", DiarizationConsent::Granted),
        ("declined", DiarizationConsent::Declined),
    ];
}

impl DiarizationConsent {
    /// Token stored in the database.
    pub fn as_str(self) -> &'static str {
        <Self as SettingToken>::as_token(self)
    }

    /// Resolves a stored token into a consent state. Unrecognized or absent values fall
    /// back to `NotAsked` — the safe default, since an unrecognized value must never be
    /// treated as consent to download.
    pub fn from_stored(token: Option<&str>) -> Self {
        <Self as SettingToken>::from_token(token)
    }
}

impl Default for DiarizationConsent {
    fn default() -> Self {
        DiarizationConsent::NotAsked
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn round_trips_through_stored_tokens() {
        for consent in [
            DiarizationConsent::NotAsked,
            DiarizationConsent::Granted,
            DiarizationConsent::Declined,
        ] {
            assert_eq!(
                DiarizationConsent::from_stored(Some(consent.as_str())),
                consent
            );
        }
    }

    #[test]
    fn unset_or_unrecognized_tokens_default_to_not_asked() {
        assert_eq!(DiarizationConsent::from_stored(None), DiarizationConsent::NotAsked);
        assert_eq!(
            DiarizationConsent::from_stored(Some("garbage")),
            DiarizationConsent::NotAsked
        );
    }
}
