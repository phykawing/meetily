// manager.rs
//
// Downloads and status-checks the diarization models listed in `models.rs`. Mirrors the
// streaming-download pattern already used by `whisper_engine::download_model` (stream,
// report progress, no automatic retry — a retry is just a fresh call, which skips models
// that already validated on disk), with one addition: bytes are streamed to a `.part`
// file and only renamed into place once the download completes and validates, so a
// connection drop mid-stream can never leave a truncated file sitting at the final path
// looking "downloaded".

use super::models::{DiarizationModel, DIARIZATION_MODELS};
use anyhow::{anyhow, Result};
use std::path::{Path, PathBuf};
use std::time::Duration;
use tokio::fs;
use tokio::io::AsyncWriteExt;

/// Whether a model's on-disk file is present and large enough to trust. A file smaller
/// than `min_valid_bytes` is treated as missing (partial/failed download), so a retry
/// re-fetches it instead of trusting a truncated file that happens to exist.
pub fn is_downloaded(path: &Path, model: &DiarizationModel) -> bool {
    match std::fs::metadata(path) {
        Ok(meta) => meta.is_file() && meta.len() >= model.min_valid_bytes,
        Err(_) => false,
    }
}

/// The directory diarization models are stored in, under the app's shared models
/// directory (`<app_data_dir>/models/diarization`).
pub fn models_dir(base_models_dir: &Path) -> PathBuf {
    base_models_dir.join("diarization")
}

/// Path a model is downloaded to while in flight, before it is validated and renamed
/// into its final `filename`. Never treated as "downloaded" by `is_downloaded`, since it
/// is not the path that function checks.
fn temp_path(path: &Path) -> PathBuf {
    let mut name = path.as_os_str().to_owned();
    name.push(".part");
    PathBuf::from(name)
}

/// Per-model download status, computed once and shared by both `all_models_downloaded`
/// and the Tauri status command so the two never drift apart.
pub fn model_statuses(base_models_dir: &Path) -> Vec<(&'static DiarizationModel, bool)> {
    let dir = models_dir(base_models_dir);
    DIARIZATION_MODELS
        .iter()
        .map(|model| (model, is_downloaded(&dir.join(model.filename), model)))
        .collect()
}

/// True when every diarization model is present and validated in `base_models_dir`.
pub fn all_models_downloaded(base_models_dir: &Path) -> bool {
    model_statuses(base_models_dir).iter().all(|(_, downloaded)| *downloaded)
}

