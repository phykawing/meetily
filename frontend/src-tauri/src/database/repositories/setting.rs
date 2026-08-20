use crate::database::models::{Setting, TranscriptSetting};
use crate::summary::CustomOpenAIConfig;
use sqlx::SqlitePool;

#[derive(serde::Deserialize, Debug)]
pub struct SaveModelConfigRequest {
    pub provider: String,
    pub model: String,
    #[serde(rename = "whisperModel")]
    pub whisper_model: String,
    #[serde(rename = "apiKey")]
    pub api_key: Option<String>,
    #[serde(rename = "ollamaEndpoint")]
    pub ollama_endpoint: Option<String>,
}

#[derive(serde::Deserialize, Debug)]
pub struct SaveTranscriptConfigRequest {
    pub provider: String,
    pub model: String,
    #[serde(rename = "apiKey")]
    pub api_key: Option<String>,
}

pub struct SettingsRepository;

// Transcript providers: localWhisper, deepgram, elevenLabs, groq, openai
// Summary providers: openai, claude, ollama, groq, added openrouter
// NOTE: Handle data exclusion in the higher layer as this is database abstraction layer(using SELECT *)

impl SettingsRepository {
    pub async fn get_model_config(
        pool: &SqlitePool,
    ) -> std::result::Result<Option<Setting>, sqlx::Error> {
        let setting = sqlx::query_as::<_, Setting>("SELECT * FROM settings LIMIT 1")
            .fetch_optional(pool)
            .await?;
        Ok(setting)
    }

    pub async fn save_model_config(
        pool: &SqlitePool,
        provider: &str,
        model: &str,
        whisper_model: &str,
        ollama_endpoint: Option<&str>,
    ) -> std::result::Result<(), sqlx::Error> {
        // Using id '1' for backward compatibility
        sqlx::query(
            r#"
            INSERT INTO settings (id, provider, model, whisperModel, ollamaEndpoint)
            VALUES ('1', $1, $2, $3, $4)
            ON CONFLICT(id) DO UPDATE SET
                provider = excluded.provider,
                model = excluded.model,
                whisperModel = excluded.whisperModel,
                ollamaEndpoint = excluded.ollamaEndpoint
            "#,
        )
        .bind(provider)
        .bind(model)
        .bind(whisper_model)
        .bind(ollama_endpoint)
        .execute(pool)
        .await?;

        Ok(())
    }

    pub async fn save_api_key(
        pool: &SqlitePool,
        provider: &str,
        api_key: &str,
    ) -> std::result::Result<(), sqlx::Error> {
        // Custom OpenAI uses JSON config (customOpenAIConfig) instead of a separate API key column
        if provider == "custom-openai" {
            return Err(sqlx::Error::Protocol(
                "custom-openai provider should use save_custom_openai_config() instead of save_api_key()".into(),
            ));
        }

        let api_key_column = match provider {
            "openai" => "openaiApiKey",
            "claude" => "anthropicApiKey",
            "ollama" => "ollamaApiKey",
            "groq" => "groqApiKey",
            "openrouter" => "openRouterApiKey",
            "builtin-ai" => return Ok(()), // No API key needed
            _ => {
                return Err(sqlx::Error::Protocol(
                    format!("Invalid provider: {}", provider).into(),
                ))
            }
        };

        let query = format!(
            r#"
            INSERT INTO settings (id, provider, model, whisperModel, "{}")
            VALUES ('1', 'openai', 'gpt-4o-2024-11-20', 'large-v3', $1)
            ON CONFLICT(id) DO UPDATE SET
                "{}" = $1
            "#,
            api_key_column, api_key_column
        );
        sqlx::query(&query).bind(api_key).execute(pool).await?;

        Ok(())
    }

    pub async fn get_api_key(
        pool: &SqlitePool,
        provider: &str,
    ) -> std::result::Result<Option<String>, sqlx::Error> {
        // Custom OpenAI uses JSON config - extract API key from there
        if provider == "custom-openai" {
            let config = Self::get_custom_openai_config(pool).await?;
            return Ok(config.and_then(|c| c.api_key));
        }

        let api_key_column = match provider {
            "openai" => "openaiApiKey",
            "ollama" => "ollamaApiKey",
            "groq" => "groqApiKey",
            "claude" => "anthropicApiKey",
            "openrouter" => "openRouterApiKey",
            "builtin-ai" => return Ok(None), // No API key needed
            _ => {
                return Err(sqlx::Error::Protocol(
                    format!("Invalid provider: {}", provider).into(),
                ))
            }
        };

        let query = format!(
            "SELECT {} FROM settings WHERE id = '1' LIMIT 1",
            api_key_column
        );
        let api_key = sqlx::query_scalar(&query).fetch_optional(pool).await?;
        Ok(api_key)
    }

