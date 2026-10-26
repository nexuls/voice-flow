import { useCallback, useEffect, useState } from "react";
import { TransitionLog } from "@/components/diagnostics/TransitionLog";
import { useTauriEvent } from "@/hooks/useTauriEvent";
import { api, inTauri, on } from "@/lib/ipc";
import type { ShortcutStatus, Transition } from "@/types/ipc";

const MAX_TRANSITIONS = 50;

export default function App() {
  const [status, setStatus] = useState<ShortcutStatus | null>(null);
  const [transitions, setTransitions] = useState<Transition[]>([]);

  useEffect(() => {
    if (inTauri()) api.shortcutStatus().then(setStatus);
  }, []);

  useTauriEvent(on.shortcutStatus, setStatus);
  useTauriEvent(
    on.transition,
    useCallback((t: Transition) => {
      setTransitions((prev) => [...prev, t].slice(-MAX_TRANSITIONS));
    }, []),
  );

  return (
    <main className="mx-auto max-w-2xl space-y-6 p-6">
      <header>
        <h1 className="text-lg font-semibold">VoiceFlow</h1>
        <p className="text-sm text-muted-foreground">Diagnostic mode: no audio is recorded.</p>
      </header>
      <section aria-labelledby="shortcut-heading" className="space-y-1 text-sm">
        <h2 id="shortcut-heading" className="font-medium">
          Shortcut
        </h2>
        {status ? (
          <p>
            {status.accelerator ?? "not registered"}
            {status.backend && <span className="text-muted-foreground"> via {status.backend}</span>}
            <span className="text-muted-foreground"> ({status.session} session)</span>
          </p>
        ) : (
          <p className="text-muted-foreground">Registering…</p>
        )}
        {status?.error && (
          <p role="alert" className="text-destructive">
            {status.error}
          </p>
        )}
      </section>
      <section aria-labelledby="transitions-heading" className="space-y-2">
        <h2 id="transitions-heading" className="text-sm font-medium">
          Transitions
        </h2>
        <TransitionLog transitions={transitions} />
      </section>
    </main>
  );
}
