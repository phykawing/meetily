// setting_store.rs
//
// One typed accessor for scalar app-wide preferences (Script, Rendering provider,
// diarization consent, meeting vocabulary, and the per-provider API keys), replacing a
// hand-written `get_*`/`save_*` pair per setting on `SettingsRepository`. Each preference
// is declared once as a `StoredSetting` const naming its table and column; enum-backed
// preferences additionally implement `SettingToken`, giving lenient reads (unknown/absent
// token resolves to the type's default — for UI display and must-not-fail paths) and
// strict writes (an unrecognized token is rejected rather than silently rewritten) from
// one token table instead of a hand-written `match` per direction.
//
// Both `settings` and `transcript_settings` are single-row tables (`id = '1'`) whose
// `provider`/`model`/`whisperModel` columns are `NOT NULL` with no SQL `DEFAULT` — so the
// first write of any scalar setting into an empty table must also supply those. The seed
// values below are that app's existing defaults (see `onboarding.rs`,
// `database/commands.rs`), not new ones: `'large-v3'` intentionally matches those, not
// `config::DEFAULT_WHISPER_MODEL` ("large-v3-turbo"), which nothing here writes.

use sqlx::SqlitePool;

/// Which single-row settings table a `StoredSetting` lives in.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SettingsTable {
    /// App-wide summary/LLM config plus app-wide policy toggles (diarization consent,
    /// Rendering provider).
    Settings,
    /// Transcription-pipeline config and preferences (meeting vocabulary, Script).
    TranscriptSettings,
}

impl SettingsTable {
    fn name(self) -> &'static str {
        match self {
            SettingsTable::Settings => "settings",
            SettingsTable::TranscriptSettings => "transcript_settings",
        }
    }
}

/// A single scalar preference: which table and column it lives in. Read and write both
/// operate on the single row `id = '1'`.
#[derive(Debug, Clone, Copy)]
pub struct StoredSetting {
    table: SettingsTable,
    column: &'static str,
}

impl StoredSetting {
    pub const fn new(table: SettingsTable, column: &'static str) -> Self {
        Self { table, column }
    }

    /// Reads the raw stored value. `None` covers both "no settings row exists yet" and
    /// "the column is NULL" — callers needing a value resolve the default themselves
    /// (`read_or_default`, or `SettingToken::from_token` for enum-backed settings).
    pub async fn read_text(&self, pool: &SqlitePool) -> Result<Option<String>, sqlx::Error> {
        let query = format!(
            "SELECT {} FROM {} WHERE id = '1' LIMIT 1",
            self.column,
            self.table.name(),
        );
        let value: Option<Option<String>> = sqlx::query_scalar(&query).fetch_optional(pool).await?;
        Ok(value.flatten())
    }

    /// Writes free text (`None` stores SQL `NULL`). Atomic upsert via `ON CONFLICT`,
    /// unlike the UPDATE-then-INSERT-if-zero-rows pattern this replaces, which left a
    /// window for two concurrent writers to both see zero rows affected and race on the
    /// fallback INSERT. `ON CONFLICT DO UPDATE` touches only this column, so an existing
    /// row's `provider`/`model` are never disturbed by writing an unrelated setting.
    pub async fn write_text(&self, pool: &SqlitePool, value: Option<&str>) -> Result<(), sqlx::Error> {
        match self.table {
            SettingsTable::Settings => {
                let sql = format!(
                    r#"
                    INSERT INTO settings (id, provider, model, whisperModel, "{col}")
                    VALUES ('1', 'openai', 'gpt-4o-2024-11-20', 'large-v3', $1)
                    ON CONFLICT(id) DO UPDATE SET "{col}" = excluded."{col}"
                    "#,
                    col = self.column,
                );
                sqlx::query(&sql).bind(value).execute(pool).await?;
            }
            SettingsTable::TranscriptSettings => {
                let sql = format!(
                    r#"
                    INSERT INTO transcript_settings (id, provider, model, "{col}")
                    VALUES ('1', 'parakeet', $1, $2)
                    ON CONFLICT(id) DO UPDATE SET "{col}" = excluded."{col}"
                    "#,
                    col = self.column,
                );
                sqlx::query(&sql)
                    .bind(crate::config::DEFAULT_PARAKEET_MODEL)
                    .bind(value)
                    .execute(pool)
                    .await?;
            }
        }
        Ok(())
    }

