# Simplified→Traditional conversion happens at ingest, and the converted text is stored

Whisper's `zh` decoding emits mostly Simplified Chinese regardless of what was spoken, so
Cantonese and Mandarin meetings need converting to Traditional. Conversion is deterministic
and idempotent (unlike the 口語→書面語 rewrite of ADR-0002, which is lossy and therefore a
Rendering), so we convert once as transcripts are stored rather than on every read, and keep
only the converted text. Conversion uses `ferrous-opencc` — pure Rust, avoiding an OpenCC
C++ build dependency on Windows — with the `s2hk` config for the default Traditional (HK)
option, since the target users are in Hong Kong.

## Consequences

- Search, summaries, exports and the LLM all see one consistent form. No consumer can forget
  to convert.
- The model's raw output is not retained. The conversion applied is recorded in meeting
  metadata so re-transcription can reproduce or change it.
- Script is a setting (Traditional HK / Simplified / leave as recognized), applied to any
  Chinese transcription, not an attribute of the language selection. Language and script are
  separate concepts in `CONTEXT.md`, and folding script into the language list would both
  contradict that and double the list.
- Summary language selection cannot rely on `whatlang`, which reports plain `zh` and cannot
  distinguish Traditional from Simplified: Cantonese transcription forces `zh-tw`, with
  character-set-membership detection as the fallback for imported meetings.
