import { render, screen } from "@testing-library/react";
import { describe, expect, it } from "vitest";
import { TransitionLog } from "./TransitionLog";

describe("TransitionLog", () => {
  it("shows a hint when empty", () => {
    render(<TransitionLog transitions={[]} />);
    expect(screen.getByText(/no transitions yet/i)).toBeInTheDocument();
  });

  it("renders transitions in order", () => {
    render(
      <TransitionLog
        transitions={[
          {
            seq: 1,
            atMs: 1200,
            input: "shortcutPressed",
            from: { phase: "idle" },
            to: { phase: "recording", session: 1 },
          },
          {
            seq: 2,
            atMs: 2500,
            input: "shortcutReleased",
            from: { phase: "recording", session: 1 },
            to: { phase: "processing", session: 1 },
          },
        ]}
      />,
    );
    const items = screen.getAllByRole("listitem");
    expect(items).toHaveLength(2);
    expect(items[0]).toHaveTextContent("1.200s");
    expect(items[1]).toHaveTextContent("recording → processing");
  });
});