    /// Clears this setting by writing SQL `NULL`, without creating a row if none exists —
    /// unlike `write_text`, which upserts. Matches the pre-existing `delete_api_key`
    /// semantics: deleting a key from a settings row that doesn't exist yet is a no-op,
    /// not a trigger to create one with a seeded provider.
    pub async fn clear(&self, pool: &SqlitePool) -> Result<(), sqlx::Error> {
        let sql = format!(
            "UPDATE {} SET {} = NULL WHERE id = '1'",
            self.table.name(),
            self.column,
        );
        sqlx::query(&sql).execute(pool).await?;
        Ok(())
    }

    /// Reads and resolves to a typed value, propagating a DB error. For call sites where
    /// a settings read failing must surface rather than silently change behaviour (e.g.
    /// the diarization consent gate, or "same as summary provider" for Rendering).
    pub async fn read<T: SettingToken>(&self, pool: &SqlitePool) -> Result<T, sqlx::Error> {
        let stored = self.read_text(pool).await?;
        Ok(T::from_token(stored.as_deref()))
    }

    /// Reads and resolves to a typed value, defaulting (and logging) on DB error instead
    /// of propagating. For call sites — like Script on the transcription path — that must
    /// not fail just because a settings read did.
    pub async fn read_or_default<T: SettingToken>(&self, pool: &SqlitePool) -> T {
        match self.read::<T>(pool).await {
            Ok(value) => value,
            Err(e) => {
                log::warn!(
                    "Failed to read setting {}.{}, using default: {}",
                    self.table.name(),
                    self.column,
                    e
                );
                T::default()
            }
        }
    }

    /// Writes a typed value as its stored token.
    pub async fn write<T: SettingToken>(&self, pool: &SqlitePool, value: T) -> Result<(), sqlx::Error> {
        self.write_text(pool, Some(value.as_token())).await
    }
}

/// A fixed set of tokens a scalar setting is stored as. `TOKENS` is the single source of
/// truth for a type's stored representation — `as_token`, the lenient `from_token` (reads)
/// and the strict `parse` (writes) are all derived from it, rather than three independently
/// maintained `match` bodies.
pub trait SettingToken: Copy + Eq + Default + 'static {
    /// Every valid token paired with its variant, in declaration order.
    const TOKENS: &'static [(&'static str, Self)];

    fn as_token(self) -> &'static str {
        Self::TOKENS
            .iter()
            .find(|(_, v)| *v == self)
            .map(|(token, _)| *token)
            .expect("SettingToken::TOKENS must cover every variant")
    }

    /// Resolves a stored token into a value. Unrecognized or absent tokens fall back to
    /// `Self::default()` — the safe choice for a read, since this reads a value a user
    /// picked from a fixed set of options and a DB read must not itself fail the caller.
    fn from_token(token: Option<&str>) -> Self {
        token
            .and_then(|t| Self::TOKENS.iter().find(|(k, _)| *k == t))
            .map(|(_, v)| *v)
            .unwrap_or_default()
    }

    /// Parses a token strictly: an unrecognized token is rejected rather than silently
    /// coerced to the default. For write paths — a stale frontend build or malformed
    /// input must surface as an error, not overwrite a real answer with an unintended one.
    fn parse(token: &str) -> Result<Self, String> {
        Self::TOKENS
            .iter()
            .find(|(k, _)| *k == token)
            .map(|(_, v)| *v)
            .ok_or_else(|| format!("Unrecognized value: {}", token))
    }
}

// ─── Setting declarations ──────────────────────────────────────────────────────────────
//
// The complete list of scalar preferences the app persists. Adding a new one is one line
// here (plus, for an enum-backed setting, a `SettingToken` impl on the type) instead of a
// migration-sized amount of repository, command and resolver code.

