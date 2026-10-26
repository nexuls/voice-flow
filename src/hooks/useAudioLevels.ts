import { useCallback, useEffect, useState } from "react";
import { on } from "@/lib/ipc";
import type { AudioLevel } from "@/types/ipc";
import { useTauriEvent } from "./useTauriEvent";

export const LEVEL_HISTORY = 32;

/** Appends a level to a bounded history, oldest first. */
export function pushLevel(history: readonly number[], level: number, max = LEVEL_HISTORY) {
  const next = history.length >= max ? history.slice(history.length - max + 1) : history.slice();
  next.push(level);
  return next;
}

/** Rolling history of microphone levels. Cleared whenever `active` turns on. */
export function useAudioLevels(active: boolean): readonly number[] {
  const [levels, setLevels] = useState<number[]>([]);

  useEffect(() => {
    if (active) setLevels([]);
  }, [active]);

  useTauriEvent(
    on.audioLevel,
    useCallback(
      (l: AudioLevel) => {
        if (active) setLevels((prev) => pushLevel(prev, l.level));
      },
      [active],
    ),
  );
  return levels;
}
