# CrownOS Voice Recorder + Voice Enhancement — Plan

Status: in progress, 2026-11-04. Runs in parallel with **crownos-camera**, which is adding video to crownui.
The "Shared seams" section below must be agreed with that team before either side edits crownui.

## 0. Decisions agreed with crownos-camera (2026-11-04)

- **The installer is owned by camera.** That covers `util/pci.rs`, `engine/ai.rs`, `Step::ConfigureAiAcceleration`,
  `oobe::AiFeatures` and the Features page. Voice does not edit crownos-installer.
- **ONNX Runtime packages are owned by camera.** One CPU core, `onnxruntime-crownos`, plus additive shared
  providers `onnxruntime-crownos-{cuda,migraphx,openvino,vitisai}`. This avoids Arch's conflicting variants
  on hybrid laptops. Voice therefore ships **one** `onnx` backend that uses whichever providers are installed;
  there is no separate OpenVINO plugin.
- **The `crownos-ai-*` metas are owned by camera.** Offline: `crownos-ai-cpu` (adds `crownos-voice`, `crownos-voice-onnx`)
  and `crownos-ai-nvidia`. Online: `crownos-ai-nvidia-cuda` (adds `onnxruntime-crownos-cuda`, `cuda`, `cudnn`),
  `crownos-ai-amd` and `crownos-ai-intel`.
- **`/etc/crownos/ai.ron` is written by the installer.** Its reader schema lives in `crownos-config/src/system/ai.rs`
  (camera). A missing file means AI is off. crown-voiced reads `backends` as its preference order.
- **crownui:** voice adds `audio` (foundations) and owns `ClockNanos(pub u64)`, the PipeWire graph clock. Camera
  adds `media` (contracts, depends on `audio`), `camera` (implementations) and `camera_preview`. Error codes:
  CUI07xx audio, CUI08xx video. Camera consumes `crownui::audio::Microphone` and drops its own mic code.
- **Voice works in the `/e/CrownOS/crownui-audio` worktree, branch `audio` from `files-app` 2d397ae.** The main
  crownui checkout has another session's uncommitted renderer work and is not touched.

Where sections below disagree with this list, this list wins.

## 1. Goals

1. A recorder app (`crownos-sound-recorder`) built with crownui. It records, plays back, keeps a library
   of recordings and can enhance them with AI.
2. Real-time AI voice enhancement: noise suppression, dereverb, gain. It is accelerated with each
   vendor's own APIs (NVIDIA, AMD, Intel) and falls back to the CPU.
3. A crownui module (`audio`) so that any CrownOS app opening the mic gets the same enhancements with a
   single option.
4. Enhancement backends load only on demand. Nothing is resident until a stream asks for enhancement,
   and everything unloads when the last stream closes.
5. When **AI features** is checked in the installer, it detects the hardware and installs the matching
   backend packages. No user choice is needed beyond the checkbox.

Non-goals for v1: transcription (crowndictator already does it), multitrack editing, a mic-permission
portal (deferred; it is a shared item with camera, see §8).

## 2. What exists today (reuse, don't rewrite)

| Need | Existing code |
|---|---|
| Lazy ONNX session, EP probe, idle unload | `crowndictator/src/asr/mod.rs` (`session_for` L49, `cuda_usable` L66, engine thread `spawn` L303) |
| PipeWire capture into an `rtrb` ring | `crownos-screencapture/src/record/audio/capture.rs` |
| Opus/Ogg encoder + muxer | `crownos-screencapture/src/record/audio/encoder.rs`, `record/mux.rs`, `record_voice` (record/mod.rs:204) |
| Virtual `Audio/Source` node (10 ms) | `crownconnect-linux/src/media/audio/pw_source.rs` (`VirtualMicrophone`) |
| Capture from a chosen node (`TARGET_OBJECT`) | `crownconnect-linux/src/media/audio/pw_sink.rs` |
| PipeWire thread, SPSC PCM ring | `crownconnect-linux/src/media/audio/{pw_thread,pcm_ring}.rs` |
| Source listing with hotplug | `crownconnect-linux/src/state/publishers/volume/graph.rs` (switch the filter to `Audio/Source`) |
| Waveform painting and spring smoothing | `crowndictator/src/waveform/{animation,paint}.rs`, `state.rs::VizState` |
| Streaming worker → signal bridge | `crownui/widgets/src/kit/theme/follow.rs::follow_config` (re-arming `cx.spawn`) |
| Level meter template | `crownui/widgets/src/kit/progress/` |
| Local IPC with fd passing | `crownos-ipc` (`protocol!`, `SO_PEERCRED`, `SCM_RIGHTS`) |
| Hardware-detection pattern | `crownos-installer/src/util/cpuinfo.rs` + `engine/probe.rs:437` (commit 63873b1) |

