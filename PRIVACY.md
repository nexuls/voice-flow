# Privacy

VoiceFlow is local-first. Speech recognition runs entirely on your computer.

## What stays on your device

- **Audio:** captured only while you hold the shortcut. It is held in memory,
  converted to 16 kHz and passed to the local engine. Buffers are zeroed
  after use. Audio is never written to disk or sent anywhere.
- **Transcripts:** held in memory only until they are inserted. If insertion
  fails, the transcript is kept in memory so you can copy it manually.
  Transcripts are never logged or persisted.
- **Logs:** contain timings, state names and error kinds only, never audio or
  transcript text.

## Network activity

| When                      | What                                                                                               | Why                   |
| ------------------------- | -------------------------------------------------------------------------------------------------- | --------------------- |
| You click "Install model" | One HTTPS download of `whistle.cact` (16.9 MB) from a pinned Hugging Face URL, verified by SHA-256 | Speech model          |
| Building from source      | `bun run fetch:needle` downloads `libneedle.a` and `needle.h` from a pinned Hugging Face revision  | Speech engine library |

Transcription itself makes no network requests. The statically linked
Needle library imports no socket functions (checked with `nm` on
`libneedle.a` for linux-x86_64). The `needle` command-line tool and the
Python package document opt-out telemetry. VoiceFlow uses neither.

## Permissions

| Permission                   | Platform                           | Used for                                                                                                                                                                             |
| ---------------------------- | ---------------------------------- | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------ |
| Microphone                   | all                                | Recording while the shortcut is held                                                                                                                                                 |
| `input` group (`/dev/input`) | Linux, evdev shortcut backend only | Detecting the push-to-talk key on Wayland compositors without the GlobalShortcuts portal. Only the configured key combination is evaluated. Other keys are ignored and never stored. |
