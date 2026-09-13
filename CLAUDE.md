# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## Project Overview

**Meetily** is a privacy-first AI meeting assistant that captures, transcribes, and summarizes meetings entirely on local infrastructure. The supported application is the Tauri desktop app with a Rust core.

1. **Frontend**: Tauri-based desktop application (Rust + Next.js + TypeScript)
2. **Rust Backend**: Tauri commands, audio capture, transcription, storage, and summarization orchestration
3. **`llama-helper`**: standalone Rust sidecar binary (llama-cpp-2) for local LLM summarization
4. **Legacy Backend Archive**: the old Python/FastAPI, Docker, and standalone whisper-server backend under `backend/` is archived and unsupported

### Cargo Workspace Layout

The repo root is a Cargo workspace with two members — run cargo commands from the **repo root**:

| Path | Package | Notes |
|---|---|---|
| `frontend/src-tauri` | `meetily` (lib target `app_lib`) | the Tauri app |
| `llama-helper` | `llama-helper` | sidecar binary, built separately and copied into `src-tauri/binaries/` |

```bash
cargo check -p meetily            # typecheck the Tauri app
cargo test -p meetily             # Rust unit tests (many modules have #[cfg(test)])
cargo test -p meetily audio::vad  # single test module / filter
cargo build -p llama-helper --release --features cuda
```

Note the crate is named `meetily` but the lib is `app_lib` — that's why `RUST_LOG` filters use `app_lib::...`.

### Key Technology Stack
- **Desktop App**: Tauri 2.x (Rust) + Next.js 14 + React 18
- **Audio Processing**: Rust (cpal, whisper-rs, professional audio mixing)
- **Transcription**: Whisper.cpp / whisper-rs and Parakeet paths in the Tauri app
- **Persistence**: SQLite via `sqlx` with compile-time-embedded migrations
- **App API Surface**: Tauri commands and events, not a separate FastAPI service
- **LLM Integration**: Ollama (local), `llama-helper` sidecar, Claude, OpenAI, Groq, OpenRouter

## Essential Development Commands

### Frontend Development (Tauri Desktop App)

**Location**: `/frontend`

```bash
# macOS Development
./clean_run.sh              # Clean build and run with info logging
./clean_run.sh debug        # Run with debug logging
./clean_build.sh            # Production build

# Windows Development
clean_run_windows.bat       # Clean build and run
clean_build_windows.bat     # Production build

# Manual Commands
pnpm install                # Install dependencies
pnpm run dev                # Next.js dev server (port 3118)
pnpm run tauri:dev          # Full Tauri development mode (auto-detects GPU)
pnpm run tauri:build        # Production build (auto-detects GPU)
pnpm run lint               # ESLint via `next lint`

# GPU-Specific Builds (force a feature instead of auto-detecting)
pnpm run tauri:dev:metal    # macOS Metal GPU
pnpm run tauri:dev:cuda     # NVIDIA CUDA
pnpm run tauri:dev:vulkan   # AMD/Intel Vulkan
pnpm run tauri:dev:hipblas  # AMD ROCm
pnpm run tauri:dev:openblas # CPU + OpenBLAS
pnpm run tauri:dev:cpu      # CPU-only (no features)
```

**GPU auto-detection vs. the sidecar** — two different entry points, easy to confuse:

- `pnpm run tauri:dev` / `tauri:build` go through `scripts/tauri-auto.js`, which runs
  `scripts/auto-detect-gpu.js` and appends `-- --features <feature>`. This builds **only the app**,
  not the `llama-helper` sidecar.
- `./dev-gpu.sh` / `./build-gpu.sh` (`.bat`/`.ps1` on Windows) do the full job: detect GPU → build
  `llama-helper` with that feature → copy the binary into `src-tauri/binaries/` with the target
  triple suffix → then run `tauri:dev`/`tauri:build`. **Use these if you touched `llama-helper` or
  need local LLM summarization to work.**
- `TAURI_GPU_FEATURE=cuda` overrides detection for both paths.

Detection priority: CUDA → HIPBlas (ROCm) → Vulkan → OpenBLAS → CPU. Having GPU *drivers* is not
enough; detection requires the development SDK (see [docs/BUILDING.md](docs/BUILDING.md)).

