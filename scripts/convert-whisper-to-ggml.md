# Converting a Hugging Face Whisper fine-tune to ggml

Cantonese fine-tunes are published in Hugging Face `transformers` format (safetensors +
`config.json`), which Meetily cannot load — the Rust core loads models through `whisper-rs`,
which expects whisper.cpp's single-file `ggml`/`GGUF` binary format (see
`whisper_engine/whisper_engine.rs`'s `validate_model_file`, which checks for the `ggml`,
`GGUF`, `ggmf`, `lmgg`, `FUGU`, `fmgg` magic headers). This is a **local, reproducible**
conversion procedure. Nothing here uploads or redistributes a model file — the output stays
on your machine and is registered with Meetily by absolute path (see `custom_models.rs`),
never copied into the repo.

The procedure is generic: it converts any standard-architecture HF Whisper checkpoint
(fine-tuned or not) to ggml. The worked example below applies it to the fine-tune currently
chosen for Cantonese (issue #6); repeat the same steps for other candidates evaluated in
issue #13.

## Prerequisites

- **Git**, and enough disk space: budget ~15 GB free for a `large-v3`/`large-v3-turbo`-based
  fine-tune (safetensors download + intermediate f32 tensors + fp16 ggml + quantized ggml).
- **Python 3.10+** with `pip`. Use one interpreter for everything below — mixing a system
  Python with a different `pip` (e.g. an App-Store Python plus an Anaconda `pip`) silently
  installs packages where the interpreter can't see them. Create a dedicated venv:
  ```bash
  python -m venv venv
  # POSIX
  source venv/bin/activate
  # Windows PowerShell
  venv\Scripts\Activate.ps1
  ```
- **A C/C++ toolchain + CMake**, to build whisper.cpp's quantization tool:
  - Windows: Visual Studio Build Tools 2022 with the "Desktop development with C++" workload
    (same MSVC requirement as building the Tauri app itself — see `docs/BUILDING.md`).
  - macOS/Linux: Xcode Command Line Tools / `build-essential`, both already required to build
    Meetily's Rust core.
- This is independent of Meetily's own Rust build — no `cargo`/Tauri toolchain is needed for
  conversion itself, only to run the verification test in the last step.

## Step 1 — set up a working directory outside the repo

Use a durable location, not the repo tree or a session-scoped scratch directory: the
converted files are multi-gigabyte and get referenced by absolute path from Meetily's
custom-model registry (issue #12), so they need to keep existing after this session ends.

```bash
mkdir cantonese-model-conversion && cd cantonese-model-conversion
python -m venv venv && source venv/bin/activate   # or Activate.ps1 on Windows
pip install torch --index-url https://download.pytorch.org/whl/cpu
pip install numpy transformers huggingface_hub
```

CPU-only `torch` is sufficient — conversion is weight repacking, not inference.

## Step 2 — clone the two repos the conversion script needs

```bash
git clone --depth 1 https://github.com/openai/whisper.git openai-whisper
git clone --depth 1 https://github.com/ggml-org/whisper.cpp.git whisper.cpp
```

- `openai-whisper` supplies `whisper/assets/mel_filters.npz` — the conversion script reads
  this one file from the clone; nothing else from that repo is used.
- `whisper.cpp` supplies `models/convert-h5-to-ggml.py` and the quantization tool's source.

## Step 3 — build whisper.cpp's quantization tool

```bash
cd whisper.cpp
cmake -B build -DCMAKE_BUILD_TYPE=Release
cmake --build build --config Release --target whisper-quantize --target whisper-cli
cd ..
```

(`whisper-cli` is built alongside `whisper-quantize` here because Steps 5 and 8 both use it
to smoke-test the converted files with real audio.)

Two things that don't match older blog posts / the upstream README's shorthand:

- The **target and binary are named `whisper-quantize`**, not `quantize`, on current
  whisper.cpp — `cmake --build build --config Release --target quantize` fails with
  `MSB1009: Project file does not exist` on Windows.
- On Windows, MSVC is a multi-config generator, so the binary lands at
  `whisper.cpp\build\bin\Release\whisper-quantize.exe`, not `build/bin/whisper-quantize`.
  On POSIX (single-config Makefiles/Ninja) it's `whisper.cpp/build/bin/whisper-quantize`.

`whisper-quantize` (no args) prints the supported type names — this build offers `q2_k`,
`q3_k`, `q4_0`, `q4_1`, `q4_k`, `q5_0`, `q5_1`, `q5_k`, `q6_k`, `q8_0`. Meetily's built-in
catalog (`frontend/src-tauri/src/config.rs::WHISPER_MODEL_CATALOG`) uses `q5_0`/`q5_1` for
its quantized variants; this procedure uses `q5_0` to match.

## Step 4 — download the Hugging Face fine-tune

Before downloading the multi-gigabyte weights, list the repo's files and check whether it is
**gated**. A gated repo requires a logged-in, terms-accepted Hugging Face account and cannot
be fetched unattended — the same problem ADR-0005 hit with pyannote's diarization models.
Prefer an ungated alternative; do not build a documented procedure around a repo a second
machine can't actually run without manual account setup.

```python
from huggingface_hub import model_info
info = model_info("<hf-org>/<hf-model>")
print("gated:", info.gated)  # False for open; True or a string like "auto" for gated
```

Then fetch the full model:

```python
from huggingface_hub import snapshot_download
snapshot_download(
    "<hf-org>/<hf-model>",
    local_dir="model-src",
    allow_patterns=["*.json", "*.txt", "*.safetensors", "*.md"],
)
```

(`allow_patterns` skips training artifacts some fine-tune repos also carry, like
TensorBoard `runs/` event files or `training_args.bin`, which the converter doesn't need.)

## Step 5 — determine the language token the model expects

whisper.cpp needs one of its ~100 language tokens forced at decode time (`language.rs` in
Meetily calls this the *engine language*); the app's UI language and the engine token are not
always the same string (see that module's doc comment). Two ways to determine it, in order
of reliability:

1. **Read it from the model author**, if the model card says so. Not every card does —
   `generation_config.json`'s `language` field is often just an unedited inherited default
   from the base checkpoint, *not* what the model was actually fine-tuned with, so don't
   trust that field alone.
2. **Verify empirically.** This needs the converted `.bin` file, so it can only run after
   Step 6 — come back to it then. Transcribe a short real clip of the target speech with
   `whisper-cli` (built in Step 3) forcing each candidate token, and compare:
   ```bash
   whisper.cpp/build/bin/Release/whisper-cli.exe -m out/ggml-<your-name>.bin -f clip.wav -l yue -nt
   whisper.cpp/build/bin/Release/whisper-cli.exe -m out/ggml-<your-name>.bin -f clip.wav -l zh -nt
   ```
   `-nt` suppresses timestamps for easier comparison. `whisper-cli` needs a 16 kHz mono WAV;
   cut one from any source audio with ffmpeg (Meetily bundles one at
   `frontend/src-tauri/binaries/ffmpeg-x86_64-pc-windows-msvc.exe` on Windows):
   ```bash
   ffmpeg -y -i source.mp3 -ss 60 -t 30 -ar 16000 -ac 1 -c:a pcm_s16le clip.wav
   ```
   This only tells you which token the model *runs* under without erroring or looping —
   it is not a quality judgment (that's issue #13's job). If a candidate token produces
   garbled or repetitive output where another produces fluent text, that's a strong signal;
   if both look similarly plausible, prefer whatever the model card documents.

Record whichever token you determine — it's what goes in the custom model registry's
`engine_language` field (issue #12's registration form) so `language.rs::engine_language_for`
forces the right token instead of falling back to the stock `zh`-plus-prompt baseline.

## Step 6 — convert to ggml (full precision)

```bash
python whisper.cpp/models/convert-h5-to-ggml.py model-src openai-whisper out
mv out/ggml-model.bin out/ggml-<your-name>.bin
```

- Positional args: `<hf model dir> <openai/whisper repo dir> <output dir>`. A 4th argument
  (any value) switches to f32 output; omit it — f16 is what Meetily's catalog calls "full
  precision" for its own built-in models, and whisper.cpp's own convention treats f16 as the
  standard full-precision distribution (its downloadable catalog ships f16, not f32) while
  roughly halving file size.
- The script reads `vocab.json`/`added_tokens.json`/`config.json` from the HF model
  directory, not from the whisper.cpp or openai/whisper clones. If a fine-tune repo ships
  only a fast-tokenizer `tokenizer.json` (some do, and it's the known cause of whisper.cpp
  issue #2598's "max_length not an integer" and missing-vocab errors), regenerate the plain
  files before converting:
  ```python
  from transformers import AutoTokenizer
  AutoTokenizer.from_pretrained("model-src").save_pretrained("model-src")
  ```
- Output starts with the file's magic bytes as written to disk in little-endian order —
  literally `lmgg`, not `ggml` — because whisper.cpp writes the 32-bit magic constant
  `0x67676d6c` with a raw `fwrite`. Meetily's own `validate_model_file` already checks for
  this exact byte order, so this is expected, not a bug.

## Step 7 — quantize

```bash
whisper.cpp/build/bin/Release/whisper-quantize.exe out/ggml-<your-name>.bin out/ggml-<your-name>-q5_0.bin q5_0
```

Produces the second required variant. Both the fp16 and quantized files are ordinary
whisper.cpp ggml files from here on — nothing about them being derived from a fine-tune
matters to the rest of the pipeline.

## Step 8 — verify both files load

Three checks, cheapest first:

1. **Magic bytes** — already covered by `validate_model_file` when you register the model
   through the app (Step 9).
2. **`whisper-cli` smoke transcription** — the Step 5 commands already double as this check:
   if either file failed to load, `whisper-cli` would have errored out immediately rather
   than printing a transcript.
3. **The exact whisper.cpp build the app ships**, which can trail or lead the
   `ggml-org/whisper.cpp` checkout used above (`whisper-rs`/`whisper-rs-sys` vendor their own
   copy — check `frontend/src-tauri/Cargo.toml` for the pinned `whisper-rs` version). An
   `#[ignore]`d test in `whisper_engine.rs` loads a model straight through the same
   `whisper-rs` the app uses:
   ```bash
   # POSIX
   MEETILY_TEST_MODEL_PATH=/path/to/ggml-<your-name>.bin \
     cargo test -p meetily --lib whisper_engine::whisper_engine::tests::load_converted_model_from_env_path -- --ignored --nocapture
   ```
   ```powershell
   # Windows PowerShell
   $env:MEETILY_TEST_MODEL_PATH = "C:\path\to\ggml-<your-name>.bin"
   cargo test -p meetily --lib whisper_engine::whisper_engine::tests::load_converted_model_from_env_path -- --ignored --nocapture
   ```
   Run it once per variant. A pass here is a stronger guarantee than `whisper-cli` loading
   the file, because it is the actual code path `load_model` uses in the running app.

## Step 9 — register in Meetily

Settings → Whisper Models → the "Custom (user-registered) models" section
(`WhisperModelManager.tsx`) → browse to the ggml file → fill in a display name, the language
token from Step 5, whether it supports Cantonese, and a description. The registry
(`custom_models.rs`) validates the path exists and the name doesn't collide with a built-in
model, then stores the entry by absolute path — the file is never copied.

Register both variants under different names (e.g. `<name>` for fp16, `<name>-q5_0` for
quantized) if you want to compare speed/quality — that's exactly what issue #13's evaluation
needs to do side by side with the built-in baseline.

## Worked example: `JackyHoCL/whisper-large-v3-turbo-cantonese-yue-english`

This is the fine-tune chosen for issue #6, converted end-to-end with the steps above.

**Why this one.** An earlier candidate, `khleeloo/whisper-large-v3-cantonese`, turned out to
be a **gated** Hugging Face repo (confirmed via `model_info(...).gated` — `hf_hub_download`
fails with `GatedRepoError` unauthenticated) and was rejected for the same reason ADR-0005
rejected gated diarization model repos: it can't be fetched by an unattended process on
another machine. `JackyHoCL/whisper-large-v3-turbo-cantonese-yue-english` is ungated,
MIT-licensed, fine-tuned from `openai/whisper-large-v3-turbo` (architecturally identical to
the `large-v3-turbo` family Meetily already whitelists for Cantonese in
`language::is_cantonese_capable_builtin`), trained on Common Voice 17.0/16.1's `yue` split
plus a purpose-built mixed Cantonese/English dataset — directly targeting the code-switching
failure mode issue #1 describes ("我哋個 deadline 係下星期"). The model card literally states
CER results separately for `yue`, `en`, and `zh-CN` test splits, which is the same three-way
distinction Meetily's language selection cares about.

**Language token — determined and recorded.** The model card states directly: *"For
Cantonese + English, use 'yue', for Cantonese + Mandarin + English, use 'zh'."* Meetily's
Cantonese UI option targets Cantonese-plus-English meetings (issue #1's problem statement),
so the token is **`yue`**. Cross-checked empirically too: `whisper-cli -l yue` and `-l zh` on
a 30-second Cantonese clip produced near-identical, fluent colloquial output under both
tokens — neither showed the repetition-loop failure mode stock checkpoints exhibit — so the
model-card guidance was used to break the tie. `-l yue` is the registration value: it matches
the author's documented intent and Meetily's own `yue` UI constant (`language::CANTONESE`).
Whether the transcript is *accurate*, not just fluent and non-degenerate, is a judgment for
issue #13's side-by-side evaluation against real audio — this step only confirms the file
loads and decodes sanely under the chosen token.

**Commands run, in order:**

```bash
# Step 4
python -c "from huggingface_hub import model_info; print(model_info('JackyHoCL/whisper-large-v3-turbo-cantonese-yue-english').gated)"
# -> False
python -c "
from huggingface_hub import snapshot_download
snapshot_download('JackyHoCL/whisper-large-v3-turbo-cantonese-yue-english', local_dir='jackyhocl-full',
                   allow_patterns=['*.json','*.txt','*.safetensors','*.md'])
"

# Step 6
python whisper.cpp/models/convert-h5-to-ggml.py jackyhocl-full openai-whisper out
mv out/ggml-model.bin out/ggml-cantonese-yue-en-turbo.bin

# Step 7
whisper.cpp/build/bin/Release/whisper-quantize.exe \
  out/ggml-cantonese-yue-en-turbo.bin out/ggml-cantonese-yue-en-turbo-q5_0.bin q5_0

# Step 8 (empirical token check + smoke test, same command)
whisper.cpp/build/bin/Release/whisper-cli.exe -m out/ggml-cantonese-yue-en-turbo.bin -f clip.wav -l yue -nt

# Step 8 (exact whisper-rs build)
MEETILY_TEST_MODEL_PATH=out/ggml-cantonese-yue-en-turbo.bin \
  cargo test -p meetily --lib whisper_engine::whisper_engine::tests::load_converted_model_from_env_path -- --ignored --nocapture
MEETILY_TEST_MODEL_PATH=out/ggml-cantonese-yue-en-turbo-q5_0.bin \
  cargo test -p meetily --lib whisper_engine::whisper_engine::tests::load_converted_model_from_env_path -- --ignored --nocapture
```

**Results:**

| File | Size | Loads via `whisper-cli` | Loads via app's `whisper-rs` (0.13.2) |
|---|---|---|---|
| `ggml-cantonese-yue-en-turbo.bin` (fp16) | 1549.3 MB | ✅ | ✅ (`load_converted_model_from_env_path` passed) |
| `ggml-cantonese-yue-en-turbo-q5_0.bin` (quantized) | 547.4 MB | ✅ | ✅ (`load_converted_model_from_env_path` passed) |

Both sizes land almost exactly on Meetily's existing `large-v3-turbo` (1549 MB) and
`large-v3-turbo-q5_0` (547 MB) catalog entries, confirming the architecture match. whisper.cpp
reports both as `type = 5 (large v3)` with `n_text_layer = 4` (the turbo family's pruned
decoder) — consistent with the declared base model.

Sample output on a 30-second clip of Cantonese speech (`-l yue`, fp16 model) — quoted verbatim,
including whatever the model actually got wrong, since this is a loading/register smoke test
and not the accuracy evaluation issue #13 owns:

> 去到幾千年後嘅今日我哋會買虎離食，主動飲咖啡飲啤酒，咁我哋又係咪真係成功征服左大陸，識得享受個虎味呢，我哋就揾來幾位同事一齊實試一下

Colloquial 口語 markers throughout (係, 嘅, 咁, 我哋, 左) — the register ADR-0002 requires for
the canonical transcript — and no forced-token repetition-loop artifacts. "虎離食"/"虎味" is
plausibly a mis-hearing (of e.g. 苦味/苦嚟食) rather than what was said; that kind of error is
exactly what #13's side-by-side comparison against the baseline is for, not something this
conversion procedure can or should judge.

**Not converted here, but the same procedure applies:** `JackyHoCL`'s own listed successor,
`JackyHoCL/whisper-large-v3-turbo-cantonese-noise-detection` ("trained base on this model, to
reduce hallucination during streaming"), is a candidate for issue #13's side-by-side
evaluation and needs no new procedure — just re-run Steps 4–9 against that repo.

## Cleanup

The working directory (`openai-whisper/`, `whisper.cpp/`, `model-src/`) is disposable once
`out/*.bin` exists — nothing downstream reads from it again. Keep only the `out/` files,
at whatever path you register them from.
