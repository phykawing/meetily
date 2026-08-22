use sqlx::SqlitePool;
use tauri::{AppHandle, Manager, Runtime, State};

use crate::database::repositories::rendering::RenderingRepository;
use crate::database::repositories::setting::SettingsRepository;
use crate::database::repositories::setting_store::{self, SettingToken};
use crate::state::AppState;
use crate::summary::llm_client::{self, LLMProvider};
use crate::summary::processor::clean_llm_markdown_output;
use crate::summary::summary_engine::{self, ModelManagerState};

use super::{
    build_rendering_chunks, build_rendering_user_prompt, fingerprint_segments, RenderingProvider,
    WrittenForm, RENDERING_SYSTEM_PROMPT,
};

/// Gets the persisted Written Form preference for a meeting ("colloquial" or "written").
/// Defaults to "colloquial" (口語) when nothing has been chosen yet.
#[tauri::command]
pub async fn get_written_form(
    meeting_id: String,
    state: State<'_, AppState>,
) -> Result<String, String> {
    let pool = state.db_manager.pool();
    let stored = RenderingRepository::get_written_form(pool, &meeting_id)
        .await
        .map_err(|e| format!("Failed to read written form preference: {}", e))?;

    Ok(WrittenForm::from_stored(stored.as_deref()).as_str().to_string())
}

/// Saves the Written Form preference for a meeting.
///
/// Rejects anything other than the two known tokens, rather than silently falling back to
/// "colloquial" — a typo or a stale frontend build must not be able to overwrite a real
/// answer with an unintended one.
#[tauri::command]
pub async fn set_written_form(
    meeting_id: String,
    written_form: String,
    state: State<'_, AppState>,
) -> Result<(), String> {
    let resolved = WrittenForm::parse(&written_form)?;
    let pool = state.db_manager.pool();

    let updated = RenderingRepository::set_written_form(pool, &meeting_id, resolved.as_str())
        .await
        .map_err(|e| format!("Failed to save written form preference: {}", e))?;

    if !updated {
        return Err(format!("No meeting found with id {}", meeting_id));
    }

    Ok(())
}

/// Gets the persisted Rendering provider setting ("local" or "summary_provider").
/// Defaults to "local" — the privacy-safe default — when nothing has been chosen yet.
#[tauri::command]
pub async fn get_rendering_provider(state: State<'_, AppState>) -> Result<String, String> {
    let provider: RenderingProvider = setting_store::RENDERING_PROVIDER
        .read(state.db_manager.pool())
        .await
        .map_err(|e| format!("Failed to read rendering provider setting: {}", e))?;

    Ok(provider.as_str().to_string())
}

/// Saves the Rendering provider setting.
///
/// Rejects anything other than the two known tokens, rather than silently falling back to
/// "local" — a typo or a stale frontend build must not be able to overwrite a real answer
/// with an unintended one.
#[tauri::command]
pub async fn set_rendering_provider(
    state: State<'_, AppState>,
    provider: String,
) -> Result<(), String> {
    let resolved = RenderingProvider::parse(&provider)?;

    setting_store::RENDERING_PROVIDER
        .write(state.db_manager.pool(), resolved)
        .await
        .map_err(|e| format!("Failed to save rendering provider setting: {}", e))
}

/// Resolved configuration for generating a Rendering through the configured summary
/// provider.
struct RemoteRenderingConfig {
    provider: LLMProvider,
    model_name: String,
    api_key: String,
    ollama_endpoint: Option<String>,
    custom_openai_endpoint: Option<String>,
    // Always positive when present — non-positive stored values are normalized away here so
    // every downstream consumer (chunk sizing, the `generate_summary` request) sees the same
    // "unconfigured" signal instead of one seeing `Some(0)` and the other falling back.
    custom_openai_max_tokens: Option<u32>,
    custom_openai_temperature: Option<f32>,
    custom_openai_top_p: Option<f32>,
}

