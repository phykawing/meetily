use crate::database::repositories::setting_store::{self, SettingToken};
use crate::state::AppState;
use crate::whisper_engine::custom_models::{self, CustomModel};
use crate::whisper_engine::language;
use crate::whisper_engine::{ModelInfo, WhisperEngine};
use std::sync::{Arc, Mutex};
use std::path::PathBuf;
use tauri::{command, Emitter, Manager, AppHandle, Runtime};
use crate::config::WHISPER_MODEL_CATALOG;

// Global whisper engine
pub static WHISPER_ENGINE: Mutex<Option<Arc<WhisperEngine>>> = Mutex::new(None);

// Global models directory path (set during app initialization)
static MODELS_DIR: Mutex<Option<PathBuf>> = Mutex::new(None);

/// Initialize the models directory path using app_data_dir
/// This should be called during app setup before whisper_init
pub fn set_models_directory<R: Runtime>(app: &AppHandle<R>) {
    let app_data_dir = app.path().app_data_dir()
        .expect("Failed to get app data dir");

    let models_dir = app_data_dir.join("models");

    // Create directory if it doesn't exist
    if !models_dir.exists() {
        if let Err(e) = std::fs::create_dir_all(&models_dir) {
            log::error!("Failed to create models directory: {}", e);
            return;
        }
    }

    log::info!("Models directory set to: {}", models_dir.display());

    let mut guard = MODELS_DIR.lock().unwrap();
    *guard = Some(models_dir);
}

/// Get the configured models directory
fn get_models_directory() -> Option<PathBuf> {
    MODELS_DIR.lock().unwrap().clone()
}

#[command]
pub async fn whisper_init() -> Result<(), String> {
    let mut guard = WHISPER_ENGINE.lock().unwrap();
    if guard.is_some() {
        return Ok(());
    }

    let models_dir = get_models_directory();
    let engine = WhisperEngine::new_with_models_dir(models_dir)
        .map_err(|e| format!("Failed to initialize whisper engine: {}", e))?;
    *guard = Some(Arc::new(engine));
    Ok(())
}

#[command]
pub async fn whisper_get_available_models() -> Result<Vec<ModelInfo>, String> {
    let engine = {
        let guard = WHISPER_ENGINE.lock().unwrap();
        guard.as_ref().cloned()
    };

    if let Some(engine) = engine {
        engine
            .discover_models()
            .await
            .map_err(|e| format!("Failed to discover models: {}", e))
    } else {
        // Fallback: scan models directory directly without initialized engine
        log::info!("Whisper engine not initialized, scanning models directory directly");
        let models_dir = get_models_directory()
            .ok_or_else(|| "Models directory not initialized".to_string())?;
        discover_models_standalone(&models_dir)
    }
}

/// Discover Whisper models by scanning the models directory directly, merging in the
/// user-registered custom models. Used when the Whisper engine isn't initialized yet (e.g.
/// a call that lands before the startup init task completes) - the engine's own
/// `discover_models` is the normal path once it is up, and this must present the same
/// custom models it would, or a registered fine-tune vanishes from the list depending on
/// timing/provider alone.
fn discover_models_standalone(models_dir: &PathBuf) -> Result<Vec<ModelInfo>, String> {
    use crate::whisper_engine::ModelStatus;

    // Whisper models are stored directly in the models directory (not in a whisper subdirectory)
    let whisper_dir = models_dir.clone();

    log::info!("Scanning for Whisper models in: {}", whisper_dir.display());

    // Use centralized model catalog from config.rs
    let model_configs = WHISPER_MODEL_CATALOG;

    let mut models = Vec::new();

    for &(name, filename, size_mb, accuracy, speed, description) in model_configs {
        let model_path = whisper_dir.join(filename);
        let status = if model_path.exists() {
            match std::fs::metadata(&model_path) {
                Ok(metadata) => {
                    let file_size_mb = metadata.len() / (1024 * 1024);
                    if file_size_mb >= 1 {
                        ModelStatus::Available
                    } else {
                        ModelStatus::Missing
                    }
                }
                Err(_) => ModelStatus::Missing,
            }
        } else {
            ModelStatus::Missing
        };

        models.push(ModelInfo {
            name: name.to_string(),
            path: model_path,
            size_mb,
            status,
            accuracy: accuracy.to_string(),
            speed: speed.to_string(),
            description: description.to_string(),
            supports_cantonese: language::is_cantonese_capable_builtin(name),
        });
    }

    // User-registered models live outside the catalog and must show up here too - the
    // engine-initialized path (whisper_engine.rs's discover_models) merges them the same way.
    for custom in custom_models::load(&whisper_dir) {
        models.push(custom_models::to_model_info(custom));
    }

    let downloaded_count = models.iter().filter(|m| matches!(m.status, ModelStatus::Available)).count();
    log::info!("Found {} downloaded Whisper models", downloaded_count);

    Ok(models)
}

