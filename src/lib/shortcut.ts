/**
 * Converts keyboard events into accelerator strings understood by the
 * backend (`src-tauri/src/shortcuts/accelerator.rs`), e.g. "Ctrl+Shift+D".
 */

const NAMED: Record<string, string> = {
  Space: "Space",
  Enter: "Enter",
  Tab: "Tab",
  Backquote: "`",
  Minus: "-",
  Equal: "=",
  BracketLeft: "[",
  BracketRight: "]",
  Backslash: "\\",
  Semicolon: ";",
  Quote: "'",
  Comma: ",",
  Period: ".",
  Slash: "/",
  Insert: "Insert",
  Delete: "Delete",
  Home: "Home",
  End: "End",
  PageUp: "PageUp",
  PageDown: "PageDown",
  ArrowUp: "Up",
  ArrowDown: "Down",
  ArrowLeft: "Left",
  ArrowRight: "Right",
  ScrollLock: "ScrollLock",
  Pause: "Pause",
};

const MODIFIER_CODES = new Set([
  "ControlLeft",
  "ControlRight",
  "AltLeft",
  "AltRight",
  "ShiftLeft",
  "ShiftRight",
  "MetaLeft",
  "MetaRight",
  "OSLeft",
  "OSRight",
]);

/** Backend key name for a `KeyboardEvent.code`, or null if unsupported. */
export function codeToKeyName(code: string): string | null {
  if (/^Key[A-Z]$/.test(code)) return code.slice(3);
  if (/^Digit[0-9]$/.test(code)) return code.slice(5);
  const f = /^F([1-9]|1[0-9]|2[0-4])$/.exec(code);
  if (f) return code;
  return NAMED[code] ?? null;
}

export interface KeyLike {
  code: string;
  ctrlKey: boolean;
  altKey: boolean;
  shiftKey: boolean;
  metaKey: boolean;
}

export type CaptureResult =
  | { kind: "pending" }
  | { kind: "accelerator"; value: string }
  | { kind: "invalid"; reason: string };

/** Builds an accelerator from a keydown event. */
export function eventToAccelerator(e: KeyLike): CaptureResult {
  if (MODIFIER_CODES.has(e.code)) return { kind: "pending" };
  const key = codeToKeyName(e.code);
  if (!key) return { kind: "invalid", reason: "This key cannot be used" };
  const mods = [
    e.ctrlKey && "Ctrl",
    e.altKey && "Alt",
    e.shiftKey && "Shift",
    e.metaKey && "Super",
  ].filter(Boolean) as string[];
  const bareAllowed = /^F\d+$/.test(key) || key === "Pause" || key === "ScrollLock";
  if (mods.length === 0 && !bareAllowed) {
    return { kind: "invalid", reason: "Add a modifier such as Ctrl or Alt" };
  }
  return { kind: "accelerator", value: [...mods, key].join("+") };
}

/** Display form for the current platform (Super is Cmd on macOS). */
export function displayAccelerator(accel: string, isMac: boolean): string {
  return isMac ? accel.replace("Super", "⌘").replace("Alt", "⌥") : accel;
}
