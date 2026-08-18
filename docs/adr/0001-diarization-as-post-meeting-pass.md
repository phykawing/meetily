# Diarization runs as a post-meeting pass, not live

`audio/pipeline.rs` mixes microphone and system audio into a single mono stream *before*
VAD, before transcription, and before the WAV is written — so by the time any transcript
exists, per-speaker information is already gone, and the saved recording is mono-mixed too.
Rather than restructure the hot path, diarization runs after the recording stops, over the
saved audio file, reusing the machinery `audio/retranscription.rs` already has for
re-reading a meeting's WAV. During recording we emit only a cheap **Audio Source** hint
(microphone vs system, from energy dominance in the mixer's two windows); the post-pass
replaces those hints with real **Speaker** labels.

## Consequences

- Imported files and re-transcribed meetings get diarization for free — they travel the
  same path. Any meeting that still has its audio can be diarized on demand.
- Speaker labels appear minutes after the meeting ends, not live. Auto-summary waits for
  the pass so summaries can attribute action items.
- Clustering is not stable across runs: a second pass may find a different number of
  speakers, and "Speaker 2" may be a different person. **Re-running therefore discards all
  user-assigned speaker names**, with a warning beforehand. Remapping names by comparing
  voice embeddings between runs was rejected as disproportionate work for a rare operation
  — it is essentially the persistent voice-print feature we deliberately declined.
- Simultaneous speech during recording is labelled `mixed` rather than guessed at; the
  post-pass overwrites it anyway.