There is no noise-suppression code anywhere in the tree today.

## 3. Architecture

```
 ┌───────────────────────┐  ┌──────────────┐  ┌──────────────┐
 │ crownos-sound-recorder│  │crownos-camera│  │ any crownui  │   apps
 └──────────┬────────────┘  └──────┬───────┘  └──────┬───────┘
            └───── crownui::audio  (client: open mic, pick enhancement, levels, timestamped frames)
                         │ crownos-ipc: voice.sock (control only)
                         ▼
               crown-voiced  (user service, socket-activated: 0 MB until first use)
               ├─ creates one PipeWire filter node per (source, profile) on demand
               ├─ RT callback ⇄ rtrb ⇄ inference worker  (deadline fallback to passthrough/CPU)
               └─ dlopen backend plugin on first stream, dlclose after idle TTL
                         │
     /usr/lib/crownos/voice/backends/
       libvoice_cpu.so      (always installed: RNNoise + DeepFilterNet3 on ORT-CPU)
       libvoice_onnx.so     (ORT load-dynamic: CUDA/TensorRT EP on NVIDIA, MIGraphX/ROCm EP on AMD)
       libvoice_openvino.so (OpenVINO runtime-linking: Intel NPU / iGPU / Arc)
       libvoice_maxine.so   (optional, NVIDIA Maxine AFX, user accepts the EULA)
```

### Why a daemon instead of running in-process
- **Memory.** A CUDA or ROCm context costs hundreds of MB of RAM and VRAM *per process*. With the
  daemon, three apps using the mic share one context and one model.
- **On-demand loading is natural.** Socket activation means nothing runs until a client connects. The
  last client disconnecting starts the idle-unload timer.
- **Non-crownui apps benefit too.** The node is an ordinary PipeWire source, so browsers and chat apps
  can use "CrownOS Mic (Enhanced)", and Settings can make it the default source.
- **Cost:** about one extra 10 ms PipeWire quantum of latency, which is acceptable for voice.

### Real-time safety
The PipeWire `process` callback never runs inference. It pushes 10 ms frames (480 samples at 48 kHz)
into an `rtrb` ring and pops processed frames from a second ring. An inference worker thread drains the
first ring. If a processed frame is not ready by the deadline (2 quanta), the callback emits the dry
frame and counts the miss. Repeated misses demote the stream to the CPU backend.

### Plugin ABI (on-demand modules)
The Rust dylib ABI is unstable, so plugins export a single `#[repr(C)]` vtable symbol:

```rust
#[repr(C)]
pub struct VoiceBackendV1 {
    pub abi_version: u32,
    pub name: *const c_char,
    pub probe: extern "C" fn(hardware: *const HardwareView) -> ProbeScore,
    pub create: extern "C" fn(config: *const StreamConfig, out: *mut *mut BackendInstance) -> Status,
    pub process: extern "C" fn(instance: *mut BackendInstance, input: *const f32, output: *mut f32, frames: usize) -> Status,
    pub destroy: extern "C" fn(instance: *mut BackendInstance),
}
```

