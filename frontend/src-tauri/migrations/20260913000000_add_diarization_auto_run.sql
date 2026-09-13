-- Migration: Add a toggle for the automatic post-recording speaker-detection pass
-- Independent of diarizationConsent: consent gates whether diarization is available at
-- all (on-demand via the Speakers button), this gates whether it also runs automatically
-- after every recording. NULL/absent means enabled (today's existing always-on behaviour
-- when models are ready), 'disabled' opts out of the automatic pass only. See
-- phykawing/meetily#31.
ALTER TABLE settings ADD COLUMN diarizationAutoRun TEXT;