#[command]
pub async fn whisper_load_model(
    app_handle: tauri::AppHandle,
    model_name: String
) -> Result<(), String> {
    let engine = {
        let guard = WHISPER_ENGINE.lock().unwrap();
        guard.as_ref().cloned()
    };

    if let Some(engine) = engine {
        // FIX 6: Emit model loading started event
        if let Err(e) = app_handle.emit(
            "model-loading-started",
            serde_json::json!({
                "modelName": model_name
            }),
        ) {
            log::error!("Failed to emit model-loading-started event: {}", e);
        }

        let result = engine
            .load_model(&model_name)
            .await
            .map_err(|e| format!("Failed to load model: {}", e));

        // FIX 6: Emit model loading completed/failed event
        if result.is_ok() {
            if let Err(e) = app_handle.emit(
                "model-loading-completed",
                serde_json::json!({
                    "modelName": model_name
                }),
            ) {
                log::error!("Failed to emit model-loading-completed event: {}", e);
            }
        } else if let Err(ref error) = result {
            if let Err(e) = app_handle.emit(
                "model-loading-failed",
                serde_json::json!({
                    "modelName": model_name,
                    "error": error
                }),
            ) {
                log::error!("Failed to emit model-loading-failed event: {}", e);
            }
        }

        result
    } else {
        Err("Whisper engine not initialized".to_string())
    }
}

#[command]
pub async fn whisper_get_current_model() -> Result<Option<String>, String> {
    let engine = {
        let guard = WHISPER_ENGINE.lock().unwrap();
        guard.as_ref().cloned()
    };

    if let Some(engine) = engine {
        Ok(engine.get_current_model().await)
    } else {
        Err("Whisper engine not initialized".to_string())
    }
}

#[command]
pub async fn whisper_is_model_loaded() -> Result<bool, String> {
    let engine = {
        let guard = WHISPER_ENGINE.lock().unwrap();
        guard.as_ref().cloned()
    };

    if let Some(engine) = engine {
        Ok(engine.is_model_loaded().await)
    } else {
        Err("Whisper engine not initialized".to_string())
    }
}

#[command]
pub async fn whisper_has_available_models() -> Result<bool, String> {
    let engine = {
        let guard = WHISPER_ENGINE.lock().unwrap();
        guard.as_ref().cloned()
    };

    if let Some(engine) = engine {
        let models = engine
            .discover_models()
            .await
            .map_err(|e| format!("Failed to discover models: {}", e))?;

        // Check if at least one model is available
        let available_models: Vec<_> = models
            .iter()
            .filter(|model| matches!(model.status, crate::whisper_engine::ModelStatus::Available))
            .collect();

        Ok(!available_models.is_empty())
    } else {
        Ok(false)
    }
}

#[command]
pub async fn whisper_validate_model_ready() -> Result<String, String> {
    let engine = {
        let guard = WHISPER_ENGINE.lock().unwrap();
        guard.as_ref().cloned()
    };

    if let Some(engine) = engine {
        // Check if a model is currently loaded
        if engine.is_model_loaded().await {
            if let Some(current_model) = engine.get_current_model().await {
                return Ok(current_model);
            }
        }

        // No model loaded, check if any models are available to load
        let models = engine
            .discover_models()
            .await
            .map_err(|e| format!("Failed to discover models: {}", e))?;

        let available_models: Vec<_> = models
            .iter()
            .filter(|model| matches!(model.status, crate::whisper_engine::ModelStatus::Available))
            .collect();

        if available_models.is_empty() {
            return Err(
                "No Whisper models are available. Please download a model to enable transcription."
                    .to_string(),
            );
        }

        // Try to load the first available model
        let first_model = &available_models[0];
        engine
            .load_model(&first_model.name)
            .await
            .map_err(|e| format!("Failed to load model {}: {}", first_model.name, e))?;

        Ok(first_model.name.clone())
    } else {
        Err("Whisper engine not initialized".to_string())
    }
}