- Every entry point wraps `catch_unwind`.
- The loader (`libloading`) keeps a `Library` only while an instance is alive.
- The vendor runtimes are themselves loaded lazily: `ort` with `load-dynamic`, `openvino` with
  `runtime-linking`. Installing a backend therefore costs nothing until it is used.

## 4. Hardware → backend matrix

| Detected (PCI vendor / accel class) | Real-time profile | Offline "Enhance" | Packages (`crownos-ai-<vendor>` meta) |
|---|---|---|---|
| NVIDIA RTX, Turing+ (0x10de) | CUDA EP (TensorRT EP when present); Maxine if installed | CUDA/TensorRT | `nvidia-open`/`nvidia-utils`, `onnxruntime-opt-cuda`, `cuda`, `cudnn` |
| AMD RDNA2+ in ROCm's support list (0x1002) | CPU (lower latency for one stream) | MIGraphX/ROCm EP | `onnxruntime-rocm` |
| AMD XDNA NPU (`amdxdna` in `/sys/class/accel`) | experimental, off by default | — | not packaged (VitisAI on Linux is immature) |
| Intel NPU, Meteor Lake+ (`intel_vpu`) | OpenVINO NPU (lowest power) | OpenVINO GPU | `openvino`, `intel-npu-driver` |
| Intel iGPU / Arc (0x8086, class 0x03) | OpenVINO GPU | OpenVINO GPU | `openvino`, `intel-compute-runtime` |
| Anything / none | CPU: DeepFilterNet3 (ORT-CPU), RNNoise for "light" | CPU | `onnxruntime-cpu` (on the ISO) |

Facts that shaped this table (verify again at implementation time):
- **AMD.** AMD's own Noise Suppression is Windows-only, so the vendor API on Linux is ROCm/MIGraphX.
- **NVIDIA.** The Maxine Linux AFX SDK targets servers, downloads are NGC-gated, and it cannot be
  redistributed on the ISO. It is therefore an optional plugin, and the CUDA EP is the default NVIDIA
  path.
- **CPU.** DeepFilterNet3 runs at about 0.19 RTF on a single laptop core, so the CPU is always a viable
  real-time fallback. The accelerators matter most for power (NPU) and for heavier offline chains
  (dGPU).

### Backend selection at runtime
For each workload (`Realtime`, `Offline`), the daemon ranks the installed plugins by `probe` score. On
first use it runs a warmup inference, the same way `cuda_usable` does, and caches the outcome in
`~/.cache/crownos/voice/backends.ron`. The user can override the choice in Settings
(`Auto | Cpu | Nvidia | Amd | Intel`).

### Enhancement chain
- **Real-time:** high-pass → noise suppression (backend) → AGC/limiter. Echo cancellation via
  `webrtc-audio-processing` is a `Call` profile, used later by apps that play audio.
- **Offline (recorder):** denoise → dereverb → EBU R128 loudness normalise (−16 LUFS). Runs faster
  than real time on a dGPU or NPU. Writes a new file and keeps the original.

### Profiles exposed to apps
```rust
pub enum Enhancement { Off, Light /* RNNoise */, NoiseSuppression /* DFN3 */, StudioVoice /* + dereverb + AGC */, Call /* + AEC */ }
```

## 5. Repositories and crates

### 5.1 `crownos-accel` (new sibling crate, **shared with camera**)
A pure library with no UI. Both the installer and the daemons use it.
- `probe/pci.rs`: scans `/sys/bus/pci/devices/*/{vendor,device,class}`. `probe/accel.rs`: scans
  `/sys/class/accel/*/device/driver`.
- `HardwareInventory { devices: SmallVec<[AccelDevice; 4]> }` with
  `AccelDevice { vendor: Vendor, kind: DeviceKind, api: AccelApi, pci_id }`.
- `packages::for_inventory(&inventory) -> PackagePlan { offline: Vec<&'static str>, online: Vec<&'static str> }`.
- Pure parsing over an injected filesystem root, so it can be unit-tested against fixture sysfs trees.
  Errors use `thiserror`.
