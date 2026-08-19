// src/audio/audio_source.rs
//
// The live "Audio Source" hint (see docs/adr/0001, docs/adr/0004): a cheap tag derived
// from mixer energy dominance while recording, tagging which capture stream a live
// transcript segment most likely came from. Distinct from the future diarized "Speaker"
// concept, which identifies a human voice rather than a capture stream.

/// Which capture stream dominated a transcript segment's mixed audio.
///
/// Known limitation: dominance is judged from raw window energy, but only the
/// microphone stream is loudness-normalized (EBU R128, see `audio_processing.rs`)
/// before this comparison - system audio is not. A quiet mic speaker against loud
/// non-speech system audio can therefore be misclassified. This is accepted rather
/// than fixed here because correcting it would mean running a second normalizer over
/// system audio just to feed this hint (touching the shared mixing/recording path is
/// out of scope for it); per docs/adr/0001 this is deliberately a cheap, approximate
/// hint that the post-meeting diarization pass overwrites with a real Speaker label.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AudioSource {
    Mic,
    System,
    /// Both streams contributed comparable energy - simultaneous speech, labelled
    /// honestly rather than guessed at (see docs/adr/0001).
    Mixed,
}

impl AudioSource {
    /// A source is only called dominant once it carries this share of the combined
    /// mic+system energy; otherwise the segment is classified `Mixed`.
    const DOMINANCE_THRESHOLD: f64 = 0.65;

    /// Classify a segment from its accumulated mic/system energy (e.g. summed
    /// mean-squared amplitude across the mixer windows that contributed to it).
    pub fn from_energy(mic_energy: f64, system_energy: f64) -> Self {
        let total = mic_energy + system_energy;
        if total <= f64::EPSILON {
            // No signal to judge dominance from - don't guess.
            return AudioSource::Mixed;
        }

        let mic_ratio = mic_energy / total;
        if mic_ratio >= Self::DOMINANCE_THRESHOLD {
            AudioSource::Mic
        } else if mic_ratio <= 1.0 - Self::DOMINANCE_THRESHOLD {
            AudioSource::System
        } else {
            AudioSource::Mixed
        }
    }

    /// The stored/wire form - matches the values `transcripts.speaker` documents
    /// ('mic', 'system'), extended with 'mixed' per docs/adr/0004.
    pub fn as_str(&self) -> &'static str {
        match self {
            AudioSource::Mic => "mic",
            AudioSource::System => "system",
            AudioSource::Mixed => "mixed",
        }
    }
}

impl std::fmt::Display for AudioSource {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_str())
    }
}

/// Mean-squared amplitude of a window - proportional to energy for equal-length windows,
/// cheap enough to compute per mixer window with no measurable latency impact.
pub fn window_energy(window: &[f32]) -> f64 {
    if window.is_empty() {
        return 0.0;
    }
    window.iter().map(|&s| (s as f64) * (s as f64)).sum::<f64>() / window.len() as f64
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn mic_dominant_energy_is_tagged_mic() {
        assert_eq!(AudioSource::from_energy(0.9, 0.1), AudioSource::Mic);
    }

    #[test]
    fn system_dominant_energy_is_tagged_system() {
        assert_eq!(AudioSource::from_energy(0.1, 0.9), AudioSource::System);
    }

    #[test]
    fn comparable_energy_is_tagged_mixed_not_guessed() {
        assert_eq!(AudioSource::from_energy(0.5, 0.5), AudioSource::Mixed);
        assert_eq!(AudioSource::from_energy(0.55, 0.45), AudioSource::Mixed);
    }

    #[test]
    fn silence_defaults_to_mixed_rather_than_a_guess() {
        assert_eq!(AudioSource::from_energy(0.0, 0.0), AudioSource::Mixed);
    }

    #[test]
    fn dominance_threshold_boundary() {
        // Exactly at the threshold counts as dominant.
        assert_eq!(AudioSource::from_energy(0.65, 0.35), AudioSource::Mic);
        assert_eq!(AudioSource::from_energy(0.35, 0.65), AudioSource::System);
        // Just under it does not.
        assert_eq!(AudioSource::from_energy(0.64, 0.36), AudioSource::Mixed);
    }

    #[test]
    fn as_str_matches_documented_speaker_column_values() {
        assert_eq!(AudioSource::Mic.as_str(), "mic");
        assert_eq!(AudioSource::System.as_str(), "system");
        assert_eq!(AudioSource::Mixed.as_str(), "mixed");
    }

    #[test]
    fn window_energy_of_silence_is_zero() {
        assert_eq!(window_energy(&[0.0; 100]), 0.0);
    }

    #[test]
    fn window_energy_of_empty_window_is_zero() {
        assert_eq!(window_energy(&[]), 0.0);
    }

    #[test]
    fn window_energy_scales_with_amplitude() {
        let quiet = window_energy(&[0.1; 100]);
        let loud = window_energy(&[0.5; 100]);
        assert!(loud > quiet);
    }
}