    pub async fn get_transcript_config(
        pool: &SqlitePool,
    ) -> std::result::Result<Option<TranscriptSetting>, sqlx::Error> {
        let setting =
            sqlx::query_as::<_, TranscriptSetting>("SELECT * FROM transcript_settings LIMIT 1")
                .fetch_optional(pool)
                .await?;
        Ok(setting)

    }

    pub async fn save_transcript_config(
        pool: &SqlitePool,
        provider: &str,
        model: &str,
    ) -> std::result::Result<(), sqlx::Error> {
        sqlx::query(
            r#"
            INSERT INTO transcript_settings (id, provider, model)
            VALUES ('1', $1, $2)
            ON CONFLICT(id) DO UPDATE SET
                provider = excluded.provider,
                model = excluded.model
            "#,
        )
        .bind(provider)
        .bind(model)
        .execute(pool)
        .await?;

        Ok(())
    }

    /// Gets the persisted meeting vocabulary (names, jargon, product terms) folded into the
    /// Whisper initial prompt. `None` when nothing has been saved yet.
    pub async fn get_meeting_vocabulary(
        pool: &SqlitePool,
    ) -> std::result::Result<Option<String>, sqlx::Error> {
        let vocabulary: Option<Option<String>> =
            sqlx::query_scalar("SELECT meetingVocabulary FROM transcript_settings WHERE id = '1' LIMIT 1")
                .fetch_optional(pool)
                .await?;
        Ok(vocabulary.flatten())
    }

    /// Saves the meeting vocabulary. Whitespace-only input is normalized to `NULL` so an
    /// empty vocabulary produces no prompt at all.
    ///
    /// Updates the existing row when one is present, so this never overwrites a provider
    /// the user already chose. Only falls back to inserting a fresh row (with the app's
    /// documented default provider) when no transcript settings exist at all yet.
    pub async fn save_meeting_vocabulary(
        pool: &SqlitePool,
        vocabulary: Option<&str>,
    ) -> std::result::Result<(), sqlx::Error> {
        let cleaned = vocabulary.map(str::trim).filter(|v| !v.is_empty());

        let result = sqlx::query("UPDATE transcript_settings SET meetingVocabulary = $1 WHERE id = '1'")
            .bind(cleaned)
            .execute(pool)
            .await?;

        if result.rows_affected() == 0 {
            sqlx::query(
                r#"
                INSERT INTO transcript_settings (id, provider, model, meetingVocabulary)
                VALUES ('1', 'parakeet', $1, $2)
                "#,
            )
            .bind(crate::config::DEFAULT_PARAKEET_MODEL)
            .bind(cleaned)
            .execute(pool)
            .await?;
        }

        Ok(())
    }

    /// Gets the persisted Script setting token (see `crate::script::ScriptSetting`), for
    /// display in Settings. `None` when nothing has been saved yet — callers resolve that
    /// to the default via `ScriptSetting::from_stored`.
    pub async fn get_script_setting(
        pool: &SqlitePool,
    ) -> std::result::Result<Option<String>, sqlx::Error> {
        let setting: Option<Option<String>> =
            sqlx::query_scalar("SELECT scriptSetting FROM transcript_settings WHERE id = '1' LIMIT 1")
                .fetch_optional(pool)
                .await?;
        Ok(setting.flatten())
    }

    /// Saves the Script setting token.
    ///
    /// Updates the existing row when one is present, so this never overwrites a provider
    /// the user already chose. Only falls back to inserting a fresh row (with the app's
    /// documented default provider) when no transcript settings exist at all yet.
    pub async fn save_script_setting(
        pool: &SqlitePool,
        script_setting: &str,
    ) -> std::result::Result<(), sqlx::Error> {
        let result = sqlx::query("UPDATE transcript_settings SET scriptSetting = $1 WHERE id = '1'")
            .bind(script_setting)
            .execute(pool)
            .await?;

        if result.rows_affected() == 0 {
            sqlx::query(
                r#"
                INSERT INTO transcript_settings (id, provider, model, scriptSetting)
                VALUES ('1', 'parakeet', $1, $2)
                "#,
            )
            .bind(crate::config::DEFAULT_PARAKEET_MODEL)
            .bind(script_setting)
            .execute(pool)
            .await?;
        }

        Ok(())
    }

