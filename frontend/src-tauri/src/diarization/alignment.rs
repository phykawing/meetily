// alignment.rs
//
// Maps diarization output (Speaker Turns, from segmentation + embedding + clustering) onto
// existing transcript chunks. Transcript chunks are VAD-cut segments that were transcribed
// before diarization ever runs, so turn boundaries generally do not line up with chunk
// boundaries — this module does not re-cut or re-transcribe chunks, only attributes them.
// See docs/adr/0001 and the "Testing Decisions" section of issue #3: this is the one part
// of the feature that is both deterministic and easy to get wrong, so it is fully unit
// tested without any ONNX runtime, model, or audio file.

/// A transcript chunk's time span within the meeting recording, plus the id used to
/// persist the result back onto its row.
#[derive(Debug, Clone, PartialEq)]
pub struct ChunkSpan {
    pub id: String,
    pub start: f64,
    pub end: f64,
}

/// A contiguous stretch of audio attributed to one Speaker by diarization. Turns for
/// different speakers may overlap (simultaneous speech / cross-talk); this module does
/// not assume they don't.
#[derive(Debug, Clone, PartialEq)]
pub struct SpeakerTurn {
    pub speaker: String,
    pub start: f64,
    pub end: f64,
}

/// The result of attributing one chunk to a speaker.
#[derive(Debug, Clone, PartialEq)]
pub struct ChunkAttribution {
    pub chunk_id: String,
    /// `None` when no turn overlaps the chunk at all — not an error, just unattributed.
    pub speaker: Option<String>,
    /// True when more than one distinct speaker overlaps the chunk, meaning it straddles
    /// a speaker change (or genuine cross-talk). The chunk is still attributed to
    /// whichever speaker has the greatest total overlap; `uncertain` just flags that the
    /// attribution is a best guess rather than a clean match.
    pub uncertain: bool,
}

/// Attributes every chunk in `chunks` to a speaker from `turns`.
///
/// Each chunk is assigned the speaker with the greatest total overlap duration, summed
/// across all of that speaker's turns intersecting the chunk (a chunk can straddle
/// multiple turns from the same speaker without that counting as a speaker change).
/// A chunk is `uncertain` exactly when more than one *distinct* speaker has nonzero
/// overlap with it, regardless of how small.
///
/// Ties in total overlap are broken in favor of whichever speaker's turn appears first in
/// `turns` — callers should pass turns in start-time order (as
/// `OfflineSpeakerDiarizationResult::sort_by_start_time` already returns them) so "first"
/// means "earliest".
pub fn align_chunks_to_turns(chunks: &[ChunkSpan], turns: &[SpeakerTurn]) -> Vec<ChunkAttribution> {
    chunks.iter().map(|chunk| attribute_chunk(chunk, turns)).collect()
}

fn attribute_chunk(chunk: &ChunkSpan, turns: &[SpeakerTurn]) -> ChunkAttribution {
    let mut overlap_by_speaker: Vec<(String, f64)> = Vec::new();

    for turn in turns {
        let overlap = overlap_duration(chunk.start, chunk.end, turn.start, turn.end);
        if overlap <= 0.0 {
            continue;
        }
        match overlap_by_speaker.iter_mut().find(|(speaker, _)| *speaker == turn.speaker) {
            Some(entry) => entry.1 += overlap,
            None => overlap_by_speaker.push((turn.speaker.clone(), overlap)),
        }
    }

    let uncertain = overlap_by_speaker.len() > 1;
    let speaker = overlap_by_speaker
        .into_iter()
        .fold(None::<(String, f64)>, |best, candidate| match &best {
            Some((_, best_overlap)) if *best_overlap >= candidate.1 => best,
            _ => Some(candidate),
        })
        .map(|(speaker, _)| speaker);

    ChunkAttribution {
        chunk_id: chunk.id.clone(),
        speaker,
        uncertain,
    }
}

