# VoiceFlow

Push-to-talk dictation that runs entirely on your machine. Hold a shortcut
(default `Ctrl+Space`), speak, and release. A small overlay shows your audio
level. The transcript is pasted into the application you were typing in.

Speech recognition uses [Cactus Whistle](https://huggingface.co/Cactus-Compute/whistle)
running on the [Needle](https://github.com/cactus-compute/needle) engine. Audio
never leaves your computer. See [PRIVACY.md](PRIVACY.md).

> Status: release candidate for Linux (tested on Wayland/niri). Windows and
> macOS builds are configured but have not been tested on real hardware. See
> [Platform status](#platform-status).

## Using VoiceFlow

1. Start VoiceFlow. On first run, Settings opens.
2. Click **Install** next to _Cactus Whistle_ (16.9 MB, one-time download,
   checksum-verified).
3. Hold the shortcut, speak, release. The text is pasted into the focused
   application. If pasting is not possible, it is copied to the clipboard and
   the overlay says _Copied — paste with Ctrl+V_.

VoiceFlow keeps running in the background. Closing Settings does not quit it.
Reopen Settings from the tray icon or by launching VoiceFlow again, and quit
from the tray menu.

Settings: shortcut, shortcut method (Linux), microphone (with a 3 s test),
language (English, German, French, Spanish, Italian, Dutch, Polish or automatic),
maximum recording length, automatic paste, clipboard restore, overlay visibility
and position, sounds, start on login and privacy-safe diagnostics. They are
stored in the OS config directory (`~/.config/dev.voiceflow.app/settings.json`
on Linux).

## Installation

Build packages as described in [Building releases](#building-releases). On
Linux, install the `.deb`, `.rpm` or AppImage.

### Permissions

| Platform                 | What                                                              | Why                                                                                           |
| ------------------------ | ----------------------------------------------------------------- | --------------------------------------------------------------------------------------------- |
| all                      | Microphone                                                        | Recording while the shortcut is held                                                          |
| macOS                    | Accessibility (System Settings → Privacy & Security)              | Sending Cmd+V to paste                                                                        |
| Linux, Wayland           | `input` group (`sudo usermod -aG input $USER`, then log in again) | Shortcut detection when the compositor has no GlobalShortcuts portal (for example niri, sway) |
| Linux, Wayland, optional | write access to `/dev/uinput`                                     | Paste fallback on compositors without the virtual-keyboard protocol (GNOME)                   |

## Troubleshooting

| Symptom                                      | Cause and fix                                                                                                                                                                          |
| -------------------------------------------- | -------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| Overlay says _Install the speech model…_     | Open Settings and install the model.                                                                                                                                                   |
| Shortcut does nothing (Wayland)              | Settings → Shortcut shows the error. Add yourself to the `input` group, or use a desktop with the GlobalShortcuts portal (KDE, GNOME 48+, Hyprland).                                   |
| _Ctrl+Alt+… is already in use_               | Another app owns the shortcut. Choose a different one. The previous shortcut stays active.                                                                                             |
| _No sound from the microphone. Is it muted?_ | The recording was silent. Check the mute switch and Settings → Test microphone.                                                                                                        |
| _No speech detected_                         | Nothing clearly above the background noise was heard. Speak closer to the microphone.                                                                                                  |
| Text copied instead of pasted                | Focus moved to another window during transcription, automatic paste is off, or no paste mechanism is available (on macOS, grant Accessibility). Paste with Ctrl+V / Cmd+V.             |
| Text pasted into a terminal shows `^V`       | The terminal is not in the known list (Ctrl+Shift+V is used for kitty, Alacritty, foot, WezTerm, GNOME Terminal, Konsole and others). Paste manually and report the terminal's app id. |
| No tray icon (Linux)                         | Install `libayatana-appindicator`, and make sure your panel supports StatusNotifierItem. Launching VoiceFlow again always opens Settings.                                              |
| Overlay appears as a normal window (Wayland) | The compositor lacks wlr-layer-shell (GNOME). It still works, but placement is up to the compositor.                                                                                   |
| Something else                               | Settings → Diagnostics → _Copy report_ (timings and settings only, no audio or text) and attach it to your bug report.                                                                 |

## Platform status

| Platform                                   | Shortcut                    | Overlay           | Paste                             | Engine                                                                    | Tested    |
| ------------------------------------------ | --------------------------- | ----------------- | --------------------------------- | ------------------------------------------------------------------------- | --------- |
| Linux, Wayland (wlroots/niri/Hyprland/KDE) | portal or evdev             | layer-shell       | virtual keyboard, uinput fallback | static `libneedle.a`                                                      | niri: yes |
| Linux, Wayland (GNOME)                     | portal (GNOME 48+) or evdev | normal window     | uinput or copy only               | same                                                                      | no        |
| Linux, X11                                 | XGrabKey                    | positioned window | XTest                             | same                                                                      | no        |
| macOS (Apple silicon only)                 | Carbon hotkey               | positioned window | CGEvent (Accessibility)           | static `libneedle.a` + libc++                                             | no        |
| Windows                                    | RegisterHotKey              | positioned window | SendInput                         | MinGW archive: needs a `*-pc-windows-gnullvm` toolchain, **not verified** | no        |

Known limitations:

- Intel Macs are not supported because Needle publishes no `macos-x86_64` engine.
- The official Windows engine archive (`windows-x86_64/libneedle.a`) is built
  with MinGW (Itanium C++ ABI) and cannot be linked by MSVC. Windows CI builds
  the app without the engine (`--no-default-features`).
- With the evdev shortcut backend, the focused application also receives the
  shortcut keystroke.
- Clipboard restore preserves text only. Images or files on the clipboard are
  replaced by the transcript.
- Inference time rises sharply above about 5 s of speech (engine behaviour,
  see TESTING.md). Silence is trimmed before inference to soften this.

## Development

```sh
bun install
bun run fetch:needle  # official Needle engine for this platform (pinned, SHA-256 verified)
bun run app:dev       # run the desktop app with hot reload
bun run dev           # frontend only (http://localhost:1420)
```

| Command             | Purpose                          |
| ------------------- | -------------------------------- |
| `bun run lint`      | Biome lint + format check        |
| `bun run fix`       | Apply Biome fixes and formatting |
| `bun run typecheck` | TypeScript type check            |
| `bun run test`      | Frontend unit tests (Vitest)     |
| `bun run rust:lint` | Clippy with warnings as errors   |
| `bun run rust:fmt`  | Format Rust code                 |
| `bun run rust:test` | Rust unit tests                  |
| `bun run check`     | All of the above                 |
| `bun run build`     | Production frontend build        |
| `bun run app:build` | Production desktop bundle        |

Environment variables for development: `VOICEFLOW_LOG` (tracing filter, e.g.
`voiceflow_lib=debug`), `VOICEFLOW_DIAGNOSTIC=1` (simulated services, no
microphone), `VOICEFLOW_SHORTCUT` (e.g. `F19`), `VOICEFLOW_PASTE_BACKEND=uinput`.

### Prerequisites

- Rust: the version pinned in `src-tauri/rust-toolchain.toml` (installed automatically by rustup)
- [Bun](https://bun.sh) 1.3+
- Tauri 2 system dependencies: <https://tauri.app/start/prerequisites/>
- Linux:
  - Arch: `webkit2gtk-4.1 gtk3 librsvg alsa-lib gtk-layer-shell libayatana-appindicator libc++ libc++abi clang`
  - Debian/Ubuntu: `libwebkit2gtk-4.1-dev libgtk-3-dev librsvg2-dev libasound2-dev libgtk-layer-shell-dev libayatana-appindicator3-dev clang libc++-dev libc++abi-dev patchelf`
- macOS: Xcode command-line tools (libc++ is part of the system).

## Building releases

Builds are reproducible from the lockfiles (`bun.lock`, `src-tauri/Cargo.lock`),
the pinned Rust toolchain and the pinned, checksum-verified engine:

```sh
bun install --frozen-lockfile
bun run fetch:needle
bunx tauri build                       # all bundle types for this OS
bunx tauri build --bundles deb,rpm     # Linux packages only
```

Outputs are written to `src-tauri/target/release/bundle/`. Release signing (macOS
notarisation, Windows Authenticode) is not configured. Linux packages install
the license texts of the bundled engine under `/usr/lib/VoiceFlow/licenses/`.
CI (`.github/workflows/ci.yml`) runs lint, type checks, unit tests and the
real-model test on Linux and macOS, plus an engine-less build check on Windows.
Linux bundles are built on manual dispatch.

## Speech model and engine

The app statically links the official prebuilt Needle engine (`libneedle.a`,
Apache-2.0) and runs the Cactus Whistle model (`whistle.cact`, Apache-2.0).
Neither file is committed to Git:

- `bun run fetch:needle` downloads the engine into `src-tauri/vendor/needle/`.
- The model is installed by the user from Settings. For development:
  `cargo run --manifest-path src-tauri/Cargo.toml --example install_model -- ~/.local/share/dev.voiceflow.app/models`.

Both are pinned to a specific Hugging Face revision and verified by SHA-256.
On Linux, libc++ is linked statically, so the app has no libc++ runtime
dependency. See [THIRD_PARTY_NOTICES.md](THIRD_PARTY_NOTICES.md).

## Documentation

[ARCHITECTURE.md](ARCHITECTURE.md) · [TESTING.md](TESTING.md) ·
[PRIVACY.md](PRIVACY.md) · [CONTRIBUTING.md](CONTRIBUTING.md) · [PLAN.md](PLAN.md)

## License

Apache-2.0 is declared in `src-tauri/Cargo.toml`. It matches the bundled
engine and model, but it is a placeholder: the project owner should confirm
it and add a `LICENSE` file before publishing.