- Camera reuses the same inventory for NVENC/VA-API encode and for vision effects such as background
  blur.

### 5.2 `crownos-voice` (new sibling repo, workspace)
```
crates/voice-abi/        #[repr(C)] vtable + safe wrappers (thiserror)
crates/voice-engine/     framing, resampler (rubato), ring plumbing, chain, offline runner, loudness
crates/voice-protocol/   crownos-ipc protocol! { OpenStream, CloseStream, Enhance(file) … }
backends/cpu/            cdylib — nnnoiseless (RNNoise) + DFN3 via ort CPU
backends/onnx/           cdylib — ort load-dynamic, EP picked from what libonnxruntime exposes
backends/openvino/       cdylib — openvino runtime-linking, NPU > GPU > CPU device order
backends/maxine/         cdylib — optional, behind a feature, built only when the SDK is present
voiced/                  bin (anyhow): socket activation, PipeWire filter nodes, idle unload
packaging/               crown-voiced.socket / .service (systemd --user), PKGBUILD split packages
models/                  DeepFilterNet3 ONNX (streaming export) + licence, shipped in crownos-voice-models
```
Split packages: `crownos-voice` (daemon + cpu backend + models), `crownos-voice-onnx`,
`crownos-voice-openvino`, `crownos-voice-maxine`. The `crownos-ai-<vendor>` meta packages depend on the
matching backend.

### 5.3 crownui: `audio` crate + kit widgets
Branch from `main`, not the dirty `files-app` tree. Do not touch `runtime/`, `impeller/`,
`view/src/cx.rs` or `windowing/src/surface.rs`, which have uncommitted path-renderer work.

- **`crownui/audio/`**, layer `foundations` with `[allow] audio = ["crownconfig"]`.
  - No `Cx` dependency. It owns a PipeWire thread and talks over channels only, which keeps the
    no-Mutex invariant.
  - `Microphone::open(MicrophoneOptions) -> Result<MicrophoneStream, AudioError>`, built with
    `MicrophoneOptions::default().source(SourceId).enhancement(Enhancement::NoiseSuppression)`.
  - `MicrophoneStream { frames: FrameConsumer, level: watch::Receiver<Level>, state: watch::Receiver<StreamState> }`.
  - `AudioFrame { samples: &[f32], pts: ClockNanos }`, where `pts` is the PipeWire graph clock. This is
    the same clock camera's PipeWire video uses, which keeps A/V in sync.
  - `sources() -> watch::Receiver<Vec<SourceInfo>>` with hotplug, keyed by PipeWire node name.
  - When `enhancement != Off`, it asks `crown-voiced` for a node and connects to it with
    `TARGET_OBJECT`. If the daemon is absent, it falls back to the raw source and reports
    `StreamState::Unenhanced(Reason)`.
  - Errors are `CUI07xx` in `docs/errors.md`.
- **`widgets/src/kit/microphone/`**: `use_microphone(cx, options) -> MicrophoneSignals { level: Signal<Level>, state: Signal<StreamState> }`.
  It bridges the `watch` receivers with the re-arming `cx.spawn` loop from `follow_config`.
- **`widgets/src/kit/level_meter/`** (modelled on `progress`) and **`widgets/src/kit/waveform/`**.
  - The waveform lifts the bar, spring and paint logic from crowndictator.
  - It has a live mode, fed by the level ring, and a static mode for a recording's precomputed peaks.
  - Each widget gets `kind.rs`, `metrics.rs`, `options.rs`, a preview and a `widgets/tests/kit_<name>.rs`.
- **Facade:** feature `audio = ["dep:audio", "widgets/audio"]`, so apps without a mic link no PipeWire.
  - Bundle: `gated("audio", "audio")` in `bundled_crate.rs`, with pipewire/rtrb/crownos-ipc added to
    `xtask/bundle/manifest.toml`.
  - API snapshot: add `audio` to `PUBLIC_API_CRATES`.
  - Docs: `audio/README.md` and rows in `docs/naming.md`.