### Tests

There is **no `pnpm test` script and CI does not run tests** — invoke them directly from `/frontend`.
Note the files use two different runners:

```bash
# bun:test files — run these individually; `bun test tests/lib` would also sweep up
# the .test.mjs file below, which is not a bun test
bun test tests/lib/blocknote-markdown.test.ts
bun test tests/lib/summary-language-preferences.test.js

# plain node script (compiles the TS module in-process via typescript + vm)
node tests/lib/onboarding-summary-model.test.mjs

# Rust
cargo test -p meetily                           # from repo root
```

### Legacy Backend Archive

**Location**: `/backend`

The Python/FastAPI backend, Docker setup, and standalone whisper-server scripts are archived for historical reference and migration context only. Do not use them for current development, new installs, production deployments, or issue triage for the supported app.

The archived FastAPI service had unauthenticated, development-oriented CORS behavior. Treat that behavior as obsolete legacy context, not as a supported production API.

### Service Endpoints
- **Frontend Dev**: http://localhost:3118

### Further Reading
- [docs/BUILDING.md](docs/BUILDING.md) - per-OS build prerequisites and GPU SDK setup
- [docs/GPU_ACCELERATION.md](docs/GPU_ACCELERATION.md) - acceleration feature matrix
- [docs/architecture.md](docs/architecture.md) - system architecture overview
- [.github/workflows/WORKFLOWS_OVERVIEW.md](.github/workflows/WORKFLOWS_OVERVIEW.md) - CI/release pipelines
  (all build workflows are `workflow_dispatch`-triggered; there is no automatic PR test job)

## High-Level Architecture

### Tauri Desktop Architecture

```
┌─────────────────────────────────────────────────────────────────┐
│                    Frontend (Tauri Desktop App)                  │
│  ┌──────────────────┐  ┌─────────────────┐  ┌────────────────┐ │
│  │   Next.js UI     │  │  Rust Backend   │  │ Whisper Engine │ │
│  │  (React/TS)      │←→│  (Audio + IPC)  │←→│  (Local STT)   │ │
│  └──────────────────┘  └─────────────────┘  └────────────────┘ │
│         ↑ Tauri Events           ↑ Audio Pipeline               │
└─────────────────────────────────────────────────────────────────┘
```

The current app does not require a separate FastAPI tier. Meeting persistence, local transcription, and summary orchestration are handled through the Rust/Tauri core.

### Audio Processing Pipeline (Critical Understanding)

The audio system has **two parallel paths** with different purposes:

```
Raw Audio (Mic + System)
         ↓
┌────────────────────────────────────────────────────────────┐
│              Audio Pipeline Manager                         │
│  (frontend/src-tauri/src/audio/pipeline.rs)                │
└─────────────┬──────────────────────────┬───────────────────┘
              ↓                          ↓
    ┌─────────────────┐        ┌─────────────────────┐
    │ Recording Path  │        │ Transcription Path  │
    │ (Pre-mixed)     │        │ (VAD-filtered)      │
    └─────────────────┘        └─────────────────────┘
              ↓                          ↓
    RecordingSaver.save()      WhisperEngine.transcribe()
```

**Key Insight**: The pipeline performs **professional audio mixing** (RMS-based ducking, clipping prevention) for recording, while simultaneously applying **Voice Activity Detection (VAD)** to send only speech segments to Whisper for transcription.

### Rust Module Map (`frontend/src-tauri/src/`)

`lib.rs` (~790 lines) declares every module and registers all Tauri commands in one
`generate_handler!`. Each subsystem follows the same shape: `commands.rs` (Tauri surface) +
implementation modules.

