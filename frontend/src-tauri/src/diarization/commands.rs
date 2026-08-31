use crate::database::repositories::setting_store::{self, SettingToken};
use crate::database::repositories::speaker::SpeakerRepository;
use crate::diarization::consent::DiarizationConsent;
use crate::diarization::manager;
use crate::diarization::models::{total_size_bytes, DIARIZATION_MODELS};
use crate::diarization::pipeline;
use crate::state::AppState;
use serde::Serialize;
use std::sync::atomic::{AtomicBool, Ordering};
use tauri::{command, AppHandle, Emitter, Manager};

/// Guards against two overlapping `download_diarization_models` calls (e.g. a stray
/// double-click, or a future second call site) writing the same files at once.
static DOWNLOAD_IN_PROGRESS: AtomicBool = AtomicBool::new(false);

fn base_models_dir(app: &AppHandle) -> Result<std::path::PathBuf, String> {
    app.path()
        .app_data_dir()
        .map(|dir| dir.join("models"))
        .map_err(|e| format!("Failed to resolve app data directory: {}", e))
}

#[derive(Serialize)]
pub struct DiarizationModelStatus {
    id: &'static str,
    #[serde(rename = "displayName")]
    display_name: &'static str,
    #[serde(rename = "sizeBytes")]
    size_bytes: u64,
    license: &'static str,
    downloaded: bool,
}

#[derive(Serialize)]
pub struct DiarizationStatusResponse {
    consent: &'static str,
    models: Vec<DiarizationModelStatus>,
    #[serde(rename = "totalSizeBytes")]
    total_size_bytes: u64,
    ready: bool,
}

/// Gets the persisted diarization model-download consent state: `"not_asked"`,
/// `"granted"`, or `"declined"`.
#[command]
pub async fn get_diarization_consent(state: tauri::State<'_, AppState>) -> Result<String, String> {
    let consent: DiarizationConsent = setting_store::DIARIZATION_CONSENT
        .read(state.db_manager.pool())
        .await
        .map_err(|e| format!("Failed to get diarization consent: {}", e))?;
    Ok(consent.as_str().to_string())
}

/// Persists the user's answer to the diarization model-download prompt. Declining is
/// recorded explicitly (not left as "not asked") so the prompt is not repeated after
/// every recording once the user has said no.
///
/// Rejects anything other than the three known tokens, rather than silently falling back
/// to "not asked" — a typo or a stale frontend build must not be able to overwrite a real
/// answer with an unintended one.
#[command]
pub async fn set_diarization_consent(
    state: tauri::State<'_, AppState>,
    consent: String,
) -> Result<(), String> {
    let resolved = DiarizationConsent::parse(&consent)?;
    setting_store::DIARIZATION_CONSENT
        .write(state.db_manager.pool(), resolved)
        .await
        .map_err(|e| format!("Failed to save diarization consent: {}", e))
}

/// Reports consent state, per-model download status, total download size, and whether
/// diarization is ready to run (consent granted and every model present on disk).
#[command]
pub async fn diarization_model_status(
    app: AppHandle,
    state: tauri::State<'_, AppState>,
) -> Result<DiarizationStatusResponse, String> {
    let consent: DiarizationConsent = setting_store::DIARIZATION_CONSENT
        .read(state.db_manager.pool())
        .await
        .map_err(|e| format!("Failed to get diarization consent: {}", e))?;
    let base_dir = base_models_dir(&app)?;

    let statuses = manager::model_statuses(&base_dir);
    let models: Vec<DiarizationModelStatus> = statuses
        .iter()
        .map(|(m, downloaded)| DiarizationModelStatus {
            id: m.id,
            display_name: m.display_name,
            size_bytes: m.size_bytes,
            license: m.license,
            downloaded: *downloaded,
        })
        .collect();

    let ready = consent == DiarizationConsent::Granted
        && statuses.iter().all(|(_, downloaded)| *downloaded);

    Ok(DiarizationStatusResponse {
        consent: consent.as_str(),
        models,
        total_size_bytes: total_size_bytes(),
        ready,
    })
}

/// Downloads every diarization model that is not already present and valid on disk.
/// Refuses to run unless consent has been granted — this is the enforced gate, not just
/// a UI convention, so nothing large can be fetched without the user having answered.
///
/// Models already on disk are skipped, so calling this again after a failure only
/// retries what did not complete. Refuses to run a second time concurrently with itself.
#[command]
pub async fn download_diarization_models(
    app: AppHandle,
    state: tauri::State<'_, AppState>,
) -> Result<(), String> {
    if DOWNLOAD_IN_PROGRESS.swap(true, Ordering::SeqCst) {
        return Err("A diarization model download is already in progress".to_string());
    }
    let result = download_diarization_models_inner(app, state).await;
    DOWNLOAD_IN_PROGRESS.store(false, Ordering::SeqCst);
    result
}