/// Reads the currently configured summary provider and assembles what's needed to call it
/// for a Rendering pass, via `summary::service::resolve_provider_credentials` — the same
/// credential resolution summary generation uses, so there is one home for "which fields a
/// provider needs" rather than two independently-maintained copies. Errors rather than
/// falling back to Local — the user explicitly asked for "same as summary provider", so a
/// missing configuration must surface, not silently switch privacy posture.
async fn resolve_configured_summary_provider(
    pool: &SqlitePool,
) -> Result<RemoteRenderingConfig, String> {
    let setting = SettingsRepository::get_model_config(pool)
        .await
        .map_err(|e| format!("Failed to read summary provider setting: {}", e))?
        .ok_or_else(|| {
            "No summary provider is configured. Configure one in Settings, or use the Local rendering provider.".to_string()
        })?;

    let provider = LLMProvider::from_str(&setting.provider)
        .map_err(|e| format!("Unsupported summary provider configured: {}", e))?;

    if provider == LLMProvider::BuiltInAI {
        return Ok(RemoteRenderingConfig {
            provider,
            model_name: setting.model,
            api_key: String::new(),
            ollama_endpoint: None,
            custom_openai_endpoint: None,
            custom_openai_max_tokens: None,
            custom_openai_temperature: None,
            custom_openai_top_p: None,
        });
    }

    let credentials =
        crate::summary::service::resolve_provider_credentials(pool, &provider, &setting.provider).await?;

    let model_name = credentials.custom_openai_model.unwrap_or(setting.model);

    Ok(RemoteRenderingConfig {
        provider,
        model_name,
        api_key: credentials.api_key,
        ollama_endpoint: credentials.ollama_endpoint,
        custom_openai_endpoint: credentials.custom_openai_endpoint,
        custom_openai_max_tokens: credentials
            .custom_openai_max_tokens
            .filter(|&t| t > 0)
            .map(|t| t as u32),
        custom_openai_temperature: credentials.custom_openai_temperature,
        custom_openai_top_p: credentials.custom_openai_top_p,
    })
}

/// Bounds the per-chunk input size for a Rendering pass generated through a remote summary
/// provider. Conservative and provider-agnostic rather than tuned per provider: rendering
/// output is roughly as long as its input and both must fit one generation call, but
/// `llm_client::generate_summary` sends no `max_tokens` at all for OpenAI/Groq/Ollama/
/// OpenRouter (provider default applies) and hardcodes 2048 for Claude, so 2048 minus
/// overhead is the tightest budget any of them are known to honor. A configured Custom
/// OpenAI `max_tokens` smaller than that is respected instead, since exceeding it would
/// itself risk a truncated response. Callers are expected to have already normalized a
/// non-positive configured value to `None` (see `RemoteRenderingConfig`).
fn resolve_remote_chunk_size_tokens(custom_openai_max_tokens: Option<u32>) -> usize {
    const CONSERVATIVE_BUDGET: usize = 2048usize.saturating_sub(256);

    match custom_openai_max_tokens {
        Some(max) => (max as usize).saturating_sub(256).min(CONSERVATIVE_BUDGET).max(1),
        None => CONSERVATIVE_BUDGET,
    }
}

/// Bounds the per-chunk input size for a Rendering pass: at most the model's generation cap
/// (minus overhead), and never more than half the model's context window, since the
/// register rewrite produces output roughly as long as its input and both must fit in one
/// generation call. Floored at 1 rather than a larger constant — `build_rendering_chunks`
/// still makes progress (one segment per chunk) at any positive budget, so there is no need
/// to risk exceeding the half-context bound on a hypothetical small-context model to keep a
/// larger floor.
fn resolve_chunk_size_tokens(context_size: u32) -> usize {
    let generation_cap = (summary_engine::models::DEFAULT_MAX_TOKENS as usize).saturating_sub(256);
    let half_context = (context_size as usize).saturating_sub(512) / 2;
    generation_cap.min(half_context).max(1)
}

