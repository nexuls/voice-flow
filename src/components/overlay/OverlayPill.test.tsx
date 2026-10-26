import { render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { describe, expect, it, vi } from "vitest";
import type { StateView } from "@/types/ipc";
import { OverlayPill } from "./OverlayPill";

const renderPill = (state: StateView, onCopy?: () => void) =>
  render(<OverlayPill state={state} levels={[0.2, 0.5, 0.9]} onCopy={onCopy} />);

describe("OverlayPill", () => {
  it("renders nothing when idle", () => {
    const { container } = renderPill({ phase: "idle" });
    expect(container).toBeEmptyDOMElement();
  });

  it("shows the waveform while recording", () => {
    renderPill({ phase: "recording", session: 1 });
    expect(screen.getByRole("status", { name: "Recording" })).toBeInTheDocument();
    expect(screen.getAllByTestId("waveform-bar").length).toBeGreaterThan(0);
  });

  it("shows progress while processing and inserting", () => {
    const { rerender } = renderPill({ phase: "processing", session: 1 });
    expect(screen.getByText("Transcribing…")).toBeInTheDocument();
    rerender(<OverlayPill state={{ phase: "inserting", session: 1 }} levels={[]} />);
    expect(screen.getByText("Inserting…")).toBeInTheDocument();
  });

  it("distinguishes pasted from copied success", () => {
    const { rerender } = renderPill({ phase: "success", session: 1, outcome: "pasted" });
    expect(screen.getByText("Inserted")).toBeInTheDocument();
    rerender(
      <OverlayPill
        state={{ phase: "success", session: 1, outcome: "copiedToClipboard" }}
        levels={[]}
      />,
    );
    expect(screen.getByText(/copied/i)).toBeInTheDocument();
  });

  it("offers manual copy only when a transcript is retained", async () => {
    const onCopy = vi.fn();
    const error = { kind: "insertion" as const, message: "Could not paste" };
    const { rerender } = renderPill(
      { phase: "error", session: 1, error, hasTranscript: false },
      onCopy,
    );
    expect(screen.queryByRole("button")).not.toBeInTheDocument();
    rerender(
      <OverlayPill
        state={{ phase: "error", session: 1, error, hasTranscript: true }}
        levels={[]}
        onCopy={onCopy}
      />,
    );
    await userEvent.click(screen.getByRole("button", { name: /copy text/i }));
    expect(onCopy).toHaveBeenCalledOnce();
  });
});
