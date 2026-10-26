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

| Module                 | Responsibility                                                                                                                                                                                                                                                                   |
| ---------------------- | -------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| `workflow::machine`    | Pure state machine: `Idle → Recording → Processing → Inserting → Success/Error → Idle`. Session ids reject stale results. No I/O.                                                                                                                                                |
| `workflow::controller` | Async loop that applies inputs in order, runs effects through an `Executor`, owns dismiss timers and reports transitions. `DiagnosticExecutor` simulates all services.                                                                                                           |
| `shortcuts`            | `Accelerator` parsing, the `ShortcutService` trait, `PressFilter` (dedupes repeats and stray releases), `ShortcutManager` (backend fallback, restores the previous shortcut on a failed change).                                                                                 |
| `shortcuts::hotkey`    | global-hotkey through the Tauri plugin: Windows, macOS and X11.                                                                                                                                                                                                                  |
| `shortcuts::portal`    | XDG GlobalShortcuts portal (Wayland).                                                                                                                                                                                                                                            |
| `shortcuts::evdev`     | `/dev/input` reader (Linux, any compositor).                                                                                                                                                                                                                                     |
| `overlay`              | `OverlayController`: hidden pre-created overlay window, bottom-center placement on the monitor under the cursor (scale-aware), click-through unless an action is offered. On Wayland with layer-shell, a bottom-anchored `Overlay`-layer surface with no keyboard interactivity. |
| `executor`             | `AppExecutor`: routes overlay effects to the `OverlayController` and the rest to the service executor.                                                                                                                                                                           |
| `commands` / `events`  | Typed IPC surface. Mirrored in `src/types/ipc.ts`.                                                                                                                                                                                                                               |
| `logging`              | `tracing` setup. Never logs audio or transcript text.                                                                                                                                                                                                                            |

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

## Overlay window

- It is created once at startup, hidden and non-focusable, so showing it costs
  one position update and a map.
- Click-through (an empty input shape) is applied only while the window is
  visible, because GTK needs a realized window for input shapes.
- The frontend (`src/components/overlay`) only renders the phase it gets from
  `dictation://transition` and the levels from `audio://level`.

## Audio pipeline

```
device callback (real-time)          capture thread (every 10 ms)             finish()
 T → f32, whole frames only  ──rtrb──▶ Accumulator: downmix → mono      ──▶  FFT resample → 16 kHz
 ring full → count dropped             level meter → 30 Hz events            Recording (zeroed on drop)
                                       dropped → silence (keeps timing)
                                       max duration → LimitReached
```

- The device is opened per recording and closed on finish or abort, so the
  microphone is only in use while recording. Opening took about 63 ms on the test machine.
- The callback never allocates, locks or blocks.
- Buffers are bounded by `max_duration` (default 120 s, hard cap 600 s).
