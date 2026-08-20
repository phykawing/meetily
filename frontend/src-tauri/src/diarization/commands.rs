use crate::database::repositories::setting::SettingsRepository;
use crate::diarization::consent::DiarizationConsent;
use crate::diarization::manager;
use crate::diarization::models::{total_size_bytes, DIARIZATION_MODELS};
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

async fn resolve_consent(state: &tauri::State<'_, AppState>) -> Result<DiarizationConsent, String> {
    let stored = SettingsRepository::get_diarization_consent(state.db_manager.pool())
        .await
        .map_err(|e| format!("Failed to get diarization consent: {}", e))?;
    Ok(DiarizationConsent::from_stored(stored.as_deref()))
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
    Ok(resolve_consent(&state).await?.as_str().to_string())
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
    let resolved = match consent.as_str() {
        "not_asked" => DiarizationConsent::NotAsked,
        "granted" => DiarizationConsent::Granted,
        "declined" => DiarizationConsent::Declined,
        other => return Err(format!("Unrecognized diarization consent value: {}", other)),
    };
    SettingsRepository::save_diarization_consent(state.db_manager.pool(), resolved.as_str())
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
    let consent = resolve_consent(&state).await?;
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
    if resolve_consent(&state).await? != DiarizationConsent::Granted {
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
