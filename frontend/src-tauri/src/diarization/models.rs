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

/// One downloadable diarization model. `Copy` since every field is a `&'static str` or
/// `u64` — lets a `spawn_blocking` checksum task take an owned copy instead of needing a
/// `'static` bound threaded through `download_model`'s signature.
#[derive(Clone, Copy)]
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
    /// Expected SHA-256 of the complete file, lowercase hex. Checked once, right after a
    /// fresh download completes (see `manager::download_model`) and before the `.part`
    /// file is renamed into place — a file that merely reaches `min_valid_bytes` is not
    /// proof it is byte-for-byte the model this app expects. Not re-checked by
    /// `is_downloaded`/`all_models_downloaded`: those run on every settings-panel open and
    /// status poll, and re-hashing tens of MB on every call would be wasted work against a
    /// file this process itself already validated and never modifies afterward.
    pub sha256: &'static str,
    /// SPDX-ish license identifier, for display and for THIRD_PARTY_NOTICES.md.
    pub license: &'static str,
}

// Both `sha256` values below were computed directly from a fresh download of `url`
// (`curl -sL <url> | sha256sum`) on 2026-09-15, for this issue (phykawing/meetily#34 item
// 1) — not copied from an upstream-published checksum, since neither source publishes one.
// `size_bytes` matched the downloaded file exactly at the same time, corroborating that the
// download was complete and not itself truncated before hashing.

pub const SEGMENTATION_MODEL: DiarizationModel = DiarizationModel {
    id: "segmentation",
    display_name: "Speaker Segmentation (pyannote segmentation-3.0)",
    url: "https://huggingface.co/csukuangfj/sherpa-onnx-pyannote-segmentation-3-0/resolve/main/model.onnx",
    filename: "segmentation-3.0.onnx",
    size_bytes: 5_992_913,
    min_valid_bytes: 5_000_000,
    sha256: "220ad67ca923bef2fa91f2390c786097bf305bceb5e261d4af67b38e938e1079",
    license: "MIT",
};

pub const EMBEDDING_MODEL: DiarizationModel = DiarizationModel {
    id: "embedding",
    display_name: "Speaker Embedding (WeSpeaker ResNet34-LM, VoxCeleb)",
    url: "https://github.com/k2-fsa/sherpa-onnx/releases/download/speaker-recongition-models/wespeaker_en_voxceleb_resnet34_LM.onnx",
    filename: "wespeaker-voxceleb-resnet34-lm.onnx",
    size_bytes: 26_530_550,
    min_valid_bytes: 20_000_000,
    sha256: "e9848563da86f263117134dfd7ad63c92355b37de492b55e325400c9d9c39012",
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

    #[test]
    fn every_model_has_a_well_formed_sha256() {
        for model in DIARIZATION_MODELS {
            assert_eq!(
                model.sha256.len(),
                64,
                "{} sha256 is not 64 hex chars",
                model.id
            );
            assert!(
                model.sha256.chars().all(|c| c.is_ascii_hexdigit() && !c.is_ascii_uppercase()),
                "{} sha256 must be lowercase hex",
                model.id
            );
        }
    }
}
