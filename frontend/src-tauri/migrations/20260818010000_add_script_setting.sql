-- Migration: Add Script setting to transcript_settings table
-- Stores which Han character set Chinese transcripts are converted into at ingest
-- (Traditional HK / Simplified / leave as recognized). NULL means unset, which the
-- application resolves to the default (Traditional HK). See docs/adr/0003.
ALTER TABLE transcript_settings ADD COLUMN scriptSetting TEXT;
