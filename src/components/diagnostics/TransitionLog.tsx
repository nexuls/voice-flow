import type { Transition } from "@/types/ipc";

interface Props {
  transitions: Transition[];
}

/** Lists recent state transitions. Shows phases and timings only, never content. */
export function TransitionLog({ transitions }: Props) {
  if (transitions.length === 0) {
    return <p className="text-sm text-muted-foreground">No transitions yet. Hold the shortcut.</p>;
  }
  return (
    <ol className="space-y-1 font-mono text-xs" aria-label="State transitions">
      {transitions.map((t) => (
        <li key={t.seq} className="flex gap-3">
          <span className="w-16 shrink-0 text-right tabular-nums text-muted-foreground">
            {(t.atMs / 1000).toFixed(3)}s
          </span>
          <span className="w-36 shrink-0 text-muted-foreground">{t.input}</span>
          <span>
            {t.from.phase} → <strong>{t.to.phase}</strong>
          </span>
        </li>
      ))}
    </ol>
  );
}
