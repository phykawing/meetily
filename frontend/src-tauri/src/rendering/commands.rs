use tauri::{AppHandle, Manager, Runtime, State};

use crate::database::repositories::rendering::RenderingRepository;
use crate::state::AppState;
use crate::summary::processor::clean_llm_markdown_output;
use crate::summary::summary_engine::{self, ModelManagerState};

use super::{
    build_rendering_chunks, build_rendering_user_prompt, fingerprint_segments, WrittenForm,
    RENDERING_SYSTEM_PROMPT,
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
#[tauri::command]
pub async fn set_written_form(
    meeting_id: String,
    written_form: String,
    state: State<'_, AppState>,
) -> Result<(), String> {
    let resolved = WrittenForm::from_stored(Some(&written_form));
    let pool = state.db_manager.pool();

    let updated = RenderingRepository::set_written_form(pool, &meeting_id, resolved.as_str())
        .await
        .map_err(|e| format!("Failed to save written form preference: {}", e))?;

    if !updated {
        return Err(format!("No meeting found with id {}", meeting_id));
    }

    Ok(())
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
/// produced). Always uses the local built-in model — Rendering is a full-transcript LLM
/// pass, and per docs/adr/0002 that makes it a privacy decision, not a formatting one.
///
/// Returns an error (rather than falling back to a remote provider) when no local model is
/// downloaded, so a display toggle can never silently upload a meeting.
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

    let rendered_text = rendered_chunks.join("\n");

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
}