    /// Gets the persisted diarization model-download consent token (see
    /// `crate::diarization::consent::DiarizationConsent`). `None` when the user has not
    /// been asked yet.
    pub async fn get_diarization_consent(
        pool: &SqlitePool,
    ) -> std::result::Result<Option<String>, sqlx::Error> {
        let consent: Option<Option<String>> =
            sqlx::query_scalar("SELECT diarizationConsent FROM settings WHERE id = '1' LIMIT 1")
                .fetch_optional(pool)
                .await?;
        Ok(consent.flatten())
    }

    /// Saves the diarization model-download consent token.
    ///
    /// Updates the existing row when one is present. Only falls back to inserting a fresh
    /// row (with the app's documented default provider) when no settings exist at all yet.
    pub async fn save_diarization_consent(
        pool: &SqlitePool,
        consent: &str,
    ) -> std::result::Result<(), sqlx::Error> {
        let result = sqlx::query("UPDATE settings SET diarizationConsent = $1 WHERE id = '1'")
            .bind(consent)
            .execute(pool)
            .await?;

        if result.rows_affected() == 0 {
            sqlx::query(
                r#"
                INSERT INTO settings (id, provider, model, whisperModel, diarizationConsent)
                VALUES ('1', 'openai', 'gpt-4o-2024-11-20', 'large-v3', $1)
                "#,
            )
            .bind(consent)
            .execute(pool)
            .await?;
        }

        Ok(())
    }

    /// Gets the persisted Rendering provider token (see
    /// `crate::rendering::RenderingProvider`). `None` when the user has not chosen yet.
    pub async fn get_rendering_provider(
        pool: &SqlitePool,
    ) -> std::result::Result<Option<String>, sqlx::Error> {
        let provider: Option<Option<String>> =
            sqlx::query_scalar("SELECT renderingProvider FROM settings WHERE id = '1' LIMIT 1")
                .fetch_optional(pool)
                .await?;
        Ok(provider.flatten())
    }

    /// Saves the Rendering provider token.
    ///
    /// Updates the existing row when one is present. Only falls back to inserting a fresh
    /// row (with the app's documented default provider) when no settings exist at all yet.
    pub async fn save_rendering_provider(
        pool: &SqlitePool,
        rendering_provider: &str,
    ) -> std::result::Result<(), sqlx::Error> {
        let result = sqlx::query("UPDATE settings SET renderingProvider = $1 WHERE id = '1'")
            .bind(rendering_provider)
            .execute(pool)
            .await?;

        if result.rows_affected() == 0 {
            sqlx::query(
                r#"
                INSERT INTO settings (id, provider, model, whisperModel, renderingProvider)
                VALUES ('1', 'openai', 'gpt-4o-2024-11-20', 'large-v3', $1)
                "#,
            )
            .bind(rendering_provider)
            .execute(pool)
            .await?;
        }

        Ok(())
    }

    pub async fn save_transcript_api_key(
        pool: &SqlitePool,
        provider: &str,
        api_key: &str,
    ) -> std::result::Result<(), sqlx::Error> {
        let api_key_column = match provider {
            "localWhisper" => "whisperApiKey",
            "parakeet" => return Ok(()), // Parakeet doesn't need an API key, return early
            "deepgram" => "deepgramApiKey",
            "elevenLabs" => "elevenLabsApiKey",
            "groq" => "groqApiKey",
            "openai" => "openaiApiKey",
            _ => {
                return Err(sqlx::Error::Protocol(
                    format!("Invalid provider: {}", provider).into(),
                ))
            }
        };

        let query = format!(
            r#"
            INSERT INTO transcript_settings (id, provider, model, "{}")
            VALUES ('1', 'parakeet', '{}', $1)
            ON CONFLICT(id) DO UPDATE SET
                "{}" = $1
            "#,
            api_key_column, crate::config::DEFAULT_PARAKEET_MODEL, api_key_column
        );
        sqlx::query(&query).bind(api_key).execute(pool).await?;

        Ok(())
    }

    pub async fn get_transcript_api_key(
        pool: &SqlitePool,
        provider: &str,
    ) -> std::result::Result<Option<String>, sqlx::Error> {
        let api_key_column = match provider {
            "localWhisper" => "whisperApiKey",
            "parakeet" => return Ok(None), // Parakeet doesn't need an API key
            "deepgram" => "deepgramApiKey",
            "elevenLabs" => "elevenLabsApiKey",
            "groq" => "groqApiKey",
            "openai" => "openaiApiKey",
            _ => {
                return Err(sqlx::Error::Protocol(
                    format!("Invalid provider: {}", provider).into(),
                ))
            }
        };

        let query = format!(
            "SELECT {} FROM transcript_settings WHERE id = '1' LIMIT 1",
            api_key_column
        );
        let api_key = sqlx::query_scalar(&query).fetch_optional(pool).await?;
        Ok(api_key)
    }