| Module | Responsibility |
|---|---|
| `audio/` | Capture, device management, mixing/VAD pipeline, recording, transcription dispatch |
| `whisper_engine/` | whisper-rs model loading, acceleration detection, parallel batch transcription |
| `parakeet_engine/` | Alternate Parakeet STT backend |
| `diarization/` | Speaker diarization: consent-gated model download (`consent.rs`, `manager.rs`), sherpa-onnx inference and clustering (`pipeline.rs`, docs/adr/0007), turn-to-chunk alignment (`alignment.rs`) |
| `rendering/` | Written Form Rendering of transcripts via an LLM provider (docs/adr/0002); `commands.rs` includes the fingerprint-based cache/regenerate decision |
| `script.rs` | Chinese script conversion (Simplified/Traditional-HK) applied to transcripts at ingest (docs/adr/0003) |
| `summary/` | Summarization orchestration: `llm_client.rs`, `processor.rs`, `templates/`, `summary_engine/` (incl. `sidecar.rs` → `llama-helper`) |
| `ollama/`, `anthropic/`, `openai/`, `groq/`, `openrouter/` | Per-provider LLM clients |
| `database/` | sqlx/SQLite: `manager.rs` (pool + migrations), `repositories/` (meeting, transcript, transcript_chunk, summary, setting) |
| `notifications/`, `tray.rs` | Native notifications and system tray |
| `analytics/`, `console_utils/`, `onboarding.rs`, `api/` | Telemetry, in-app log console, first-run flow, misc commands |

**Dead code — do not treat as current architecture**: `audio_v2/` is not declared in `lib.rs` (no
`pub mod audio_v2`), and `lib_old_complex.rs`, `audio/core-old.rs`, `audio/recording_saver_old.rs`,
`audio/recording_commands.rs.backup` are unreferenced leftovers. Verify a module is in the `lib.rs`
mod list before editing it.

### Audio Module Routing

`audio/` is large (~40 files plus `devices/`, `capture/`, `transcription/` subdirs). Route by symptom
rather than browsing the tree:

| Symptom | Where to look |
|---|---|
| Device enumeration / missing devices | `devices/discovery.rs`, `devices/platform/{windows,macos,linux}.rs` |
| Default mic or speaker selection | `devices/microphone.rs`, `devices/speakers.rs`, `devices/fallback.rs` |
| Capture stream errors | `capture/microphone.rs`, `capture/system.rs`, `capture/core_audio.rs` (macOS) |
| Device unplugged mid-recording | `device_monitor.rs`, `device_detection.rs` |
| Mixing, ducking, VAD | `pipeline.rs`, `vad.rs`, `ffmpeg_mixer.rs` |
| Recording lifecycle / state | `recording_manager.rs`, `recording_state.rs`, `recording_commands.rs` |
| File writing, crash-safety | `recording_saver.rs`, `incremental_saver.rs`, `encode.rs` |
| Transcription backend selection | `transcription/engine.rs`, `transcription/{whisper,parakeet}_provider.rs`, `transcription/worker.rs` |
| Importing / re-transcribing existing audio | `import.rs`, `decoder.rs`, `retranscription.rs` |
| Bluetooth / playback-quality complaints | `playback_monitor.rs` (see [BLUETOOTH_PLAYBACK_NOTICE.md](BLUETOOTH_PLAYBACK_NOTICE.md)) |

### Database and Migrations

`database/manager.rs` runs `sqlx::migrate!("./migrations")` at startup, so migrations are **embedded
at compile time** — adding a `.sql` file requires a rebuild, not just an app restart. Files are named
`YYYYMMDDHHMMSS_description.sql` and are append-only; never edit an applied migration. The database
lives at `<app_data_dir>/meeting_minutes.db`, and `database/commands.rs` contains legacy-path
migration logic for DBs left over from the archived Python backend.

### Rust ↔ Frontend Communication (Tauri Architecture)

**Command Pattern** (Frontend → Rust):
```typescript
// Frontend: src/app/page.tsx
await invoke('start_recording', {
  mic_device_name: "Built-in Microphone",
  system_device_name: "BlackHole 2ch",
  meeting_name: "Team Standup"
});
```

```rust
// Rust: src/lib.rs
#[tauri::command]
async fn start_recording<R: Runtime>(
    app: AppHandle<R>,
    mic_device_name: Option<String>,
    system_device_name: Option<String>,
    meeting_name: Option<String>
) -> Result<(), String> {
    // Implementation delegates to audio::recording_commands
}
```

