# Architecture

VoiceFlow is a Tauri 2 app. The Rust backend owns every system integration:
shortcuts, audio, inference, clipboard and windows. The React frontend only
renders state and sends typed commands.

## Data flow

```
 ShortcutService ──press/release──▶ WorkflowHandle ──▶ controller loop
   (hotkey | portal | evdev)                           │  Machine::handle(input)
                                                       │    → new state + effects
                                    StateObserver ◀────┤
                                    (emits dictation://transition)
                                                       ▼
                                                 Executor
                          BeginCapture / FinishCapture / Insert / ShowOverlay …
                                                       │
              results come back as Inputs (TranscriptReady, InsertionDone, …)
```

## Modules (`src-tauri/src`)

| Module                 | Responsibility                                                                                                                                                                                   |
| ---------------------- | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------ |
| `workflow::machine`    | Pure state machine: `Idle → Recording → Processing → Inserting → Success/Error → Idle`. Session ids reject stale results. No I/O.                                                                |
| `workflow::controller` | Async loop that applies inputs in order, runs effects through an `Executor`, owns dismiss timers and reports transitions. `DiagnosticExecutor` simulates all services.                           |
| `shortcuts`            | `Accelerator` parsing, the `ShortcutService` trait, `PressFilter` (dedupes repeats and stray releases), `ShortcutManager` (backend fallback, restores the previous shortcut on a failed change). |
| `shortcuts::hotkey`    | global-hotkey through the Tauri plugin: Windows, macOS and X11.                                                                                                                                  |
| `shortcuts::portal`    | XDG GlobalShortcuts portal (Wayland).                                                                                                                                                            |
| `shortcuts::evdev`     | `/dev/input` reader (Linux, any compositor).                                                                                                                                                     |
| `commands` / `events`  | Typed IPC surface. Mirrored in `src/types/ipc.ts`.                                                                                                                                               |
| `logging`              | `tracing` setup. Never logs audio or transcript text.                                                                                                                                            |

## State machine rules

- Press while idle, or while showing a result, starts a new session.
- Press during `Processing` or `Inserting` is ignored, so the current transcript
  can never be corrupted by a second recording.
- Results carry their session id. A result for any session other than the
  current one is dropped.
- An empty transcript becomes a `noSpeech` error, which dismisses itself.
- When insertion fails, the transcript stays in memory for manual copying. The
  serialised state only exposes `hasTranscript`.
- Cancel during `Inserting` is deferred because a paste in flight cannot be
  undone. Shutdown returns to `Idle` from any state.

## Threading

- **Main thread:** Tauri event loop and window management.
- **Tokio runtime:** the controller loop, timers and async commands.
- **Shortcut threads:** portal (its own current-thread runtime) and evdev (a `poll(2)` loop that wakes every 200 ms to check for stop).
