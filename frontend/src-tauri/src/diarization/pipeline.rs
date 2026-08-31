// pipeline.rs
//
// Orchestrates a full diarization pass: decode the meeting's saved audio, run
// segmentation + embedding + clustering (sherpa-onnx, CPU-only - see docs/adr/0005 for
// where the models come from), align the resulting Speaker Turns onto the meeting's
// existing transcript chunks (`diarization::alignment`), and persist the result. Reads
// the same saved audio file a re-transcription pass would (`audio::retranscription`)
// rather than the live pipeline - see docs/adr/0001.
//
// ONNX inference, clustering accuracy, and the audio file itself are explicitly not
// unit-tested (issue #3's Testing Decisions) - the alignment math this module feeds is
// fully covered in `diarization::alignment`.

use crate::audio::decoder::decode_audio_file;
use crate::audio::retranscription::find_audio_file;
use crate::database::repositories::speaker::{SpeakerName, SpeakerRepository};
use crate::diarization::alignment::{align_chunks_to_turns, SpeakerTurn};
use crate::diarization::manager;
use crate::diarization::models::{EMBEDDING_MODEL, SEGMENTATION_MODEL};
use anyhow::{anyhow, Result};
use log::info;
use serde::{Deserialize, Serialize};
use sherpa_onnx::{
    FastClusteringConfig, OfflineSpeakerDiarization, OfflineSpeakerDiarizationConfig,
    OfflineSpeakerDiarizationSegment, OfflineSpeakerSegmentationModelConfig,
    OfflineSpeakerSegmentationPyannoteModelConfig, SpeakerEmbeddingExtractorConfig,
};
use sqlx::SqlitePool;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Mutex;
use tauri::{AppHandle, Emitter, Runtime};

/// Global flag guarding against two overlapping diarization passes. Diarization is
/// CPU-only and single-shot per meeting; there's no use case for running two at once, and
/// sharing one flag process-wide (like `audio::retranscription`'s guard) is simplest.
static DIARIZATION_IN_PROGRESS: AtomicBool = AtomicBool::new(false);

/// The `meeting_id` of the pass that currently holds `DIARIZATION_IN_PROGRESS`, so a
/// caller can tell "a pass is running for *this* meeting" from "a pass is running for a
/// different meeting" - the two need opposite handling in the UI (phykawing/meetily#18).
/// Set and cleared together with the flag by `DiarizationGuard`.
static DIARIZATION_MEETING_ID: Mutex<Option<String>> = Mutex::new(None);

struct DiarizationGuard;

impl DiarizationGuard {
    fn acquire(meeting_id: &str) -> Result<Self, String> {
        if DIARIZATION_IN_PROGRESS
            .compare_exchange(false, true, Ordering::SeqCst, Ordering::SeqCst)
            .is_err()
        {
            return Err("Diarization already in progress".to_string());
        }
        *DIARIZATION_MEETING_ID.lock().unwrap() = Some(meeting_id.to_string());
        Ok(DiarizationGuard)
    }
}

impl Drop for DiarizationGuard {
    fn drop(&mut self) {
        *DIARIZATION_MEETING_ID.lock().unwrap() = None;
        DIARIZATION_IN_PROGRESS.store(false, Ordering::SeqCst);
    }
}

pub fn is_diarization_in_progress() -> bool {
    DIARIZATION_IN_PROGRESS.load(Ordering::SeqCst)
}