    pub async fn delete_api_key(
        pool: &SqlitePool,
        provider: &str,
    ) -> std::result::Result<(), sqlx::Error> {
        // Custom OpenAI uses JSON config - clear the entire config
        if provider == "custom-openai" {
            sqlx::query("UPDATE settings SET customOpenAIConfig = NULL WHERE id = '1'")
                .execute(pool)
                .await?;
            return Ok(());
        }

        let api_key_column = match provider {
            "openai" => "openaiApiKey",
            "ollama" => "ollamaApiKey",
            "groq" => "groqApiKey",
            "claude" => "anthropicApiKey",
            "openrouter" => "openRouterApiKey",
            "builtin-ai" => return Ok(()), // No API key needed
            _ => {
                return Err(sqlx::Error::Protocol(
                    format!("Invalid provider: {}", provider).into(),
                ))
            }
        };

        let query = format!(
            "UPDATE settings SET {} = NULL WHERE id = '1'",
            api_key_column
        );
        sqlx::query(&query).execute(pool).await?;

        Ok(())
    }

    // ===== CUSTOM OPENAI CONFIG METHODS =====

    /// Gets the custom OpenAI configuration from JSON
    ///
    /// # Returns
    /// * `Ok(Some(CustomOpenAIConfig))` - Config exists and is valid JSON
    /// * `Ok(None)` - No config stored
    /// * `Err(sqlx::Error)` - Database error
    pub async fn get_custom_openai_config(
        pool: &SqlitePool,
    ) -> std::result::Result<Option<CustomOpenAIConfig>, sqlx::Error> {
        use sqlx::Row;

        let row = sqlx::query(
            r#"
            SELECT customOpenAIConfig
            FROM settings
            WHERE id = '1'
            LIMIT 1
            "#
        )
        .fetch_optional(pool)
        .await?;

        match row {
            Some(record) => {
                let config_json: Option<String> = record.get("customOpenAIConfig");

                if let Some(json) = config_json {
                    // Parse JSON into CustomOpenAIConfig
                    let config: CustomOpenAIConfig = serde_json::from_str(&json)
                        .map_err(|e| sqlx::Error::Protocol(
                            format!("Invalid JSON in customOpenAIConfig: {}", e).into()
                        ))?;

                    Ok(Some(config))
                } else {
                    Ok(None)
                }
            }
            None => Ok(None),
        }
    }

