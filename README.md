# VoiceFlow

Push-to-talk dictation that runs entirely on your machine. Hold a shortcut
(default `Ctrl+Space`), speak, and release. A small overlay shows your audio level.
The transcript is pasted into the application you were typing in.

Speech recognition uses [Cactus Whistle](https://huggingface.co/Cactus-Compute/whistle)
running on the [Needle](https://github.com/cactus-compute/needle) engine. Audio
never leaves your computer.

> Status: early development. See [PLAN.md](PLAN.md) for progress.

## Using VoiceFlow

- VoiceFlow runs in the background. Settings opens on first run, from the tray
  icon, or when you launch the app again.
- Closing the Settings window does not quit the app. Use **Quit VoiceFlow** in the tray menu.
- Install the speech model from Settings (16.9 MB, one-time download).
- Hold the shortcut (default `Ctrl+Space`), speak, release. The text is pasted
  into the focused application. If pasting is not possible, it is copied to the
  clipboard and the overlay says so.

Settings: shortcut, shortcut method (Linux), microphone (with a test),
language (7 languages or automatic), maximum recording length, automatic
paste, clipboard restore, overlay visibility and position, sounds, start on
login and privacy-safe diagnostics. They are stored in the OS config directory
(`~/.config/dev.voiceflow.app/settings.json` on Linux).

## Prerequisites

- [Rust](https://rustup.rs) 1.90+
- [Bun](https://bun.sh) 1.3+
- Tauri 2 system dependencies: <https://tauri.app/start/prerequisites/>
  - Arch Linux: `webkit2gtk-4.1 gtk3 libappindicator-gtk3 librsvg alsa-lib`
- Linux: `gtk-layer-shell` (overlay placement on Wayland). Build without it
  using `--no-default-features`.
- Linux, optional: `libayatana-appindicator` for the tray icon. Without it,
  VoiceFlow runs without a tray, and launching it again opens Settings.

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
microphone), `VOICEFLOW_SHORTCUT` (e.g. `F19`).

## Speech model

The app links the official prebuilt [Needle](https://github.com/cactus-compute/needle)
engine (`libneedle.a`, Apache-2.0). It runs the
[Cactus Whistle](https://huggingface.co/Cactus-Compute/whistle) model
(`whistle.cact`, 16.9 MB, Apache-2.0). Neither file is committed:

- `bun run fetch:needle` downloads the engine into `src-tauri/vendor/needle/`.
- The model is installed from inside the app on first run. During
  development you can also run
  `cargo run --manifest-path src-tauri/Cargo.toml --example install_model -- ~/.local/share/dev.voiceflow.app/models`.

Both are pinned to a specific Hugging Face revision and checked against
SHA-256 checksums.

Supported engine targets: `linux-x86_64`, `linux-arm64`, `macos-arm64`,
`windows-x86_64` and `windows-arm64`. Needle publishes no Intel macOS build.
On Linux, libc++ is linked statically (Arch: `libc++ libc++abi`).

## License

Apache-2.0