/// Gets the 書面語 Rendering for a meeting, generating and caching it if the cache is
/// missing or stale (the Canonical Transcript has changed since the cached Rendering was
/// produced). Uses the local built-in model by default, or the configured summary provider
/// when the Rendering provider setting says so — see `RenderingProvider` and
/// docs/adr/0002. Either way this is a full-transcript LLM pass, which is why the provider
/// is an explicit setting rather than a per-toggle choice.
///
/// Returns an error (rather than silently falling back to a different provider) when the
/// selected provider isn't usable — no local model downloaded for Local, or no summary
/// provider configured for "same as summary provider" — so a display toggle can never
/// silently change where a meeting's text goes.
///
/// The provider setting only takes effect the next time a Rendering is actually generated:
/// like Written Form caching in general (see the module doc), the cache is keyed on the
/// transcript's fingerprint only, not on which provider produced it. Changing the setting
/// after a meeting already has a cached Rendering doesn't retroactively regenerate that
/// Rendering under the new provider — this is never a privacy regression in either
/// direction (a cache already produced locally never becomes remote by switching the
/// setting, and vice versa), so it isn't worth invalidating a cache over.
#[tauri::command]
pub async fn get_transcript_rendering<R: Runtime>(
    app: AppHandle<R>,
    meeting_id: String,
    state: State<'_, AppState>,
    model_manager_state: State<'_, ModelManagerState>,
) -> Result<String, String> {
    let pool = state.db_manager.pool();

    let segments = RenderingRepository::get_canonical_segments(pool, &meeting_id)
        .await
        .map_err(|e| format!("Failed to read transcript: {}", e))?;

    if segments.is_empty() {
        return Err("This meeting has no transcript yet.".to_string());
    }

    let fingerprint = fingerprint_segments(&segments);

    if let Some((rendered_text, cached_fingerprint)) = RenderingRepository::get_cached_rendering(pool, &meeting_id)
        .await
        .map_err(|e| format!("Failed to read cached rendering: {}", e))?
    {
        if cached_fingerprint == fingerprint {
            return Ok(rendered_text);
        }
        log::info!(
            "Cached rendering for meeting {} is stale (transcript changed); regenerating",
            meeting_id
        );
    }

    let rendering_provider: RenderingProvider = setting_store::RENDERING_PROVIDER
        .read(pool)
        .await
        .map_err(|e| format!("Failed to read rendering provider setting: {}", e))?;

    // "Same as summary provider" only actually changes anything when that provider isn't
    // itself the local built-in model — otherwise this is the same path as `Local`.
    let remote = match rendering_provider {
        RenderingProvider::Local => None,
        RenderingProvider::SummaryProvider => {
            let config = resolve_configured_summary_provider(pool).await?;
            if config.provider == LLMProvider::BuiltInAI {
                None
            } else {
                Some(config)
            }
        }
    };

    let rendered_text = match remote {
        None => {
            let model_name = summary_engine::builtin_ai_get_available_summary_model(app.clone(), model_manager_state)
                .await?
                .ok_or_else(|| {
                    "No local model is downloaded. Download a Built-in AI model in Settings to enable Written Form rendering.".to_string()
                })?;

            let model_def = summary_engine::get_model_by_name(&model_name)
                .ok_or_else(|| format!("Unknown local model: {}", model_name))?;

            let app_data_dir = app
                .path()
                .app_data_dir()
                .map_err(|e| format!("Failed to resolve app data directory: {}", e))?;

            let chunk_size_tokens = resolve_chunk_size_tokens(model_def.context_size);
            let chunks = build_rendering_chunks(&segments, chunk_size_tokens);
            let num_chunks = chunks.len();
            let mut rendered_chunks = Vec::with_capacity(num_chunks);

            for (i, chunk) in chunks.iter().enumerate() {
                log::info!("Rendering chunk {}/{} for meeting {}", i + 1, num_chunks, meeting_id);

                let user_prompt = build_rendering_user_prompt(chunk);
                let raw = summary_engine::generate_with_builtin(
                    &app_data_dir,
                    &model_name,
                    RENDERING_SYSTEM_PROMPT,
                    &user_prompt,
                    None,
                )
                .await
                .map_err(|e| format!("Rendering generation failed: {}", e))?;

                rendered_chunks.push(clean_llm_markdown_output(&raw));
            }

            rendered_chunks.join("\n")
        }
        Some(remote) => {
            let chunk_size_tokens = resolve_remote_chunk_size_tokens(remote.custom_openai_max_tokens);
            let chunks = build_rendering_chunks(&segments, chunk_size_tokens);
            let num_chunks = chunks.len();
            let mut rendered_chunks = Vec::with_capacity(num_chunks);
            let client = reqwest::Client::new();

            for (i, chunk) in chunks.iter().enumerate() {
                log::info!(
                    "Rendering chunk {}/{} for meeting {} via configured summary provider",
                    i + 1,
                    num_chunks,
                    meeting_id
                );

                let user_prompt = build_rendering_user_prompt(chunk);
                let raw = llm_client::generate_summary(
                    &client,
                    &remote.provider,
                    &remote.model_name,
                    &remote.api_key,
                    RENDERING_SYSTEM_PROMPT,
                    &user_prompt,
                    remote.ollama_endpoint.as_deref(),
                    remote.custom_openai_endpoint.as_deref(),
                    remote.custom_openai_max_tokens,
                    remote.custom_openai_temperature,
                    remote.custom_openai_top_p,
                    None,
                    None,
                )
                .await
                .map_err(|e| format!("Rendering generation failed: {}", e))?;

                rendered_chunks.push(clean_llm_markdown_output(&raw));
            }

            rendered_chunks.join("\n")
        }
    };

    RenderingRepository::save_rendering(pool, &meeting_id, &rendered_text, &fingerprint)
        .await
        .map_err(|e| format!("Failed to cache rendering: {}", e))?;

    Ok(rendered_text)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn chunk_size_is_bounded_by_generation_cap_and_half_context() {
        // Small context window: half-context bound dominates.
        assert_eq!(resolve_chunk_size_tokens(1024), 256);
        // Large context window: generation cap dominates.
        assert_eq!(
            resolve_chunk_size_tokens(32768),
            (summary_engine::models::DEFAULT_MAX_TOKENS as usize) - 256
        );
    }

    /// A hypothetical model with a very small context window must never get a chunk size
    /// above half its context — the floor exists only so `build_rendering_chunks` always
    /// makes progress, not to guarantee a minimum chunk size.
    #[test]
    fn chunk_size_never_exceeds_half_context_even_for_a_tiny_context_window() {
        let chunk_size = resolve_chunk_size_tokens(700);
        assert!(
            chunk_size <= 700 / 2,
            "chunk_size {} exceeds half of a 700-token context",
            chunk_size
        );
    }

    #[test]
    fn remote_chunk_size_defaults_to_the_conservative_budget_when_unconfigured() {
        assert_eq!(resolve_remote_chunk_size_tokens(None), 2048 - 256);
    }

    /// `RemoteRenderingConfig` normalizes a non-positive stored `max_tokens` to `None`
    /// before it ever reaches chunk sizing or the `generate_summary` call — this is what
    /// that normalization looks like, so a `Some(0)`/`Some(-1)` from the database can never
    /// reach here in the first place and disagree with what the API request actually sends.
    #[test]
    fn non_positive_stored_max_tokens_normalizes_to_none() {
        let normalize = |v: Option<i32>| v.filter(|&t| t > 0).map(|t| t as u32);
        assert_eq!(normalize(Some(0)), None);
        assert_eq!(normalize(Some(-1)), None);
        assert_eq!(normalize(None), None);
        assert_eq!(normalize(Some(1024)), Some(1024));
    }

    #[test]
    fn remote_chunk_size_honors_a_smaller_configured_max_tokens() {
        assert_eq!(resolve_remote_chunk_size_tokens(Some(1024)), 1024 - 256);
    }

    #[test]
    fn remote_chunk_size_never_exceeds_the_conservative_budget_even_with_a_large_configured_max_tokens() {
        assert_eq!(resolve_remote_chunk_size_tokens(Some(100_000)), 2048 - 256);
    }
}