/// Free text folded into the Whisper initial prompt. Use `write_meeting_vocabulary` to
/// write it, not `write_text` directly — it normalizes whitespace-only input to "unset".
pub const MEETING_VOCABULARY: StoredSetting =
    StoredSetting::new(SettingsTable::TranscriptSettings, "meetingVocabulary");

/// Which Han character set Chinese transcripts are converted into at ingest. See
/// `crate::script::ScriptSetting` and docs/adr/0003.
pub const SCRIPT: StoredSetting = StoredSetting::new(SettingsTable::TranscriptSettings, "scriptSetting");

/// Whether the user has answered the diarization model-download prompt. See
/// `crate::diarization::consent::DiarizationConsent` and docs/adr/0005.
pub const DIARIZATION_CONSENT: StoredSetting =
    StoredSetting::new(SettingsTable::Settings, "diarizationConsent");

/// Which LLM provider performs a Written Form Rendering. See
/// `crate::rendering::RenderingProvider` and docs/adr/0002.
pub const RENDERING_PROVIDER: StoredSetting =
    StoredSetting::new(SettingsTable::Settings, "renderingProvider");

/// Saves the meeting vocabulary, normalizing whitespace-only input to no vocabulary at
/// all — an empty vocabulary should produce no prompt, not an empty-string one.
pub async fn write_meeting_vocabulary(
    pool: &SqlitePool,
    vocabulary: Option<&str>,
) -> Result<(), sqlx::Error> {
    let cleaned = vocabulary.map(str::trim).filter(|v| !v.is_empty());
    MEETING_VOCABULARY.write_text(pool, cleaned).await
}

