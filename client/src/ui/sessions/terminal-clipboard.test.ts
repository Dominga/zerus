import { describe, expect, it, vi } from "vitest";
import { clipboardKeyHandler, copyKey, terminalMenu } from "./terminal-clipboard";

const key = (key: string, code: string, mods: Partial<Record<"ctrlKey" | "metaKey" | "altKey" | "shiftKey", boolean>> = {}) =>
  ({ key, code, ctrlKey: false, metaKey: false, altKey: false, shiftKey: false, ...mods });

describe("Terminal's copy key (Qt's copyKey)", () => {
  it("is Ctrl+Shift+C on Linux and Windows and leaves Ctrl+C to the agent", () => {
    for (const on of ["linux", "windows"] as const) {
      expect(copyKey(key("C", "KeyC", { ctrlKey: true, shiftKey: true }), on)).toBe(true);
      expect(copyKey(key("c", "KeyC", { ctrlKey: true }), on)).toBe(false);
      expect(copyKey(key("C", "KeyC", { ctrlKey: true, shiftKey: true, altKey: true }), on)).toBe(false);
      expect(copyKey(key("V", "KeyV", { ctrlKey: true, shiftKey: true }), on)).toBe(false);
    }
  });

  it("is Command-C on macOS and leaves Control to the agent", () => {
    expect(copyKey(key("c", "KeyC", { metaKey: true }), "macos")).toBe(true);
    expect(copyKey(key("c", "KeyC", { ctrlKey: true }), "macos")).toBe(false);
    expect(copyKey(key("C", "KeyC", { metaKey: true, shiftKey: true }), "macos")).toBe(false);
  });

  it("reads the layout's letter, and the key position only for another script", () => {
    expect(copyKey(key("С", "KeyC", { ctrlKey: true, shiftKey: true }), "linux")).toBe(true);
    // Dvorak: the key that types J sits where QWERTY has C.
    expect(copyKey(key("J", "KeyC", { ctrlKey: true, shiftKey: true }), "linux")).toBe(false);
    expect(copyKey(key("C", "KeyI", { ctrlKey: true, shiftKey: true }), "linux")).toBe(true);
  });
});

describe("xterm's key handler", () => {
  const event = (type: string, ...init: Parameters<typeof key>) =>
    ({ ...key(...init), type, preventDefault: vi.fn() }) as unknown as KeyboardEvent & { preventDefault: ReturnType<typeof vi.fn> };

  it("copies once on keydown and keeps the copy key from the agent", () => {
    const copy = vi.fn();
    const handle = clipboardKeyHandler("linux", copy);
    const down = event("keydown", "C", "KeyC", { ctrlKey: true, shiftKey: true });
    expect(handle(down)).toBe(false);
    expect(down.preventDefault).toHaveBeenCalled();
    expect(handle(event("keyup", "C", "KeyC", { ctrlKey: true, shiftKey: true }))).toBe(false);
    expect(copy).toHaveBeenCalledTimes(1);
  });

  it("leaves Ctrl+C, Ctrl+V and the paste keys to xterm and the engine", () => {
    const copy = vi.fn();
    const handle = clipboardKeyHandler("linux", copy);
    for (const [k, code, mods] of [["c", "KeyC", { ctrlKey: true }], ["v", "KeyV", { ctrlKey: true }], ["V", "KeyV", { ctrlKey: true, shiftKey: true }],
      ["Insert", "Insert", { shiftKey: true }]] as const) {
      const down = event("keydown", k, code, mods);
      expect(handle(down)).toBe(true);
      expect(down.preventDefault).not.toHaveBeenCalled();
    }
    expect(copy).not.toHaveBeenCalled();
  });
});

describe("Terminal's right-click menu (Qt's contextMenuEvent)", () => {
  it("offers Copy selection only with a selection, and Paste", () => {
    const copy = vi.fn(), paste = vi.fn();
    const items = terminalMenu({ selection: true, mouseApp: false, shift: false, on: "linux" }, copy, paste)!;
    expect(items.map((item) => "label" in item && [item.label, item.kbd, !!item.disabled])).toEqual([
      ["Copy selection", "Ctrl+Shift+C", false], ["Paste", "Ctrl+Shift+V", false]]);
    for (const item of items) if ("action" in item) item.action?.();
    expect(copy).toHaveBeenCalledTimes(1);
    expect(paste).toHaveBeenCalledTimes(1);
    const empty = terminalMenu({ selection: false, mouseApp: false, shift: false, on: "macos" }, copy, paste)!;
    expect(empty[0]).toMatchObject({ label: "Copy selection", kbd: "⌘C", disabled: true, action: undefined });
    expect(empty[1]).toMatchObject({ label: "Paste", kbd: "⌘V" });
  });

  it("leaves the click to an agent that reads the mouse, unless Shift is held", () => {
    expect(terminalMenu({ selection: true, mouseApp: true, shift: false, on: "linux" }, vi.fn(), vi.fn())).toBeUndefined();
    expect(terminalMenu({ selection: true, mouseApp: true, shift: true, on: "linux" }, vi.fn(), vi.fn())).toHaveLength(2);
  });
});
