use crate::api::{MeetingDetails, MeetingTranscript};
use crate::database::models::{MeetingModel, Transcript};
use chrono::Utc;
use sqlx::{Connection, Error as SqlxError, SqliteConnection, SqlitePool};
use tracing::{error, info};

pub struct MeetingsRepository;

impl MeetingsRepository {
    pub async fn get_meetings(pool: &SqlitePool) -> Result<Vec<MeetingModel>, sqlx::Error> {
        let meetings =
            sqlx::query_as::<_, MeetingModel>("SELECT * FROM meetings ORDER BY created_at DESC")
                .fetch_all(pool)
                .await?;
        Ok(meetings)
    }

    pub async fn delete_meeting(pool: &SqlitePool, meeting_id: &str) -> Result<bool, SqlxError> {
        if meeting_id.trim().is_empty() {
            return Err(SqlxError::Protocol(
                "meeting_id cannot be empty".to_string(),
            ));
        }

        let mut conn = pool.acquire().await?;
        let mut transaction = conn.begin().await?;

        match delete_meeting_with_transaction(&mut transaction, meeting_id).await {
            Ok(success) => {
                if success {
                    transaction.commit().await?;
                    info!(
                        "Successfully deleted meeting {} and all associated data",
                        meeting_id
                    );
                    Ok(true)
                } else {
                    transaction.rollback().await?;
                    Ok(false)
                }
            }
            Err(e) => {
                let _ = transaction.rollback().await;
                error!("Failed to delete meeting {}: {}", meeting_id, e);
                Err(e)
            }
        }
    }

    pub async fn get_meeting(
        pool: &SqlitePool,
        meeting_id: &str,
    ) -> Result<Option<MeetingDetails>, SqlxError> {
        if meeting_id.trim().is_empty() {
            return Err(SqlxError::Protocol(
                "meeting_id cannot be empty".to_string(),
            ));
        }

        let mut conn = pool.acquire().await?;
        let mut transaction = conn.begin().await?;

        // Get meeting details
        let meeting: Option<MeetingModel> =
            sqlx::query_as("SELECT id, title, created_at, updated_at, folder_path FROM meetings WHERE id = ?")
                .bind(meeting_id)
                .fetch_optional(&mut *transaction)
                .await?;

        if meeting.is_none() {
            transaction.rollback().await?;
            return Err(SqlxError::RowNotFound);
        }

        if let Some(meeting) = meeting {
            // Get all transcripts for this meeting
            let transcripts =
                sqlx::query_as::<_, Transcript>("SELECT * FROM transcripts WHERE meeting_id = ?")
                    .bind(meeting_id)
                    .fetch_all(&mut *transaction)
                    .await?;

            transaction.commit().await?;

            // Convert Transcript to MeetingTranscript
            let meeting_transcripts = transcripts
                .into_iter()
                .map(|t| MeetingTranscript {
                    id: t.id,
                    text: t.transcript,
                    timestamp: t.timestamp,
                    audio_start_time: t.audio_start_time,
                    audio_end_time: t.audio_end_time,
                    duration: t.duration,
                    audio_source: t.audio_source,
                    speaker_label: t.speaker_label,
                    speaker_uncertain: t.speaker_uncertain,
                })
                .collect::<Vec<_>>();

            Ok(Some(MeetingDetails {
                id: meeting.id,
                title: meeting.title,
                created_at: meeting.created_at.0.to_rfc3339(),
                updated_at: meeting.updated_at.0.to_rfc3339(),
                transcripts: meeting_transcripts,
            }))
        } else {
            transaction.rollback().await?;
            Ok(None)
        }
    }

    /// Get meeting metadata without transcripts (for pagination)
    pub async fn get_meeting_metadata(
        pool: &SqlitePool,
        meeting_id: &str,
    ) -> Result<Option<MeetingModel>, SqlxError> {
        if meeting_id.trim().is_empty() {
            return Err(SqlxError::Protocol(
                "meeting_id cannot be empty".to_string(),
            ));
        }

        let meeting: Option<MeetingModel> =
            sqlx::query_as("SELECT id, title, created_at, updated_at, folder_path FROM meetings WHERE id = ?")
                .bind(meeting_id)
                .fetch_optional(pool)
                .await?;

        Ok(meeting)
    }