/// Internal version of whisper_validate_model_ready that respects user's transcript config
pub async fn whisper_validate_model_ready_with_config<R: tauri::Runtime>(
    app: &tauri::AppHandle<R>,
) -> Result<String, String> {
    let engine = {
        let guard = WHISPER_ENGINE.lock().unwrap();
        guard.as_ref().cloned()
    };

    if let Some(engine) = engine {
        // Refresh the meeting vocabulary from the database before every session so edits
        // take effect on the next transcription, regardless of whether a model reload
        // happens below.
        refresh_vocabulary_from_db(app, &engine).await;

        // Check if a model is currently loaded
        if engine.is_model_loaded().await {
            if let Some(current_model) = engine.get_current_model().await {
                log::info!("Model already loaded: {}", current_model);
                return Ok(current_model);
            }
        }

        // No model loaded - try to load user's configured model from transcript config
        let model_to_load = match crate::api::api::api_get_transcript_config(
            app.clone(),
            app.state(),
            None,
        )
        .await
        {
            Ok(Some(config)) => {
                log::info!(
                    "Got transcript config from API - provider: {}, model: {}",
                    config.provider,
                    config.model
                );
                if config.provider == "localWhisper" && !config.model.is_empty() {
                    log::info!("Using user's configured model: {}", config.model);
                    Some(config.model)
                } else {
                    log::info!(
                        "API config uses non-local provider ({}) or empty model, will auto-select",
                        config.provider
                    );
                    None
                }
            }
            Ok(None) => {
                log::info!("No transcript config found in API, will auto-select model");
                None
            }
            Err(e) => {
                log::warn!(
                    "Failed to get transcript config from API: {}, will auto-select model",
                    e
                );
                None
            }
        };

        // Check available models
        let models = engine
            .discover_models()
            .await
            .map_err(|e| format!("Failed to discover models: {}", e))?;

        let available_models: Vec<_> = models
            .iter()
            .filter(|model| matches!(model.status, crate::whisper_engine::ModelStatus::Available))
            .collect();

        if available_models.is_empty() {
            return Err(
                "No Whisper models are available. Please download a model to enable transcription."
                    .to_string(),
            );
        }

        // Try to load user's configured model if specified
        let model_name = if let Some(configured_model) = model_to_load {
            // Check if configured model is available
            if available_models.iter().any(|m| m.name == configured_model) {
                log::info!("Loading user's configured model: {}", configured_model);
                configured_model
            } else {
                log::warn!(
                    "Configured model '{}' not found, falling back to first available: {}",
                    configured_model,
                    available_models[0].name
                );
                available_models[0].name.clone()
            }
        } else {
            // No configured model, use first available
            log::info!(
                "No configured model, loading first available: {}",
                available_models[0].name
            );
            available_models[0].name.clone()
        };

        engine
            .load_model(&model_name)
            .await
            .map_err(|e| format!("Failed to load model {}: {}", model_name, e))?;

        Ok(model_name)
    } else {
        Err("Whisper engine not initialized".to_string())
    }
}

#[command]
pub async fn whisper_transcribe_audio(audio_data: Vec<f32>) -> Result<String, String> {
    let engine = {
        let guard = WHISPER_ENGINE.lock().unwrap();
        guard.as_ref().cloned()
    };

    if let Some(engine) = engine {
        // Get language preference
        let language = crate::get_language_preference_internal();
        engine
            .transcribe_audio(audio_data, language)
            .await
            .map_err(|e| format!("Transcription failed: {}", e))
    } else {
        Err("Whisper engine not initialized".to_string())
    }
}

#[command]
pub async fn whisper_get_models_directory() -> Result<String, String> {
    let engine = {
        let guard = WHISPER_ENGINE.lock().unwrap();
        guard.as_ref().cloned()
    };

    if let Some(engine) = engine {
        let path = engine.get_models_directory().await;
        Ok(path.to_string_lossy().to_string())
    } else {
        Err("Whisper engine not initialized".to_string())
    }
}

