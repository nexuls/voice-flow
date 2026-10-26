import { useEffect } from "react";
import { OverlayPill } from "@/components/overlay/OverlayPill";
import { useAudioLevels } from "@/hooks/useAudioLevels";
import { useDictationState } from "@/hooks/useDictationState";
import { api, inTauri } from "@/lib/ipc";

/** Root of the overlay window. The window itself is managed natively. */
export function OverlayApp() {
  const state = useDictationState();
  const recording = state.phase === "recording";
  const levels = useAudioLevels(recording);
  const session = recording ? state.session : null;

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
