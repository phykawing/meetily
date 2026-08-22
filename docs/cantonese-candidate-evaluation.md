# Cantonese candidate evaluation (issue #13)

Side-by-side comparison of every Cantonese transcription candidate against the built-in
baseline, on real meeting-style audio, ending in an explicit adoption decision. This is the
gate for the whole Cantonese effort (#1): a candidate is adopted only if it beats the
baseline on this audio.

## Evaluation set

**Provenance.** `MM_Weekly_pedia.mp3` at the repo root (13:15, 44.1 kHz stereo MP3,
recorded 2026-08-20). Treated here as the evaluation recording described in the project's
working notes ("recording 3-5 minutes of representative code-switched Cantonese"); it runs
longer than that estimate, which only means more coverage. **This file is never committed**
— it is real recorded speech, kept local and referenced here by filename, duration and
description only. If this assumption about which file is the evaluation set is wrong,
everything below needs to be re-run against the correct one.

**Content**, from listening to a sample of the transcripts: a multi-speaker segment (a host
plus several participants) built around a bitterness-tasting challenge/game format, with
frequent code-switching into English words and phrases and several proper nouns/brand names
(Wikipedia, Amazon, Switch, Youtuber, Espresso, product/personal names).

## Method

Every candidate is run through the **real engine code path**, not a standalone CLI, so the
comparison reflects exactly what the app would produce:

- `whisper_engine::language::engine_language_for` resolves the UI's "Cantonese" choice to
  whichever token the loaded model actually gets (the stock `zh`-plus-prompt baseline, or a
  registered model's declared token).
- `whisper_engine::language::build_initial_prompt` composes the same
  `CANTONESE_PROMPT_SEED` initial prompt for every arm (it depends only on the UI language,
  not on which model is loaded — so the baseline and every yue-token candidate all get the
  same script/register bias prompt).
- `WhisperEngine::transcribe_audio_with_confidence` runs the actual decode with the app's
  production `FullParams` (adaptive beam size, no-timestamps, suppression thresholds, etc.)
  — unchanged from what a real recording or import would use.
- Audio is decoded with `audio::decoder::decode_audio_file(...).to_whisper_format()`, the
  same 16 kHz-mono conversion the app's own import/retranscription path uses.

This is wired up as an opt-in `#[ignore]`'d test,
`whisper_engine::whisper_engine::tests::transcribe_env_audio_with_env_model` (added for this
issue), parameterised by env vars so the same harness runs every arm:

```powershell
$env:MEETILY_TEST_MODEL_PATH = "<ggml file>"
$env:MEETILY_TEST_AUDIO_PATH = "<audio file>"
$env:MEETILY_TEST_MODEL_NAME = "<label, default 'large-v3-turbo'>"
$env:MEETILY_TEST_ENGINE_LANGUAGE = "<token>"   # omit to exercise the builtin large-v3 path
$env:MEETILY_TEST_UI_LANGUAGE = "yue"
$env:MEETILY_TEST_OUTPUT_PATH = "<where to write the transcript>"
$env:CFLAGS = "/utf-8"; $env:CXXFLAGS = "/utf-8"   # MSVC needs this for whisper.cpp's non-ASCII literals
cargo test -p meetily --lib --release `
  whisper_engine::whisper_engine::tests::transcribe_env_audio_with_env_model `
  -- --ignored --nocapture --test-threads=1
```

When `MEETILY_TEST_ENGINE_LANGUAGE` is set, the harness registers the model as a custom
entry via the real `custom_models::add()` (issue #12's own API — not hand-written JSON)
before loading it, so registration validation runs for free and the resolved token is
exactly what a real registration would produce.

**Build note**: this machine's whisper-rs-sys build has no GPU/BLAS acceleration compiled
in (a plain CPU CMake build, per `scripts/convert-whisper-to-ggml.md`), so wall-clock timing
below is **not representative of the CUDA build** the target hardware (RTX 2070 Super) would
actually run. Timing is recorded for completeness but is not part of the adoption decision;
content quality is.

**Scope note on "exactly what the app would produce"**: this covers the transcription
engine itself — language resolution, prompt, decode params. It does **not** cover ingest-time
post-processing that happens after transcription, in particular Simplified→Traditional
script conversion (ADR-0003), which is applied to every Chinese transcript regardless of
what the model emitted and is not exercised by this harness or by
`transcribe_audio_with_confidence` itself. The **Script** row below reports each model's
*raw* output only; in the shipped app all three arms would read Traditional after ingest
regardless of this row, so Script is not itself a differentiator between candidates — it's
included for interest, not as a decision input.

## Candidates

| Arm | Model | Engine token | Source |
|---|---|---|---|
| Baseline | `large-v3-turbo` (builtin) | `zh` + `CANTONESE_PROMPT_SEED` | Meetily's own catalog |
| Candidate A | `cantonese-yue-en-turbo` (fp16) | `yue` | `JackyHoCL/whisper-large-v3-turbo-cantonese-yue-english`, converted per `scripts/convert-whisper-to-ggml.md` |
| Candidate B | `cantonese-yue-en-turbo-q5_0` (quantized) | `yue` | Same fine-tune, quantized |

The `khleeloo/whisper-large-v3-cantonese` model considered during conversion (#6) was
rejected before this stage — it is a gated Hugging Face repo, so it never produced a ggml
file to evaluate. `JackyHoCL`'s own listed successor
(`whisper-large-v3-turbo-cantonese-noise-detection`) was not converted or evaluated here;
flagged below only if the primary comparison turns out inconclusive or hallucination-heavy
enough to warrant it.

## Results

### Baseline — `large-v3-turbo`, `zh` token

- **Wall clock**: 6576s (~110 min) for 13:15 of audio, CPU-only build.
- **Confidence** (engine's own segment-length heuristic, not a calibrated metric): 0.876.
- **Script**: Traditional (繁體) — confirmed (e.g. 討厭, 臨床, 學, 紀錄).
- **Register**: reads as **書面語** (Standard Written Chinese), not verbatim **口語**
  colloquial Cantonese. No 係/唔係/嘅/喺/咗 markers anywhere in the transcript; Cantonese
  speech is being normalised into standard-register Mandarin-style written Chinese rather
  than transcribed as spoken. This is the opposite of what ADR-0002 requires from a
  canonical transcript.
- **Code-switching**: embedded English mostly survives untranslated (Wikipedia, Amazon,
  Switch, Youtuber, Mastery, OK). One clear mishearing: "espresso" transcribed as
  "Expressio".
- Full transcript: `D:\phykawing\cantonese-model-conversion\eval-results\baseline-zh.txt`
  (not committed — see provenance note above).

### Candidate A — `cantonese-yue-en-turbo` (fp16), `yue` token

- **Wall clock**: 6522s (~109 min) for the same 13:15 audio — no meaningful speed
  difference from baseline on this CPU-only build.
- **Confidence**: 0.703 (baseline 0.876) — the engine's own length-based heuristic, not a
  calibrated quality signal, but directionally consistent with the coverage gap below.
- **Script**: Traditional (繁體).
- **Register**: genuinely **口語** colloquial Cantonese throughout — 係, 唔係, 嘅, 喺, 咗,
  我哋, 咁, 呢, 佢 appear constantly. This is exactly what ADR-0002 wants from the canonical
  transcript and exactly what the baseline fails to produce.
- **Code-switching**: English words are kept untranslated and lower-case, matching natural
  code-switched speech better than the baseline's capitalized renderings ("coffee shop",
  "tips", "mastery" vs. baseline's "Wikipedia"-style caps) — though "espresso" is mis-heard
  here too, as "expercial" (baseline: "Expressio"; neither candidate gets this word right).
- **Coverage gap — the serious finding**: the transcript is **2936 characters vs. the
  baseline's 6561** (45%). This is not a trimming or truncation artifact of the harness —
  both arms ran the identical decoded 13:15 audio through the same `transcribe_audio_with_confidence`
  call. Comparing the two transcripts by content, whole narrative sections present in the
  baseline (the four participants' bitter-taste-test play-by-play, the salt/water/milk
  remedy segment, the Nintendo Switch reference, several minutes of back-and-forth) are
  simply **absent** from the candidate's output, not summarized or garbled — they don't
  appear at all. The candidate's own confidence score (0.703) is lower, consistent with more
  segments being suppressed or dropped during decode rather than emitted with low confidence.
- **Decode artifacts**: two `�` (U+FFFD) replacement characters appear where the model
  produced a malformed token sequence whisper-rs could not decode as valid UTF-8 (verified:
  the file itself is valid UTF-8, so this is the model's output, not a file-encoding bug).
  The baseline transcript has zero such artifacts.
- Full transcript: `D:\phykawing\cantonese-model-conversion\eval-results\candidate-fp16-yue.txt`

### Candidate B — `cantonese-yue-en-turbo-q5_0` (quantized), `yue` token

- **Wall clock**: 9171s (~153 min) — the *slowest* arm despite being the smallest model
  file (547 MB vs. 1549 MB), consistent with the decoder spending most of its time on
  low-probability/failed decode attempts rather than genuine speedup from quantization.
- **Confidence**: 0.199 — far below both other arms.
- **Coverage**: **348 characters total** (5% of baseline, 12% of the fp16 candidate).
  Effectively transcribes only the first ~15-20 seconds of the recording, then produces
  nothing usable for the remaining ~13 minutes.
- **Quality of what it did produce**: partially hallucinated even in that short opening —
  "天使唔使瘦發車" ("angel doesn't need to slim-drive-a-car") is not coherent Cantonese and
  doesn't correspond to anything in the baseline or fp16 transcripts of the same audio.
  Also reproduces the 苦→虎 ("bitter"→"tiger") mishearing seen in the fp16 candidate and in
  the conversion doc's own smoke-test sample, suggesting it's a systematic weakness of this
  fine-tune rather than a one-off.
- **Verdict for this arm on its own**: unusable. Quantizing this particular fine-tune to
  q5_0 destroys it far beyond the acceptable quality loss quantization normally costs —
  this is a collapse, not a degradation.
- Full transcript: `D:\phykawing\cantonese-model-conversion\eval-results\candidate-q5_0-yue.txt`

## Comparison

| | Baseline (`large-v3-turbo`, `zh`) | Candidate A (fp16, `yue`) | Candidate B (q5_0, `yue`) |
|---|---|---|---|
| Content coverage | 6561 chars (100%) | 2936 chars (45%) | 348 chars (5%) |
| Script (raw model output; ingest normalizes all arms to Traditional regardless — not a differentiator) | Traditional ✅ | Traditional ✅ | Traditional ✅ |
| Register | 書面語 ❌ (ADR-0002 wants 口語) | 口語 ✅ | too little output to judge |
| Code-switched English preserved | Mostly ✅ (1 mishearing) | Mostly ✅ (1 mishearing) | too little output to judge |
| Decode artifacts | None | 2× `�` malformed tokens | Hallucinated opening line |
| Engine confidence | 0.876 | 0.703 | 0.199 |
| Wall clock (CPU-only build) | ~110 min | ~109 min | ~153 min |

**On the axis the whole Cantonese effort exists for** — writing what was actually said, in
the register it was actually said in — candidate A is the only arm that gets this right.
The baseline's fluent, complete, wrong-register output is exactly the failure #1's problem
statement describes: readable Chinese that isn't what a Cantonese speaker actually said.

**But completeness dominates for a meeting transcript.** A transcript missing 55% of what
was said — entire participant turns, entire segments of the conversation — is not a usable
substitute for the baseline, register correctness notwithstanding. Nobody can act on
decisions or action items that were silently dropped. Candidate B is not a candidate at
all; it fails outright.

**A plausible cause for candidate A's coverage gap**: `JackyHoCL/whisper-large-v3-turbo-cantonese-yue-english`
was trained on Common Voice's `yue` split (short, single-speaker, low-noise utterances) plus
a "purpose-built mixed dataset" (per the conversion doc) — not on long multi-speaker
recordings with background noise, overlapping speech, and audio-game-show production values
like this evaluation set. It plausibly generalizes worse than the far-more-broadly-trained
stock `large-v3-turbo` checkpoint to exactly the audio conditions real meetings have. This is
circumstantial, not confirmed — no segment-level logging was captured in this pass to prove
where or why segments were dropped — but it lines up with why the model author's own listed
successor is described as reducing "hallucination during streaming": the same base model
family evidently has a known coverage/hallucination weakness on demanding audio, and a
follow-up fine-tune exists specifically to address it.

**Residual**: this evaluation checks completeness, script, register and code-switching —
axes that don't require having heard the meeting live. It cannot certify word-level
transcription *accuracy* for the content each arm did produce, since nobody who was in the
room reviewed these transcripts against the actual audio. That check, if wanted, is on the
user.

## Verdict

**No candidate beats the baseline. The baseline is retained.**

- Candidate A (fp16, `yue`) proves the *approach* works — a properly-token-declared
  Cantonese fine-tune genuinely produces correct 口語 register and script, which the
  built-in models structurally cannot (see `language.rs`'s comment on why `yue` is never
  forced on stock checkpoints). But it is not adoptable as-is: a 55%-incomplete meeting
  transcript is a worse product than a complete one in the wrong register, for the intended
  use case (meeting minutes people need to act on).
- Candidate B (q5_0, `yue`) is rejected outright — unusable, not merely worse.
- **Neither candidate is registered.** Registering a model that structurally drops over
  half of every meeting would misrepresent it as a vetted, ready-to-use option to a user
  who has no way to know about this evaluation's findings.
- **Recommended follow-up**, not part of this issue's scope: evaluate
  `JackyHoCL/whisper-large-v3-turbo-cantonese-noise-detection` (the same author's
  hallucination-focused successor, not converted or evaluated here — see
  `scripts/convert-whisper-to-ggml.md`'s worked example) against this same audio file using
  the harness this issue built (`transcribe_env_audio_with_env_model`). If it closes the
  coverage gap while keeping candidate A's register correctness, it would be the first real
  adoption candidate. Until then, Cantonese users get the baseline's `zh`-plus-prompt path,
  and can still register `cantonese-yue-en-turbo` (fp16) themselves through the model
  manager (#12) if they want to experiment — that path was built and works — with the
  caveat documented here that it drops significant content on demanding audio.