#[command]
pub async fn whisper_download_model(
    app_handle: tauri::AppHandle,
    model_name: String,
) -> Result<(), String> {
    let engine = {
        let guard = WHISPER_ENGINE.lock().unwrap();
        guard.as_ref().cloned()
    };

    if let Some(engine) = engine {
        // Create progress callback that emits events
        let app_handle_clone = app_handle.clone();
        let model_name_clone = model_name.clone();

        let progress_callback = Box::new(move |progress: u8| {
            log::info!("Download progress for {}: {}%", model_name_clone, progress);

            // Emit download progress event
            if let Err(e) = app_handle_clone.emit(
                "model-download-progress",
                serde_json::json!({
                    "modelName": model_name_clone,
                    "progress": progress
                }),
            ) {
                log::error!("Failed to emit download progress event: {}", e);
            }
        });

        let result = engine
            .download_model(&model_name, Some(progress_callback))
            .await;

        match result {
            Ok(()) => {
                // Emit completion event
                if let Err(e) = app_handle.emit(
                    "model-download-complete",
                    serde_json::json!({
                        "modelName": model_name
                    }),
                ) {
                    log::error!("Failed to emit download complete event: {}", e);
                }
                Ok(())
            }
            Err(e) => {
                // Emit error event
                if let Err(emit_e) = app_handle.emit(
                    "model-download-error",
                    serde_json::json!({
                        "modelName": model_name,
                        "error": e.to_string()
                    }),
                ) {
                    log::error!("Failed to emit download error event: {}", emit_e);
                }
                Err(format!("Failed to download model: {}", e))
            }
        }
    } else {
        Err("Whisper engine not initialized".to_string())
    }
}

#[command]
pub async fn whisper_cancel_download(model_name: String) -> Result<(), String> {
    let engine = {
        let guard = WHISPER_ENGINE.lock().unwrap();
        guard.as_ref().cloned()
    };

    if let Some(engine) = engine {
        engine
            .cancel_download(&model_name)
            .await
            .map_err(|e| format!("Failed to cancel download: {}", e))
    } else {
        Err("Whisper engine not initialized".to_string())
    }
}

#[command]
pub async fn whisper_delete_corrupted_model(model_name: String) -> Result<String, String> {
    let engine = {
        let guard = WHISPER_ENGINE.lock().unwrap();
        guard.as_ref().cloned()
    };

    if let Some(engine) = engine {
        engine
            .delete_model(&model_name)
            .await
            .map_err(|e| format!("Failed to delete model: {}", e))
    } else {
        Err("Whisper engine not initialized".to_string())
    }
}

// ============================================================================
// Custom (user-registered) models
//
// A locally converted fine-tune - a Cantonese one, typically - is referenced in place
// rather than downloaded, and declares the language token it was trained with. Registration
// and removal read and write the registry directly (via the models directory, not the
// engine), so they work even before the engine has finished initializing; when the engine
// is up, both also refresh its custom-model cache in the same call, since that cache is
// what resolve_decoding() reads to pick the token - a registration that only touched the
// JSON file would not take effect until the next restart.
// ============================================================================

/// The registered custom models, for the model manager's list. Reads the models directory
/// directly rather than going through the engine, so this works before the engine has
/// finished initializing - the same reason `discover_models_standalone` exists.
#[command]
pub async fn whisper_list_custom_models() -> Result<Vec<CustomModel>, String> {
    let models_dir = require_models_directory()?;
    Ok(custom_models::load(&models_dir))
}

/// Registers a ggml file as a selectable model. Returns the full registry so the caller
/// does not need a second round-trip.
#[command]
pub async fn whisper_register_custom_model(
    name: String,
    path: String,
    engine_language: Option<String>,
    supports_cantonese: bool,
    description: Option<String>,
) -> Result<Vec<CustomModel>, String> {
    let models_dir = require_models_directory()?;

    let model = CustomModel {
        name,
        path: PathBuf::from(path),
        engine_language,
        supports_cantonese,
        description: description.unwrap_or_default(),
    };
    let registered = custom_models::add(&models_dir, model)
        .map_err(|e| format!("Failed to register model: {}", e))?;

    refresh_custom_model_cache().await?;
    Ok(registered)
}