/// The `meeting_id` of the diarization pass running right now, or `None` if none is.
pub fn diarization_in_progress_meeting() -> Option<String> {
    DIARIZATION_MEETING_ID.lock().unwrap().clone()
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DiarizationProgress {
    pub meeting_id: String,
    pub stage: String,
    pub progress_percentage: u32,
    pub message: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DiarizationResult {
    pub meeting_id: String,
    pub num_speakers: usize,
    pub num_segments_flagged: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DiarizationError {
    pub meeting_id: String,
    pub error: String,
}

fn emit_progress<R: Runtime>(
    app: &AppHandle<R>,
    meeting_id: &str,
    stage: &str,
    progress: u32,
    message: &str,
) {
    let _ = app.emit(
        "diarization-progress",
        DiarizationProgress {
            meeting_id: meeting_id.to_string(),
            stage: stage.to_string(),
            progress_percentage: progress,
            message: message.to_string(),
        },
    );
}

/// Runs a full diarization pass for `meeting_id` and persists the result, emitting
/// progress events throughout plus a final `diarization-complete` / `diarization-error`
/// event. Callers must have already confirmed `diarization_model_status().ready` -
/// consent and model presence are not re-checked here (see docs/adr/0005: that status
/// check is the single source of truth this is meant to consume, not reassemble).
///
/// The single-flight guard is acquired *inside* this function (not by the caller) and its
/// failure still goes through the same `diarization-error` emission as any other failure:
/// the caller (`run_diarization_command`) checks `is_diarization_in_progress()` before
/// spawning this as a background task and returning success to the frontend, but that
/// check and this guard acquisition aren't atomic with each other, so two near-simultaneous
/// calls can both pass the caller's check. Without this, the losing call's task would
/// return `Err` before ever reaching the emit logic, and the UI (which is already showing
/// a "detecting…" state after the earlier Ok) would wait forever for an event that never
/// arrives.
pub async fn run_diarization<R: Runtime>(
    app: AppHandle<R>,
    pool: SqlitePool,
    meeting_id: String,
    meeting_folder_path: String,
    base_models_dir: PathBuf,
) -> Result<DiarizationResult> {
    let result = run_diarization_guarded(
        &app,
        pool,
        &meeting_id,
        meeting_folder_path,
        base_models_dir,
    )
    .await;

    match &result {
        Ok(res) => {
            let _ = app.emit("diarization-complete", res.clone());
        }
        Err(e) => {
            let _ = app.emit(
                "diarization-error",
                DiarizationError {
                    meeting_id: meeting_id.clone(),
                    error: e.to_string(),
                },
            );
        }
    }

    result
}

async fn run_diarization_guarded<R: Runtime>(
    app: &AppHandle<R>,
    pool: SqlitePool,
    meeting_id: &str,
    meeting_folder_path: String,
    base_models_dir: PathBuf,
) -> Result<DiarizationResult> {
    let _guard = DiarizationGuard::acquire(meeting_id).map_err(|e| anyhow!(e))?;
    run_diarization_inner(app, &pool, meeting_id, &meeting_folder_path, &base_models_dir).await
}

async fn run_diarization_inner<R: Runtime>(
    app: &AppHandle<R>,
    pool: &SqlitePool,
    meeting_id: &str,
    meeting_folder_path: &str,
    base_models_dir: &Path,
) -> Result<DiarizationResult> {
    emit_progress(app, meeting_id, "locating", 5, "Locating meeting audio...");
    let folder_path = PathBuf::from(meeting_folder_path);
    let audio_path = find_audio_file(&folder_path)?;

    emit_progress(app, meeting_id, "decoding", 15, "Decoding audio file...");
    let path_for_decode = audio_path.clone();
    let decoded = tokio::task::spawn_blocking(move || decode_audio_file(&path_for_decode))
        .await
        .map_err(|e| anyhow!("Audio decode task panicked: {}", e))??;

    emit_progress(app, meeting_id, "decoding", 25, "Converting audio format...");
    let samples = tokio::task::spawn_blocking(move || decoded.to_whisper_format())
        .await
        .map_err(|e| anyhow!("Audio conversion task panicked: {}", e))?;

    emit_progress(app, meeting_id, "diarizing", 35, "Finding distinct speakers...");
    let models_dir = manager::models_dir(base_models_dir);
    let segmentation_path = models_dir.join(SEGMENTATION_MODEL.filename);
    let embedding_path = models_dir.join(EMBEDDING_MODEL.filename);

    let segments = tokio::task::spawn_blocking(move || {
        run_sherpa_diarization(&segmentation_path, &embedding_path, &samples, CLUSTERING_THRESHOLD)
    })
    .await
    .map_err(|e| anyhow!("Diarization task panicked: {}", e))??;

    info!(
        "Diarization for meeting {} found {} segments across {} speaker labels",
        meeting_id,
        segments.len(),
        count_distinct_speakers(&segments)
    );

    emit_progress(app, meeting_id, "aligning", 70, "Attributing transcript to speakers...");
    let turns: Vec<SpeakerTurn> = segments
        .iter()
        .map(|s| SpeakerTurn {
            speaker: speaker_label(s.speaker),
            start: s.start as f64,
            end: s.end as f64,
        })
        .collect();

    let chunks = SpeakerRepository::get_chunk_spans(pool, meeting_id).await?;
    let attributions = align_chunks_to_turns(&chunks, &turns);
    let speaker_names = default_speaker_names(&turns);
    let num_flagged = attributions.iter().filter(|a| a.uncertain).count();

    emit_progress(app, meeting_id, "saving", 90, "Saving speaker labels...");
    SpeakerRepository::replace_diarization_results(pool, meeting_id, &attributions, &speaker_names)
        .await?;

    emit_progress(app, meeting_id, "complete", 100, "Speaker detection complete");

    Ok(DiarizationResult {
        meeting_id: meeting_id.to_string(),
        num_speakers: speaker_names.len(),
        num_segments_flagged: num_flagged,
    })
}

/// The label sherpa-onnx assigns each cluster (0-based) is turned into a stable,
/// zero-padded string so it can be stored and compared as text.
fn speaker_label(index: i32) -> String {
    format!("speaker_{:02}", index)
}

fn count_distinct_speakers(segments: &[OfflineSpeakerDiarizationSegment]) -> usize {
    let mut seen = std::collections::HashSet::new();
    for s in segments {
        seen.insert(s.speaker);
    }
    seen.len()
}

/// Default "Speaker N" names, numbered in order of first appearance in `turns` (which are
/// already start-time sorted) so "Speaker 1" is whoever spoke first.
fn default_speaker_names(turns: &[SpeakerTurn]) -> Vec<SpeakerName> {
    let mut names = Vec::new();
    for turn in turns {
        if !names.iter().any(|n: &SpeakerName| n.label == turn.speaker) {
            names.push(SpeakerName {
                label: turn.speaker.clone(),
                name: format!("Speaker {}", names.len() + 1),
            });
        }
    }
    names
}

/// Clustering merge threshold, empirically raised from the sherpa-onnx crate's own
/// default (0.5) - see docs/adr/0007. At the default, a ~13-minute two/three-person
/// recording over-segmented into 12 spurious speakers; 0.75 produced 3, a materially more
/// plausible count for that same recording. Not user-configurable - see issue #3's Out of
/// Scope ("no exposed sensitivity setting").
const CLUSTERING_THRESHOLD: f32 = 0.75;

/// Runs segmentation + embedding + clustering over `samples` (16kHz mono f32, matching
/// what the pyannote segmentation model expects) and returns turns sorted by start time.
/// Synchronous and CPU-bound - callers run this inside `spawn_blocking`.
fn run_sherpa_diarization(
    segmentation_path: &Path,
    embedding_path: &Path,
    samples: &[f32],
    clustering_threshold: f32,
) -> Result<Vec<OfflineSpeakerDiarizationSegment>> {
    let config = OfflineSpeakerDiarizationConfig {
        segmentation: OfflineSpeakerSegmentationModelConfig {
            pyannote: OfflineSpeakerSegmentationPyannoteModelConfig {
                model: Some(segmentation_path.to_string_lossy().into_owned()),
            },
            // CPU only - see docs/adr/0001: diarization must not compete for video memory
            // with transcription or summarisation. This is the crate's own default; kept
            // explicit here so a future dependency bump changing that default is caught.
            provider: Some("cpu".to_string()),
            ..Default::default()
        },
        embedding: SpeakerEmbeddingExtractorConfig {
            model: Some(embedding_path.to_string_lossy().into_owned()),
            provider: Some("cpu".to_string()),
            ..Default::default()
        },
        // num_clusters left at -1 (auto, threshold-driven): the number of speakers is
        // discovered, not declared in advance. A meeting with one voice throughout
        // naturally collapses to a single cluster rather than erroring.
        clustering: FastClusteringConfig { num_clusters: -1, threshold: clustering_threshold },
        ..Default::default()
    };

    let diarizer = OfflineSpeakerDiarization::create(&config)
        .ok_or_else(|| anyhow!("Failed to initialize speaker diarization models"))?;

    let result = diarizer
        .process(samples)
        .ok_or_else(|| anyhow!("Speaker diarization failed to process the audio"))?;

    Ok(result.sort_by_start_time())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn turn(speaker: &str, start: f64, end: f64) -> SpeakerTurn {
        SpeakerTurn { speaker: speaker.to_string(), start, end }
    }

    #[test]
    fn speaker_label_is_zero_padded() {
        assert_eq!(speaker_label(0), "speaker_00");
        assert_eq!(speaker_label(7), "speaker_07");
        assert_eq!(speaker_label(12), "speaker_12");
    }

    #[test]
    fn default_speaker_names_are_numbered_by_first_appearance() {
        let turns = [
            turn("speaker_01", 0.0, 5.0),
            turn("speaker_00", 5.0, 10.0),
            turn("speaker_01", 10.0, 15.0),
        ];

        let names = default_speaker_names(&turns);

        assert_eq!(names.len(), 2);
        assert_eq!(names[0].label, "speaker_01");
        assert_eq!(names[0].name, "Speaker 1");
        assert_eq!(names[1].label, "speaker_00");
        assert_eq!(names[1].name, "Speaker 2");
    }

    #[test]
    fn default_speaker_names_for_no_turns_is_empty() {
        assert!(default_speaker_names(&[]).is_empty());
    }

    #[test]
    fn single_speaker_meeting_gets_one_sensible_name() {
        let turns = [turn("speaker_00", 0.0, 60.0)];
        let names = default_speaker_names(&turns);
        assert_eq!(names.len(), 1);
        assert_eq!(names[0].name, "Speaker 1");
    }
}
