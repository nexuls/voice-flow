import { describe, expect, it } from "vitest";
import { codeToKeyName, displayAccelerator, eventToAccelerator, type KeyLike } from "./shortcut";

const ev = (code: string, mods: Partial<KeyLike> = {}): KeyLike => ({
  code,
  ctrlKey: false,
  altKey: false,
  shiftKey: false,
  metaKey: false,
  ...mods,
});

describe("codeToKeyName", () => {
  it("maps letters, digits, function and named keys", () => {
    expect(codeToKeyName("KeyD")).toBe("D");
    expect(codeToKeyName("Digit7")).toBe("7");
    expect(codeToKeyName("F24")).toBe("F24");
    expect(codeToKeyName("Space")).toBe("Space");
    expect(codeToKeyName("ArrowUp")).toBe("Up");
    expect(codeToKeyName("Backslash")).toBe("\\");
  });
  it("rejects unsupported keys", () => {
    expect(codeToKeyName("F25")).toBeNull();
    expect(codeToKeyName("NumpadEnter")).toBeNull();
    expect(codeToKeyName("IntlBackslash")).toBeNull();
  });
});

describe("eventToAccelerator", () => {
  it("builds the default shortcut in canonical modifier order", () => {
    expect(eventToAccelerator(ev("Space", { ctrlKey: true }))).toEqual({
      kind: "accelerator",
      value: "Ctrl+Space",
    });
    expect(
      eventToAccelerator(ev("KeyD", { shiftKey: true, ctrlKey: true, metaKey: true })),
    ).toEqual({ kind: "accelerator", value: "Ctrl+Shift+Super+D" });
  });
  it("waits while only modifiers are held", () => {
    expect(eventToAccelerator(ev("ControlLeft", { ctrlKey: true }))).toEqual({ kind: "pending" });
  });
  it("requires a modifier except for function keys", () => {
    expect(eventToAccelerator(ev("KeyA")).kind).toBe("invalid");
    expect(eventToAccelerator(ev("Space")).kind).toBe("invalid");
    expect(eventToAccelerator(ev("F9"))).toEqual({ kind: "accelerator", value: "F9" });
  });
});

describe("displayAccelerator", () => {
  it("uses macOS symbols on Mac only", () => {
    expect(displayAccelerator("Alt+Super+K", true)).toBe("⌥+⌘+K");
    expect(displayAccelerator("Alt+Super+K", false)).toBe("Alt+Super+K");
  });
});
