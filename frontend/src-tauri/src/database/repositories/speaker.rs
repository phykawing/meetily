// The diarized Speaker's persistence: per-chunk attribution on `transcripts`, and the
// per-meeting label -> display-name mapping in `meeting_speakers`. Kept distinct from the
// live Audio Source (`transcripts.speaker`) - see docs/adr/0004 and docs/adr/0001.

use crate::diarization::alignment::{ChunkAttribution, ChunkSpan};
use sqlx::{Connection, Error as SqlxError, SqlitePool};

pub struct SpeakerRepository;

/// One discovered speaker's default display name, in first-appearance order.
pub struct SpeakerName {
    pub label: String,
    pub name: String,
}

impl SpeakerRepository {
    /// Fetches every chunk in the meeting with known audio timing, as the `ChunkSpan`s
    /// `diarization::alignment` attributes. Chunks without `audio_start_time`/
    /// `audio_end_time` (meetings recorded before those columns existed) are excluded -
    /// they have no time span to overlap a speaker turn with, so they are left
    /// unattributed rather than fed in as a degenerate zero-length span.
    pub async fn get_chunk_spans(
        pool: &SqlitePool,
        meeting_id: &str,
    ) -> Result<Vec<ChunkSpan>, SqlxError> {
        let rows: Vec<(String, f64, f64)> = sqlx::query_as(
            "SELECT id, audio_start_time, audio_end_time FROM transcripts \
             WHERE meeting_id = ? AND audio_start_time IS NOT NULL AND audio_end_time IS NOT NULL \
             ORDER BY audio_start_time ASC",
        )
        .bind(meeting_id)
        .fetch_all(pool)
        .await?;

        Ok(rows
            .into_iter()
            .map(|(id, start, end)| ChunkSpan { id, start, end })
            .collect())
    }

    /// Replaces every diarization result for a meeting: clears prior attribution and
    /// speaker names, then writes the new ones, all in one transaction. Re-running
    /// diarization always calls this rather than merging, since clustering is not stable
    /// across runs (ADR-0001) - a stale "speaker_01" from a previous run must never
    /// survive next to a new run's different "speaker_01".
    pub async fn replace_diarization_results(
        pool: &SqlitePool,
        meeting_id: &str,
        attributions: &[ChunkAttribution],
        speaker_names: &[SpeakerName],
    ) -> Result<(), SqlxError> {
        let mut conn = pool.acquire().await?;
        let mut tx = conn.begin().await?;

        sqlx::query(
            "UPDATE transcripts SET speaker_label = NULL, speaker_uncertain = 0 WHERE meeting_id = ?",
        )
        .bind(meeting_id)
        .execute(&mut *tx)
        .await?;

        sqlx::query("DELETE FROM meeting_speakers WHERE meeting_id = ?")
            .bind(meeting_id)
            .execute(&mut *tx)
            .await?;

        for attribution in attributions {
            sqlx::query(
                "UPDATE transcripts SET speaker_label = ?, speaker_uncertain = ? WHERE id = ?",
            )
            .bind(&attribution.speaker)
            .bind(attribution.uncertain)
            .bind(&attribution.chunk_id)
            .execute(&mut *tx)
            .await?;
        }

        for speaker in speaker_names {
            sqlx::query(
                "INSERT INTO meeting_speakers (meeting_id, speaker_label, display_name) VALUES (?, ?, ?)",
            )
            .bind(meeting_id)
            .bind(&speaker.label)
            .bind(&speaker.name)
            .execute(&mut *tx)
            .await?;
        }

        tx.commit().await
    }

