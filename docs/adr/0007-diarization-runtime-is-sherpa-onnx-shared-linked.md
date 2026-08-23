# Diarization inference uses the `sherpa-onnx` crate, shared-linked

Issue #16 needs to actually run the two models ADR-0005 downloads: segmentation, speaker
embedding, and clustering into Speaker Turns. Rather than drive `ort` (already a
dependency, used by `parakeet_engine`) by hand — reimplementing pyannote's powerset
segmentation decoding, sliding-window stitching, kaldi-compatible fbank extraction for the
embedding model, and agglomerative clustering — this uses the official
[`sherpa-onnx`](https://github.com/k2-fsa/sherpa-onnx) Rust crate's
`OfflineSpeakerDiarization` API, which wraps exactly that pipeline in C++ and is built
against the same two model files ADR-0005 already downloads.

## Shared linking, not the crate's default static linking

`sherpa-onnx-sys` downloads prebuilt native libraries at build time (no cmake/C++ toolchain
needed) and defaults to `static` linking. On this project's Windows build that default
fails the link with `LNK2005` duplicate-symbol errors: the prebuilt static libraries use
MSVC's static CRT (`/MT`), while `whisper-rs-sys`'s cmake-built `whisper.cpp` uses the
dynamic CRT (`/MD`) — two different CRT copies cannot both be statically linked into one
binary. Switching to the crate's `shared` feature (`default-features = false, features =
["shared"]` in `frontend/src-tauri/Cargo.toml`) resolves this: sherpa-onnx's CRT usage is
isolated inside its own DLLs (`sherpa-onnx-c-api.dll`, `sherpa-onnx-cxx-api.dll`,
`onnxruntime.dll`, `onnxruntime_providers_shared.dll`), which the build script copies next
to the build output (`target/<profile>/`).

**Consequence — Windows packaging is unfinished.** Those four DLLs must sit next to the
*installed* `meetily.exe` for the OS loader to resolve sherpa-onnx-c-api.dll's implicit
imports; `cargo run`/`tauri dev` gets this for free since the exe and DLLs land in the same
`target/<profile>/` directory, but the NSIS/MSI bundle does not yet copy them there.
`tauri.conf.json`'s `bundle.resources` was tried and reverted: Tauri's build script
validates every resource's *source* path exists at compile time, and `target/release/*.dll`
does not exist during a `debug` build (or vice versa) — the resources array can't easily
reference a profile-dependent path. **Whoever ships the first Windows build with
diarization must solve this** (most likely: a build step that copies the four DLLs into
`src-tauri/binaries/` and adds them to `externalBin`, or a `/DELAYLOAD` + `SetDllDirectory`
approach) **and smoke-test that build** — without it, the packaged app fails to launch at
all, not just diarization.

**Also unresolved: `onnxruntime.dll` name collision with `ort`.** Windows 11 ships its own
system `onnxruntime.dll` (WinML) in `System32`. When sherpa's own copy is missing from the
loading executable's directory (e.g. a `cargo test` binary, which lives in
`target/debug/deps/` while the DLLs land in `target/debug/`), the loader silently falls
back to the stale system copy — sherpa-onnx-c-api.dll was built against a newer ONNX
Runtime API version than that system copy supports, producing a version-mismatch crash.
This does not affect the real Tauri app (exe and DLLs share a directory), but it means
diarization cannot be exercised from a plain `cargo test`/`cargo run` of a test binary
without manually copying the four DLLs into `target/debug/deps/` first.

## Clustering threshold raised from the crate's default

`FastClusteringConfig::default()`'s `threshold: 0.5` badly over-segments: a ~13-minute
two/three-speaker recording (validated by hand, see below) produced 12 spurious speaker
labels. Raising it to `0.75` (`pipeline::CLUSTERING_THRESHOLD`) produced 3, a materially
more plausible count for that same recording. This is not user-configurable — issue #3
explicitly declines an exposed sensitivity setting — and is not unit-tested, since
clustering accuracy is out of scope for automation (issue #3's Testing Decisions); it was
validated by running the real pipeline against a real recording once, per the "judged by
listening" note, not derived analytically.

## Consequences

- `diarization::alignment` (the deterministic seam) is unaffected by any of this — it's
  fully unit tested against synthetic turns and never touches sherpa-onnx.
- `diarization::pipeline::run_sherpa_diarization` is not unit tested, matching issue #3's
  explicit scope: ONNX inference, clustering accuracy, and the audio file are not
  automated. It was validated once by hand against `docs/adr/0005`'s two downloaded models
  and a real recording; that validation is not repeatable in CI.
- A future change to clustering behavior (crate upgrade, threshold tuning) should be
  re-validated the same way — by ear against a real recording — not assumed correct from
  a passing test suite, since none of the tests can catch a clustering regression.
