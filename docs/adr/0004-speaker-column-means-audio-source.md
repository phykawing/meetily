# `transcripts.speaker` means Audio Source; diarized Speakers get their own columns

Migration `20251110000001_add_speaker_field.sql` added a `speaker TEXT` column to
`transcripts` documented as holding `'mic'` or `'system'`, but nothing in Rust or TypeScript
ever read or wrote it — it is not even a field on the `Transcript` struct. Rather than
repurpose it for diarization labels, we use it for its documented meaning: the live
**Audio Source** hint from ADR-0001. The diarized **Speaker** is a genuinely different
concept — one is which stream carried the audio, the other is whose voice it is — and gets
its own columns plus a table mapping per-meeting speaker labels to user-assigned names.

## Consequences

- Migrations are compile-embedded and append-only, so this is effectively permanent. A
  future reader finding a long-dormant column suddenly in use has this note to explain it.
- A transcript row can carry both: an Audio Source recorded live, and a Speaker filled in by
  the post-meeting pass. They can disagree, and that is not a bug.
- Note that much of the existing Rust code uses "speaker" to mean a loudspeaker (output
  device). New code says "output device" for that, "Audio Source" for the stream, and
  "Speaker" only for a human voice.
