-- Migration: Add the Rendering provider setting to the app-wide settings table
-- A Rendering (書面語 register rewrite, see phykawing/meetily#8) is a full-transcript LLM
-- pass, so which provider performs it is its own privacy decision, distinct from the
-- summary provider setting. NULL means unset, which resolves to "local" (the privacy-safe
-- default) — see docs/adr/0002 and phykawing/meetily#15.
ALTER TABLE settings ADD COLUMN renderingProvider TEXT;