**Event Pattern** (Rust → Frontend):
```rust
// Rust: Emit transcript updates
app.emit("transcript-update", TranscriptUpdate {
    text: "Hello world".to_string(),
    timestamp: chrono::Utc::now(),
    // ...
})?;
```

```typescript
// Frontend: Listen for events
await listen<TranscriptUpdate>('transcript-update', (event) => {
  setTranscripts(prev => [...prev, event.payload]);
});
```

### Whisper Model Management

**Model Storage Locations** — resolved in `whisper_engine.rs`, which falls back through several
candidates rather than using one fixed path:
- **Production**: `<app_data_dir>/models/`, where `app_data_dir` derives from the bundle identifier
  `com.meetily.ai` (`~/Library/Application Support/com.meetily.ai/models` on macOS,
  `%APPDATA%\com.meetily.ai\models` on Windows)
- **Development**: `./models/` or `../models/` relative to the working directory, checked first

**Model Loading** (frontend/src-tauri/src/whisper_engine/whisper_engine.rs):
```rust
pub async fn load_model(&self, model_name: &str) -> Result<()> {
    // Automatically detects GPU capabilities (Metal/CUDA/Vulkan)
    // Falls back to CPU if GPU unavailable
}
```

**GPU Acceleration**:
- **macOS**: Metal + CoreML (automatically enabled)
- **Windows/Linux**: CUDA (NVIDIA), Vulkan (AMD/Intel), or CPU
- Configure via Cargo features: `--features cuda`, `--features vulkan`

## Critical Development Patterns

### 1. Audio Buffer Management

**Ring Buffer Mixing** (pipeline.rs):
- Mic and system audio arrive asynchronously at different rates
- Ring buffer accumulates samples until both streams have aligned windows (50ms)
- Professional mixing applies RMS-based ducking to prevent system audio from drowning out microphone
- Uses `VecDeque` for efficient windowed processing

### 2. Thread Safety and Async Boundaries

**Recording State** (recording_state.rs):
```rust
pub struct RecordingState {
    is_recording: Arc<AtomicBool>,
    audio_sender: Arc<RwLock<Option<mpsc::UnboundedSender<AudioChunk>>>>,
    // ...
}
```

**Key Pattern**: Use `Arc<RwLock<T>>` for shared state across async tasks, `Arc<AtomicBool>` for simple flags.

### 3. Error Handling and Logging

**Performance-Aware Logging** (lib.rs):
```rust
#[cfg(debug_assertions)]
macro_rules! perf_debug {
    ($($arg:tt)*) => { log::debug!($($arg)*) };
}

#[cfg(not(debug_assertions))]
macro_rules! perf_debug {
    ($($arg:tt)*) => {};  // Zero overhead in release builds
}
```

**Usage**: Use `perf_debug!()` and `perf_trace!()` for hot-path logging that should be eliminated in production.

### 4. Frontend State Management

**Sidebar Context** (components/Sidebar/SidebarProvider.tsx):
- Global state for meetings list, current meeting, recording status
- Communicates with the Rust/Tauri core through Tauri commands and events
- Keeps React state synchronized with native recording, meeting, transcript, and summary state

**Pattern**: Tauri commands update Rust state → Emit events → Frontend listeners update React state → Context propagates to components

## Common Development Tasks

### Adding a New Audio Device Platform

1. Create platform file: `audio/devices/platform/{platform_name}.rs`
2. Implement device enumeration for the platform
3. Add platform-specific configuration in `audio/devices/configuration.rs`
4. Update `audio/devices/platform/mod.rs` to export new platform functions
5. Test with `cargo check` and platform-specific device tests

### Adding a New Tauri Command

1. Define command in `src/lib.rs`:
   ```rust
   #[tauri::command]
   async fn my_command(arg: String) -> Result<String, String> { /* ... */ }
   ```
2. Register in `tauri::Builder`:
   ```rust
   .invoke_handler(tauri::generate_handler![
       start_recording,
       my_command,  // Add here
   ])
   ```
3. Call from frontend:
   ```typescript
   const result = await invoke<string>('my_command', { arg: 'value' });
   ```

### Modifying Audio Pipeline Behavior

