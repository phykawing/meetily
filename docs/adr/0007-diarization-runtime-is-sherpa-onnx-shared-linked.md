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

**Windows packaging — resolved (phykawing/meetily#35).** Those four DLLs must sit next to
the *installed* `meetily.exe` for the OS loader to resolve sherpa-onnx-c-api.dll's implicit
imports; `cargo run`/`tauri dev` gets this for free since the exe and DLLs land in the same
`target/<profile>/` directory, but the NSIS/MSI bundle did not copy them there.

Pointing `tauri.conf.json`'s `bundle.resources` array straight at `target/<profile>/*.dll`
fails: Tauri validates every resource's *source* path at compile time, and that path is
profile-dependent and absent during the other profile's build. The fix routes through a
stable committed path instead:

- `build/sherpa.rs` (`ensure_runtime_dlls`, called from `build.rs` **before**
  `tauri_build::build()`) copies the four DLLs into `src-tauri/runtime-dlls/` (git-ignored,
  like `binaries/`). It reads them from the pristine `target/sherpa-onnx-prebuilt/.../lib/`
  extraction, falling back to `target/[<triple>/]<profile>/` and `$SHERPA_ONNX_LIB_DIR`;
  it panics if none is found, so a Windows build can't silently produce a broken installer.
- `sherpa-onnx-sys` is now a **direct** dependency of the app crate. Its
  `links = "sherpa-onnx"` gives Cargo the edge that runs its build script (which stages the
  DLLs) before ours; a transitive-only dependency gives no such ordering guarantee.
- `tauri.windows.conf.json` (auto-merged on Windows only) lists each DLL under
  `bundle.resources` in map form, mapped to a bare filename, so the bundler places them at
  the installation root next to the executable. It re-lists `templates/*.json` because a
  JSON-merge-patch object replaces the base array rather than extending it.

The remaining `onnxruntime.dll` name collision below is **not** fixed by this and is
tracked separately (phykawing/meetily#43).

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
