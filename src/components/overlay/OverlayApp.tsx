import { useEffect, useRef, useState } from "react";
import { OverlayPill } from "@/components/overlay/OverlayPill";
import { useAudioLevels } from "@/hooks/useAudioLevels";
import { useDictationState } from "@/hooks/useDictationState";
import { useTauriEvent } from "@/hooks/useTauriEvent";
import { api, inTauri, on } from "@/lib/ipc";
import { startTone, stopTone } from "@/lib/sound";
import type { Phase } from "@/types/ipc";

/** Root of the overlay window. The window itself is managed natively. */
export function OverlayApp() {
  const state = useDictationState();
  const recording = state.phase === "recording";
  const levels = useAudioLevels(recording);
  const session = recording ? state.session : null;

  // Optional tones. The overlay page also runs while the overlay is hidden,
  // so sounds work without the visual overlay.
  const [sounds, setSounds] = useState(false);
  useEffect(() => {
    if (inTauri()) api.getSettings().then((s) => setSounds(s.soundFeedback));
  }, []);
  useTauriEvent(on.settingsChanged, (s) => setSounds(s.soundFeedback));
  const previous = useRef<Phase>("idle");
  useEffect(() => {
    const before = previous.current;
    previous.current = state.phase;
    if (!sounds || before === state.phase) return;
    if (state.phase === "recording") startTone();
    else if (before === "recording") stopTone();
  }, [state.phase, sounds]);

  // Report when the recording state has been painted, for latency metrics.
  // The second frame callback runs after the first frame was presented.
  useEffect(() => {
    if (session === null || !inTauri()) return;
    let inner = 0;
    const outer = requestAnimationFrame(() => {
      inner = requestAnimationFrame(() => {
        api.overlayRendered(session).catch(() => {});
      });
    });
    return () => {
      cancelAnimationFrame(outer);
      cancelAnimationFrame(inner);
    };
  }, [session]);

  return (
    <div className="flex h-screen w-screen items-center justify-center p-1">
      <OverlayPill
        state={state}
        levels={levels}
        onCopy={() => {
          api.copyTranscript().catch(() => {});
        }}
      />
    </div>
  );
}
