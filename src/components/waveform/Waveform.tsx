import { memo } from "react";
import { cn } from "@/lib/utils";

interface Props {
  /** Recent levels in [0, 1], oldest first. */
  levels: readonly number[];
  bars?: number;
  className?: string;
}

/** Perceptual mapping so quiet speech is still visible. */
export function levelToHeight(level: number): number {
  const clamped = Math.min(1, Math.max(0, level));
  return Math.max(0.08, Math.sqrt(clamped));
}

/**
 * Bar visualizer of recent audio levels. The newest level is on the right.
 * Purely presentational: the backend computes RMS and throttles updates.
 */
export const Waveform = memo(function Waveform({ levels, bars = 24, className }: Props) {
  const recent = levels.slice(-bars);
  const padded = Array.from({ length: bars - recent.length }, () => 0).concat(recent);
  return (
    <div className={cn("flex h-5 items-center gap-[2px]", className)} aria-hidden="true">
      {padded.map((level, i) => (
        <span
          // Fixed-length positional list; position is the identity.
          // biome-ignore lint/suspicious/noArrayIndexKey: bars are positional
          key={i}
          data-testid="waveform-bar"
          className="h-full w-[3px] origin-center rounded-full bg-pill-fg/90 transition-transform duration-75 will-change-transform motion-reduce:transition-none"
          style={{ transform: `scaleY(${levelToHeight(level).toFixed(3)})` }}
        />
      ))}
    </div>
  );
});
