# VoiceFlow

Push-to-talk dictation that runs entirely on your machine. Hold a shortcut
(default `Ctrl+Space`), speak, and release. A small overlay shows your audio level.
The transcript is pasted into the application you were typing in.

Speech recognition uses [Cactus Whistle](https://huggingface.co/Cactus-Compute/whistle)
running on the [Needle](https://github.com/cactus-compute/needle) engine. Audio
never leaves your computer.

> Status: early development. See [PLAN.md](PLAN.md) for progress.

## Prerequisites

- [Rust](https://rustup.rs) 1.90+
- [Bun](https://bun.sh) 1.3+
- Tauri 2 system dependencies: <https://tauri.app/start/prerequisites/>
  - Arch Linux: `webkit2gtk-4.1 gtk3 libappindicator-gtk3 librsvg alsa-lib`
- Linux: `gtk-layer-shell` (overlay placement on Wayland). Build without it
  using `--no-default-features`.

## Development

```sh
bun install
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

## License

Apache-2.0