/// Maps a provider token to its API-key column. `Ok(None)` means the provider needs no key
/// (`builtin-ai`, `parakeet`); an unrecognized provider is `Err`, matching the
/// `sqlx::Error::Protocol("Invalid provider: ...")` convention `SettingsRepository`
/// already used before this module existed.
pub(crate) fn api_key_column(
    table: SettingsTable,
    provider: &str,
) -> Result<Option<&'static str>, sqlx::Error> {
    let column = match (table, provider) {
        (SettingsTable::Settings, "openai") => "openaiApiKey",
        (SettingsTable::Settings, "claude") => "anthropicApiKey",
        (SettingsTable::Settings, "ollama") => "ollamaApiKey",
        (SettingsTable::Settings, "groq") => "groqApiKey",
        (SettingsTable::Settings, "openrouter") => "openRouterApiKey",
        (SettingsTable::Settings, "builtin-ai") => return Ok(None),
        (SettingsTable::TranscriptSettings, "localWhisper") => "whisperApiKey",
        (SettingsTable::TranscriptSettings, "parakeet") => return Ok(None),
        (SettingsTable::TranscriptSettings, "deepgram") => "deepgramApiKey",
        (SettingsTable::TranscriptSettings, "elevenLabs") => "elevenLabsApiKey",
        (SettingsTable::TranscriptSettings, "groq") => "groqApiKey",
        (SettingsTable::TranscriptSettings, "openai") => "openaiApiKey",
        _ => {
            return Err(sqlx::Error::Protocol(
                format!("Invalid provider: {}", provider).into(),
            ))
        }
    };
    Ok(Some(column))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::diarization::consent::DiarizationConsent;
    use crate::rendering::RenderingProvider;
    use crate::script::ScriptSetting;

    async fn migrated_pool() -> SqlitePool {
        let pool = SqlitePool::connect("sqlite::memory:").await.unwrap();
        sqlx::migrate!("./migrations").run(&pool).await.unwrap();
        pool
    }

    #[tokio::test]
    async fn script_setting_round_trips_and_defaults() {
        let pool = migrated_pool().await;
        assert_eq!(SCRIPT.read_or_default::<ScriptSetting>(&pool).await, ScriptSetting::TraditionalHk);

        SCRIPT.write(&pool, ScriptSetting::Simplified).await.unwrap();
        assert_eq!(SCRIPT.read_or_default::<ScriptSetting>(&pool).await, ScriptSetting::Simplified);
        assert_eq!(SCRIPT.read::<ScriptSetting>(&pool).await.unwrap(), ScriptSetting::Simplified);
    }

    #[tokio::test]
    async fn diarization_consent_round_trips_and_defaults() {
        let pool = migrated_pool().await;
        assert_eq!(
            DIARIZATION_CONSENT.read::<DiarizationConsent>(&pool).await.unwrap(),
            DiarizationConsent::NotAsked
        );

        DIARIZATION_CONSENT.write(&pool, DiarizationConsent::Granted).await.unwrap();
        assert_eq!(
            DIARIZATION_CONSENT.read::<DiarizationConsent>(&pool).await.unwrap(),
            DiarizationConsent::Granted
        );
    }

    #[tokio::test]
    async fn rendering_provider_round_trips_and_defaults() {
        let pool = migrated_pool().await;
        assert_eq!(
            RENDERING_PROVIDER.read::<RenderingProvider>(&pool).await.unwrap(),
            RenderingProvider::Local
        );

        RENDERING_PROVIDER.write(&pool, RenderingProvider::SummaryProvider).await.unwrap();
        assert_eq!(
            RENDERING_PROVIDER.read::<RenderingProvider>(&pool).await.unwrap(),
            RenderingProvider::SummaryProvider
        );
    }

    #[tokio::test]
    async fn meeting_vocabulary_round_trips() {
        let pool = migrated_pool().await;
        assert_eq!(MEETING_VOCABULARY.read_text(&pool).await.unwrap(), None);

        write_meeting_vocabulary(&pool, Some("陳大文, Zackriya")).await.unwrap();
        assert_eq!(
            MEETING_VOCABULARY.read_text(&pool).await.unwrap(),
            Some("陳大文, Zackriya".to_string())
        );
    }

    #[tokio::test]
    async fn whitespace_only_vocabulary_is_stored_as_none() {
        let pool = migrated_pool().await;
        write_meeting_vocabulary(&pool, Some("   \n\t  ")).await.unwrap();
        assert_eq!(MEETING_VOCABULARY.read_text(&pool).await.unwrap(), None);
    }

    #[tokio::test]
    async fn clearing_vocabulary_removes_it() {
        let pool = migrated_pool().await;
        write_meeting_vocabulary(&pool, Some("Zackriya")).await.unwrap();
        write_meeting_vocabulary(&pool, None).await.unwrap();
        assert_eq!(MEETING_VOCABULARY.read_text(&pool).await.unwrap(), None);
    }

    #[test]
    fn parse_rejects_an_unknown_token_that_from_token_accepts_leniently() {
        assert!(ScriptSetting::parse("bogus").is_err());
        assert_eq!(ScriptSetting::from_token(Some("bogus")), ScriptSetting::TraditionalHk);

        assert!(DiarizationConsent::parse("bogus").is_err());
        assert_eq!(DiarizationConsent::from_token(Some("bogus")), DiarizationConsent::NotAsked);
    }

    #[test]
    fn parse_accepts_every_declared_token() {
        for (token, expected) in ScriptSetting::TOKENS {
            assert_eq!(ScriptSetting::parse(token).unwrap(), *expected);
        }
        for (token, expected) in DiarizationConsent::TOKENS {
            assert_eq!(DiarizationConsent::parse(token).unwrap(), *expected);
        }
        for (token, expected) in RenderingProvider::TOKENS {
            assert_eq!(RenderingProvider::parse(token).unwrap(), *expected);
        }
    }

    /// The property every one of the four settings must have, checked once instead of
    /// once per setting: writing a scalar setting into an empty table seeds a row with the
    /// app's documented default provider, and a later write of a *different* setting must
    /// not disturb that provider (the reason the old UPDATE-then-INSERT-fallback existed).
    #[tokio::test]
    async fn writing_a_setting_never_overwrites_an_already_chosen_provider() {
        // transcript_settings: choose a provider first, then write both of its settings.
        let pool = migrated_pool().await;
        crate::database::repositories::setting::SettingsRepository::save_transcript_config(
            &pool, "localWhisper", "large-v3",
        )
        .await
        .unwrap();
        SCRIPT.write(&pool, ScriptSetting::Simplified).await.unwrap();
        write_meeting_vocabulary(&pool, Some("hello")).await.unwrap();
        let config = crate::database::repositories::setting::SettingsRepository::get_transcript_config(&pool)
            .await
            .unwrap()
            .unwrap();
        assert_eq!(config.provider, "localWhisper");
        assert_eq!(config.model, "large-v3");

        // settings: same property for the other table.
        crate::database::repositories::setting::SettingsRepository::save_model_config(
            &pool, "openai", "gpt-4o", "large-v3", None,
        )
        .await
        .unwrap();
        DIARIZATION_CONSENT.write(&pool, DiarizationConsent::Granted).await.unwrap();
        RENDERING_PROVIDER.write(&pool, RenderingProvider::SummaryProvider).await.unwrap();
        let config = crate::database::repositories::setting::SettingsRepository::get_model_config(&pool)
            .await
            .unwrap()
            .unwrap();
        assert_eq!(config.provider, "openai");
        assert_eq!(config.model, "gpt-4o");
    }

    #[tokio::test]
    async fn writing_one_setting_does_not_disturb_another_in_the_same_table() {
        let pool = migrated_pool().await;
        SCRIPT.write(&pool, ScriptSetting::Simplified).await.unwrap();
        write_meeting_vocabulary(&pool, Some("hello")).await.unwrap();
        assert_eq!(SCRIPT.read_or_default::<ScriptSetting>(&pool).await, ScriptSetting::Simplified);

        DIARIZATION_CONSENT.write(&pool, DiarizationConsent::Granted).await.unwrap();
        RENDERING_PROVIDER.write(&pool, RenderingProvider::SummaryProvider).await.unwrap();
        assert_eq!(
            DIARIZATION_CONSENT.read::<DiarizationConsent>(&pool).await.unwrap(),
            DiarizationConsent::Granted
        );
    }

    #[tokio::test]
    async fn api_key_round_trips_per_provider_and_table() {
        let pool = migrated_pool().await;
        for provider in ["openai", "claude", "ollama", "groq", "openrouter"] {
            let column = api_key_column(SettingsTable::Settings, provider).unwrap().unwrap();
            let setting = StoredSetting::new(SettingsTable::Settings, column);
            setting.write_text(&pool, Some("secret-key")).await.unwrap();
            assert_eq!(setting.read_text(&pool).await.unwrap(), Some("secret-key".to_string()));
        }
        for provider in ["localWhisper", "deepgram", "elevenLabs", "groq", "openai"] {
            let column = api_key_column(SettingsTable::TranscriptSettings, provider).unwrap().unwrap();
            let setting = StoredSetting::new(SettingsTable::TranscriptSettings, column);
            setting.write_text(&pool, Some("secret-key")).await.unwrap();
            assert_eq!(setting.read_text(&pool).await.unwrap(), Some("secret-key".to_string()));
        }
    }

    #[test]
    fn providers_needing_no_key_resolve_to_none() {
        assert_eq!(api_key_column(SettingsTable::Settings, "builtin-ai").unwrap(), None);
        assert_eq!(api_key_column(SettingsTable::TranscriptSettings, "parakeet").unwrap(), None);
    }

    #[test]
    fn unknown_provider_is_rejected() {
        assert!(api_key_column(SettingsTable::Settings, "bogus").is_err());
        assert!(api_key_column(SettingsTable::TranscriptSettings, "bogus").is_err());
    }

    #[tokio::test]
    async fn clear_nulls_the_column_without_creating_a_row() {
        let pool = migrated_pool().await;
        let setting = StoredSetting::new(SettingsTable::Settings, "openaiApiKey");
        setting.clear(&pool).await.unwrap();
        assert!(
            crate::database::repositories::setting::SettingsRepository::get_model_config(&pool)
                .await
                .unwrap()
                .is_none()
        );

        setting.write_text(&pool, Some("secret-key")).await.unwrap();
        setting.clear(&pool).await.unwrap();
        assert_eq!(setting.read_text(&pool).await.unwrap(), None);
    }
}
