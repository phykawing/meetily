// models.rs
//
// The two ONNX models a diarization pass needs: a segmentation model (finds speaker turn
// boundaries) and a speaker-embedding model (clusters turns into distinct speakers). Both
// are ports of pyannote.audio checkpoints, but pyannote's own Hugging Face repositories
// gate access behind an account and a terms-acceptance click-through, which would fail an
// unattended in-app download. The k2-fsa/sherpa-onnx project (maintainer csukuangfj)
// publishes ungated ONNX conversions of the same checkpoints, from Hugging Face and from
// GitHub release assets — used here instead. See docs/adr/0005 and THIRD_PARTY_NOTICES.md
// for licensing/attribution.

/// One downloadable diarization model.
pub struct DiarizationModel {
    /// Stable identifier used in commands, events and the on-disk filename.
    pub id: &'static str,
    /// Human-readable name shown in the UI.
    pub display_name: &'static str,
    /// Direct download URL. Both sources here redirect to the actual bytes and are
    /// followed automatically by `reqwest`.
    pub url: &'static str,
    /// Filename written under the diarization models directory.
    pub filename: &'static str,
    /// Size in bytes at the time this manifest was written, used for the consent
    /// prompt's "approximate size" and as the expected size for validation.
    pub size_bytes: u64,
    /// A downloaded file smaller than this is treated as missing/corrupt (partial or
    /// failed download), so a retry re-fetches it instead of trusting a truncated file.
    pub min_valid_bytes: u64,
    /// SPDX-ish license identifier, for display and for THIRD_PARTY_NOTICES.md.
    pub license: &'static str,
}

pub const SEGMENTATION_MODEL: DiarizationModel = DiarizationModel {
    id: "segmentation",
    display_name: "Speaker Segmentation (pyannote segmentation-3.0)",
    url: "https://huggingface.co/csukuangfj/sherpa-onnx-pyannote-segmentation-3-0/resolve/main/model.onnx",
    filename: "segmentation-3.0.onnx",
    size_bytes: 5_992_913,
    min_valid_bytes: 5_000_000,
    license: "MIT",
};

pub const EMBEDDING_MODEL: DiarizationModel = DiarizationModel {
    id: "embedding",
    display_name: "Speaker Embedding (WeSpeaker ResNet34-LM, VoxCeleb)",
    url: "https://github.com/k2-fsa/sherpa-onnx/releases/download/speaker-recongition-models/wespeaker_en_voxceleb_resnet34_LM.onnx",
    filename: "wespeaker-voxceleb-resnet34-lm.onnx",
    size_bytes: 26_530_550,
    min_valid_bytes: 20_000_000,
    license: "CC-BY-4.0",
};

/// All models a diarization pass needs. Downloaded and validated together.
pub const DIARIZATION_MODELS: &[DiarizationModel] = &[SEGMENTATION_MODEL, EMBEDDING_MODEL];

/// Total size of all models, for the consent prompt.
pub fn total_size_bytes() -> u64 {
    DIARIZATION_MODELS.iter().map(|m| m.size_bytes).sum()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn total_size_is_the_sum_of_each_model() {
        let expected: u64 = DIARIZATION_MODELS.iter().map(|m| m.size_bytes).sum();
        assert_eq!(total_size_bytes(), expected);
        assert!(total_size_bytes() > 0);
    }

    #[test]
    fn every_model_has_a_sane_min_valid_threshold() {
        for model in DIARIZATION_MODELS {
            assert!(model.min_valid_bytes > 0);
            assert!(model.min_valid_bytes <= model.size_bytes);
        }
    }
}