### 5.4 `crownos-sound-recorder` (this repo, single binary crate, anyhow)
```
src/main.rs                    run(AppConfig) only
src/app.rs                     root view, window
src/ui/{mod,library,recorder,transport,enhance_menu,input_picker}.rs
src/recording/{mod,session,library,peaks,playback,enhance}.rs
src/encode/{mod,opus,flac,container}.rs   (from screencapture; see shared seam S6)
src/settings.rs                crownconfig section `recorder`
src/util/{paths,duration_text}.rs
```
- **Record.** `use_microphone` drives the live waveform. A session thread drains `frames` into the
  encoder: Opus/Ogg by default, FLAC as the lossless option. Files go to
  `$XDG_MUSIC_DIR/Recordings/<date time>.ogg`.
- **Library.** Recordings are listed from that directory, newest first, in the kit sidebar layout. A
  peaks file (`.peaks`, about 1 KB per minute) is written during recording, so the list never decodes
  audio.
- **Enhance.** Runs offline through `voice-protocol::Enhance` with progress, writes `<name> (enhanced).ogg`
  and keeps the original.
- **Playback.** ffmpeg decode → PipeWire output stream, with seeking on the static waveform.
- **Targets:** idle RSS < 40 MB; no backend loaded while idle; recording with CPU denoise < 5 % of
  one core.

## 6. Installer and ISO

| Where | Change |
|---|---|
| `crownos-installer/src/engine/probe.rs` | `System` trait gains `read_dir`. `SystemSnapshot.ai_hardware: HardwareInventory` is filled via `crownos_accel::probe` next to `microarch` (l.437) |
| `ui/` | New **Features** page before Confirm with a crownui `toggle`: **AI features** (covers voice *and* camera). It shows the detected devices and the download size. Update `ISO_STEPS`, `next_page`, `Page::step` |
| `engine/plan.rs` | `IsoInput.ai_features: bool`. `system_steps()` adds `Step::InstallAiSupport { root, plan: PackagePlan }` after `InstallBaseSystem`, plus `Step::WriteAiConfig` |
| `engine/exec.rs` | Offline packages: pacstrap from the bundled mirror. Online packages: chroot `pacman -S --needed` when `context.online`, otherwise queued |
| `autoinstall.rs` | Add `ai_features: bool` explicitly (`deny_unknown_fields`), and update `tests/autoinstall.json` |
| `/etc/crownos/ai.ron` (new, system-wide) | `(enabled, inventory, installed: [...], pending: [...])`, read by crown-voiced, camera and Settings |
| `crownos-ai-setup.service` (new, oneshot, `After=network-online.target`) | Installs `pending` on first online boot, notifies via crownotify, clears `pending` |
| `crownos-iso/profile/install.packages` | Small items only: `onnxruntime-cpu`, `openvino`, `intel-npu-driver`, `crownos-voice*`, `crownos-ai-*`. CUDA and ROCm are multi-GB and stay online-only |
| `crownos-iso/components.txt` | Add `Crown-OS/crownos-voice`, `Crown-OS/crownos-sound-recorder`, `Crown-OS/crownos-accel` (if released separately) |
| `crownos-pkgbuilds/` | `crownos-ai-{cpu,nvidia,amd,intel}` meta PKGBUILDs. Camera appends its effect backends to the same metas |

**Gap to close first:** the installer installs no NVIDIA proprietary driver today. CUDA needs
`nvidia-open` + `nvidia-utils`, so `crownos-ai-nvidia` must pull them in, and the installer must also
add the `nvidia_drm.modeset=1` kernel parameter. NVENC for camera needs the same driver, so this is
shared work.

Hardware detection also runs at runtime: crown-voiced re-probes on start. If the hardware changes
(a new GPU, for example), Settings offers "Set up AI acceleration for <device>", which reuses the same
`PackagePlan`.

