-- Migration: Add meeting vocabulary to transcript_settings table
-- Stores the user's meeting vocabulary (names, jargon, product terms) folded into the
-- Whisper initial prompt for every transcription language.

ALTER TABLE transcript_settings ADD COLUMN meetingVocabulary TEXT;
