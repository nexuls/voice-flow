# Testing

## Automated

```sh
bun run check          # lint, typecheck, Vitest, clippy, cargo test
bun run test           # frontend only
bun run rust:test      # Rust unit tests
```

### Hardware-dependent tests (ignored by default)

```sh
# Linux: needs write access to /dev/uinput and read access to /dev/input
cargo test --manifest-path src-tauri/Cargo.toml -- --ignored uinput
```

```sh
# Records 1 s from the default microphone and checks duration, levels and device reuse
cargo test --manifest-path src-tauri/Cargo.toml -- --ignored microphone --nocapture

# Writes exactly what the engine receives (16 kHz mono) for listening checks
cargo run --manifest-path src-tauri/Cargo.toml --example record -- /tmp/check.wav 3
```

### Real speech model

```sh
bun run fetch:needle
cargo run --manifest-path src-tauri/Cargo.toml --example install_model -- ~/.local/share/dev.voiceflow.app/models
cargo test --manifest-path src-tauri/Cargo.toml --test whistle -- --ignored --nocapture
```

This test covers: not loaded, missing model, corrupt model, load, the known
English recording (exact keywords), German detection, forced language, word
timestamps, silence, empty audio, 40 s windowing and the service thread.

### Benchmark

```sh
cargo run --release --manifest-path src-tauri/Cargo.toml --example bench_whistle -- \
  ~/.local/share/dev.voiceflow.app/models/whistle.cact src-tauri/tests/fixtures/quick_brown_fox_en.wav 3
```

Baseline on an AMD Ryzen 5 5500H (8 logical CPUs available to the process), Linux, warm engine, middle of 3 runs:

| Audio | Inference | TTFT   | Real-time factor |
| ----- | --------- | ------ | ---------------- |
| 1 s   | 30 ms     | 11 ms  | 0.03             |
| 3 s   | 83 ms     | 28 ms  | 0.03             |
| 5 s   | 149 ms    | 53 ms  | 0.03             |
| 7 s   | 674 ms    | 596 ms | 0.10             |
| 10 s  | 987 ms    | 837 ms | 0.10             |
| 30 s  | 3.0 s     | 2.5 s  | 0.10             |
| 60 s  | 7.3 s     | 2.5 s  | 0.12             |

Between 5 s and 7 s the engine's time to first token jumps from about 10 ms to
about 85 ms per second of audio. This happens inside Needle. Splitting audio
into ≤ 5 s chunks could avoid it but may cut words at the boundaries, so it is
not done without an accuracy evaluation. Model load takes 18–25 ms.

## Manual procedures

### Shortcut and state machine (diagnostic mode)

Until audio capture is wired in, the app runs the workflow with simulated
services.

1. `VOICEFLOW_LOG=voiceflow_lib=debug bun run app:dev`
2. Check that the log line `shortcut registered backend=…` names the expected backend.
3. Hold `Ctrl+Space` for about 1 s, then release. The settings window should list
   `idle → recording → processing → inserting → success → idle`.
4. Tap the shortcut repeatedly and hold it for 5 s or more (key repeat). Each
   hold should produce exactly one `recording` and one `processing`.
5. Press the shortcut again while `processing` is shown. It should be ignored.

Automated variant on Linux without touching the keyboard:

```sh
VOICEFLOW_SHORTCUT=F19 VOICEFLOW_LOG=voiceflow_lib=debug ./src-tauri/target/debug/voiceflow &
cargo run --manifest-path src-tauri/Cargo.toml --example hold_key -- F19 1200
```

The compositor also receives keys from the virtual keyboard. Use F19 only:
in the default XKB map, F20 is `XF86AudioMicMute` and F21-F23 toggle the
touchpad.

### Overlay

1. Hold the shortcut. The pill should appear at the bottom-center within roughly
   100 ms, with a moving waveform (synthetic levels in diagnostic mode).
2. The focused application must keep keyboard focus. Type while holding the
   shortcut: the keystrokes should still reach that application.
3. Release. The pill should show "Transcribing…", then a success message, then disappear.
4. Click where the pill is. The click should reach the window underneath.
5. Multi-monitor (X11/Windows/macOS): move the cursor to another monitor and hold the
   shortcut. The pill should appear on that monitor.

## Test environments

| Environment                                                                           | Shortcut backend                                       | Result                                                                                                                                                |
| ------------------------------------------------------------------------------------- | ------------------------------------------------------ | ----------------------------------------------------------------------------------------------------------------------------------------------------- |
| Arch Linux, niri 26.04 (Wayland), xdg-desktop-portal 1.22 with the gtk/gnome backends | Portal refused the bind (`Other`). Fell back to evdev. | evdev: press/release verified with uinput and in the app. Overlay: layer surface verified by screenshot. It does not take focus (`niri msg windows`). |
| Same machine, PipeWire default source (48 kHz, stereo, f32)                           | n/a                                                    | Capture opened in 63 ms. 1 s recorded, 29 level events, 0 dropped frames                                                                              |
| X11                                                                                   | global-hotkey                                          | not yet tested                                                                                                                                        |
| Windows                                                                               | global-hotkey                                          | not yet tested                                                                                                                                        |
| macOS                                                                                 | global-hotkey                                          | not yet tested                                                                                                                                        |

## Known limitations

- **Overlay on Wayland without layer-shell (GNOME/Mutter):** the compositor
  positions and focuses the window. niri without layer-shell gave the overlay
  focus and a 954×1032 size.

- **evdev backend:** the focused application also receives the shortcut
  keystroke, and the backend needs the `input` group.
- **Portal backend:** needs an installed `dev.voiceflow.app.desktop` entry, because
  the host Registry portal rejects unknown app ids. It also needs a portal backend
  that implements GlobalShortcuts for the running compositor.
- **Windows:** release is detected by 50 ms polling inside global-hotkey.