    /// Get meeting transcripts with pagination support
    pub async fn get_meeting_transcripts_paginated(
        pool: &SqlitePool,
        meeting_id: &str,
        limit: i64,
        offset: i64,
    ) -> Result<(Vec<Transcript>, i64), SqlxError> {
        if meeting_id.trim().is_empty() {
            return Err(SqlxError::Protocol(
                "meeting_id cannot be empty".to_string(),
            ));
        }

        // Get total count of transcripts for this meeting
        let total: (i64,) = sqlx::query_as(
            "SELECT COUNT(*) FROM transcripts WHERE meeting_id = ?"
        )
        .bind(meeting_id)
        .fetch_one(pool)
        .await?;

        // Ordered to match `RenderingRepository::get_canonical_segments` exactly (see
        // phykawing/meetily#28): `audio_start_time` is nullable, and SQLite sorts NULL
        // before any real value in `ASC` order, so untimed segments are ordered last via
        // the `(audio_start_time IS NULL) ASC` clause, with `rowid` as the tie-breaker
        // among them — otherwise the 口語 view and the 書面語 Rendering can disagree on
        // sequence for any meeting mixing timed and untimed rows. `rowid` (not the
        // app-assigned `id`, a random UUID) is used so ties fall back to insertion order
        // instead of scrambling into random order — this matters most for a meeting whose
        // segments *all* lack timing, where every row ties and `id ASC` would otherwise
        // sort the whole transcript randomly.
        let transcripts = sqlx::query_as::<_, Transcript>(
            "SELECT * FROM transcripts
             WHERE meeting_id = ?
             ORDER BY (audio_start_time IS NULL) ASC, audio_start_time ASC, rowid ASC
             LIMIT ? OFFSET ?"
        )
        .bind(meeting_id)
        .bind(limit)
        .bind(offset)
        .fetch_all(pool)
        .await?;

        Ok((transcripts, total.0))
    }

    pub async fn update_meeting_title(
        pool: &SqlitePool,
        meeting_id: &str,
        new_title: &str,
    ) -> Result<bool, SqlxError> {
        if meeting_id.trim().is_empty() {
            return Err(SqlxError::Protocol(
                "meeting_id cannot be empty".to_string(),
            ));
        }

        let mut conn = pool.acquire().await?;
        let mut transaction = conn.begin().await?;

        let now = Utc::now().naive_utc();

        let rows_affected =
            sqlx::query("UPDATE meetings SET title = ?, updated_at = ? WHERE id = ?")
                .bind(new_title)
                .bind(now)
                .bind(meeting_id)
                .execute(&mut *transaction)
                .await?;
        if rows_affected.rows_affected() == 0 {
            transaction.rollback().await?;
            return Ok(false);
        }
        transaction.commit().await?;
        Ok(true)
    }

    pub async fn update_meeting_name(
        pool: &SqlitePool,
        meeting_id: &str,
        new_title: &str,
    ) -> Result<bool, SqlxError> {
        let mut transaction = pool.begin().await?;
        let now = Utc::now();

        // Update meetings table
        let meeting_update =
            sqlx::query("UPDATE meetings SET title = ?, updated_at = ? WHERE id = ?")
                .bind(new_title)
                .bind(now)
                .bind(meeting_id)
                .execute(&mut *transaction)
                .await?;

        if meeting_update.rows_affected() == 0 {
            transaction.rollback().await?;
            return Ok(false); // Meeting not found
        }

        // Update transcript_chunks table
        sqlx::query("UPDATE transcript_chunks SET meeting_name = ? WHERE meeting_id = ?")
            .bind(new_title)
            .bind(meeting_id)
            .execute(&mut *transaction)
            .await?;

        transaction.commit().await?;
        Ok(true)
    }
}

