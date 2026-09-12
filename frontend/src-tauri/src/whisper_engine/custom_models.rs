// whisper_engine/custom_models.rs
//
// Registry of user-supplied ggml models that Meetily did not download.
//
// Cantonese fine-tunes are not distributed as ggml by their authors, so they are converted
// locally (see scripts/convert-whisper-to-ggml.md) and registered here by pointing at the
// resulting file. A registered model declares which language token it was trained with,
// which is what lets "Cantonese" in the UI mean `yue` for one model and `zh` for another —
// see whisper_engine/language.rs.
//
// The registry is a JSON file in the models directory; the model files themselves are
// referenced in place by absolute path and never copied.

use std::path::{Path, PathBuf};

use anyhow::{anyhow, Result};
use serde::{Deserialize, Serialize};

use crate::config::WHISPER_MODEL_CATALOG;
use super::language;
use super::whisper_engine::{ModelInfo, ModelStatus};

const REGISTRY_FILE: &str = "custom-models.json";

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct CustomModel {
    /// Unique name, shown in the model picker. Must not collide with a catalog model.
    pub name: String,
    /// Absolute path to the ggml file.
    pub path: PathBuf,
    /// The language token this model was fine-tuned with (e.g. `yue`, `zh`). `None` means
    /// it behaves like a stock model and gets the default mapping.
    pub engine_language: Option<String>,
    /// Whether this model may be selected for Cantonese. Declared by the user when
    /// registering, because nothing in a ggml file says what it was trained on.
    pub supports_cantonese: bool,
    pub description: String,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
struct Registry {
    models: Vec<CustomModel>,
}

fn registry_path(models_dir: &Path) -> PathBuf {
    models_dir.join(REGISTRY_FILE)
}

/// Reads the registry. A missing or unreadable file is an empty registry, not an error —
/// a corrupt registry must not stop the app from transcribing with catalog models.
pub fn load(models_dir: &Path) -> Vec<CustomModel> {
    let path = registry_path(models_dir);
    let Ok(bytes) = std::fs::read(&path) else {
        return Vec::new();
    };
    match serde_json::from_slice::<Registry>(&bytes) {
        Ok(registry) => registry.models,
        Err(e) => {
            log::warn!(
                "Custom model registry at {} is unreadable ({}); ignoring it",
                path.display(),
                e
            );
            Vec::new()
        }
    }
}

fn save(models_dir: &Path, models: &[CustomModel]) -> Result<()> {
    std::fs::create_dir_all(models_dir)?;
    let registry = Registry {
        models: models.to_vec(),
    };
    let json = serde_json::to_vec_pretty(&registry)?;

    // Write via a temp file so an interrupted write cannot truncate the registry.
    let path = registry_path(models_dir);
    let temp = path.with_extension("json.tmp");
    std::fs::write(&temp, json)?;
    std::fs::rename(&temp, &path)?;
    Ok(())
}

/// Registers a model file. Rejects names that collide with the catalog, and paths that do
/// not point at a readable file — a broken entry would only surface much later as a
/// confusing load failure.
pub fn add(models_dir: &Path, model: CustomModel) -> Result<Vec<CustomModel>> {
    let model = normalize(model);
    validate(&model)?;

    let mut models = load(models_dir);
    if models.iter().any(|m| m.name == model.name) {
        return Err(anyhow!("A custom model named '{}' already exists", model.name));
    }
    models.push(model);
    save(models_dir, &models)?;
    Ok(models)
}

pub fn remove(models_dir: &Path, name: &str) -> Result<Vec<CustomModel>> {
    let mut models = load(models_dir);
    let before = models.len();
    models.retain(|m| m.name != name);
    if models.len() == before {
        return Err(anyhow!("No custom model named '{}'", name));
    }
    save(models_dir, &models)?;
    Ok(models)
}

/// Trims the free-text fields and drops blank ones. The registration form sends empty
/// strings for anything the user left alone, and an `engine_language` of `Some("")` would
/// be forced on whisper.cpp as a language token; blank means "no declaration" instead.
/// Runs before validation so whitespace cannot smuggle a name past the collision checks.
fn normalize(model: CustomModel) -> CustomModel {
    CustomModel {
        name: model.name.trim().to_string(),
        engine_language: model
            .engine_language
            .map(|token| token.trim().to_string())
            .filter(|token| !token.is_empty()),
        description: model.description.trim().to_string(),
        ..model
    }
}

fn validate(model: &CustomModel) -> Result<()> {
    if model.name.trim().is_empty() {
        return Err(anyhow!("Custom model name cannot be empty"));
    }
    if WHISPER_MODEL_CATALOG.iter().any(|&(name, ..)| name == model.name) {
        return Err(anyhow!(
            "'{}' is the name of a built-in model; choose a different name",
            model.name
        ));
    }
    // A name that merely starts with "large-v3" (e.g. `large-v3-mine`) is not caught by the
    // exact-match check above, but `language::is_cantonese_capable_builtin` matches on that
    // same prefix — such a name would silently be treated as a stock Cantonese-capable
    // checkpoint and forced to `zh`, ignoring its own declaration. Checked via the same
    // predicate rather than a generic catalog-prefix scan: only this one family has
    // prefix-based special-casing anywhere in the engine, and a scan over every catalog
    // name (`tiny`, `small`, `medium`, ...) would reject perfectly good names like
    // `small-cantonese` that nothing actually treats specially.
    if language::is_cantonese_capable_builtin(&model.name) {
        return Err(anyhow!(
            "'{}' would be treated as a built-in Cantonese-capable model (its name matches \
             the large-v3 family); choose a different name",
            model.name
        ));
    }
    // Nothing in a ggml file says what language it was trained on, so a Cantonese claim
    // with no declared token can't be honoured by `resolve_decoding` — it would fall
    // through to the builtin capability rule, which never matches a non-catalog name, and
    // silently resolve as unsupported despite being advertised as Cantonese-capable.
    if model.supports_cantonese && model.engine_language.is_none() {
        return Err(anyhow!(
            "A Cantonese-capable model must declare the language token it was trained with \
             (e.g. 'yue' or 'zh')"
        ));
    }
    if !model.path.is_file() {
        return Err(anyhow!(
            "No file at {} — the model file must exist when it is registered",
            model.path.display()
        ));
    }
    Ok(())
}

/// Size in MB, for display. Zero when the file has gone missing since registration.
pub fn size_mb(model: &CustomModel) -> u32 {
    std::fs::metadata(&model.path)
        .map(|m| (m.len() / (1024 * 1024)) as u32)
        .unwrap_or(0)
}

/// Builds the `ModelInfo` shown in the model manager for a registered custom model. Shared
/// by `WhisperEngine::discover_models` and the standalone fallback (used before the engine
/// initializes) so the two paths cannot drift on how a custom model is presented.
pub fn to_model_info(model: CustomModel) -> ModelInfo {
    let size = size_mb(&model);
    let status = if model.path.is_file() {
        ModelStatus::Available
    } else {
        log::warn!(
            "Custom model '{}' is registered but its file is missing: {}",
            model.name,
            model.path.display()
        );
        ModelStatus::Missing
    };
    ModelInfo {
        name: model.name,
        size_mb: size,
        path: model.path,
        accuracy: "Custom".to_string(),
        speed: "Custom".to_string(),
        status,
        description: model.description,
        supports_cantonese: model.supports_cantonese,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_dir(tag: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("meetily-custom-models-{tag}"));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    fn dummy_model_file(dir: &Path) -> PathBuf {
        let path = dir.join("ggml-cantonese.bin");
        std::fs::write(&path, b"not really a model").unwrap();
        path
    }

    fn model(name: &str, path: PathBuf) -> CustomModel {
        CustomModel {
            name: name.to_string(),
            path,
            engine_language: Some("yue".to_string()),
            supports_cantonese: true,
            description: "Cantonese fine-tune".to_string(),
        }
    }

    #[test]
    fn add_then_load_round_trips() {
        let dir = temp_dir("round-trip");
        let file = dummy_model_file(&dir);
        add(&dir, model("cantonese-turbo", file.clone())).unwrap();

        let loaded = load(&dir);
        assert_eq!(loaded.len(), 1);
        assert_eq!(loaded[0].name, "cantonese-turbo");
        assert_eq!(loaded[0].engine_language.as_deref(), Some("yue"));
        assert!(loaded[0].supports_cantonese);
    }

    #[test]
    fn blank_and_padded_fields_are_normalized_before_storing() {
        // The registration form sends empty strings for fields the user left alone. An
        // engine_language of Some("") would be forced on whisper.cpp as a language token;
        // it has to become None, which is what "behaves like a stock model" means.
        let dir = temp_dir("normalize");
        let file = dummy_model_file(&dir);
        let mut model = model("  cantonese-turbo  ", file);
        model.engine_language = Some("  ".to_string());
        // Blanking engine_language would otherwise collide with the "Cantonese needs a
        // declared token" rule; this test is about trimming, not that rule.
        model.supports_cantonese = false;
        model.description = "  spaced out  ".to_string();
        add(&dir, model).unwrap();

        let loaded = load(&dir);
        assert_eq!(loaded[0].name, "cantonese-turbo");
        assert_eq!(loaded[0].engine_language, None);
        assert_eq!(loaded[0].description, "spaced out");
    }

    #[test]
    fn a_padded_engine_language_keeps_its_token() {
        let dir = temp_dir("normalize-token");
        let file = dummy_model_file(&dir);
        let mut model = model("cantonese-turbo", file);
        model.engine_language = Some(" yue ".to_string());
        add(&dir, model).unwrap();

        assert_eq!(load(&dir)[0].engine_language.as_deref(), Some("yue"));
    }

    #[test]
    fn a_padded_catalog_name_is_still_rejected() {
        // Normalization happens before validation, so whitespace cannot smuggle a
        // catalog name past the collision check.
        let dir = temp_dir("padded-catalog-clash");
        let file = dummy_model_file(&dir);
        let err = add(&dir, model("  large-v3-turbo  ", file)).unwrap_err();
        assert!(err.to_string().contains("built-in"));
    }

    #[test]
    fn catalog_names_are_rejected() {
        let dir = temp_dir("catalog-clash");
        let file = dummy_model_file(&dir);
        let err = add(&dir, model("large-v3-turbo", file)).unwrap_err();
        assert!(err.to_string().contains("built-in"));
    }

    #[test]
    fn a_large_v3_prefixed_name_is_rejected() {
        // "large-v3-mine" is not itself a catalog name, but
        // `language::is_cantonese_capable_builtin` matches on `starts_with("large-v3")` -
        // registering it would silently inherit stock large-v3 Cantonese capability.
        let dir = temp_dir("family-prefix-clash");
        let file = dummy_model_file(&dir);
        let err = add(&dir, model("large-v3-mine", file)).unwrap_err();
        assert!(err.to_string().contains("large-v3"), "unexpected error: {err}");
    }

    #[test]
    fn a_name_prefixed_by_a_non_cantonese_family_is_allowed() {
        // Only the large-v3 family gets prefix-based special-casing anywhere in the engine
        // (see `language::is_cantonese_capable_builtin`); a name starting with `small`,
        // `medium`, `base`, or `tiny` isn't treated specially by anything, and is exactly
        // the kind of name a fine-tune based on one of those checkpoints would use.
        let dir = temp_dir("non-cantonese-family-prefix");
        for name in ["small-cantonese", "medium-mine", "base-improved", "tiny-custom"] {
            let file = dir.join(format!("{name}.bin"));
            std::fs::write(&file, b"not really a model").unwrap();
            let mut m = model(name, file);
            m.supports_cantonese = false;
            add(&dir, m).unwrap_or_else(|e| panic!("{name} should register: {e}"));
        }
    }

    #[test]
    fn cantonese_capability_without_a_declared_token_is_rejected() {
        let dir = temp_dir("cantonese-no-token");
        let file = dummy_model_file(&dir);
        let mut m = model("mine", file);
        m.engine_language = None;
        m.supports_cantonese = true;
        let err = add(&dir, m).unwrap_err();
        assert!(err.to_string().contains("language token"), "unexpected error: {err}");
    }

    #[test]
    fn cantonese_capability_with_a_declared_token_is_accepted() {
        let dir = temp_dir("cantonese-with-token");
        let file = dummy_model_file(&dir);
        let m = model("mine", file);
        assert!(add(&dir, m).is_ok());
    }

    #[test]
    fn duplicate_names_are_rejected() {
        let dir = temp_dir("duplicate");
        let file = dummy_model_file(&dir);
        add(&dir, model("mine", file.clone())).unwrap();
        let err = add(&dir, model("mine", file)).unwrap_err();
        assert!(err.to_string().contains("already exists"));
    }

    #[test]
    fn missing_files_are_rejected_at_registration() {
        let dir = temp_dir("missing-file");
        let err = add(&dir, model("ghost", dir.join("nope.bin"))).unwrap_err();
        assert!(err.to_string().contains("No file at"));
    }

    #[test]
    fn a_corrupt_registry_reads_as_empty_rather_than_failing() {
        let dir = temp_dir("corrupt");
        std::fs::write(registry_path(&dir), b"{ this is not json").unwrap();
        assert!(load(&dir).is_empty());
    }

    #[test]
    fn remove_reports_unknown_names() {
        let dir = temp_dir("remove");
        assert!(remove(&dir, "nothing-here").is_err());
    }

    #[test]
    fn to_model_info_reports_missing_when_the_file_is_gone() {
        let dir = temp_dir("to-model-info-missing");
        let m = model("ghost", dir.join("nope.bin"));
        let info = to_model_info(m);
        assert!(matches!(info.status, ModelStatus::Missing));
        assert_eq!(info.size_mb, 0);
    }

    #[test]
    fn to_model_info_reports_available_when_the_file_exists() {
        let dir = temp_dir("to-model-info-available");
        let file = dummy_model_file(&dir);
        let m = model("mine", file);
        let info = to_model_info(m);
        assert!(matches!(info.status, ModelStatus::Available));
        assert!(info.supports_cantonese);
    }
}
