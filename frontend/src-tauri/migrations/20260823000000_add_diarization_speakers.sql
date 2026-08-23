-- Migration: Add diarized Speaker storage, kept distinct from the Audio Source
--
-- transcripts.speaker (see 20251110000001_add_speaker_field.sql, docs/adr/0004) already
-- holds the live Audio Source hint ('mic' / 'system' / 'mixed'). The diarized Speaker is a
-- different concept — whose voice it is, not which stream carried it — so it gets its own
-- columns here rather than overloading that one. See docs/adr/0001 and docs/adr/0004.
--
-- speaker_label is the per-meeting cluster id a diarization pass assigned (e.g.
-- "speaker_00"), or NULL for a chunk not yet covered by any run. speaker_uncertain flags a
-- chunk that straddled a speaker change (more than one distinct speaker overlapped it) —
-- see diarization::alignment. Both are wiped and rewritten on every diarization run,
-- including re-runs.
ALTER TABLE transcripts ADD COLUMN speaker_label TEXT;
ALTER TABLE transcripts ADD COLUMN speaker_uncertain INTEGER NOT NULL DEFAULT 0;

-- Maps each meeting's discovered speaker labels to a user-facing name. Diarization seeds
-- this with a default "Speaker N" (ordinal by first appearance); a later rename overwrites
-- display_name. Clustering is not stable across runs (ADR-0001), so re-running diarization
-- for a meeting deletes and rebuilds this table's rows for that meeting rather than merging.
CREATE TABLE IF NOT EXISTS meeting_speakers (
    meeting_id TEXT NOT NULL,
    speaker_label TEXT NOT NULL,
    display_name TEXT NOT NULL,
    PRIMARY KEY (meeting_id, speaker_label),
    FOREIGN KEY (meeting_id) REFERENCES meetings(id) ON DELETE CASCADE
);