## 7. Settings (follow-up, small)
- New `voice` section in `crownos-config/src/schema/voice.rs`:
  `enhance_all_apps: bool`, `default_enhancement: Enhancement`, `backend: BackendPreference`.
  crownui reads it through `crownconfig` only.
- A Settings **Voice** page edits these fields. With `enhance_all_apps` on, the daemon keeps a
  persistent node and makes it the default PipeWire source.
- Migrate the Input page and crowndictator from cpal device names to PipeWire node names, which is the
  mismatch found today.

## 8. Shared seams with crownos-camera (agree before coding)

| # | Seam | Proposal | Owner |
|---|---|---|---|
| S1 | crownui crate names and layers | `audio` (us), `video` (camera), both in foundations; one PR adds both rows to `layers.toml` | joint |
| S2 | Error code ranges | `CUI07xx` audio, `CUI08xx` video, reserved in `docs/errors.md` now | us, first PR |
| S3 | Facade features and bundle | `audio` and `video` features; each side adds its own `BundledCrate` entry and manifest deps in separate commits; rebase on conflicts in `bundled_crate.rs` / `manifest.toml` / `crownui/Cargo.toml` | each |
| S4 | Hardware inventory and AI package plan | `crownos-accel` + `/etc/crownos/ai.ron` + `crownos-ai-<vendor>` metas, with **one** installer toggle | us builds; camera reviews and extends |
| S5 | Media clock | Every frame carries the PipeWire graph clock in ns (`ClockNanos`), so the camera recorder muxes our audio frames directly | joint type in `audio`, re-exported by `video` |
| S6 | Encoding and muxing | Extract screencapture's `record/{audio/encoder,mux}.rs` into a shared `crownos-media-encode` (audio: Opus/FLAC; video: camera adds) | camera (needs video) or us (whoever starts first) |
| S7 | Device-access permission | One protocol `crownos-device-access-v1` with `DeviceKind { Microphone, Camera }`, modelled on `request_agent_access` | deferred, joint |
| S8 | Lazy backend loading | Same pattern (C-ABI vtable + `libloading` + idle unload). Consider a shared `crownos-accel::plugin` module once both sides have one implementation | later |
| S9 | Camera recording audio | Camera consumes `crownui::audio::Microphone`, so camera videos get the same enhancement for free | camera |

Write these into `crownui/docs/media.md` in the first PR. That doc is the meeting point for both teams.

## 9. Milestones (each ends green on `cargo fmt --check && cargo clippy -D warnings && cargo test`)

| M | Deliverable | Depends on | Parallel with |
|---|---|---|---|
| M0 | `crownui/docs/media.md` seams doc, CUI07/08 reservation, layers rows agreed with camera | — | camera M0 |
| M1 | `crownos-accel` probe + `PackagePlan`, with fixture tests for NVIDIA, AMD+XDNA, Intel+NPU and hybrid laptops | M0 | M3 widgets |
| M2 | `voice-abi`, `voice-engine`, CPU backend. Offline WAV test: SNR improves on a noise fixture; frame-time bench under 10 ms | — | M1 |
| M3 | crownui `kit/level_meter`, `kit/waveform` (previews, tests) on fake data | M0 | M1, M2 |
| M4 | `crown-voiced`: socket activation, filter node per stream, RT rings + deadline fallback, idle unload | M2 | M3 |
| M5 | crownui `audio` crate + `use_microphone`; daemon-absent fallback; bundle/api snapshot | M4, M3 | M6 |
| M6 | GPU backends: `onnx` (CUDA/TensorRT, MIGraphX/ROCm), `openvino` (NPU/GPU), with a warmup probe and cache | M2 | M5 |
| M7 | Recorder app: record, library, peaks, playback, offline Enhance | M5 | M8 |
| M8 | Installer Features page, `InstallAiSupport`, `ai.ron`, first-boot service, metas, ISO lists, autoinstall test | M1, M6 | M7 |
| M9 | Settings Voice page, `enhance_all_apps`, PipeWire-name migration; optional Maxine plugin | M7, M8 | — |

