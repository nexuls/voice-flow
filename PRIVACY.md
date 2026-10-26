# Privacy

VoiceFlow is local-first. Speech recognition runs entirely on your computer.

## What stays on your device

- **Audio:** captured only while you hold the shortcut. It is held in memory,
  converted to 16 kHz and passed to the local engine. Buffers are zeroed
  after use. Audio is never written to disk or sent anywhere.
- **Transcripts:** held in memory only until they are inserted. If insertion
  fails, the transcript is kept in memory so you can copy it manually.
  Transcripts are never logged or persisted.
- **Window information:** only the focused window's id and application id
  (for example `kitty`) are read, to avoid pasting into the wrong window and to
  choose the paste shortcut. Titles and contents are never read.
- **Logs:** contain timings, state names and error kinds only, never audio or
  transcript text.

## Settings

Preferences are stored locally as JSON (`settings.json` in the OS config
directory). They contain no audio, transcripts or window information. Start
on login uses the OS mechanism: `~/.config/autostart` on Linux, a LaunchAgent on
macOS, the registry Run key on Windows.

## Clipboard

To paste, VoiceFlow briefly replaces the clipboard with the transcript. If
the clipboard held text, that text is read beforehand and put back about
0.6 s after the paste, but only if the clipboard still holds the transcript.
Non-text clipboard content (images, files) cannot be saved and is replaced
by the transcript. Clipboard contents are never logged or stored.

When the focused window changed during transcription, or no paste mechanism
is available, the transcript is left on the clipboard for you to paste.

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
| Accessibility                | macOS                              | Sending the paste keystroke (Cmd+V)                                                                                                                                                  |
| `/dev/uinput` write access   | Linux, fallback only               | Paste keystroke on compositors without the virtual-keyboard protocol                                                                                                                 |
| `input` group (`/dev/input`) | Linux, evdev shortcut backend only | Detecting the push-to-talk key on Wayland compositors without the GlobalShortcuts portal. Only the configured key combination is evaluated. Other keys are ignored and never stored. |
