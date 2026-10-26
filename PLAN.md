# VoiceFlow plan

Local-first push-to-talk dictation: hold a shortcut, speak, release, and the
transcript is pasted into the focused application. Speech recognition runs
on-device with Cactus Whistle on the Needle engine.

## Key technical decisions

| Area             | Decision                                                                                                                                   | Reason                                                                                                        |
| ---------------- | ------------------------------------------------------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------------------- |
| Speech engine    | Static link of the official `libneedle.a` + `needle.h` from `Cactus-Compute/needle3` (pinned revision, SHA-256 verified)                   | Documented C API (`needle_load`, `needle_transcribe`). No Python runtime.                                     |
| Model            | `whistle.cact` (16.9 MB) from `Cactus-Compute/whistle`, downloaded in-app at a pinned revision and checked against its SHA-256             | Weights stay out of Git.                                                                                      |
| Engine threading | One dedicated inference thread owns the engine                                                                                             | `needle.h`: "one process-global, non-thread-safe model per kind".                                             |
| Audio            | `cpal` capture, lock-free ring buffer to a worker thread, which handles downmix, resampling to 16 kHz and RMS                              | Keeps the real-time callback allocation-free.                                                                 |
| Shortcut         | `global-hotkey` (via the Tauri plugin) on Windows, macOS and X11. On Wayland: the XDG GlobalShortcuts portal, plus an opt-in evdev backend | Wayland does not allow X11-style key grabs. The portal reports press and release (`Activated`/`Deactivated`). |
| Insertion        | Native clipboard (`arboard`) plus a synthetic paste keystroke (`enigo`), then race-aware clipboard restore                                 | Paste behaviour differs per OS. A manual-copy fallback is always available.                                   |

## Platform constraints

- **Linux/Wayland:** windows cannot grab global keys, choose their own position or force
  always-on-top. The compositor decides. GlobalShortcuts portal support varies
  by compositor (KDE, GNOME 48+ and Hyprland support it). The evdev fallback needs membership of the
  `input` group. Synthetic input needs the virtual-keyboard protocol (wlroots,
  niri, KDE) or uinput.
- **Linux/X11:** XGrabKey + XTest work without extra permissions.
- **macOS:** Accessibility permission is needed for synthetic paste, and Microphone
  permission for capture. Needle ships `macos-arm64` only, so Intel Macs are not supported.
- **Windows:** the official `windows-x86_64/libneedle.a` is a GNU-style archive.
  Linking it with MSVC has not been verified yet.

## Milestones

- [x] 1. Project foundation
- [x] 2. Native shortcut prototype
- [x] 3. Floating OSD
- [x] 4. Microphone capture
- [x] 5. Cactus Whistle integration
- [x] 6. End-to-end push-to-talk workflow
- [ ] 7. Reliable text insertion
- [ ] 8. Settings and configuration
- [ ] 9. Reliability and accessibility
- [ ] 10. Packaging and release readiness