## 10. Task checklist (status 2026-11-05)

- [x] M0: `crownui/docs/media.md`, CUI07xx reserved, CUI08xx noted for video; seams agreed with camera (§0)
- [~] M1: hardware probe: the installer side is owned by camera (`util/pci.rs`, `engine/ai.rs`). The voice runtime probes plugins instead (`voiced/src/registry/`)
- [x] M2: `voice-abi` (vtable with latency), `voice-engine`, CPU backend (RNNoise + DeepFilterNet3 via `deep-filter-rt`)
- [x] M3: `kit/level_meter`, `kit/waveform`, previews and tests
- [x] M4: `crown-voiced`: socket activation, idle exit, node per stream, deadline fallback and demotion, probe cache, self-capture guard, offline jobs
- [x] M5: crownui `audio` crate + `kit::use_microphone` (merged into crownui `main`)
- [x] M6: `onnx` backend (offline only). Tested on GitHub CI against Arch `onnxruntime-cpu` 1.29 (CPU EP only; no GPU on runners)
- [x] M7: recorder app
- [~] M8: packaging and CI/CD in all repos, v0.1.0 releases. crownos-voice is an optdepend of the recorder; the ISO ships the recorder, and voice waits for the AI-features path (crownos-ai-* metas), which doesn't exist yet
- [x] M9: `voice` config section, `enhance_all_apps` (persistent `crownos-voice.all-apps` node as default source, started at login), backend preference with `ServiceStatus::{realtime,offline}_backend`, Settings Voice page, Settings Input page and crowndictator moved from cpal to PipeWire node names via the `audio` crate
- [x] Licences (MIT OR Apache-2.0), CI integration workflow (`crownos-voice/.github/workflows/integration.yml`: ONNX, daemon E2E per profile, headless PipeWire live stream)

### Follow-ups
- [ ] Tag recorder v0.1.1 (crownos-voice as optdepend) so the ISO can ship it
- [ ] Live-test GPU execution providers (CUDA/MIGraphX/OpenVINO/VitisAI) on real hardware once `onnxruntime-crownos-*` exist
- [ ] Real-time GPU denoise needs a stateful (GRU state as I/O) DeepFilterNet3 ONNX export
- [ ] Optional Maxine plugin (NGC-gated SDK)
- [ ] The `audio` crate captures nothing until crown-voiced answers (up to 3 s); start raw and switch to enhanced, so push-to-talk never loses its first words
- [ ] Per-app streams with no source while all-apps is on capture the highest-priority mic, not the one the all-apps node follows
- [ ] crownui gaps (handed to the crownui session): tabular numerals, `text_input` autofocus, public `follow_watch`, `cx.spawn` handler scope, crowntest task executor

## 11. Risks

| Risk | Mitigation |
|---|---|
| Maxine licence and NGC gating; the Linux SDK is server-targeted | Optional plugin only; CUDA EP is the default NVIDIA path |
| AMD NPU (VitisAI) on Linux falls back to CPU or gives wrong outputs (per community reports) | Ship disabled; enable when AMD's Linux stack is stable |
| ROCm supports only some consumer GPUs | `crownos-accel` keeps an allowlist of supported gfx IDs; others use CPU real-time + CPU offline |
| CUDA/ROCm too large for the ISO | Online during install, otherwise first-boot queued install |
| A GPU round trip per 10 ms frame can be slower than the CPU | Warmup-measured selection per workload; real-time defaults to NPU/CPU, dGPU used for offline |
| Arch onnxruntime variants conflict (`-cpu`, `-cuda`, `-rocm`) | One backend `.so` against `load-dynamic`; the meta package picks exactly one variant |
| Conflicts with uncommitted crownui work and with camera | Work on a branch from `main`; touch only S1–S3 shared files; seams doc first |
