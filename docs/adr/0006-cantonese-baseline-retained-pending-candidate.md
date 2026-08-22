# Cantonese baseline retained; no fine-tune candidate adopted yet

Issue #13 gated the whole Cantonese effort (#1) on a side-by-side comparison: no candidate
fine-tune ships as recommended unless it beats the built-in `large-v3-turbo` (`zh` token +
`CANTONESE_PROMPT_SEED`) baseline on real, representative meeting audio. Full method,
transcripts and analysis are in `docs/cantonese-candidate-evaluation.md`; this ADR records
the decision itself.

## Decision

**The baseline is retained.** Neither converted candidate
(`JackyHoCL/whisper-large-v3-turbo-cantonese-yue-english`, fp16 or q5_0 quantized — #6) is
registered or recommended.

- The fp16 candidate gets script and register right — genuine 口語 colloquial Cantonese,
  which the baseline structurally cannot produce (see `language.rs`'s comment on why `yue`
  is never forced on stock checkpoints: forcing it collapses the decoder). But on the
  evaluation recording it transcribed only **45% of the content** the baseline captured —
  entire participant turns and segments are simply missing, not garbled or summarized.
- The q5_0 quantized variant is unusable outright: 5% coverage, a hallucinated opening line,
  and the slowest wall-clock time of any arm despite being the smallest file.
- A transcript missing over half of what was said is a worse product than one that is
  complete but in the wrong written register, for the intended use (meeting minutes people
  need to act on). Register correctness does not outweigh a coverage collapse this severe.

## Consequences

- Cantonese transcription continues to use the `zh`-plus-prompt baseline for every user
  until a candidate demonstrably closes the coverage gap. Its known limitation (書面語
  register instead of verbatim 口語) remains open.
- Nothing is registered in any user's custom model registry as a result of this evaluation
  — registering a model that drops over half of every meeting would misrepresent it as
  vetted, which it is not.
- The conversion procedure (`scripts/convert-whisper-to-ggml.md`) and the registration path
  (#12) both work end-to-end and remain available to a user who wants to experiment with
  `cantonese-yue-en-turbo` (fp16) themselves, with this ADR's coverage caveat as documented
  context.
- **Recommended follow-up, out of this issue's scope**: convert and evaluate
  `JackyHoCL/whisper-large-v3-turbo-cantonese-noise-detection` — the same author's
  hallucination-focused successor model — against the same evaluation audio, using the
  harness added for this issue (`whisper_engine::whisper_engine::tests::transcribe_env_audio_with_env_model`,
  parameterised by env vars, in `whisper_engine.rs`). If it closes the coverage gap while
  keeping the fp16 candidate's register correctness, it becomes the first real adoption
  candidate and this ADR should be superseded.
- The evaluation harness itself is reusable for that follow-up and any future candidate
  without further engineering — it exercises the exact production decode path
  (`resolve_decoding` + `transcribe_audio_with_confidence`), not a standalone CLI, so future
  comparisons stay faithful to what the app actually does.
