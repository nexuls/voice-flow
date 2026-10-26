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

## Speech engine

- `build.rs` links `vendor/needle/<platform>/libneedle.a`, fetched and
  verified by `scripts/fetch-needle.ts`, plus libc++ (static on Linux).
  `--no-default-features` builds without the engine.
- Needle keeps one global model per kind and has no unload call. The app
  loads the model once and keeps the bytes for the process lifetime.
- The engine call cannot be interrupted. Cancelling discards the result, and
  the workflow's session ids drop late results.

## End-to-end flow

```
press ─▶ ShowOverlay + BeginCapture
           ├─ engine not ready → CaptureFailed(modelUnavailable), mic never opened
           ├─ AudioControl.start (device open ≈ 70 ms)
           └─ insertion.capture_target()          (focus at press time)
release ─▶ FinishCapture
           └─ AudioControl.finish → Recording (16 kHz)
                 < 300 ms         → CaptureEmpty(tooShort)
                 peak < 0.001     → CaptureEmpty(noSpeech: mic muted?)
                 no speech region → CaptureEmpty(noSpeech)
                 else trim to the speech region → TranscriptionService.transcribe
                      → TranscriptReady / TranscriptionFailed
TranscriptReady ─▶ Insert → InsertionDone / InsertionFailed (transcript kept for copy)
```

Each dictation emits `diagnostics://timings` with latencies only.
`VOICEFLOW_DIAGNOSTIC=1` swaps the live executor for the simulated one.

## Text insertion

```
press   → focus.focused()          remember target window id + app id
insert  → focus.focused() again    changed? → copy only (CopiedToClipboard)
        → save clipboard text      (text only; images/files cannot be saved)
        → set transcript
        → paste chord              (Ctrl+V | Ctrl+Shift+V in terminals | Cmd+V)
             failed twice → copy only (CopiedToClipboard)
        → report Pasted
after   → wait 600 ms → clipboard still ours? → restore saved text
```

| Session                       | Keystrokes                                               | Focus check           | Status     |
| ----------------------------- | -------------------------------------------------------- | --------------------- | ---------- |
| Wayland (niri)                | virtual keyboard (enigo), uinput fallback                | niri IPC              | verified   |
| Wayland (Hyprland, sway, KDE) | virtual keyboard                                         | Hyprland IPC / none   | not tested |
| Wayland (GNOME)               | uinput if `/dev/uinput` is writable, otherwise copy only | none                  | not tested |
| X11                           | XTest                                                    | `_NET_ACTIVE_WINDOW`  | not tested |
| Windows                       | SendInput                                                | `GetForegroundWindow` | not tested |
| macOS                         | CGEvent (Accessibility permission)                       | none                  | not tested |

## Application shell

- Single instance: a second launch shows Settings in the running instance.
- The Settings window is hidden at startup. It opens on first run or while
  the model is missing, unless the app was started with `--background`
  (start on login).
- A tray menu (Settings…, Quit) is shown when an AppIndicator library is
  available. The tray crate would panic without one, so this is probed first.

## Reliability notes (milestone 9 audit)

- Workflow types (`Input`, `Effect`, `State`) have hand-written `Debug` impls
  that replace transcript text with its length, so debug logs and panic
  messages cannot leak dictated text (covered by a unit test).
- Per-session contexts are pruned when a new session starts. Overlay reports
  for unknown sessions are ignored.
- The shortcut manager lock can be held while a portal dialog is open. Status
  reads use a cached copy, and exit uses `try_lock`.
- The evdev backend remembers non-keyboard devices, so its 2 s rescan does
  not reopen them.
- The tray is created only if an AppIndicator library can be loaded.