async fn delete_meeting_with_transaction(
    transaction: &mut SqliteConnection,
    meeting_id: &str,
) -> Result<bool, SqlxError> {
    // Check if meeting exists
    let meeting_exists: Option<(i64,)> = sqlx::query_as("SELECT 1 FROM meetings WHERE id = ?")
        .bind(meeting_id)
        .fetch_optional(&mut *transaction)
        .await?;

    if meeting_exists.is_none() {
        error!("Meeting {} not found for deletion", meeting_id);
        return Ok(false);
    }

    // Delete from related tables in proper order
    // 1. Delete from transcript_chunks
    sqlx::query("DELETE FROM transcript_chunks WHERE meeting_id = ?")
        .bind(meeting_id)
        .execute(&mut *transaction)
        .await?;

    // 2. Delete from summary_processes
    sqlx::query("DELETE FROM summary_processes WHERE meeting_id = ?")
        .bind(meeting_id)
        .execute(&mut *transaction)
        .await?;

    // 3. Delete from transcripts
    sqlx::query("DELETE FROM transcripts WHERE meeting_id = ?")
        .bind(meeting_id)
        .execute(&mut *transaction)
        .await?;

    // 4. Delete cached transcript renderings (see rendering.rs / phykawing/meetily#8).
    // Explicit, like the deletes above, rather than relying on the table's
    // `ON DELETE CASCADE`: this app does not enable SQLite foreign key enforcement, so an
    // unenforced CASCADE would silently leave orphaned renderings behind.
    sqlx::query("DELETE FROM transcript_renderings WHERE meeting_id = ?")
        .bind(meeting_id)
        .execute(&mut *transaction)
        .await?;

    // 5. Delete diarized speaker names (see database/repositories/speaker.rs,
    // phykawing/meetily#16). Same reasoning as transcript_renderings above: no enforced
    // FK cascade, so this must be explicit.
    sqlx::query("DELETE FROM meeting_speakers WHERE meeting_id = ?")
        .bind(meeting_id)
        .execute(&mut *transaction)
        .await?;

    // 6. Finally, delete the meeting
    let result = sqlx::query("DELETE FROM meetings WHERE id = ?")
        .bind(meeting_id)
        .execute(&mut *transaction)
        .await?;

    Ok(result.rows_affected() > 0)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::api::TranscriptSegment;
    use crate::database::repositories::rendering::RenderingRepository;
    use crate::database::repositories::transcript::TranscriptsRepository;

    async fn migrated_pool() -> SqlitePool {
        let pool = SqlitePool::connect("sqlite::memory:").await.unwrap();
        sqlx::migrate!("./migrations").run(&pool).await.unwrap();
        pool
    }

    /// `transcript_renderings` declares `ON DELETE CASCADE`, but this app never enables
    /// SQLite foreign key enforcement, so that constraint is inert. Deleting a meeting must
    /// therefore clean up its cached rendering explicitly, the same way it does for every
    /// other per-meeting table, or the rendering (a full copy of that meeting's content)
    /// survives the meeting it belonged to.
    #[tokio::test]
    async fn deleting_a_meeting_removes_its_cached_rendering() {
        let pool = migrated_pool().await;

        let meeting_id = TranscriptsRepository::save_transcript(
            &pool,
            "Test meeting",
            &[TranscriptSegment {
                id: "seg-0".to_string(),
                text: "係咁㗎啦".to_string(),
                timestamp: "0".to_string(),
                audio_start_time: Some(0.0),
                audio_end_time: Some(1.0),
                duration: Some(1.0),
                audio_source: None,
            }],
            None,
        )
        .await
        .unwrap();

        RenderingRepository::save_rendering(&pool, &meeting_id, "是這樣的", "fp-1")
            .await
            .unwrap();
        assert!(RenderingRepository::get_cached_rendering(&pool, &meeting_id)
            .await
            .unwrap()
            .is_some());

        let deleted = MeetingsRepository::delete_meeting(&pool, &meeting_id)
            .await
            .unwrap();
        assert!(deleted);

        assert!(RenderingRepository::get_cached_rendering(&pool, &meeting_id)
            .await
            .unwrap()
            .is_none());
    }

    /// Same reasoning as `deleting_a_meeting_removes_its_cached_rendering`:
    /// `meeting_speakers` also declares an inert `ON DELETE CASCADE`, so deleting a meeting
    /// must clean it up explicitly or a diarized meeting's speaker names survive it.
    #[tokio::test]
    async fn deleting_a_meeting_removes_its_speaker_names() {
        use crate::database::repositories::speaker::{SpeakerName, SpeakerRepository};

        let pool = migrated_pool().await;

        let meeting_id = TranscriptsRepository::save_transcript(
            &pool,
            "Test meeting",
            &[TranscriptSegment {
                id: "seg-0".to_string(),
                text: "hello".to_string(),
                timestamp: "0".to_string(),
                audio_start_time: Some(0.0),
                audio_end_time: Some(1.0),
                duration: Some(1.0),
                audio_source: None,
            }],
            None,
        )
        .await
        .unwrap();

        SpeakerRepository::replace_diarization_results(
            &pool,
            &meeting_id,
            &[],
            &[SpeakerName { label: "speaker_00".to_string(), name: "Speaker 1".to_string() }],
        )
        .await
        .unwrap();
        assert_eq!(
            SpeakerRepository::get_meeting_speakers(&pool, &meeting_id).await.unwrap().len(),
            1
        );

        let deleted = MeetingsRepository::delete_meeting(&pool, &meeting_id)
            .await
            .unwrap();
        assert!(deleted);

        assert!(SpeakerRepository::get_meeting_speakers(&pool, &meeting_id)
            .await
            .unwrap()
            .is_empty());
    }

    /// `get_meeting_transcripts_paginated` must order segments identically to
    /// `RenderingRepository::get_canonical_segments`, or the 口語 view and the 書面語
    /// Rendering can present a mixed timed/untimed meeting in a different sequence (see
    /// phykawing/meetily#28). SQLite sorts NULL first in `ASC`, so untimed segments must be
    /// pushed after timed ones via `(audio_start_time IS NULL) ASC`, with `rowid` as the
    /// tie-breaker among untimed rows.
    #[tokio::test]
    async fn paginated_transcripts_order_null_audio_start_time_after_timed_segments() {
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

        let (transcripts, total) =
            MeetingsRepository::get_meeting_transcripts_paginated(&pool, &meeting_id, 10, 0)
                .await
                .unwrap();

        assert_eq!(total, 3);
        let texts: Vec<&str> = transcripts.iter().map(|t| t.transcript.as_str()).collect();
        assert_eq!(texts, vec!["first", "second", "untimed"]);

        // Matches `RenderingRepository::get_canonical_segments`'s ordering for the same
        // meeting exactly, so the two views never disagree on sequence.
        let rendering_segments =
            RenderingRepository::get_canonical_segments(&pool, &meeting_id)
                .await
                .unwrap();
        let rendering_texts: Vec<&str> = rendering_segments.iter().map(|s| s.text.as_str()).collect();
        assert_eq!(texts, rendering_texts);
    }

    /// When every segment in a meeting lacks `audio_start_time` (e.g. a meeting that
    /// entirely predates the column), the tie-break must fall back to insertion order via
    /// `rowid`, not to the app-assigned `id` (a random UUID in production) — otherwise the
    /// whole transcript would sort into effectively random order. `id` is deliberately
    /// assigned here in reverse-alphabetical order so the test would fail if the query
    /// still tie-broke on `id ASC`.
    #[tokio::test]
    async fn paginated_transcripts_all_untimed_preserve_insertion_order() {
        let pool = migrated_pool().await;
        let meeting_id = TranscriptsRepository::save_transcript(
            &pool,
            "Test meeting",
            &[
                TranscriptSegment {
                    id: "zzz-first".to_string(),
                    text: "first".to_string(),
                    timestamp: "0".to_string(),
                    audio_start_time: None,
                    audio_end_time: None,
                    duration: None,
                    audio_source: None,
                },
                TranscriptSegment {
                    id: "mmm-second".to_string(),
                    text: "second".to_string(),
                    timestamp: "1".to_string(),
                    audio_start_time: None,
                    audio_end_time: None,
                    duration: None,
                    audio_source: None,
                },
                TranscriptSegment {
                    id: "aaa-third".to_string(),
                    text: "third".to_string(),
                    timestamp: "2".to_string(),
                    audio_start_time: None,
                    audio_end_time: None,
                    duration: None,
                    audio_source: None,
                },
            ],
            None,
        )
        .await
        .unwrap();

        let (transcripts, _total) =
            MeetingsRepository::get_meeting_transcripts_paginated(&pool, &meeting_id, 10, 0)
                .await
                .unwrap();

        let texts: Vec<&str> = transcripts.iter().map(|t| t.transcript.as_str()).collect();
        assert_eq!(texts, vec!["first", "second", "third"]);
    }
}
