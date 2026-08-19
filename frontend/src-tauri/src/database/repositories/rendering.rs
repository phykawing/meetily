use chrono::Utc;
use sqlx::{Error as SqlxError, SqlitePool};

pub struct RenderingRepository;

impl RenderingRepository {
    /// Gets the persisted Written Form preference token for a meeting. `None` when the
    /// meeting does not exist; callers resolve a `Some(_)` value via
    /// `crate::rendering::WrittenForm::from_stored`.
    pub async fn get_written_form(
        pool: &SqlitePool,
        meeting_id: &str,
    ) -> Result<Option<String>, SqlxError> {
        let value: Option<String> =
            sqlx::query_scalar("SELECT written_form FROM meetings WHERE id = ?")
                .bind(meeting_id)
                .fetch_optional(pool)
                .await?;
        Ok(value)
    }

    /// Saves the Written Form preference for a meeting. Returns `false` when no meeting
    /// with that id exists.
    pub async fn set_written_form(
        pool: &SqlitePool,
        meeting_id: &str,
        written_form: &str,
    ) -> Result<bool, SqlxError> {
        let result = sqlx::query("UPDATE meetings SET written_form = ? WHERE id = ?")
            .bind(written_form)
            .bind(meeting_id)
            .execute(pool)
            .await?;
        Ok(result.rows_affected() > 0)
    }

    /// Gets the Canonical Transcript for a meeting as ordered segment texts. Ordering
    /// matches playback order, so the same meeting always fingerprints the same way.
    ///
    /// `audio_start_time` is nullable (added by a later migration, with no backfill for
    /// rows that predate it), and SQLite sorts NULL before any real value in `ASC` order —
    /// so segments without timing are ordered first, not last, before `id` as the
    /// tie-breaker for a stable order among them.
    pub async fn get_canonical_segments(
        pool: &SqlitePool,
        meeting_id: &str,
    ) -> Result<Vec<String>, SqlxError> {
        let rows: Vec<(String,)> = sqlx::query_as(
            "SELECT transcript FROM transcripts WHERE meeting_id = ? \
             ORDER BY (audio_start_time IS NULL) ASC, audio_start_time ASC, id ASC",
        )
        .bind(meeting_id)
        .fetch_all(pool)
        .await?;

        Ok(rows.into_iter().map(|(text,)| text).collect())
    }

    /// Gets the cached Rendering for a meeting, if any: `(rendered_text, source_fingerprint)`.
    /// The caller is responsible for checking the fingerprint against the current transcript
    /// to decide whether the cache is stale.
    pub async fn get_cached_rendering(
        pool: &SqlitePool,
        meeting_id: &str,
    ) -> Result<Option<(String, String)>, SqlxError> {
        let row: Option<(String, String)> = sqlx::query_as(
            "SELECT rendered_text, source_fingerprint FROM transcript_renderings WHERE meeting_id = ?",
        )
        .bind(meeting_id)
        .fetch_optional(pool)
        .await?;

        Ok(row)
    }

