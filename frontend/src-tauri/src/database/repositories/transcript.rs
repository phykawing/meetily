use crate::api::{TranscriptSearchResult, TranscriptSegment};
use chrono::Utc;
use sqlx::{Connection, Error as SqlxError, SqlitePool};
use tracing::{error, info};
use uuid::Uuid;

pub struct TranscriptsRepository;

impl TranscriptsRepository {
    /// Saves a new meeting and its associated transcript segments.
    /// This function uses a transaction to ensure that either both the meeting
    /// and all its transcripts are saved, or none of them are.
    pub async fn save_transcript(
        pool: &SqlitePool,
        meeting_title: &str,
        transcripts: &[TranscriptSegment],
        folder_path: Option<String>,
    ) -> Result<String, SqlxError> {
        let meeting_id = format!("meeting-{}", Uuid::new_v4());

        let mut conn = pool.acquire().await?;
        let mut transaction = conn.begin().await?;

        let now = Utc::now();

        // 1. Create the new meeting
        let result = sqlx::query(
            "INSERT INTO meetings (id, title, created_at, updated_at, folder_path) VALUES (?, ?, ?, ?, ?)",
        )
        .bind(&meeting_id)
        .bind(meeting_title)
        .bind(now)
        .bind(now)
        .bind(&folder_path)
        .execute(&mut *transaction)
        .await;

        if let Err(e) = result {
            error!("Failed to create meeting '{}': {}", meeting_title, e);
            transaction.rollback().await?;
            return Err(e);
        }

        info!("Successfully created meeting with id: {}", meeting_id);

        // 2. Save each transcript segment with audio timing fields
        for segment in transcripts {
            let transcript_id = format!("transcript-{}", Uuid::new_v4());
            // `speaker` stores the live Audio Source hint ('mic' / 'system' / 'mixed'),
            // not a diarized Speaker - see docs/adr/0004.
            let result = sqlx::query(
                "INSERT INTO transcripts (id, meeting_id, transcript, timestamp, audio_start_time, audio_end_time, duration, speaker)
                 VALUES (?, ?, ?, ?, ?, ?, ?, ?)"
            )
            .bind(&transcript_id)
            .bind(&meeting_id)
            .bind(&segment.text)
            .bind(&segment.timestamp)
            .bind(segment.audio_start_time)
            .bind(segment.audio_end_time)
            .bind(segment.duration)
            .bind(&segment.audio_source)
            .execute(&mut *transaction)
            .await;

            if let Err(e) = result {
                error!(
                    "Failed to save transcript segment for meeting {}: {}",
                    meeting_id, e
                );
                transaction.rollback().await?;
                return Err(e);
            }
        }

        info!(
            "Successfully saved {} transcript segments for meeting {}",
            transcripts.len(),
            meeting_id
        );

        // Commit the transaction
        transaction.commit().await?;

        Ok(meeting_id)
    }

    /// Searches for a query string within the transcripts.
    /// It returns a list of matching transcripts with context.
    pub async fn search_transcripts(
        pool: &SqlitePool,
        query: &str,
    ) -> Result<Vec<TranscriptSearchResult>, SqlxError> {
        if query.trim().is_empty() {
            return Ok(Vec::new());
        }

        let search_query = format!("%{}%", query.to_lowercase());

        let rows = sqlx::query_as::<_, (String, String, String, String)>(
            "SELECT m.id, m.title, t.transcript, t.timestamp
             FROM meetings m
             JOIN transcripts t ON m.id = t.meeting_id
             WHERE LOWER(t.transcript) LIKE ?",
        )
        .bind(&search_query)
        .fetch_all(pool)
        .await?;

        let results = rows
            .into_iter()
            .map(|(id, title, transcript, timestamp)| {
                let match_context = Self::get_match_context(&transcript, query);
                TranscriptSearchResult {
                    id,
                    title,
                    match_context,
                    timestamp,
                }
            })
            .collect();

        Ok(results)
    }

