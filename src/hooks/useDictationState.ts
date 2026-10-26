import { useCallback, useEffect, useState } from "react";
import { api, inTauri, on } from "@/lib/ipc";
import type { StateView, Transition } from "@/types/ipc";
import { useTauriEvent } from "./useTauriEvent";

/** Current workflow state, kept in sync with backend transitions. */
export function useDictationState(): StateView {
  const [state, setState] = useState<StateView>({ phase: "idle" });

  useEffect(() => {
    if (inTauri()) api.dictationState().then(setState);
  }, []);

  useTauriEvent(
    on.transition,
    useCallback((t: Transition) => setState(t.to), []),
  );
  return state;
}