    /// Saves (or replaces) the cached Rendering for a meeting.
    pub async fn save_rendering(
        pool: &SqlitePool,
        meeting_id: &str,
        rendered_text: &str,
        source_fingerprint: &str,
    ) -> Result<(), SqlxError> {
        let now = Utc::now().to_rfc3339();

        sqlx::query(
            r#"
            INSERT INTO transcript_renderings (meeting_id, rendered_text, source_fingerprint, generated_at)
            VALUES (?, ?, ?, ?)
            ON CONFLICT(meeting_id) DO UPDATE SET
                rendered_text = excluded.rendered_text,
                source_fingerprint = excluded.source_fingerprint,
                generated_at = excluded.generated_at
            "#,
        )
        .bind(meeting_id)
        .bind(rendered_text)
        .bind(source_fingerprint)
        .bind(now)
        .execute(pool)
        .await?;

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::database::repositories::transcript::TranscriptsRepository;
    use crate::api::TranscriptSegment;

    async fn migrated_pool() -> SqlitePool {
        let pool = SqlitePool::connect("sqlite::memory:").await.unwrap();
        sqlx::migrate!("./migrations").run(&pool).await.unwrap();
        pool
    }

    async fn seed_meeting(pool: &SqlitePool, texts: &[&str]) -> String {
        let segments: Vec<TranscriptSegment> = texts
            .iter()
            .enumerate()
            .map(|(i, text)| TranscriptSegment {
                id: format!("seg-{}", i),
                text: text.to_string(),
                timestamp: format!("{}", i),
                audio_start_time: Some(i as f64),
                audio_end_time: Some(i as f64 + 1.0),
                duration: Some(1.0),
                audio_source: None,
            })
            .collect();

        TranscriptsRepository::save_transcript(pool, "Test meeting", &segments, None)
            .await
            .unwrap()
    }

    #[tokio::test]
    async fn written_form_defaults_to_colloquial_and_round_trips() {
        let pool = migrated_pool().await;
        let meeting_id = seed_meeting(&pool, &["係咁㗎啦"]).await;

        let stored = RenderingRepository::get_written_form(&pool, &meeting_id)
            .await
            .unwrap();
        assert_eq!(stored.as_deref(), Some("colloquial"));

        let updated = RenderingRepository::set_written_form(&pool, &meeting_id, "written")
            .await
            .unwrap();
        assert!(updated);

        let stored = RenderingRepository::get_written_form(&pool, &meeting_id)
            .await
            .unwrap();
        assert_eq!(stored.as_deref(), Some("written"));
    }

    #[tokio::test]
    async fn setting_written_form_on_missing_meeting_reports_no_update() {
        let pool = migrated_pool().await;
        let updated = RenderingRepository::set_written_form(&pool, "does-not-exist", "written")
            .await
            .unwrap();
        assert!(!updated);
    }

    #[tokio::test]
    async fn canonical_segments_are_ordered_by_audio_start_time() {
        let pool = migrated_pool().await;
        let meeting_id = seed_meeting(&pool, &["first", "second", "third"]).await;

        let segments = RenderingRepository::get_canonical_segments(&pool, &meeting_id)
            .await
            .unwrap();
        assert_eq!(segments, vec!["first", "second", "third"]);
    }

    /// Segments without `audio_start_time` (rows from before the column existed, or any
    /// insert path that omits timing) must not sort before timed segments: SQLite orders
    /// NULL first in `ASC`, so a naive `ORDER BY audio_start_time ASC` would scramble a
    /// meeting that mixes timed and untimed rows.
    #[tokio::test]
    async fn canonical_segments_with_null_audio_start_time_sort_after_timed_segments() {
        let pool = migrated_pool().await;
        let meeting_id = TranscriptsRepository::save_transcript(
            &pool,
            "Test meeting",
            &[
                TranscriptSegment {
                    id: "seg-0".to_string(),
                    text: "first".to_string(),
                    timestamp: "0".to_string(),
                    audio_start_time: Some(0.0),
                    audio_end_time: Some(1.0),
                    duration: Some(1.0),
                    audio_source: None,
                },
                TranscriptSegment {
                    id: "seg-1".to_string(),
                    text: "untimed".to_string(),
                    timestamp: "1".to_string(),
                    audio_start_time: None,
                    audio_end_time: None,
                    duration: None,
                    audio_source: None,
                },
                TranscriptSegment {
                    id: "seg-2".to_string(),
                    text: "second".to_string(),
                    timestamp: "2".to_string(),
                    audio_start_time: Some(1.0),
                    audio_end_time: Some(2.0),
                    duration: Some(1.0),
                    audio_source: None,
                },
            ],
            None,
        )
        .await
        .unwrap();

        let segments = RenderingRepository::get_canonical_segments(&pool, &meeting_id)
            .await
            .unwrap();
        assert_eq!(segments, vec!["first", "second", "untimed"]);
    }

    #[tokio::test]
    async fn rendering_cache_round_trips_and_can_be_overwritten() {
        let pool = migrated_pool().await;
        let meeting_id = seed_meeting(&pool, &["係咁㗎啦"]).await;

        assert!(RenderingRepository::get_cached_rendering(&pool, &meeting_id)
            .await
            .unwrap()
            .is_none());

        RenderingRepository::save_rendering(&pool, &meeting_id, "是這樣的", "fp-1")
            .await
            .unwrap();

        let cached = RenderingRepository::get_cached_rendering(&pool, &meeting_id)
            .await
            .unwrap();
        assert_eq!(cached, Some(("是這樣的".to_string(), "fp-1".to_string())));

        RenderingRepository::save_rendering(&pool, &meeting_id, "是這樣的呀", "fp-2")
            .await
            .unwrap();

        let cached = RenderingRepository::get_cached_rendering(&pool, &meeting_id)
            .await
            .unwrap();
        assert_eq!(cached, Some(("是這樣的呀".to_string(), "fp-2".to_string())));
    }
}
