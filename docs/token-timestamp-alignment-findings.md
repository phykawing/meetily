# Token-level Whisper timestamps for speaker alignment (issue #32)

Carried forward from issue #3 as an open empirical question: `whisper_engine.rs` requests
per-token timestamps (`params.set_token_timestamps(true)`) at the same two call sites that
suppress timestamp *tokens* in decoding (`params.set_no_timestamps(true)`), but nothing
reads them — `diarization::alignment` works at VAD-chunk granularity instead, flagging a
chunk `uncertain` when more than one speaker overlaps it. The question: are the per-token
timestamps whisper.cpp produces under that combination sane enough to attribute sub-chunk
token runs to speaker turns, or does suppressing timestamp tokens in decoding also corrupt
the token-level timestamps that ride along with `token_timestamps(true)`?

## Method

Ran the real production decoding path (adaptive beam search, `no_timestamps(true)` +
`token_timestamps(true)`, all the same suppression thresholds
`transcribe_audio_with_confidence` uses) against the full 13:15 evaluation recording from
`docs/cantonese-candidate-evaluation.md` (`MM_Weekly_pedia.mp3`, kept local and never
committed — real recorded speech), using the `large-v3-turbo` model already used for that
evaluation. Added an opt-in `#[ignore]`'d harness,
`whisper_engine::whisper_engine::tests::dump_token_level_timestamps`
(`frontend/src-tauri/src/whisper_engine/whisper_engine.rs`), that builds its own context and
params (mirroring `transcribe_audio_with_confidence` exactly) and reads every token's
`t0`/`t1` via `WhisperState::full_get_token_data`, checking `t1 >= t0` and non-decreasing
`t0` across the whole file.

```text
MEETILY_TEST_MODEL_PATH=<ggml file> \
MEETILY_TEST_AUDIO_PATH=<audio file> \
  cargo test -p meetily --lib --release \
  whisper_engine::whisper_engine::tests::dump_token_level_timestamps \
  -- --ignored --nocapture --test-threads=1
```

On this hardware (CPU-only, no GPU acceleration compiled in), the full run took **~2 hours**
for 13:15 of audio — reproducing this is a real time investment, not a quick check.

## Result: timestamps are sane

- **919 tokens across 34 segments, zero monotonicity violations.** Every token's `t1 >= t0`,
  and `t0` never regressed against the previous token's `t1`, for the entire file.
- **Absolute values check out.** `t0`/`t1` are in whisper.cpp's usual centisecond units
  (value ÷ 100 = seconds) — segment boundaries land exactly on the 30-second decode window
  (`t=3000`, `t=6000`, ...), confirming `no_timestamps(true)` suppresses only the *decoded
  timestamp tokens*, not whisper.cpp's internal per-token timestamp computation.
- Timestamps stayed sane across both English and Chinese-script tokens (the recording
  code-switches, as `docs/cantonese-candidate-evaluation.md` describes), including a single
  Chinese character (苦, "bitter" — thematically on-topic for a bitterness-tasting
  recording) landing at a plausible `t0=46.55s, t1=46.84s`.

**Answer: yes, token-level timestamps are usable for alignment.**

## Two caveats worth designing around

1. **Zero-width and clustered timestamps are not rare.** 36 of 919 tokens (~4%) had
   `t0 == t1`, and 6 of those were exact duplicates of the immediately preceding token's
   span — a run of several consecutive tokens all reporting the identical instant. This
   showed up concentrated in a lower-confidence stretch near the end of the file
   (`p` in the 0.3–0.7 range) rather than spread evenly. A token-level aligner needs a
   tie-break for a run of same-timestamp tokens (e.g. fall back to the enclosing VAD
   chunk's attribution for that run), not just `align_chunks_to_turns`'s existing per-span
   overlap math.
2. **Per-token *text* fragments multi-byte Han characters.** Whisper's byte-level BPE
   tokenizer can split one Han character's UTF-8 bytes across two or three consecutive
   tokens; decoding a single token's text in isolation then produces `�` replacement
   characters (only the *concatenation* of those tokens decodes correctly). This doesn't
   affect the timestamps — each of those tokens still carries a valid, monotonic `t0`/`t1`
   — but reconstructing readable text for a token run assigned to one speaker requires
   concatenating the run's raw bytes before UTF-8 decoding, not decoding token text
   independently the way segment text already is.

## What a prototype would actually need — and what it doesn't

`diarization::alignment::align_chunks_to_turns` already operates on a generic
`ChunkSpan { id, start, end }` — it has no notion of "VAD chunk" beyond the name. Handed a
`ChunkSpan` per *token* instead of per VAD chunk, with `id` set to the token's index and
`start`/`end` converted to seconds, it produces a `ChunkAttribution` per token using the
exact same overlap-and-tie-break logic already fully unit-tested for chunk granularity — see
the new `token_spans_are_just_another_kind_of_chunk_span` test added to
`alignment.rs` alongside this note, which exercises it directly. **No new alignment
algorithm is needed.**

What token-level alignment would actually require, none of which exists yet:

- Plumbing per-token `(text bytes, t0, t1)` out of `transcribe_audio_with_confidence` /
  `transcribe_audio` (today they discard everything but the joined segment text) and
  persisting it per transcript chunk, instead of only the chunk's own start/end.
- The zero-width tie-break from the caveat above.
- Byte-level (not per-token) UTF-8 reassembly when rendering an attributed token run's text.

## What this note does not answer

The issue also asks to "compare attribution quality against the current chunk-level pass by
ear" once the timestamps check out. That comparison needs a completed diarization run
(consent granted, ~32 MB of models downloaded, the sherpa-onnx pipeline actually executed)
against real multi-speaker audio, followed by a human listening to both the existing
chunk-level attribution and a token-level one and judging which sounds more accurate at
speaker-change boundaries — the same "judged by listening, not derived analytically" method
ADR-0007 used for the clustering threshold. That is a subjective, human-in-the-loop step
this session cannot perform (no audio playback, and no diarization models downloaded
locally). **This is left for a human to run before token-level alignment is adopted in
production** — the empirical blocker from issue #3 is cleared, but adoption itself is not
yet decided.
