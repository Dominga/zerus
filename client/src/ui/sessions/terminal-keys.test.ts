import { describe, expect, it } from "vitest";
import { moveFocus, selectionRange, terminalKey } from "./terminal-keys";

type Mods = Partial<Record<"ctrlKey" | "metaKey" | "altKey" | "shiftKey", boolean>>;
const key = (key: string, code: string, mods: Mods = {}) =>
  ({ key, code, ctrlKey: false, metaKey: false, altKey: false, shiftKey: false, ...mods });

describe("Terminal's own keys (owner, 2026-11-13)", () => {
  it("copies with Ctrl+Shift+C, Ctrl+Insert, and Ctrl+C over a selection on Windows", () => {
    for (const on of ["windows", "linux"] as const) {
      expect(terminalKey(key("C", "KeyC", { ctrlKey: true, shiftKey: true }), on, false)).toEqual({ kind: "copy", clear: false });
      expect(terminalKey(key("Insert", "Insert", { ctrlKey: true }), on, true)).toEqual({ kind: "copy", clear: false });
      expect(terminalKey(key("Insert", "Insert", { ctrlKey: true }), on, false)).toEqual({ kind: "pass" });
    }
    expect(terminalKey(key("c", "KeyC", { ctrlKey: true }), "windows", true)).toEqual({ kind: "copy", clear: true });
    // Without a selection, and on Linux, Ctrl+C stays the agent's interrupt.
    expect(terminalKey(key("c", "KeyC", { ctrlKey: true }), "windows", false)).toEqual({ kind: "pass" });
    expect(terminalKey(key("c", "KeyC", { ctrlKey: true }), "linux", true)).toEqual({ kind: "pass" });
    // Another layout: the key's position.
    expect(terminalKey(key("с", "KeyC", { ctrlKey: true }), "windows", true)).toEqual({ kind: "copy", clear: true });
    // Command-C on macOS; Control-C is the agent's there.
    expect(terminalKey(key("c", "KeyC", { metaKey: true }), "macos", true)).toEqual({ kind: "copy", clear: false });
    expect(terminalKey(key("c", "KeyC", { ctrlKey: true }), "macos", true)).toEqual({ kind: "pass" });
  });

  it("sends a new line for Ctrl+Enter and Shift+Enter and leaves Enter to submit", () => {
    for (const on of ["windows", "linux", "macos"] as const) {
      expect(terminalKey(key("Enter", "Enter", { ctrlKey: true }), on, false)).toEqual({ kind: "newline" });
      expect(terminalKey(key("Enter", "Enter", { shiftKey: true }), on, false)).toEqual({ kind: "newline" });
      expect(terminalKey(key("Enter", "Enter"), on, false)).toEqual({ kind: "pass" });
      // Ctrl+Shift+Enter is a window command and Alt+Enter the agent's.
      expect(terminalKey(key("Enter", "Enter", { ctrlKey: true, shiftKey: true }), on, false)).toEqual({ kind: "pass" });
      expect(terminalKey(key("Enter", "Enter", { altKey: true }), on, false)).toEqual({ kind: "pass" });
    }
  });

  it("selects with Shift and the arrows, Home and End, and selects everything", () => {
    for (const [name, move] of [["ArrowLeft", "left"], ["ArrowRight", "right"], ["ArrowUp", "up"], ["ArrowDown", "down"], ["Home", "home"], ["End", "end"]] as const) {
      expect(terminalKey(key(name, name, { shiftKey: true }), "windows", false)).toEqual({ kind: "select", move });
      // Without Shift the arrows stay the agent's.
      expect(terminalKey(key(name, name), "windows", false)).toEqual({ kind: "pass" });
    }
    expect(terminalKey(key("ArrowLeft", "ArrowLeft", { shiftKey: true, ctrlKey: true }), "windows", false)).toEqual({ kind: "pass" });
    expect(terminalKey(key("A", "KeyA", { ctrlKey: true, shiftKey: true }), "windows", false)).toEqual({ kind: "selectAll" });
    expect(terminalKey(key("a", "KeyA", { ctrlKey: true }), "windows", false)).toEqual({ kind: "pass" });
    expect(terminalKey(key("a", "KeyA", { metaKey: true }), "macos", false)).toEqual({ kind: "selectAll" });
  });

  it("scrolls by a page, a line or to an edge", () => {
    expect(terminalKey(key("PageUp", "PageUp", { shiftKey: true }), "windows", false)).toEqual({ kind: "scroll", by: "page", up: true });
    expect(terminalKey(key("PageDown", "PageDown", { shiftKey: true }), "windows", false)).toEqual({ kind: "scroll", by: "page", up: false });
    expect(terminalKey(key("ArrowUp", "ArrowUp", { ctrlKey: true, shiftKey: true }), "windows", false)).toEqual({ kind: "scroll", by: "line", up: true });
    expect(terminalKey(key("End", "End", { ctrlKey: true, shiftKey: true }), "windows", false)).toEqual({ kind: "scroll", by: "edge", up: false });
    // Page keys without Shift are the agent's.
    expect(terminalKey(key("PageUp", "PageUp"), "windows", false)).toEqual({ kind: "pass" });
  });

  it("leaves typing and the agent's chords alone", () => {
    for (const k of [key("a", "KeyA"), key("Escape", "Escape"), key("Tab", "Tab", { shiftKey: true }), key("v", "KeyV", { ctrlKey: true }), key("l", "KeyL", { ctrlKey: true })]) {
      expect(terminalKey(k, "windows", true)).toEqual({ kind: "pass" });
    }
  });
});

describe("the keyboard selection", () => {
  const cols = 10, lines = 5, length = (y: number) => [4, 10, 0, 7, 2][y];
  it("moves by a cell across lines, by a line, and to its line's text ends, inside the buffer", () => {
    expect(moveFocus({ x: 0, y: 1 }, "left", cols, lines, length)).toEqual({ x: 9, y: 0 });
    expect(moveFocus({ x: 9, y: 0 }, "right", cols, lines, length)).toEqual({ x: 0, y: 1 });
    expect(moveFocus({ x: 0, y: 0 }, "left", cols, lines, length)).toEqual({ x: 0, y: 0 });
    expect(moveFocus({ x: 3, y: 0 }, "up", cols, lines, length)).toEqual({ x: 3, y: 0 });
    expect(moveFocus({ x: 3, y: 4 }, "down", cols, lines, length)).toEqual({ x: 3, y: 4 });
    expect(moveFocus({ x: 3, y: 2 }, "down", cols, lines, length)).toEqual({ x: 3, y: 3 });
    expect(moveFocus({ x: 3, y: 3 }, "home", cols, lines, length)).toEqual({ x: 0, y: 3 });
    expect(moveFocus({ x: 0, y: 3 }, "end", cols, lines, length)).toEqual({ x: 7, y: 3 });
  });
  it("selects the cells between both ends, in either direction", () => {
    expect(selectionRange({ x: 2, y: 1 }, { x: 3, y: 1 }, cols)).toEqual({ column: 2, row: 1, length: 1 });
    expect(selectionRange({ x: 2, y: 1 }, { x: 8, y: 0 }, cols)).toEqual({ column: 8, row: 0, length: 4 });
    expect(selectionRange({ x: 2, y: 1 }, { x: 2, y: 3 }, cols)).toEqual({ column: 2, row: 1, length: 20 });
    expect(selectionRange({ x: 2, y: 1 }, { x: 2, y: 1 }, cols)).toBeUndefined();
  });
});