/// Unregisters a custom model. The model file itself is left alone - it was never copied
/// into the models directory, and it is not Meetily's to delete.
#[command]
pub async fn whisper_remove_custom_model(name: String) -> Result<Vec<CustomModel>, String> {
    let models_dir = require_models_directory()?;

    let remaining = custom_models::remove(&models_dir, &name)
        .map_err(|e| format!("Failed to remove model: {}", e))?;

    refresh_custom_model_cache().await?;
    Ok(remaining)
}

/// Opens a file picker for a ggml model file. Returns `None` when the user cancels.
/// Lives on the Rust side, like the audio import picker, so no dialog capability has to
/// be granted to the webview.
#[command]
pub async fn whisper_select_custom_model_file<R: Runtime>(
    app: AppHandle<R>,
) -> Result<Option<String>, String> {
    use tauri_plugin_dialog::DialogExt;

    let app = app.clone();
    let picked = tokio::task::spawn_blocking(move || {
        app.dialog()
            .file()
            .add_filter("Whisper models", &["bin"])
            .blocking_pick_file()
    })
    .await
    .map_err(|e| format!("File dialog task failed: {}", e))?;

    Ok(picked.map(|path| path.to_string()))
}

fn require_models_directory() -> Result<PathBuf, String> {
    get_models_directory().ok_or_else(|| "Models directory not initialized".to_string())
}

/// Re-reads the registry into the engine, so a just-registered model is selectable and
/// decodes with its declared token without a restart. A no-op while the engine hasn't
/// finished initializing yet - its own `discover_models` will pick up the registry the
/// first time it runs.
async fn refresh_custom_model_cache() -> Result<(), String> {
    let engine = {
        let guard = WHISPER_ENGINE.lock().unwrap();
        guard.as_ref().cloned()
    };
    let Some(engine) = engine else {
        return Ok(());
    };
    engine
        .discover_models()
        .await
        .map(|_| ())
        .map_err(|e| format!("Failed to refresh model list: {}", e))
}

/// Open the models folder in the system file explorer
#[command]
pub async fn open_models_folder() -> Result<(), String> {
    let models_dir = get_models_directory()
        .ok_or_else(|| "Models directory not initialized".to_string())?;

    // Ensure directory exists before trying to open it
    if !models_dir.exists() {
        std::fs::create_dir_all(&models_dir)
            .map_err(|e| format!("Failed to create directory: {}", e))?;
    }

    let folder_path = models_dir.to_string_lossy().to_string();

    #[cfg(target_os = "windows")]
    {
        std::process::Command::new("explorer")
            .arg(&folder_path)
            .spawn()
            .map_err(|e| format!("Failed to open folder: {}", e))?;
    }

    #[cfg(target_os = "macos")]
    {
        std::process::Command::new("open")
            .arg(&folder_path)
            .spawn()
            .map_err(|e| format!("Failed to open folder: {}", e))?;
    }

    #[cfg(target_os = "linux")]
    {
        std::process::Command::new("xdg-open")
            .arg(&folder_path)
            .spawn()
            .map_err(|e| format!("Failed to open folder: {}", e))?;
    }

    log::info!("Opened models folder: {}", folder_path);
    Ok(())
}

/// Loads the persisted meeting vocabulary into `engine` from the database. Called at the
/// start of every transcription session so an edit made in Settings takes effect on the
/// next transcription without a model reload or restart.
pub async fn refresh_vocabulary_from_db<R: Runtime>(app: &AppHandle<R>, engine: &WhisperEngine) {
    let app_state = app.state::<AppState>();
    let pool = app_state.db_manager.pool();
    match setting_store::MEETING_VOCABULARY.read_text(pool).await {
        Ok(vocabulary) => engine.set_vocabulary(vocabulary).await,
        Err(e) => log::warn!("Failed to load meeting vocabulary: {}", e),
    }
}

/// Gets the persisted meeting vocabulary, for display in Settings.
#[command]
pub async fn get_meeting_vocabulary(state: tauri::State<'_, AppState>) -> Result<Option<String>, String> {
    setting_store::MEETING_VOCABULARY
        .read_text(state.db_manager.pool())
        .await
        .map_err(|e| format!("Failed to get meeting vocabulary: {}", e))
}

