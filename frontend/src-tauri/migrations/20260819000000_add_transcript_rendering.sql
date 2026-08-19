-- Migration: Add Written Form preference and cached transcript Rendering
--
-- The Canonical Transcript (transcripts table) stays verbatim 口語. A 書面語 Rendering is
-- a derived, disposable view: cached per meeting and invalidated whenever the underlying
-- transcript changes. See docs/adr/0002-canonical-transcript-is-verbatim-colloquial.md.
--
-- written_form is the per-meeting view preference (colloquial / written), defaulting to
-- colloquial. transcript_renderings holds the cached 書面語 text plus a fingerprint of the
-- canonical transcript it was generated from; a fingerprint mismatch at read time means the
-- transcript changed since generation, so the cached row is treated as stale.
ALTER TABLE meetings ADD COLUMN written_form TEXT NOT NULL DEFAULT 'colloquial';

CREATE TABLE IF NOT EXISTS transcript_renderings (
    meeting_id TEXT PRIMARY KEY NOT NULL,
    rendered_text TEXT NOT NULL,
    source_fingerprint TEXT NOT NULL,
    generated_at TEXT NOT NULL,
    FOREIGN KEY (meeting_id) REFERENCES meetings(id) ON DELETE CASCADE
);