async fn download_diarization_models_inner(
    app: AppHandle,
    state: tauri::State<'_, AppState>,
) -> Result<(), String> {
    let consent: DiarizationConsent = setting_store::DIARIZATION_CONSENT
        .read(state.db_manager.pool())
        .await
        .map_err(|e| format!("Failed to get diarization consent: {}", e))?;
    if consent != DiarizationConsent::Granted {
        return Err("Diarization models cannot be downloaded before consent is granted".to_string());
    }

    let base_dir = base_models_dir(&app)?;
    let dir = manager::models_dir(&base_dir);

    let mut first_error: Option<String> = None;

    for model in DIARIZATION_MODELS {
        let path = dir.join(model.filename);
        if manager::is_downloaded(&path, model) {
            continue;
        }

        let app_for_progress = app.clone();
        let model_id = model.id;
        let result = manager::download_model(model, &path, move |progress| {
            let _ = app_for_progress.emit(
                "diarization-model-download-progress",
                serde_json::json!({ "modelId": model_id, "progress": progress }),
            );
        })
        .await;

        match result {
            Ok(()) => {
                let _ = app.emit(
                    "diarization-model-download-complete",
                    serde_json::json!({ "modelId": model.id }),
                );
            }
            Err(e) => {
                let message = e.to_string();
                let _ = app.emit(
                    "diarization-model-download-error",
                    serde_json::json!({ "modelId": model.id, "error": message }),
                );
                if first_error.is_none() {
                    first_error = Some(message);
                }
            }
        }
    }

    match first_error {
        Some(e) => Err(e),
        None => Ok(()),
    }
}

/// Response when a diarization pass has been kicked off in the background. Matches
/// `audio::retranscription::RetranscriptionStarted`'s plain snake_case shape.
#[derive(Serialize)]
pub struct DiarizationStarted {
    meeting_id: String,
    message: String,
}

/// One discovered speaker's display name, for resolving `speaker_label` on transcript
/// rows into something human-readable.
#[derive(Serialize)]
pub struct MeetingSpeakerInfo {
    label: String,
    name: String,
}

/// Starts a diarization pass for `meeting_id` in the background, emitting
/// `diarization-progress` / `diarization-complete` / `diarization-error` events as it
/// runs (see `diarization::pipeline`).
///
/// Re-checks consent and model readiness itself rather than trusting the caller — this is
/// the enforced gate, mirroring `download_diarization_models`. The frontend must not be
/// able to reach a diarization run by skipping past a declined/unanswered consent prompt
/// (see docs/adr/0005, which blocked this issue on #10 specifically so it could consume
/// that readiness check rather than reassemble it).
#[command]
pub async fn run_diarization_command(
    app: AppHandle,
    state: tauri::State<'_, AppState>,
    meeting_id: String,
    meeting_folder_path: String,
) -> Result<DiarizationStarted, String> {
    if pipeline::is_diarization_in_progress() {
        return Err("Speaker detection is already running".to_string());
    }
    // Retranscription deletes and re-inserts a meeting's transcript rows with fresh ids
    // (audio/retranscription.rs); if that races with a diarization pass reading/updating
    // those same rows by id, diarization's updates silently match nothing and it would
    // still report success. Refusing to start against each other closes that window.
    if crate::audio::retranscription::is_retranscription_in_progress() {
        return Err(
            "Speaker detection can't run while this meeting is being retranscribed".to_string(),
        );
    }

    let consent: DiarizationConsent = setting_store::DIARIZATION_CONSENT
        .read(state.db_manager.pool())
        .await
        .map_err(|e| format!("Failed to get diarization consent: {}", e))?;
    let base_dir = base_models_dir(&app)?;
    if consent != DiarizationConsent::Granted || !manager::all_models_downloaded(&base_dir) {
        return Err(
            "Speaker detection isn't enabled yet. Enable it under Settings > Preferences first."
                .to_string(),
        );
    }

    let pool = state.db_manager.pool().clone();
    let app_for_task = app.clone();
    let meeting_id_for_task = meeting_id.clone();
    tauri::async_runtime::spawn(async move {
        if let Err(e) = pipeline::run_diarization(
            app_for_task,
            pool,
            meeting_id_for_task,
            meeting_folder_path,
            base_dir,
        )
        .await
        {
            // The pipeline already emits a `diarization-error` event; this is just for
            // the log.
            log::error!("Diarization failed: {}", e);
        }
    });

    Ok(DiarizationStarted {
        meeting_id,
        message: "Speaker detection started".to_string(),
    })
}

/// Renames one discovered speaker for one meeting. The new name is trimmed; an empty or
/// whitespace-only name is rejected. The rename applies to the whole meeting at once -
/// every transcript row resolves its speaker through this one label -> name mapping - and
/// is scoped to this meeting only (see docs/adr/0001, docs/adr/0004).
///
/// Errors if the label is no longer part of the meeting: a re-run of speaker detection
/// discards and rebuilds the labels (ADR-0001), so a rename attempted from a stale, still-
/// open transcript view must fail loudly rather than silently do nothing.
#[command]
pub async fn rename_meeting_speaker(
    state: tauri::State<'_, AppState>,
    meeting_id: String,
    speaker_label: String,
    name: String,
) -> Result<(), String> {
    let trimmed = name.trim();
    if trimmed.is_empty() {
        return Err("Speaker name can't be empty".to_string());
    }

    let updated = SpeakerRepository::rename_speaker(
        state.db_manager.pool(),
        &meeting_id,
        &speaker_label,
        trimmed,
    )
    .await
    .map_err(|e| format!("Failed to rename speaker: {}", e))?;

    if updated == 0 {
        return Err(
            "That speaker is no longer part of this meeting - a new detection run may have replaced it."
                .to_string(),
        );
    }

    Ok(())
}

/// The meeting's discovered speakers and their display names. Empty until a diarization
/// pass has completed for this meeting.
#[command]
pub async fn get_meeting_speakers(
    meeting_id: String,
    state: tauri::State<'_, AppState>,
) -> Result<Vec<MeetingSpeakerInfo>, String> {
    let speakers = SpeakerRepository::get_meeting_speakers(state.db_manager.pool(), &meeting_id)
        .await
        .map_err(|e| format!("Failed to get meeting speakers: {}", e))?;

    Ok(speakers
        .into_iter()
        .map(|s| MeetingSpeakerInfo { label: s.label, name: s.name })
        .collect())
}
