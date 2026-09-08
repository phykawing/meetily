// ============================================================================
// sherpa-onnx / ONNX Runtime shared-library bundling (Windows)
// ============================================================================
// The `sherpa-onnx` crate is linked with `features = ["shared"]` (see
// docs/adr/0007 and Cargo.toml) to dodge an LNK2005 CRT clash with
// whisper-rs-sys. Shared linking leaves the diarization runtime in four DLLs
// that must sit next to `meetily.exe` at runtime, or the OS loader cannot
// resolve `sherpa-onnx-c-api.dll`'s implicit imports and the app fails to
// launch at all (not just diarization — phykawing/meetily#35):
//
//   sherpa-onnx-c-api.dll, sherpa-onnx-cxx-api.dll,
//   onnxruntime.dll, onnxruntime_providers_shared.dll
//
// `sherpa-onnx-sys`'s build script drops them into `target/<profile>/` (which
// covers `cargo run` / `tauri dev`, where the exe lives there too) and keeps a
// pristine extraction under `target/sherpa-onnx-prebuilt/.../lib/`. This step
// copies them into `src-tauri/runtime-dlls/` — a stable, committed-path
// directory that `tauri.windows.conf.json` lists under `bundle.resources`, each
// mapped to a bare filename so the NSIS/MSI installer places them at the
// installation root beside the executable.
//
// `sherpa-onnx-sys` is a direct dependency of this crate purely so its
// `links = "sherpa-onnx"` gives Cargo an ordering edge that runs its build
// script (the DLL producer) before this one (the consumer).

use std::env;
use std::ffi::OsStr;
use std::fs;
use std::path::{Path, PathBuf};

const RUNTIME_DLLS: [&str; 4] = [
    "sherpa-onnx-c-api.dll",
    "sherpa-onnx-cxx-api.dll",
    "onnxruntime.dll",
    "onnxruntime_providers_shared.dll",
];

/// Copy the sherpa-onnx / ONNX Runtime DLLs into `src-tauri/runtime-dlls/` so
/// the Windows bundler can ship them next to the executable. No-op on every
/// non-Windows target.
pub fn ensure_runtime_dlls() {
    println!("cargo:rerun-if-env-changed=SHERPA_ONNX_LIB_DIR");

    let target_os = env::var("CARGO_CFG_TARGET_OS").unwrap_or_default();
    if target_os != "windows" {
        return;
    }

    let manifest_dir =
        PathBuf::from(env::var("CARGO_MANIFEST_DIR").expect("CARGO_MANIFEST_DIR not set"));
    let dest_dir = manifest_dir.join("runtime-dlls");
    fs::create_dir_all(&dest_dir)
        .unwrap_or_else(|e| panic!("Failed to create {}: {e}", dest_dir.display()));

    let search_dirs = candidate_source_dirs();

    for dll in RUNTIME_DLLS {
        let source = search_dirs
            .iter()
            .map(|dir| dir.join(dll))
            .find(|path| path.is_file())
            .unwrap_or_else(|| {
                panic!(
                    "Diarization runtime DLL `{dll}` not found. Searched: [{}]. It is emitted \
                     by `sherpa-onnx-sys`; run a normal `cargo build` first so its build script \
                     stages `target/sherpa-onnx-prebuilt/`. A Windows installer built without \
                     this DLL produces an app that fails to launch (phykawing/meetily#35).",
                    search_dirs
                        .iter()
                        .map(|d| d.display().to_string())
                        .collect::<Vec<_>>()
                        .join(", ")
                )
            });

        let dest = dest_dir.join(dll);
        if needs_copy(&source, &dest) {
            fs::copy(&source, &dest).unwrap_or_else(|e| {
                panic!("Failed to copy {} -> {}: {e}", source.display(), dest.display())
            });
        }
        println!("cargo:rerun-if-changed={}", source.display());
    }

    println!(
        "cargo:warning=🔊 Staged {} diarization runtime DLLs in runtime-dlls/ for the Windows bundle",
        RUNTIME_DLLS.len()
    );
}

/// Whether `dest` is missing or differs from `source`. A pure size match with
/// no usable mtime on either side counts as "same" — rewriting an unchanged
/// file bumps its mtime and, since the Tauri build script watches
/// `runtime-dlls/` as bundle resources, that would spin an endless rebuild loop.
fn needs_copy(source: &Path, dest: &Path) -> bool {
    let (Ok(src_meta), Ok(dst_meta)) = (fs::metadata(source), fs::metadata(dest)) else {
        return true;
    };
    if src_meta.len() != dst_meta.len() {
        return true;
    }
    match (src_meta.modified(), dst_meta.modified()) {
        (Ok(src_mtime), Ok(dst_mtime)) => src_mtime > dst_mtime,
        _ => false,
    }
}

