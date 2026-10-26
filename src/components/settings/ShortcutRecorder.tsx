import { useEffect, useRef, useState } from "react";
import { Button } from "@/components/ui/button";
import { displayAccelerator, eventToAccelerator } from "@/lib/shortcut";
import { cn } from "@/lib/utils";

const isMac = typeof navigator !== "undefined" && /mac/i.test(navigator.userAgent);

interface Props {
  id?: string;
  value: string;
  disabled?: boolean;
  onChange: (accelerator: string) => void;
}

/** Shows the current shortcut and records a new one from the keyboard. */
export function ShortcutRecorder({ id, value, disabled, onChange }: Props) {
  const [recording, setRecording] = useState(false);
  const [hint, setHint] = useState<string | null>(null);
  const boxRef = useRef<HTMLButtonElement>(null);

  useEffect(() => {
    if (recording) boxRef.current?.focus();
  }, [recording]);

  const onKeyDown = (e: React.KeyboardEvent) => {
    if (!recording) return;
    e.preventDefault();
    e.stopPropagation();
    if (e.code === "Escape") {
      setRecording(false);
      setHint(null);
      return;
    }
    const r = eventToAccelerator(e);
    if (r.kind === "invalid") setHint(r.reason);
    if (r.kind === "accelerator") {
      setRecording(false);
      setHint(null);
      if (r.value !== value) onChange(r.value);
    }
  };

  return (
    <div className="flex flex-col items-end gap-1">
      <div className="flex items-center gap-2">
        <button
          id={id}
          ref={boxRef}
          type="button"
          disabled={disabled}
          onClick={() => setRecording((r) => !r)}
          onKeyDown={onKeyDown}
          onBlur={() => setRecording(false)}
          aria-label={recording ? "Press the new shortcut" : `Shortcut ${value}. Click to change.`}
          className={cn(
            "min-w-36 rounded-md border bg-muted/40 px-3 py-1.5 font-mono text-sm outline-none focus-visible:ring-3 focus-visible:ring-ring/50",
            recording && "border-primary text-muted-foreground",
          )}
        >
          {recording ? "Press keys…" : displayAccelerator(value, isMac)}
        </button>
        {!recording && (
          <Button
            variant="outline"
            size="sm"
            disabled={disabled}
            onClick={() => setRecording(true)}
          >
            Change
          </Button>
        )}
      </div>
      <p className="h-4 text-xs text-muted-foreground" aria-live="polite">
        {recording ? (hint ?? "Hold modifiers, then press a key. Esc cancels.") : ""}
      </p>
    </div>
  );
}