/// Persists the meeting vocabulary and, if the Whisper engine is initialized, applies it
/// immediately so the change takes effect on the next transcription.
#[command]
pub async fn save_meeting_vocabulary(
    state: tauri::State<'_, AppState>,
    vocabulary: Option<String>,
) -> Result<(), String> {
    setting_store::write_meeting_vocabulary(state.db_manager.pool(), vocabulary.as_deref())
        .await
        .map_err(|e| format!("Failed to save meeting vocabulary: {}", e))?;

    let engine = {
        let guard = WHISPER_ENGINE.lock().unwrap();
        guard.as_ref().cloned()
    };
    if let Some(engine) = engine {
        engine.set_vocabulary(vocabulary).await;
    }

    Ok(())
}

/// Gets the persisted Script setting, for display in Settings. Defaults to Traditional
/// (Hong Kong) when nothing has been saved yet.
#[command]
pub async fn get_script_setting(state: tauri::State<'_, AppState>) -> Result<String, String> {
    let script: crate::script::ScriptSetting = setting_store::SCRIPT
        .read(state.db_manager.pool())
        .await
        .map_err(|e| format!("Failed to get script setting: {}", e))?;
    Ok(script.as_str().to_string())
}

/// Persists the Script setting. Applied the next time a transcript is stored (recording,
/// import, or retranscription) — not retroactively to existing meetings.
///
/// Rejects anything other than the three known tokens, rather than silently coercing to
/// the default — a typo or a stale frontend build must not be able to overwrite a real
/// answer with an unintended one.
#[command]
pub async fn save_script_setting(
    state: tauri::State<'_, AppState>,
    script_setting: String,
) -> Result<(), String> {
    let resolved = crate::script::ScriptSetting::parse(&script_setting)?;
    setting_store::SCRIPT
        .write(state.db_manager.pool(), resolved)
        .await
        .map_err(|e| format!("Failed to save script setting: {}", e))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::whisper_engine::ModelStatus;

    /// Reproduces issue #24: a registered custom model must appear in the model manager
    /// even when the engine hasn't initialized yet (e.g. Parakeet is the live-transcription
    /// provider, or a caller races the startup init task) - not just once the engine path
    /// runs discover_models().
    #[test]
    fn discover_models_standalone_includes_registered_custom_models() {
        let dir = tempfile::tempdir().expect("temp models dir");
        let model_file = dir.path().join("ggml-cantonese.bin");
        std::fs::write(&model_file, b"not really a model").expect("write model file");
        custom_models::add(
            dir.path(),
            CustomModel {
                name: "cantonese-turbo".to_string(),
                path: model_file,
                engine_language: Some("yue".to_string()),
                supports_cantonese: true,
                description: "Cantonese fine-tune".to_string(),
            },
        )
        .expect("register model");

        let models = discover_models_standalone(&dir.path().to_path_buf())
            .expect("standalone discovery succeeds");

        let custom = models
            .iter()
            .find(|m| m.name == "cantonese-turbo")
            .expect("registered model missing from standalone discovery");
        assert!(matches!(custom.status, ModelStatus::Available));
        assert!(custom.supports_cantonese);
    }

    #[test]
    fn discover_models_standalone_labels_a_missing_custom_model() {
        let dir = tempfile::tempdir().expect("temp models dir");
        let model_file = dir.path().join("ggml-gone.bin");
        std::fs::write(&model_file, b"not really a model").expect("write model file");
        custom_models::add(
            dir.path(),
            CustomModel {
                name: "gone".to_string(),
                path: model_file.clone(),
                engine_language: None,
                supports_cantonese: false,
                description: String::new(),
            },
        )
        .expect("register model");
        std::fs::remove_file(&model_file).expect("delete the registered file");

        let models = discover_models_standalone(&dir.path().to_path_buf())
            .expect("standalone discovery succeeds");

        let custom = models
            .iter()
            .find(|m| m.name == "gone")
            .expect("registered model missing from standalone discovery");
        assert!(matches!(custom.status, ModelStatus::Missing));
    }
}
