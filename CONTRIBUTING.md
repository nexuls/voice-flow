# Contributing

## Workflow

1. Run `bun install`.
2. Make one coherent change at a time.
3. Run `bun run check` before committing. It runs lint, typecheck, frontend tests, clippy and Rust tests.
4. Review `git diff --staged` for model weights, recordings, build output or secrets.
   These must never be committed.

## Commit messages

Use [Conventional Commits](https://www.conventionalcommits.org/):
`feat:`, `fix:`, `chore:`, `docs:`, `test:`, `build:`, `refactor:`.

## Code guidelines

- Keep platform code behind the service traits (`ShortcutService`,
  `AudioCaptureService`, `TranscriptionEngine`, `TextInsertionService`,
  `OverlayController`, `SettingsStore`).
- Never log audio samples or transcript text.
- Never allocate, lock or block inside the audio callback.
- Engine calls happen only on the inference thread.
