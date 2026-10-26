import { fireEvent, render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { describe, expect, it, vi } from "vitest";
import { TimingsTable } from "@/components/diagnostics/TimingsTable";
import type { ModelInfo } from "@/types/ipc";
import { formatBytes, modelLabel } from "./ModelSection";
import { ShortcutRecorder } from "./ShortcutRecorder";

const model = (over: Partial<ModelInfo>): ModelInfo => ({
  id: "whistle",
  name: "Cactus Whistle",
  size: 16_919_407,
  license: "Apache-2.0",
  source: "https://example.invalid",
  status: { state: "notInstalled" },
  installing: false,
  loaded: false,
  ...over,
});

describe("model helpers", () => {
  it("formats sizes", () => {
    expect(formatBytes(500)).toBe("500 B");
    expect(formatBytes(16_919_407)).toBe("16.1 MB");
  });
  it("labels every model state", () => {
    expect(modelLabel(model({})).text).toBe("Not installed");
    expect(modelLabel(model({ installing: true })).text).toBe("Installing");
    const installed = { state: "installed" as const, path: "/m", size: 1 };
    expect(modelLabel(model({ status: installed })).text).toBe("Loading");
    expect(modelLabel(model({ status: installed, loaded: true }))).toEqual({
      text: "Ready",
      tone: "ok",
    });
    expect(modelLabel(model({ status: { state: "invalid", path: "/m", reason: "x" } })).tone).toBe(
      "warn",
    );
  });
});

describe("ShortcutRecorder", () => {
  it("records a new shortcut from the keyboard", async () => {
    const onChange = vi.fn();
    render(<ShortcutRecorder value="Ctrl+Space" onChange={onChange} />);
    await userEvent.click(screen.getByRole("button", { name: "Change" }));
    const box = screen.getByRole("button", { name: /press the new shortcut/i });
    fireEvent.keyDown(box, { code: "ControlLeft", ctrlKey: true });
    expect(onChange).not.toHaveBeenCalled();
    fireEvent.keyDown(box, { code: "KeyD", ctrlKey: true, altKey: true });
    expect(onChange).toHaveBeenCalledWith("Ctrl+Alt+D");
  });

  it("explains invalid shortcuts and cancels with Escape", async () => {
    const onChange = vi.fn();
    render(<ShortcutRecorder value="Ctrl+Space" onChange={onChange} />);
    await userEvent.click(screen.getByRole("button", { name: "Change" }));
    const box = screen.getByRole("button", { name: /press the new shortcut/i });
    fireEvent.keyDown(box, { code: "KeyA" });
    expect(screen.getByText(/add a modifier/i)).toBeInTheDocument();
    fireEvent.keyDown(box, { code: "Escape" });
    expect(screen.getByRole("button", { name: /shortcut ctrl\+space/i })).toBeInTheDocument();
    expect(onChange).not.toHaveBeenCalled();
  });

  it("does not report an unchanged shortcut", async () => {
    const onChange = vi.fn();
    render(<ShortcutRecorder value="Ctrl+Space" onChange={onChange} />);
    await userEvent.click(screen.getByRole("button", { name: "Change" }));
    fireEvent.keyDown(screen.getByRole("button", { name: /press the new shortcut/i }), {
      code: "Space",
      ctrlKey: true,
    });
    expect(onChange).not.toHaveBeenCalled();
  });
});

describe("TimingsTable", () => {
  it("renders timings and placeholders", () => {
    render(
      <TimingsTable
        rows={[
          {
            session: 3,
            captureOpenMs: 69.4,
            overlayVisibleMs: 108,
            recordedMs: 8448,
            finalizeMs: 14,
            inferenceMs: 892,
            engineTtftMs: 784,
            insertMs: null,
            releaseToInsertMs: 908,
          },
        ]}
      />,
    );
    const row = screen.getAllByRole("row")[1];
    expect(row).toHaveTextContent("69 ms");
    expect(row).toHaveTextContent("8.45 s");
    expect(row).toHaveTextContent("—");
  });
});
