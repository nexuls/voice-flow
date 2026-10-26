import type { UnlistenFn } from "@tauri-apps/api/event";
import { useEffect, useRef } from "react";
import { inTauri } from "@/lib/ipc";

/**
 * Subscribes to a backend event for the component's lifetime. Handles the
 * async unlisten race when the component unmounts before `listen` resolves.
 */
export function useTauriEvent<T>(
  subscribe: (cb: (payload: T) => void) => Promise<UnlistenFn>,
  handler: (payload: T) => void,
) {
  const handlerRef = useRef(handler);
  handlerRef.current = handler;

  useEffect(() => {
    if (!inTauri()) return;
    let unlisten: UnlistenFn | undefined;
    let disposed = false;
    subscribe((p) => handlerRef.current(p)).then((fn) => {
      if (disposed) fn();
      else unlisten = fn;
    });
    return () => {
      disposed = true;
      unlisten?.();
    };
  }, [subscribe]);
}