    /// The meeting's discovered speakers and their display names, for resolving
    /// `speaker_label` on transcript rows into something human-readable. Empty until a
    /// diarization pass has run.
    pub async fn get_meeting_speakers(
        pool: &SqlitePool,
        meeting_id: &str,
    ) -> Result<Vec<SpeakerName>, SqlxError> {
        let rows: Vec<(String, String)> = sqlx::query_as(
            "SELECT speaker_label, display_name FROM meeting_speakers WHERE meeting_id = ? \
             ORDER BY speaker_label ASC",
        )
        .bind(meeting_id)
        .fetch_all(pool)
        .await?;

        Ok(rows
            .into_iter()
            .map(|(label, name)| SpeakerName { label, name })
            .collect())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::api::TranscriptSegment;
    use crate::database::models::Transcript as TranscriptRow;
    use crate::database::repositories::transcript::TranscriptsRepository;

    async fn migrated_pool() -> SqlitePool {
        let pool = SqlitePool::connect("sqlite::memory:").await.unwrap();
        sqlx::migrate!("./migrations").run(&pool).await.unwrap();
        pool
    }

    // `TranscriptsRepository::save_transcript` generates its own row ids rather than
    // using `TranscriptSegment::id`, so tests seed by (distinguishing) text and look the
    // real ids up afterward - mirroring the pattern in `transcript.rs`'s own tests.
    fn segment(text: &str, start: f64, end: f64) -> TranscriptSegment {
        TranscriptSegment {
            id: text.to_string(),
            text: text.to_string(),
            timestamp: "0".to_string(),
            audio_start_time: Some(start),
            audio_end_time: Some(end),
            duration: Some(end - start),
            audio_source: None,
        }
    }

    async fn id_for_text(pool: &SqlitePool, meeting_id: &str, text: &str) -> String {
        sqlx::query_scalar::<_, String>(
            "SELECT id FROM transcripts WHERE meeting_id = ? AND transcript = ?",
        )
        .bind(meeting_id)
        .bind(text)
        .fetch_one(pool)
        .await
        .unwrap()
    }

    async fn fetch_row(pool: &SqlitePool, id: &str) -> TranscriptRow {
        sqlx::query_as::<_, TranscriptRow>("SELECT * FROM transcripts WHERE id = ?")
            .bind(id)
            .fetch_one(pool)
            .await
            .unwrap()
    }

    #[tokio::test]
    async fn chunk_spans_excludes_rows_without_audio_timing() {
        let pool = migrated_pool().await;
        let meeting_id = TranscriptsRepository::save_transcript(
            &pool,
            "Test meeting",
            &[segment("timed", 0.0, 1.0)],
            None,
        )
        .await
        .unwrap();
        let timed_id = id_for_text(&pool, &meeting_id, "timed").await;
        // A legacy row with no timing, inserted directly (save_transcript always sets it).
        sqlx::query(
            "INSERT INTO transcripts (id, meeting_id, transcript, timestamp) VALUES ('seg-untimed', ?, 'x', '0')",
        )
        .bind(&meeting_id)
        .execute(&pool)
        .await
        .unwrap();

        let spans = SpeakerRepository::get_chunk_spans(&pool, &meeting_id).await.unwrap();

        assert_eq!(spans.len(), 1);
        assert_eq!(spans[0].id, timed_id);
    }

    #[tokio::test]
    async fn replace_diarization_results_writes_attribution_and_names() {
        let pool = migrated_pool().await;
        let meeting_id = TranscriptsRepository::save_transcript(
            &pool,
            "Test meeting",
            &[segment("first", 0.0, 5.0), segment("second", 5.0, 10.0)],
            None,
        )
        .await
        .unwrap();
        let id1 = id_for_text(&pool, &meeting_id, "first").await;
        let id2 = id_for_text(&pool, &meeting_id, "second").await;

        let attributions = vec![
            ChunkAttribution {
                chunk_id: id1.clone(),
                speaker: Some("speaker_00".to_string()),
                uncertain: false,
            },
            ChunkAttribution {
                chunk_id: id2.clone(),
                speaker: Some("speaker_01".to_string()),
                uncertain: true,
            },
        ];
        let names = vec![
            SpeakerName { label: "speaker_00".to_string(), name: "Speaker 1".to_string() },
            SpeakerName { label: "speaker_01".to_string(), name: "Speaker 2".to_string() },
        ];

        SpeakerRepository::replace_diarization_results(&pool, &meeting_id, &attributions, &names)
            .await
            .unwrap();

        let row1 = fetch_row(&pool, &id1).await;
        assert_eq!(row1.speaker_label.as_deref(), Some("speaker_00"));
        assert!(!row1.speaker_uncertain);

        let row2 = fetch_row(&pool, &id2).await;
        assert_eq!(row2.speaker_label.as_deref(), Some("speaker_01"));
        assert!(row2.speaker_uncertain);

        let speakers = SpeakerRepository::get_meeting_speakers(&pool, &meeting_id).await.unwrap();
        assert_eq!(speakers.len(), 2);
        assert_eq!(speakers[0].name, "Speaker 1");
        assert_eq!(speakers[1].name, "Speaker 2");
    }

    #[tokio::test]
    async fn replace_diarization_results_discards_the_previous_run_entirely() {
        let pool = migrated_pool().await;
        let meeting_id = TranscriptsRepository::save_transcript(
            &pool,
            "Test meeting",
            &[segment("first", 0.0, 5.0), segment("second", 5.0, 10.0)],
            None,
        )
        .await
        .unwrap();
        let id1 = id_for_text(&pool, &meeting_id, "first").await;
        let id2 = id_for_text(&pool, &meeting_id, "second").await;

        // First run: two speakers, seg-2 uncertain.
        SpeakerRepository::replace_diarization_results(
            &pool,
            &meeting_id,
            &[
                ChunkAttribution { chunk_id: id1.clone(), speaker: Some("speaker_00".to_string()), uncertain: false },
                ChunkAttribution { chunk_id: id2.clone(), speaker: Some("speaker_01".to_string()), uncertain: true },
            ],
            &[
                SpeakerName { label: "speaker_00".to_string(), name: "Speaker 1".to_string() },
                SpeakerName { label: "speaker_01".to_string(), name: "Speaker 2".to_string() },
            ],
        )
        .await
        .unwrap();

        // Re-run: clustering now finds only one speaker covering the first chunk, and
        // does not attribute the second at all. The stale speaker_01 name and the
        // second chunk's old label/flag must not survive.
        SpeakerRepository::replace_diarization_results(
            &pool,
            &meeting_id,
            &[ChunkAttribution { chunk_id: id1.clone(), speaker: Some("speaker_00".to_string()), uncertain: false }],
            &[SpeakerName { label: "speaker_00".to_string(), name: "Speaker 1".to_string() }],
        )
        .await
        .unwrap();

        let row2 = fetch_row(&pool, &id2).await;
        assert_eq!(row2.speaker_label, None);
        assert!(!row2.speaker_uncertain);

        let speakers = SpeakerRepository::get_meeting_speakers(&pool, &meeting_id).await.unwrap();
        assert_eq!(speakers.len(), 1);
        assert_eq!(speakers[0].name, "Speaker 1");
    }

    #[tokio::test]
    async fn meeting_with_no_diarization_run_yet_has_no_speakers() {
        let pool = migrated_pool().await;
        let meeting_id = TranscriptsRepository::save_transcript(
            &pool,
            "Test meeting",
            &[segment("seg-1", 0.0, 5.0)],
            None,
        )
        .await
        .unwrap();

        let speakers = SpeakerRepository::get_meeting_speakers(&pool, &meeting_id).await.unwrap();
        assert!(speakers.is_empty());
    }
}
