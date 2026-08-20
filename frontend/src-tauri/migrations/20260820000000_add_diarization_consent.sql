-- Migration: Add diarization model download consent to the app-wide settings table
-- Tracks whether the user has answered the "download speaker-diarization models?" prompt:
-- NULL means not asked yet, 'granted' or 'declined' record the answer. See docs/adr/0005.
ALTER TABLE settings ADD COLUMN diarizationConsent TEXT;
