import { AlertTriangle } from "lucide-react";
import { useCallback, useEffect, useState } from "react";
import { TimingsTable } from "@/components/diagnostics/TimingsTable";
import { TransitionLog } from "@/components/diagnostics/TransitionLog";
import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import {
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from "@/components/ui/select";
import { Switch } from "@/components/ui/switch";
import { useSettings } from "@/hooks/useSettings";
import { useTauriEvent } from "@/hooks/useTauriEvent";
import { api, inTauri, on } from "@/lib/ipc";
import type {
  BackendPreference,
  LanguageCode,
  OverlayPosition,
  SessionTimings,
  ShortcutStatus,
  Transition,
} from "@/types/ipc";
import { MicrophoneRows } from "./MicrophoneSection";
import { ModelSection } from "./ModelSection";
import { Row, Section } from "./Section";
import { ShortcutRecorder } from "./ShortcutRecorder";

const LANGUAGES: [LanguageCode, string][] = [
  ["en", "English"],
  ["de", "German"],
  ["fr", "French"],
  ["es", "Spanish"],
  ["it", "Italian"],
  ["nl", "Dutch"],
  ["pl", "Polish"],
];
const AUTO = "auto";
const DURATIONS: [number, string][] = [
  [30, "30 seconds"],
  [60, "1 minute"],
  [120, "2 minutes"],
  [300, "5 minutes"],
  [600, "10 minutes"],
];
const BACKENDS: [BackendPreference, string][] = [
  ["auto", "Automatic"],
  ["globalHotkey", "Global hotkey (X11)"],
  ["portal", "Desktop portal (Wayland)"],
  ["evdev", "Input devices (evdev)"],
];
const BACKEND_NAMES: Record<string, string> = {
  globalHotkey: "global hotkey",
  portal: "desktop portal",
  evdev: "input devices",
};
const MAX_ROWS = 20;

export function SettingsApp() {
  const { settings, saving, warnings, error, update, dismissWarnings } = useSettings();
  const [shortcut, setShortcut] = useState<ShortcutStatus | null>(null);
  const [transitions, setTransitions] = useState<Transition[]>([]);
  const [timings, setTimings] = useState<SessionTimings[]>([]);

  useEffect(() => {
    if (inTauri()) api.shortcutStatus().then(setShortcut);
  }, []);
  useTauriEvent(on.shortcutStatus, setShortcut);
  useTauriEvent(
    on.transition,
    useCallback((t: Transition) => setTransitions((p) => [...p, t].slice(-MAX_ROWS)), []),
  );
  useTauriEvent(
    on.timings,
    useCallback((t: SessionTimings) => setTimings((p) => [t, ...p].slice(0, MAX_ROWS)), []),
  );

  if (!settings) {
    return (
      <main className="p-6 text-sm text-muted-foreground">
        {error ? `Settings could not be loaded: ${error}` : "Loading…"}
      </main>
    );
  }

  const onLinux = shortcut?.session === "wayland" || shortcut?.session === "x11";

  return (
    <main className="mx-auto max-w-2xl space-y-6 px-5 py-6">
      <header className="flex items-center justify-between">
        <div>
          <h1 className="text-lg font-semibold tracking-tight">VoiceFlow</h1>
          <p className="text-sm text-muted-foreground">
            Hold{" "}
            <kbd className="rounded border bg-muted px-1.5 py-0.5 font-mono text-xs">
              {shortcut?.accelerator ?? settings.shortcut}
            </kbd>{" "}
            and speak. Release to insert the text.
          </p>
        </div>
        {saving && <Badge variant="secondary">Saving…</Badge>}
      </header>

      {(warnings.length > 0 || error) && (
        <div
          role="alert"
          className="flex items-start gap-2 rounded-lg border border-destructive/40 bg-destructive/5 p-3 text-sm"
        >
          <AlertTriangle className="mt-0.5 size-4 shrink-0 text-destructive" />
          <div className="flex-1 space-y-1">
            {error && <p>{error}</p>}
            {warnings.map((w) => (
              <p key={w}>{w}</p>
            ))}
          </div>
          <Button variant="ghost" size="xs" onClick={dismissWarnings}>
            Dismiss
          </Button>
        </div>
      )}

      <ModelSection />

      <Section title="Shortcut">
        <Row
          label="Push-to-talk"
          description={
            shortcut?.error ? (
              <span className="text-destructive">{shortcut.error}</span>
            ) : shortcut?.backend ? (
              `Active via ${BACKEND_NAMES[shortcut.backend] ?? shortcut.backend}`
            ) : (
              "Not registered yet"
            )
          }
          control={(id) => (
            <ShortcutRecorder
              id={id}
              value={settings.shortcut}
              disabled={saving}
              onChange={(v) => update({ shortcut: v })}
            />
          )}
        />
        {onLinux && (
          <Row
            label="Shortcut method"
            description="On Wayland, the desktop portal is used when available; otherwise input devices (needs the input group)."
            control={(id) => (
              <Select
                value={settings.shortcutBackend}
                onValueChange={(v) => update({ shortcutBackend: v as BackendPreference })}
              >
                <SelectTrigger id={id} className="w-56">
                  <SelectValue />
                </SelectTrigger>
                <SelectContent>
                  {BACKENDS.map(([v, l]) => (
                    <SelectItem key={v} value={v}>
                      {l}
                    </SelectItem>
                  ))}
                </SelectContent>
              </Select>
            )}
          />
        )}
      </Section>

      <Section title="Recording">
        <MicrophoneRows
          value={settings.microphone}
          disabled={saving}
          onChange={(microphone) => update({ microphone })}
        />
        <Row
          label="Language"
          description="Automatic detection supports the seven languages Whistle knows."
          control={(id) => (
            <Select
              value={settings.language ?? AUTO}
              onValueChange={(v) => update({ language: v === AUTO ? null : (v as LanguageCode) })}
            >
              <SelectTrigger id={id} className="w-44">
                <SelectValue />
              </SelectTrigger>
              <SelectContent>
                <SelectItem value={AUTO}>Detect automatically</SelectItem>
                {LANGUAGES.map(([code, name]) => (
                  <SelectItem key={code} value={code}>
                    {name}
                  </SelectItem>
                ))}
              </SelectContent>
            </Select>
          )}
        />
        <Row
          label="Maximum recording length"
          description="Recording stops and is transcribed when the limit is reached."
          control={(id) => (
            <Select
              value={String(settings.maxRecordingSecs)}
              onValueChange={(v) => update({ maxRecordingSecs: Number(v) })}
            >
              <SelectTrigger id={id} className="w-36">
                <SelectValue />
              </SelectTrigger>
              <SelectContent>
                {DURATIONS.some(([s]) => s === settings.maxRecordingSecs) ? null : (
                  <SelectItem value={String(settings.maxRecordingSecs)}>
                    {settings.maxRecordingSecs} seconds
                  </SelectItem>
                )}
                {DURATIONS.map(([s, l]) => (
                  <SelectItem key={s} value={String(s)}>
                    {l}
                  </SelectItem>
                ))}
              </SelectContent>
            </Select>
          )}
        />
      </Section>

      <Section title="Text insertion">
        <Row
          label="Paste automatically"
          description="Off: the text is only copied to the clipboard."
          control={(id) => (
            <Switch
              id={id}
              checked={settings.autoPaste}
              onCheckedChange={(autoPaste) => update({ autoPaste })}
            />
          )}
        />
        <Row
          label="Restore clipboard"
          description="Puts your previous clipboard text back after pasting."
          control={(id) => (
            <Switch
              id={id}
              checked={settings.restoreClipboard}
              disabled={!settings.autoPaste}
              onCheckedChange={(restoreClipboard) => update({ restoreClipboard })}
            />
          )}
        />
      </Section>

      <Section title="Feedback">
        <Row
          label="Show overlay"
          control={(id) => (
            <Switch
              id={id}
              checked={settings.showOverlay}
              onCheckedChange={(showOverlay) => update({ showOverlay })}
            />
          )}
        />
        <Row
          label="Overlay position"
          control={(id) => (
            <Select
              value={settings.overlayPosition}
              disabled={!settings.showOverlay}
              onValueChange={(v) => update({ overlayPosition: v as OverlayPosition })}
            >
              <SelectTrigger id={id} className="w-36">
                <SelectValue />
              </SelectTrigger>
              <SelectContent>
                <SelectItem value="bottom">Bottom</SelectItem>
                <SelectItem value="top">Top</SelectItem>
              </SelectContent>
            </Select>
          )}
        />
        <Row
          label="Sounds"
          description="A soft tone when recording starts and stops."
          control={(id) => (
            <Switch
              id={id}
              checked={settings.soundFeedback}
              onCheckedChange={(soundFeedback) => update({ soundFeedback })}
            />
          )}
        />
      </Section>

      <Section title="System">
        <Row
          label="Start on login"
          control={(id) => (
            <Switch
              id={id}
              checked={settings.startOnLogin}
              onCheckedChange={(startOnLogin) => update({ startOnLogin })}
            />
          )}
        />
        <Row
          label="Diagnostics"
          description="Shows timings and state changes below. Never includes audio or text."
          control={(id) => (
            <Switch
              id={id}
              checked={settings.diagnostics}
              onCheckedChange={(diagnostics) => update({ diagnostics })}
            />
          )}
        />
      </Section>

      {settings.diagnostics && (
        <Section title="Diagnostics">
          <div className="space-y-4 px-4 py-3">
            <TimingsTable rows={timings} />
            <TransitionLog transitions={transitions} />
          </div>
        </Section>
      )}

      <Section title="Privacy">
        <div className="space-y-2 px-4 py-3 text-sm text-muted-foreground">
          <p>
            Audio is recorded only while you hold the shortcut, processed in memory and discarded.
            Speech recognition runs on this computer; no audio or text is sent anywhere.
          </p>
          <p>
            The only network request is the model download you start above. Transcripts are never
            saved or logged.
          </p>
        </div>
      </Section>
    </main>
  );
}