/// Overlap duration between `[a_start, a_end)` and `[b_start, b_end)`. Zero for
/// non-overlapping or exactly-touching spans, never negative.
fn overlap_duration(a_start: f64, a_end: f64, b_start: f64, b_end: f64) -> f64 {
    (a_end.min(b_end) - a_start.max(b_start)).max(0.0)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn chunk(id: &str, start: f64, end: f64) -> ChunkSpan {
        ChunkSpan { id: id.to_string(), start, end }
    }

    fn turn(speaker: &str, start: f64, end: f64) -> SpeakerTurn {
        SpeakerTurn { speaker: speaker.to_string(), start, end }
    }

    #[test]
    fn chunk_fully_inside_one_turn_is_attributed_and_not_uncertain() {
        let chunks = [chunk("c1", 2.0, 4.0)];
        let turns = [turn("speaker_00", 0.0, 10.0)];

        let result = align_chunks_to_turns(&chunks, &turns);

        assert_eq!(result[0].speaker.as_deref(), Some("speaker_00"));
        assert!(!result[0].uncertain);
    }

    #[test]
    fn chunk_straddling_two_speakers_is_assigned_the_dominant_one_and_flagged() {
        // c1 spans [4, 10): 4s with speaker_00 ([0,8) overlap = [4,8) = 4s),
        // 2s with speaker_01 ([8,15) overlap = [8,10) = 2s). speaker_00 dominates.
        let chunks = [chunk("c1", 4.0, 10.0)];
        let turns = [turn("speaker_00", 0.0, 8.0), turn("speaker_01", 8.0, 15.0)];

        let result = align_chunks_to_turns(&chunks, &turns);

        assert_eq!(result[0].speaker.as_deref(), Some("speaker_00"));
        assert!(result[0].uncertain);
    }

    #[test]
    fn chunk_spanning_three_turns_is_assigned_by_greatest_total_overlap() {
        // c1 spans [0, 12). Overlaps: speaker_00 [0,2)=2s, speaker_01 [2,10)=8s,
        // speaker_02 [10,12)=2s. speaker_01 dominates with three distinct speakers
        // present, so still flagged.
        let chunks = [chunk("c1", 0.0, 12.0)];
        let turns = [
            turn("speaker_00", 0.0, 2.0),
            turn("speaker_01", 2.0, 10.0),
            turn("speaker_02", 10.0, 14.0),
        ];

        let result = align_chunks_to_turns(&chunks, &turns);

        assert_eq!(result[0].speaker.as_deref(), Some("speaker_01"));
        assert!(result[0].uncertain);
    }

    #[test]
    fn exactly_touching_boundary_counts_as_zero_overlap() {
        // Turn ends exactly where the chunk starts — no actual overlap.
        let chunks = [chunk("c1", 5.0, 10.0)];
        let turns = [turn("speaker_00", 0.0, 5.0)];

        let result = align_chunks_to_turns(&chunks, &turns);

        assert_eq!(result[0].speaker, None);
        assert!(!result[0].uncertain);
    }

    #[test]
    fn chunk_with_no_overlapping_turn_is_unattributed_not_an_error() {
        let chunks = [chunk("c1", 100.0, 105.0)];
        let turns = [turn("speaker_00", 0.0, 10.0)];

        let result = align_chunks_to_turns(&chunks, &turns);

        assert_eq!(result[0].speaker, None);
        assert!(!result[0].uncertain);
    }

    #[test]
    fn single_speaker_meeting_straddling_two_of_that_speakers_turns_is_not_flagged() {
        // Two turns from the SAME speaker with a chunk straddling the boundary between
        // them must not be treated as a speaker change.
        let chunks = [chunk("c1", 3.0, 7.0)];
        let turns = [turn("speaker_00", 0.0, 5.0), turn("speaker_00", 5.0, 10.0)];

        let result = align_chunks_to_turns(&chunks, &turns);

        assert_eq!(result[0].speaker.as_deref(), Some("speaker_00"));
        assert!(!result[0].uncertain);
    }

    #[test]
    fn empty_turn_list_leaves_every_chunk_unattributed_without_panicking() {
        let chunks = [chunk("c1", 0.0, 5.0), chunk("c2", 5.0, 10.0)];
        let turns: [SpeakerTurn; 0] = [];

        let result = align_chunks_to_turns(&chunks, &turns);

        assert_eq!(result.len(), 2);
        assert!(result.iter().all(|a| a.speaker.is_none() && !a.uncertain));
    }

    #[test]
    fn overlapping_turns_from_different_speakers_flag_the_chunk_they_both_cover() {
        // Cross-talk: speaker_00 and speaker_01 turns overlap each other directly.
        let chunks = [chunk("c1", 0.0, 10.0)];
        let turns = [turn("speaker_00", 0.0, 8.0), turn("speaker_01", 3.0, 10.0)];

        let result = align_chunks_to_turns(&chunks, &turns);

        // speaker_00: 8s, speaker_01: 7s -> speaker_00 dominates but both are present.
        assert_eq!(result[0].speaker.as_deref(), Some("speaker_00"));
        assert!(result[0].uncertain);
    }

    #[test]
    fn exact_overlap_tie_is_broken_by_whichever_speaker_appears_first_in_turns() {
        let chunks = [chunk("c1", 0.0, 10.0)];
        let turns = [turn("speaker_00", 0.0, 5.0), turn("speaker_01", 5.0, 10.0)];

        let result = align_chunks_to_turns(&chunks, &turns);

        assert_eq!(result[0].speaker.as_deref(), Some("speaker_00"));
        assert!(result[0].uncertain);
    }

    #[test]
    fn multiple_chunks_are_attributed_independently() {
        let chunks = [chunk("c1", 0.0, 4.0), chunk("c2", 6.0, 10.0)];
        let turns = [turn("speaker_00", 0.0, 5.0), turn("speaker_01", 5.0, 10.0)];

        let result = align_chunks_to_turns(&chunks, &turns);

        assert_eq!(result[0].chunk_id, "c1");
        assert_eq!(result[0].speaker.as_deref(), Some("speaker_00"));
        assert!(!result[0].uncertain);

        assert_eq!(result[1].chunk_id, "c2");
        assert_eq!(result[1].speaker.as_deref(), Some("speaker_01"));
        assert!(!result[1].uncertain);
    }
}