    /// Saves the custom OpenAI configuration as JSON
    ///
    /// # Arguments
    /// * `pool` - Database connection pool
    /// * `config` - CustomOpenAIConfig to save (includes endpoint, apiKey, model, maxTokens, temperature, topP)
    ///
    /// # Returns
    /// * `Ok(())` - Config saved successfully
    /// * `Err(sqlx::Error)` - Database or JSON serialization error
    pub async fn save_custom_openai_config(
        pool: &SqlitePool,
        config: &CustomOpenAIConfig,
    ) -> std::result::Result<(), sqlx::Error> {
        // Serialize config to JSON
        let config_json = serde_json::to_string(config)
            .map_err(|e| sqlx::Error::Protocol(
                format!("Failed to serialize config to JSON: {}", e).into()
            ))?;

        // Upsert into settings table
        sqlx::query(
            r#"
            INSERT INTO settings (id, provider, model, whisperModel, customOpenAIConfig)
            VALUES ('1', 'custom-openai', $1, 'large-v3', $2)
            ON CONFLICT(id) DO UPDATE SET
                customOpenAIConfig = excluded.customOpenAIConfig
            "#,
        )
        .bind(&config.model)
        .bind(config_json)
        .execute(pool)
        .await?;

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    async fn migrated_pool() -> SqlitePool {
        let pool = SqlitePool::connect("sqlite::memory:").await.unwrap();
        sqlx::migrate!("./migrations").run(&pool).await.unwrap();
        pool
    }

    #[tokio::test]
    async fn meeting_vocabulary_round_trips() {
        let pool = migrated_pool().await;

        assert_eq!(SettingsRepository::get_meeting_vocabulary(&pool).await.unwrap(), None);

        SettingsRepository::save_meeting_vocabulary(&pool, Some("陳大文, Zackriya"))
            .await
            .unwrap();
        assert_eq!(
            SettingsRepository::get_meeting_vocabulary(&pool).await.unwrap().as_deref(),
            Some("陳大文, Zackriya")
        );

        // Overwriting must not disturb an unrelated column already set on the row.
        SettingsRepository::save_transcript_config(&pool, "localWhisper", "large-v3")
            .await
            .unwrap();
        assert_eq!(
            SettingsRepository::get_meeting_vocabulary(&pool).await.unwrap().as_deref(),
            Some("陳大文, Zackriya")
        );
    }

    #[tokio::test]
    async fn whitespace_only_vocabulary_is_stored_as_none() {
        let pool = migrated_pool().await;

        SettingsRepository::save_meeting_vocabulary(&pool, Some("   \n\t  "))
            .await
            .unwrap();
        assert_eq!(SettingsRepository::get_meeting_vocabulary(&pool).await.unwrap(), None);
    }

    #[tokio::test]
    async fn clearing_vocabulary_removes_it() {
        let pool = migrated_pool().await;

        SettingsRepository::save_meeting_vocabulary(&pool, Some("Zackriya")).await.unwrap();
        SettingsRepository::save_meeting_vocabulary(&pool, None).await.unwrap();
        assert_eq!(SettingsRepository::get_meeting_vocabulary(&pool).await.unwrap(), None);
    }

    #[tokio::test]
    async fn saving_vocabulary_never_overwrites_an_already_chosen_provider() {
        let pool = migrated_pool().await;

        SettingsRepository::save_transcript_config(&pool, "localWhisper", "large-v3")
            .await
            .unwrap();
        SettingsRepository::save_meeting_vocabulary(&pool, Some("Zackriya"))
            .await
            .unwrap();

        let config = SettingsRepository::get_transcript_config(&pool).await.unwrap().unwrap();
        assert_eq!(config.provider, "localWhisper");
        assert_eq!(config.model, "large-v3");
    }

    #[tokio::test]
    async fn script_setting_round_trips() {
        let pool = migrated_pool().await;

        assert_eq!(SettingsRepository::get_script_setting(&pool).await.unwrap(), None);

        SettingsRepository::save_script_setting(&pool, "simplified")
            .await
            .unwrap();
        assert_eq!(
            SettingsRepository::get_script_setting(&pool).await.unwrap().as_deref(),
            Some("simplified")
        );

        // Overwriting must not disturb an unrelated column already set on the row.
        SettingsRepository::save_transcript_config(&pool, "localWhisper", "large-v3")
            .await
            .unwrap();
        assert_eq!(
            SettingsRepository::get_script_setting(&pool).await.unwrap().as_deref(),
            Some("simplified")
        );
    }

    #[tokio::test]
    async fn diarization_consent_round_trips() {
        let pool = migrated_pool().await;

        assert_eq!(SettingsRepository::get_diarization_consent(&pool).await.unwrap(), None);

        SettingsRepository::save_diarization_consent(&pool, "granted")
            .await
            .unwrap();
        assert_eq!(
            SettingsRepository::get_diarization_consent(&pool).await.unwrap().as_deref(),
            Some("granted")
        );

        // Overwriting must not disturb an unrelated column already set on the same row.
        SettingsRepository::save_model_config(&pool, "openai", "gpt-4o", "large-v3", None)
            .await
            .unwrap();
        assert_eq!(
            SettingsRepository::get_diarization_consent(&pool).await.unwrap().as_deref(),
            Some("granted")
        );
    }

    #[tokio::test]
    async fn rendering_provider_round_trips() {
        let pool = migrated_pool().await;

        assert_eq!(SettingsRepository::get_rendering_provider(&pool).await.unwrap(), None);

        SettingsRepository::save_rendering_provider(&pool, "summary_provider")
            .await
            .unwrap();
        assert_eq!(
            SettingsRepository::get_rendering_provider(&pool).await.unwrap().as_deref(),
            Some("summary_provider")
        );

        // Overwriting must not disturb an unrelated column already set on the same row.
        SettingsRepository::save_model_config(&pool, "openai", "gpt-4o", "large-v3", None)
            .await
            .unwrap();
        assert_eq!(
            SettingsRepository::get_rendering_provider(&pool).await.unwrap().as_deref(),
            Some("summary_provider")
        );
    }

    #[tokio::test]
    async fn saving_script_setting_never_overwrites_an_already_chosen_provider() {
        let pool = migrated_pool().await;

        SettingsRepository::save_transcript_config(&pool, "localWhisper", "large-v3")
            .await
            .unwrap();
        SettingsRepository::save_script_setting(&pool, "as-recognized")
            .await
            .unwrap();

        let config = SettingsRepository::get_transcript_config(&pool).await.unwrap().unwrap();
        assert_eq!(config.provider, "localWhisper");
        assert_eq!(config.model, "large-v3");
    }
}