**Location**: `frontend/src-tauri/src/audio/pipeline.rs`

Key components:
- `AudioMixerRingBuffer`: Manages mic + system audio synchronization
- `ProfessionalAudioMixer`: RMS-based ducking and mixing
- `AudioPipelineManager`: Orchestrates VAD, mixing, and distribution

**Testing Audio Changes**:
```bash
# Enable verbose audio logging
RUST_LOG=app_lib::audio=debug ./clean_run.sh

# Monitor audio metrics in real-time
# Check Developer Console in the app (Cmd+Shift+I on macOS)
```

### Tauri Backend Development

Current app behavior should be implemented in the Rust/Tauri core, not in the archived Python backend. Add new frontend-facing behavior through Tauri commands/events and existing Rust services under `frontend/src-tauri/src`.

Do not add new endpoints to `backend/app/main.py`; that FastAPI code is legacy archive material only.

## Testing and Debugging

### Frontend Debugging

**Enable Rust Logging**:
```bash
# macOS
RUST_LOG=debug ./clean_run.sh

# Windows (PowerShell)
$env:RUST_LOG="debug"; ./clean_run_windows.bat
```

**Developer Tools**:
- Open DevTools: `Cmd+Shift+I` (macOS) or `Ctrl+Shift+I` (Windows)
- Console Toggle: Built into app UI (console icon)
- View Rust logs: Check terminal output

### Audio Pipeline Debugging

**Key Metrics** (emitted by pipeline):
- Buffer sizes (mic/system)
- Mixing window count
- VAD detection rate
- Dropped chunk warnings

**Monitor via Developer Console**: The app includes real-time metrics display when recording.

## Platform-Specific Notes

### macOS
- **Audio Capture**: Uses ScreenCaptureKit for system audio (macOS 13+)
- **GPU**: Metal + CoreML automatically enabled
- **Permissions**: Requires microphone + screen recording permissions
- **System Audio**: Requires virtual audio device (BlackHole) for system capture

### Windows
- **Audio Capture**: Uses WASAPI (Windows Audio Session API)
- **GPU**: CUDA (NVIDIA) or Vulkan (AMD/Intel) via Cargo features
- **Build Tools**: Requires Visual Studio Build Tools with C++ workload
- **System Audio**: Uses WASAPI loopback for system capture

### Linux
- **Audio Capture**: ALSA/PulseAudio
- **GPU**: CUDA (NVIDIA) or Vulkan via Cargo features
- **Dependencies**: Requires cmake, llvm, libomp

## Performance Optimization Guidelines

### Audio Processing
- Use `perf_debug!()` / `perf_trace!()` for hot-path logging (zero cost in release)
- Batch audio metrics using `AudioMetricsBatcher` (pipeline.rs)
- Pre-allocate buffers with `AudioBufferPool` (buffer_pool.rs)
- VAD filtering reduces Whisper load by ~70% (only processes speech)

### Whisper Transcription
- **Model Selection**: Balance accuracy vs speed
  - Development: `base` or `small` (fast iteration)
  - Production: `medium` or `large-v3` (best quality)
- **GPU Acceleration**: 5-10x faster than CPU
- **Parallel Processing**: Available in `whisper_engine/parallel_processor.rs` for batch workloads

### Frontend Performance
- React state updates batched via Sidebar context
- Transcript rendering virtualized for large meetings
- Audio level monitoring throttled to 60fps

## Important Constraints and Gotchas

1. **Audio Chunk Size**: Pipeline expects consistent 48kHz sample rate. Resampling happens at capture time.

2. **Platform Audio Quirks**:
   - macOS: ScreenCaptureKit requires macOS 13+, needs screen recording permission
   - Windows: WASAPI exclusive mode can conflict with other apps
   - System audio requires virtual device (BlackHole on macOS, WASAPI loopback on Windows)

3. **Whisper Model Loading**: Models are loaded once and cached. Changing models requires app restart or manual unload/reload.

4. **No Separate Backend Dependency**: Meeting persistence, transcription, and LLM features are handled by the Tauri app. Do not reintroduce the archived FastAPI backend as a supported requirement.