/// Downloads one model to `path`, invoking `on_progress(percent)` as bytes arrive.
/// Streams to a temporary `.part` file and only renames it to `path` once the download
/// completes and passes validation — a failure at any point (including a dropped
/// connection) leaves `path` exactly as it was, so `is_downloaded(path, model)` never
/// reports a corrupt or partial file as present. Callers should skip this call entirely
/// for models that already pass `is_downloaded`.
pub async fn download_model(
    model: &DiarizationModel,
    path: &Path,
    mut on_progress: impl FnMut(u8) + Send,
) -> Result<()> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)
            .await
            .map_err(|e| anyhow!("Failed to create diarization models directory: {}", e))?;
    }

    // A generous overall timeout: these models are tens of MB, so even a slow
    // connection finishes well inside this, while a stalled/dead connection cannot
    // hang the download (and the Tauri command awaiting it) forever.
    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(600))
        .build()
        .map_err(|e| anyhow!("Failed to build HTTP client: {}", e))?;
    let response = client
        .get(model.url)
        .send()
        .await
        .map_err(|e| anyhow!("Failed to start download of {}: {}", model.display_name, e))?;

    if !response.status().is_success() {
        return Err(anyhow!(
            "Download of {} failed with status: {}",
            model.display_name,
            response.status()
        ));
    }

    let total_size = response.content_length().unwrap_or(model.size_bytes);
    let tmp_path = temp_path(path);
    let mut file = fs::File::create(&tmp_path)
        .await
        .map_err(|e| anyhow!("Failed to create file for {}: {}", model.display_name, e))?;

    use futures_util::StreamExt;
    let mut stream = response.bytes_stream();
    let mut downloaded = 0u64;
    let mut last_reported = 0u8;
    on_progress(0);

    while let Some(chunk_result) = stream.next().await {
        let chunk = match chunk_result {
            Ok(chunk) => chunk,
            Err(e) => {
                let _ = fs::remove_file(&tmp_path).await;
                return Err(anyhow!("Failed reading {} download: {}", model.display_name, e));
            }
        };
        if let Err(e) = file.write_all(&chunk).await {
            let _ = fs::remove_file(&tmp_path).await;
            return Err(anyhow!("Failed writing {}: {}", model.display_name, e));
        }
        downloaded += chunk.len() as u64;

        let progress = if total_size > 0 {
            ((downloaded as f64 / total_size as f64) * 100.0).min(100.0) as u8
        } else {
            0
        };
        if progress >= last_reported + 1 || progress == 100 {
            on_progress(progress);
            last_reported = progress;
        }
    }

    if let Err(e) = file.flush().await {
        let _ = fs::remove_file(&tmp_path).await;
        return Err(anyhow!("Failed to flush {}: {}", model.display_name, e));
    }
    drop(file);
    on_progress(100);

    if !is_downloaded(&tmp_path, model) {
        let _ = fs::remove_file(&tmp_path).await;
        return Err(anyhow!(
            "Downloaded file for {} is smaller than expected ({} bytes) — the download may have been interrupted",
            model.display_name,
            model.min_valid_bytes
        ));
    }

    fs::rename(&tmp_path, path)
        .await
        .map_err(|e| anyhow!("Failed to finalize download of {}: {}", model.display_name, e))?;

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::diarization::models::DiarizationModel;

    const TEST_MODEL: DiarizationModel = DiarizationModel {
        id: "test",
        display_name: "Test Model",
        url: "https://example.invalid/model.onnx",
        filename: "test-model.onnx",
        size_bytes: 100,
        min_valid_bytes: 50,
        license: "MIT",
    };

    #[test]
    fn missing_file_is_not_downloaded() {
        let dir = tempfile::tempdir().expect("temp dir");
        let path = dir.path().join(TEST_MODEL.filename);
        assert!(!is_downloaded(&path, &TEST_MODEL));
    }

    #[test]
    fn undersized_file_is_not_downloaded() {
        let dir = tempfile::tempdir().expect("temp dir");
        let path = dir.path().join(TEST_MODEL.filename);
        std::fs::write(&path, vec![0u8; 10]).unwrap();
        assert!(!is_downloaded(&path, &TEST_MODEL));
    }

    #[test]
    fn file_at_or_above_min_valid_bytes_is_downloaded() {
        let dir = tempfile::tempdir().expect("temp dir");
        let path = dir.path().join(TEST_MODEL.filename);
        std::fs::write(&path, vec![0u8; 50]).unwrap();
        assert!(is_downloaded(&path, &TEST_MODEL));
    }

    #[test]
    fn a_partial_file_left_at_the_temp_path_does_not_count_as_downloaded() {
        // Simulates an interrupted download: bytes landed in the `.part` file but the
        // final path was never written because the stream errored before renaming.
        let dir = tempfile::tempdir().expect("temp dir");
        let path = dir.path().join(TEST_MODEL.filename);
        std::fs::write(temp_path(&path), vec![0u8; 90]).unwrap();
        assert!(!is_downloaded(&path, &TEST_MODEL));
    }

    #[test]
    fn models_dir_is_a_subdirectory_of_the_shared_models_dir() {
        let base = PathBuf::from("/tmp/app-data/models");
        assert_eq!(models_dir(&base), PathBuf::from("/tmp/app-data/models/diarization"));
    }

    #[test]
    fn all_models_downloaded_is_false_when_directory_does_not_exist() {
        let dir = tempfile::tempdir().expect("temp dir");
        assert!(!all_models_downloaded(dir.path()));
    }

    #[test]
    fn model_statuses_covers_every_manifest_model() {
        let dir = tempfile::tempdir().expect("temp dir");
        let statuses = model_statuses(dir.path());
        assert_eq!(statuses.len(), DIARIZATION_MODELS.len());
        assert!(statuses.iter().all(|(_, downloaded)| !downloaded));
    }
}
