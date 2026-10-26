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

### End-to-end dictation without speaking (Linux, PipeWire)

This plays a fixture into a temporary null sink and records from its monitor.
Only VoiceFlow is pointed at it (`PULSE_SOURCE`). System default devices are
not changed. **The transcript replaces the clipboard contents.**

```sh
MOD=$(pactl load-module module-null-sink sink_name=vf_sink channel_map=mono)
PULSE_SOURCE=vf_sink.monitor VOICEFLOW_SHORTCUT=F19 VOICEFLOW_LOG=voiceflow_lib=debug \
  ./src-tauri/target/release/voiceflow &
sleep 3
./src-tauri/target/debug/examples/hold_key F19 8500 &
sleep 3; pw-play --target vf_sink src-tauri/tests/fixtures/quick_brown_fox_en.wav
sleep 6; wl-paste
pactl unload-module "$MOD"
```

Measured, release build, 8.4 s recording:

| Latency                                        | Value                       |
| ---------------------------------------------- | --------------------------- |
| Shortcut to microphone capture                 | 69 ms                       |
| Shortcut to overlay painted (overlay-reported) | 108 ms                      |
| Release to finalised 16 kHz audio              | 14 ms (resampling 4.9 ms)   |
| Inference                                      | 892 ms (engine TTFT 784 ms) |
| Transcript to insertion attempt                | 2 ms                        |
| **Release to text inserted**                   | **908 ms**                  |

The transcript matched except for 1–3 words between runs. Debug builds
resample about 70 times slower (≈ 315 ms for 8 s), so measure latency only
on release builds.

### Automatic paste (Linux, Wayland)

`scripts/dev/paste_target.py <out.txt> <seconds>` opens a GTK window that
writes its text buffer to `out.txt`. Steps:

1. Save your clipboard, then `printf 'VF-ORIGINAL-CLIPBOARD' | wl-copy`.
2. Start VoiceFlow with the virtual-microphone setup above, then open the target window so it has focus.
3. Run the hold/play sequence from the end-to-end test.
4. The window should contain the transcript, and `wl-paste` should print `VF-ORIGINAL-CLIPBOARD` again (restored).
5. Focus change: open a second window while recording. Neither window should
   receive text, and the transcript should stay on the clipboard (`outcome: CopiedToClipboard`).
6. `VOICEFLOW_PASTE_BACKEND=uinput` forces the uinput backend.

Results on niri 26.04:

| Case                                               | Result                                                      |
| -------------------------------------------------- | ----------------------------------------------------------- |
| Paste via virtual keyboard                         | transcript inserted into the GTK window, clipboard restored |
| Paste via uinput (forced)                          | transcript inserted, clipboard restored                     |
| Focus moved to another window during transcription | nothing pasted, transcript left on the clipboard            |
| Paste into a terminal                              | not tested live (unit-tested chord selection)               |

## Test environments

| Environment                                                                           | Shortcut backend                                       | Result                                                                                                                                                |
| ------------------------------------------------------------------------------------- | ------------------------------------------------------ | ----------------------------------------------------------------------------------------------------------------------------------------------------- |
| Arch Linux, niri 26.04 (Wayland), xdg-desktop-portal 1.22 with the gtk/gnome backends | Portal refused the bind (`Other`). Fell back to evdev. | evdev: press/release verified with uinput and in the app. Overlay: layer surface verified by screenshot. It does not take focus (`niri msg windows`). |
| Same machine, PipeWire default source (48 kHz, stereo, f32)                           | n/a                                                    | Capture opened in 63 ms. 1 s recorded, 29 level events, 0 dropped frames                                                                              |
| X11                                                                                   | global-hotkey                                          | not yet tested                                                                                                                                        |
| Windows                                                                               | global-hotkey                                          | not yet tested                                                                                                                                        |
| macOS                                                                                 | global-hotkey                                          | not yet tested                                                                                                                                        |

## Known limitations

- **Noise without speech** can produce invented text. One run that
  captured only room noise from the real microphone returned a short French
  sentence. Whistle documents this for music and recommends a voice activity
  detector.
- **Leading or trailing silence** adds inference time. Recordings longer than
  about 5 s take noticeably longer (see the benchmark).

- **Overlay on Wayland without layer-shell (GNOME/Mutter):** the compositor
  positions and focuses the window. niri without layer-shell gave the overlay
  focus and a 954×1032 size.

- **evdev backend:** the focused application also receives the shortcut
  keystroke, and the backend needs the `input` group.
- **Portal backend:** needs an installed `dev.voiceflow.app.desktop` entry, because
  the host Registry portal rejects unknown app ids. It also needs a portal backend
  that implements GlobalShortcuts for the running compositor.
- **Windows:** release is detected by 50 ms polling inside global-hotkey.