5. **Legacy FastAPI Security Context**: The archived FastAPI/CORS behavior is unsupported legacy code and must not be treated as a supported production API.

6. **File Paths**: Use Tauri's path APIs (`downloadDir`, etc.) for cross-platform compatibility. Never hardcode paths.

7. **Audio Permissions**: Request permissions early. macOS requires both microphone AND screen recording for system audio.

## Repository-Specific Conventions

- **Logging Format**: Rust logs should include enough module context to diagnose app behavior
- **Error Handling**: Rust uses `anyhow::Result`, frontend uses try-catch with user-friendly messages
- **Naming**: Audio devices use "microphone" and "system" consistently (not "input"/"output")
- **Git Branches** (per [CONTRIBUTING.md](CONTRIBUTING.md)):
  - `main`: production / stable releases
  - `devtest`: development and testing branch
  - Feature branches are cut **from `devtest`**, and PRs target `devtest` — not `main`
- **Commits**: Conventional Commits — `<type>(<scope>): <subject>` with types
  `feat|fix|docs|style|refactor|test|chore`
- **Version bumps**: the release version is read from `frontend/src-tauri/tauri.conf.json`; keep
  `frontend/package.json` and `frontend/src-tauri/Cargo.toml` in sync with it

## Key Files Reference

**Core Coordination**:
- [frontend/src-tauri/src/lib.rs](frontend/src-tauri/src/lib.rs) - Main Tauri entry point, module list, command registration
- [frontend/src-tauri/src/audio/mod.rs](frontend/src-tauri/src/audio/mod.rs) - Audio module exports
- [frontend/src-tauri/src/database/manager.rs](frontend/src-tauri/src/database/manager.rs) - SQLite pool, DB path, migration runner
- [frontend/src-tauri/tauri.conf.json](frontend/src-tauri/tauri.conf.json) - App version, bundle config, permissions

**Summarization / LLM**:
- [frontend/src-tauri/src/summary/service.rs](frontend/src-tauri/src/summary/service.rs) - Summary orchestration
- [frontend/src-tauri/src/summary/summary_engine/sidecar.rs](frontend/src-tauri/src/summary/summary_engine/sidecar.rs) - `llama-helper` sidecar bridge
- [llama-helper/src/main.rs](llama-helper/src/main.rs) - Local LLM sidecar binary

**Build Tooling**:
- [frontend/scripts/tauri-auto.js](frontend/scripts/tauri-auto.js) - GPU auto-detection wrapper for `tauri:dev`/`tauri:build`
- [frontend/scripts/auto-detect-gpu.js](frontend/scripts/auto-detect-gpu.js) - Hardware/SDK probing logic

**Audio System**:
- [frontend/src-tauri/src/audio/recording_manager.rs](frontend/src-tauri/src/audio/recording_manager.rs) - Recording orchestration
- [frontend/src-tauri/src/audio/pipeline.rs](frontend/src-tauri/src/audio/pipeline.rs) - Audio mixing and VAD
- [frontend/src-tauri/src/audio/recording_saver.rs](frontend/src-tauri/src/audio/recording_saver.rs) - Audio file writing

**UI Components**:
- [frontend/src/app/page.tsx](frontend/src/app/page.tsx) - Main recording interface
- [frontend/src/components/Sidebar/SidebarProvider.tsx](frontend/src/components/Sidebar/SidebarProvider.tsx) - Global state management

**Whisper Integration**:
- [frontend/src-tauri/src/whisper_engine/whisper_engine.rs](frontend/src-tauri/src/whisper_engine/whisper_engine.rs) - Whisper model management and transcription

## Agent skills

### Issue tracker

Issues live as GitHub issues on `phykawing/meetily` (`origin`; `upstream` is `Zackriya-Solutions/meetily`), managed via the `gh` CLI. See `docs/agents/issue-tracker.md`.

### Triage labels

The five canonical roles use their default label strings (`needs-triage`, `needs-info`, `ready-for-agent`, `ready-for-human`, `wontfix`). See `docs/agents/triage-labels.md`.

### Domain docs

Single-context: `CONTEXT.md` at the repo root and ADRs under `docs/adr/`, both created lazily. See `docs/agents/domain.md`.
