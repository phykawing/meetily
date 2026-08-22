use crate::database::models::{Setting, TranscriptSetting};
use crate::database::repositories::setting_store::{self, SettingsTable, StoredSetting};
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
        let setting = sqlx::query_as::<_, Setting>("SELECT * FROM settings WHERE id = '1' LIMIT 1")
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

        match setting_store::api_key_column(SettingsTable::Settings, provider)? {
            Some(column) => {
                StoredSetting::new(SettingsTable::Settings, column)
                    .write_text(pool, Some(api_key))
                    .await
            }
            None => Ok(()), // No API key needed (builtin-ai)
        }
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

        match setting_store::api_key_column(SettingsTable::Settings, provider)? {
            Some(column) => StoredSetting::new(SettingsTable::Settings, column).read_text(pool).await,
            None => Ok(None), // No API key needed (builtin-ai)
        }
    }

    pub async fn get_transcript_config(
        pool: &SqlitePool,
    ) -> std::result::Result<Option<TranscriptSetting>, sqlx::Error> {
        let setting = sqlx::query_as::<_, TranscriptSetting>(
            "SELECT * FROM transcript_settings WHERE id = '1' LIMIT 1",
        )
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

    pub async fn save_transcript_api_key(
        pool: &SqlitePool,
        provider: &str,
        api_key: &str,
    ) -> std::result::Result<(), sqlx::Error> {
        match setting_store::api_key_column(SettingsTable::TranscriptSettings, provider)? {
            Some(column) => {
                StoredSetting::new(SettingsTable::TranscriptSettings, column)
                    .write_text(pool, Some(api_key))
                    .await
            }
            None => Ok(()), // No API key needed (parakeet)
        }
    }

    pub async fn get_transcript_api_key(
        pool: &SqlitePool,
        provider: &str,
    ) -> std::result::Result<Option<String>, sqlx::Error> {
        match setting_store::api_key_column(SettingsTable::TranscriptSettings, provider)? {
            Some(column) => {
                StoredSetting::new(SettingsTable::TranscriptSettings, column)
                    .read_text(pool)
                    .await
            }
            None => Ok(None), // No API key needed (parakeet)
        }
    }

    /// Currently unreachable from the frontend: its only caller, `api_delete_api_key`, is
    /// not registered in the `tauri::generate_handler!` list in `lib.rs`. Kept working and
    /// tested rather than deleted, since wiring it up is a one-line change elsewhere.
    pub async fn delete_api_key(
        pool: &SqlitePool,
        provider: &str,
    ) -> std::result::Result<(), sqlx::Error> {
        // Custom OpenAI uses JSON config - clear the entire config, not just one column.
        if provider == "custom-openai" {
            sqlx::query("UPDATE settings SET customOpenAIConfig = NULL WHERE id = '1'")
                .execute(pool)
                .await?;
            return Ok(());
        }

        match setting_store::api_key_column(SettingsTable::Settings, provider)? {
            Some(column) => StoredSetting::new(SettingsTable::Settings, column).clear(pool).await,
            None => Ok(()), // No API key needed (builtin-ai)
        }
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

    // The four scalar-token settings (meeting vocabulary, Script, diarization consent,
    // Rendering provider) moved to `setting_store` — see its test module for their
    // round-trip and no-clobber coverage. Tests here cover what stays in this file: whole
    // rows, and the API-key functions, which had zero coverage before this refactor.

    #[tokio::test]
    async fn summary_api_key_round_trips_per_provider() {
        let pool = migrated_pool().await;
        for provider in ["openai", "claude", "ollama", "groq", "openrouter"] {
            assert_eq!(SettingsRepository::get_api_key(&pool, provider).await.unwrap(), None);
            SettingsRepository::save_api_key(&pool, provider, "secret-key").await.unwrap();
            assert_eq!(
                SettingsRepository::get_api_key(&pool, provider).await.unwrap().as_deref(),
                Some("secret-key")
            );
        }
    }

    #[tokio::test]
    async fn transcript_api_key_round_trips_per_provider() {
        let pool = migrated_pool().await;
        for provider in ["localWhisper", "deepgram", "elevenLabs", "groq", "openai"] {
            assert_eq!(
                SettingsRepository::get_transcript_api_key(&pool, provider).await.unwrap(),
                None
            );
            SettingsRepository::save_transcript_api_key(&pool, provider, "secret-key")
                .await
                .unwrap();
            assert_eq!(
                SettingsRepository::get_transcript_api_key(&pool, provider)
                    .await
                    .unwrap()
                    .as_deref(),
                Some("secret-key")
            );
        }
    }

    #[tokio::test]
    async fn builtin_ai_and_parakeet_need_no_key() {
        let pool = migrated_pool().await;

        assert_eq!(SettingsRepository::get_api_key(&pool, "builtin-ai").await.unwrap(), None);
        SettingsRepository::save_api_key(&pool, "builtin-ai", "ignored").await.unwrap();
        assert_eq!(SettingsRepository::get_api_key(&pool, "builtin-ai").await.unwrap(), None);

        assert_eq!(
            SettingsRepository::get_transcript_api_key(&pool, "parakeet").await.unwrap(),
            None
        );
        SettingsRepository::save_transcript_api_key(&pool, "parakeet", "ignored").await.unwrap();
        assert_eq!(
            SettingsRepository::get_transcript_api_key(&pool, "parakeet").await.unwrap(),
            None
        );
    }

    #[tokio::test]
    async fn unknown_provider_is_rejected_on_read_save_and_delete() {
        let pool = migrated_pool().await;
        assert!(SettingsRepository::get_api_key(&pool, "bogus").await.is_err());
        assert!(SettingsRepository::save_api_key(&pool, "bogus", "key").await.is_err());
        assert!(SettingsRepository::delete_api_key(&pool, "bogus").await.is_err());
        assert!(SettingsRepository::get_transcript_api_key(&pool, "bogus").await.is_err());
        assert!(SettingsRepository::save_transcript_api_key(&pool, "bogus", "key").await.is_err());
    }

    #[tokio::test]
    async fn saving_an_api_key_never_overwrites_an_already_chosen_provider() {
        let pool = migrated_pool().await;

        SettingsRepository::save_model_config(&pool, "claude", "claude-3", "large-v3", None)
            .await
            .unwrap();
        SettingsRepository::save_api_key(&pool, "openai", "secret-key").await.unwrap();
        let config = SettingsRepository::get_model_config(&pool).await.unwrap().unwrap();
        assert_eq!(config.provider, "claude");
        assert_eq!(config.model, "claude-3");

        SettingsRepository::save_transcript_config(&pool, "localWhisper", "large-v3")
            .await
            .unwrap();
        SettingsRepository::save_transcript_api_key(&pool, "groq", "secret-key").await.unwrap();
        let config = SettingsRepository::get_transcript_config(&pool).await.unwrap().unwrap();
        assert_eq!(config.provider, "localWhisper");
        assert_eq!(config.model, "large-v3");
    }

    #[tokio::test]
    async fn delete_api_key_nulls_exactly_one_column() {
        let pool = migrated_pool().await;
        SettingsRepository::save_api_key(&pool, "openai", "openai-key").await.unwrap();
        SettingsRepository::save_api_key(&pool, "claude", "claude-key").await.unwrap();

        SettingsRepository::delete_api_key(&pool, "openai").await.unwrap();

        assert_eq!(SettingsRepository::get_api_key(&pool, "openai").await.unwrap(), None);
        assert_eq!(
            SettingsRepository::get_api_key(&pool, "claude").await.unwrap().as_deref(),
            Some("claude-key")
        );
    }

    #[tokio::test]
    async fn deleting_an_api_key_on_an_empty_table_is_a_no_op() {
        let pool = migrated_pool().await;
        // No settings row exists yet — deleting must not create one.
        SettingsRepository::delete_api_key(&pool, "openai").await.unwrap();
        assert!(SettingsRepository::get_model_config(&pool).await.unwrap().is_none());
    }

    #[tokio::test]
    async fn custom_openai_api_key_delegates_to_the_json_config() {
        let pool = migrated_pool().await;

        assert_eq!(SettingsRepository::get_api_key(&pool, "custom-openai").await.unwrap(), None);
        assert!(SettingsRepository::save_api_key(&pool, "custom-openai", "ignored")
            .await
            .is_err());

        let config = CustomOpenAIConfig {
            endpoint: "https://example.com".to_string(),
            api_key: Some("custom-key".to_string()),
            model: "custom-model".to_string(),
            max_tokens: None,
            temperature: None,
            top_p: None,
        };
        SettingsRepository::save_custom_openai_config(&pool, &config).await.unwrap();
        assert_eq!(
            SettingsRepository::get_api_key(&pool, "custom-openai").await.unwrap().as_deref(),
            Some("custom-key")
        );

        // Deleting a custom-openai key clears the whole config, not just the key, unlike
        // every other provider's delete.
        SettingsRepository::delete_api_key(&pool, "custom-openai").await.unwrap();
        assert!(SettingsRepository::get_custom_openai_config(&pool).await.unwrap().is_none());
    }

    #[tokio::test]
    async fn model_config_and_transcript_config_round_trip() {
        let pool = migrated_pool().await;
        assert!(SettingsRepository::get_model_config(&pool).await.unwrap().is_none());
        assert!(SettingsRepository::get_transcript_config(&pool).await.unwrap().is_none());

        SettingsRepository::save_model_config(&pool, "openai", "gpt-4o", "large-v3", None)
            .await
            .unwrap();
        let config = SettingsRepository::get_model_config(&pool).await.unwrap().unwrap();
        assert_eq!(config.provider, "openai");
        assert_eq!(config.model, "gpt-4o");

        SettingsRepository::save_transcript_config(&pool, "parakeet", "parakeet-tdt-0.6b-v3-int8")
            .await
            .unwrap();
        let config = SettingsRepository::get_transcript_config(&pool).await.unwrap().unwrap();
        assert_eq!(config.provider, "parakeet");
    }
}
