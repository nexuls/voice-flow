import { render, screen } from "@testing-library/react";
import { describe, expect, it } from "vitest";
import { pushLevel } from "@/hooks/useAudioLevels";
import { levelToHeight, Waveform } from "./Waveform";

describe("Waveform", () => {
  it("always renders the requested number of bars", () => {
    render(<Waveform levels={[0.5]} bars={10} />);
    expect(screen.getAllByTestId("waveform-bar")).toHaveLength(10);
  });

  it("puts the newest level on the right", () => {
    render(<Waveform levels={[0, 1]} bars={4} />);
    const bars = screen.getAllByTestId("waveform-bar");
    expect(bars[3]).toHaveStyle({ transform: "scaleY(1.000)" });
    expect(bars[0]).toHaveStyle({ transform: "scaleY(0.080)" });
  });

  it("maps levels with a floor and clamps", () => {
    expect(levelToHeight(-1)).toBe(0.08);
    expect(levelToHeight(0)).toBe(0.08);
    expect(levelToHeight(0.25)).toBeCloseTo(0.5);
    expect(levelToHeight(4)).toBe(1);
  });
});

describe("pushLevel", () => {
  it("keeps a bounded history without mutating input", () => {
    const start = [1, 2, 3];
    const next = pushLevel(start, 4, 3);
    expect(next).toEqual([2, 3, 4]);
    expect(start).toEqual([1, 2, 3]);
  });
});
