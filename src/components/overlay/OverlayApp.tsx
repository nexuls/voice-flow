import { OverlayPill } from "@/components/overlay/OverlayPill";
import { useAudioLevels } from "@/hooks/useAudioLevels";
import { useDictationState } from "@/hooks/useDictationState";

/** Root of the overlay window. The window itself is managed natively. */
export function OverlayApp() {
  const state = useDictationState();
  const levels = useAudioLevels(state.phase === "recording");
  return (
    <div className="flex h-screen w-screen items-center justify-center p-1">
      <OverlayPill state={state} levels={levels} />
    </div>
  );
}