/// Directories that may hold the prebuilt DLLs, most-authoritative first:
///   0. `$SHERPA_ONNX_LIB_DIR` — an explicit override a builder may point at
///      their own sherpa-onnx libs; when set, `sherpa-onnx-sys` skips the
///      prebuilt download entirely so nothing else below would exist.
///   1. `target/[<triple>/]<profile>/` — `sherpa-onnx-sys` re-copies the DLLs
///      here on every build where its script runs, and this crate's direct
///      dependency on it guarantees that happens before this script. This is
///      what keeps a version bump from serving a stale runtime out of (2).
///   2. `target/<profile>/build/sherpa-onnx-sys-*/out/lib*/` — its own OUT_DIR
///      staging, a fallback if a future release stops copying to the profile
///      dir.
///   3. `target/sherpa-onnx-prebuilt/<archive>/lib/` — the pristine extraction
///      it keeps around, newest first so a leftover older extraction can't win.
fn candidate_source_dirs() -> Vec<PathBuf> {
    let out_dir = PathBuf::from(env::var("OUT_DIR").expect("OUT_DIR not set"));
    let mut dirs = Vec::new();

    if let Some(explicit) = env::var_os("SHERPA_ONNX_LIB_DIR") {
        dirs.push(PathBuf::from(explicit));
    }

    if let Some(profile_dir) = profile_dir(&out_dir) {
        dirs.extend(sys_out_dir_libs(&profile_dir.join("build")));
        dirs.push(profile_dir);
    }

    let prebuilt_root = target_dir(&out_dir).join("sherpa-onnx-prebuilt");
    if let Ok(entries) = fs::read_dir(&prebuilt_root) {
        let mut libs: Vec<(std::time::SystemTime, PathBuf)> = entries
            .flatten()
            .map(|entry| entry.path().join("lib"))
            .filter(|lib| lib.is_dir())
            .map(|lib| {
                let mtime = fs::metadata(&lib)
                    .and_then(|m| m.modified())
                    .unwrap_or(std::time::UNIX_EPOCH);
                (mtime, lib)
            })
            .collect();
        libs.sort_by(|a, b| b.0.cmp(&a.0));
        dirs.extend(libs.into_iter().map(|(_, lib)| lib));
    }

    dirs
}

/// `sherpa-onnx-sys`'s per-build OUT_DIR lib dirs under `<profile>/build/`
/// (`sherpa-onnx-sys-<hash>/out/lib` or `.../out/lib64`), newest first.
fn sys_out_dir_libs(build_root: &Path) -> Vec<PathBuf> {
    let Ok(entries) = fs::read_dir(build_root) else {
        return Vec::new();
    };
    let mut found: Vec<(std::time::SystemTime, PathBuf)> = entries
        .flatten()
        .filter(|entry| {
            entry
                .file_name()
                .to_str()
                .is_some_and(|name| name.starts_with("sherpa-onnx-sys-"))
        })
        .flat_map(|entry| {
            let out = entry.path().join("out");
            [out.join("lib"), out.join("lib64"), out]
        })
        .filter(|dir| dir.is_dir())
        .map(|dir| {
            let mtime = fs::metadata(&dir)
                .and_then(|m| m.modified())
                .unwrap_or(std::time::UNIX_EPOCH);
            (mtime, dir)
        })
        .collect();
    found.sort_by(|a, b| b.0.cmp(&a.0));
    found.into_iter().map(|(_, dir)| dir).collect()
}

/// The Cargo `target` directory, mirroring `sherpa-onnx-sys`'s own resolution:
/// an *absolute* `CARGO_TARGET_DIR` if set, else the nearest `OUT_DIR` ancestor
/// named `target`, else `OUT_DIR` itself. A relative `CARGO_TARGET_DIR` is
/// resolved by Cargo against the workspace root, not this script's CWD, so it
/// is ignored here in favour of the always-absolute ancestor walk.
fn target_dir(out_dir: &Path) -> PathBuf {
    if let Some(explicit) = env::var_os("CARGO_TARGET_DIR") {
        let path = PathBuf::from(explicit);
        if path.is_absolute() {
            return path;
        }
    }
    out_dir
        .ancestors()
        .find(|path| path.file_name() == Some(OsStr::new("target")))
        .map(Path::to_path_buf)
        .unwrap_or_else(|| out_dir.to_path_buf())
}

/// The active profile output directory (`target/[<triple>/]<profile>`), found
/// as the nearest `OUT_DIR` ancestor whose name matches `PROFILE` — the same
/// lookup `sherpa-onnx-sys` uses when it copies the DLLs there.
fn profile_dir(out_dir: &Path) -> Option<PathBuf> {
    let profile = env::var("PROFILE").ok()?;
    out_dir
        .ancestors()
        .find(|path| path.file_name() == Some(OsStr::new(&profile)))
        .map(Path::to_path_buf)
}