    /// Helper function to extract a snippet of text around the first match of a query.
    fn get_match_context(transcript: &str, query: &str) -> String {
        let transcript_lower = transcript.to_lowercase();
        let query_lower = query.to_lowercase();

        match transcript_lower.find(&query_lower) {
            Some(match_index) => {
                let start_index = match_index.saturating_sub(100);
                let end_index = (match_index + query.len() + 100).min(transcript.len());

                let mut context = String::new();
                if start_index > 0 {
                    context.push_str("...");
                }
                context.push_str(&transcript[start_index..end_index]);
                if end_index < transcript.len() {
                    context.push_str("...");
                }
                context
            }
            None => transcript.chars().take(200).collect(), // Fallback to the start of the transcript
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::api::TranscriptSegment;
    use crate::database::models::Transcript as TranscriptRow;

    async fn migrated_pool() -> SqlitePool {
        let pool = SqlitePool::connect("sqlite::memory:").await.unwrap();
        sqlx::migrate!("./migrations").run(&pool).await.unwrap();
        pool
    }

    fn segment(id: &str, text: &str, audio_source: Option<&str>) -> TranscriptSegment {
        TranscriptSegment {
            id: id.to_string(),
            text: text.to_string(),
            timestamp: "0".to_string(),
            audio_start_time: Some(0.0),
            audio_end_time: Some(1.0),
            duration: Some(1.0),
            audio_source: audio_source.map(|s| s.to_string()),
        }
    }

    /// The Audio Source hint (mic/system/mixed) must round-trip through the `speaker`
    /// column exactly as saved - it's the whole point of ADR-0004 giving that dormant
    /// column a documented meaning.
    #[tokio::test]
    async fn audio_source_hint_round_trips_through_the_speaker_column() {
        let pool = migrated_pool().await;

        let meeting_id = TranscriptsRepository::save_transcript(
            &pool,
            "Test meeting",
            &[
                segment("seg-mic", "from the mic", Some("mic")),
                segment("seg-system", "from the call", Some("system")),
                segment("seg-mixed", "everyone talking", Some("mixed")),
                segment("seg-untagged", "no hint captured", None),
            ],
            None,
        )
        .await
        .unwrap();

        let mut rows = sqlx::query_as::<_, TranscriptRow>(
            "SELECT * FROM transcripts WHERE meeting_id = ? ORDER BY id",
        )
        .bind(&meeting_id)
        .fetch_all(&pool)
        .await
        .unwrap();
        rows.sort_by(|a, b| a.id.cmp(&b.id));

        let by_text: std::collections::HashMap<String, Option<String>> = rows
            .into_iter()
            .map(|r| (r.transcript, r.audio_source))
            .collect();

        assert_eq!(by_text["from the mic"].as_deref(), Some("mic"));
        assert_eq!(by_text["from the call"].as_deref(), Some("system"));
        assert_eq!(by_text["everyone talking"].as_deref(), Some("mixed"));
        assert_eq!(by_text["no hint captured"], None);
    }

    /// Meetings saved before this field existed have `speaker IS NULL`; reading them
    /// back must not error or fabricate a value.
    #[tokio::test]
    async fn meetings_recorded_before_this_change_still_read_correctly() {
        let pool = migrated_pool().await;

        // Simulate a pre-existing row inserted the old way, without a `speaker` value.
        sqlx::query(
            "INSERT INTO meetings (id, title, created_at, updated_at) VALUES ('meeting-old', 'Old meeting', datetime('now'), datetime('now'))",
        )
        .execute(&pool)
        .await
        .unwrap();
        sqlx::query(
            "INSERT INTO transcripts (id, meeting_id, transcript, timestamp) VALUES ('t-old', 'meeting-old', 'legacy text', '0')",
        )
        .execute(&pool)
        .await
        .unwrap();

        let row = sqlx::query_as::<_, TranscriptRow>("SELECT * FROM transcripts WHERE id = ?")
            .bind("t-old")
            .fetch_one(&pool)
            .await
            .unwrap();

        assert_eq!(row.transcript, "legacy text");
        assert_eq!(row.audio_source, None);
    }
}
