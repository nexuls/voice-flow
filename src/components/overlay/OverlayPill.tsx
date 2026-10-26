import { AlertCircle, Check, Copy, Loader2, Mic } from "lucide-react";
import type { ReactNode } from "react";
import { Waveform } from "@/components/waveform/Waveform";
import { cn } from "@/lib/utils";
import type { StateView } from "@/types/ipc";

interface Props {
  state: StateView;
  levels: readonly number[];
  onCopy?: () => void;
}

function Row({ children, label }: { children: ReactNode; label: string }) {
  return (
    <div className="flex items-center gap-2.5" role="status" aria-live="polite" aria-label={label}>
      {children}
    </div>
  );
}

function Label({ children, muted }: { children: ReactNode; muted?: boolean }) {
  return (
    <span className={cn("text-[13px] font-medium leading-none", muted && "text-pill-muted")}>
      {children}
    </span>
  );
}

const PASTE_HINT =
  typeof navigator !== "undefined" && /mac/i.test(navigator.userAgent) ? "⌘V" : "Ctrl+V";

/** Content of the floating dictation pill for each workflow phase. */
export function OverlayPill({ state, levels, onCopy }: Props) {
  if (state.phase === "idle") return null;

  let content: ReactNode;
  switch (state.phase) {
    case "recording":
      content = (
        <Row label="Recording">
          <span className="relative flex size-6 items-center justify-center rounded-full bg-rec/20">
            <Mic className="size-3.5 text-rec" strokeWidth={2.5} />
            <span className="vf-pulse absolute right-0.5 top-0.5 size-1.5 rounded-full bg-rec" />
          </span>
          <Waveform levels={levels} />
        </Row>
      );
      break;
    case "processing":
      content = (
        <Row label="Transcribing">
          <Loader2 className="size-4 animate-spin text-pill-muted motion-reduce:animate-none" />
          <Label muted>Transcribing…</Label>
        </Row>
      );
      break;
    case "inserting":
      content = (
        <Row label="Inserting text">
          <Loader2 className="size-4 animate-spin text-pill-muted motion-reduce:animate-none" />
          <Label muted>Inserting…</Label>
        </Row>
      );
      break;
    case "success":
      content = (
        <Row label={state.outcome === "pasted" ? "Inserted" : "Copied to clipboard"}>
          <Check className="size-4 text-ok" strokeWidth={2.5} />
          <Label>
            {state.outcome === "pasted" ? "Inserted" : `Copied — paste with ${PASTE_HINT}`}
          </Label>
        </Row>
      );
      break;
    case "error": {
      const subtle = state.error.kind === "noSpeech" || state.error.kind === "tooShort";
      content = (
        <Row label={`Error: ${state.error.message}`}>
          <AlertCircle className={cn("size-4", subtle ? "text-pill-muted" : "text-warn")} />
          <Label muted={subtle}>{state.error.message}</Label>
          {state.hasTranscript && onCopy && (
            <button
              type="button"
              onClick={onCopy}
              className="ml-1 inline-flex items-center gap-1 rounded-full bg-white/10 px-2.5 py-1 text-xs font-medium hover:bg-white/15 focus-visible:outline-2 focus-visible:outline-pill-fg"
            >
              <Copy className="size-3" />
              Copy text
            </button>
          )}
        </Row>
      );
      break;
    }
  }

  return (
    <div
      key={state.phase}
      className="vf-pill-in inline-flex h-11 max-w-full items-center rounded-full border border-pill-border bg-pill px-4 text-pill-fg shadow-[0_4px_14px_rgb(0_0_0/0.35)] backdrop-blur-md"
    >
      {content}
    </div>
  );
}
