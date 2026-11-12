import type { Platform } from "../../store/shortcuts";
import { copyKey } from "./terminal-clipboard";

// Terminal's own keys, as Windows Terminal and the usual terminals have them
// (owner, 2026-11-13: "most terminal shortcuts do not work"). Everything else
// still goes to the agent unchanged; paste keys (Ctrl+Shift+V, Shift+Insert,
// Command-V) stay with the engine's paste event.

type Keys = Pick<KeyboardEvent, "key" | "code" | "ctrlKey" | "metaKey" | "altKey" | "shiftKey">;

/** Where a keyboard selection's moving end goes. */
export type Move = "left" | "right" | "up" | "down" | "home" | "end";

export type KeyAction =
  | { kind: "pass" }
  | { kind: "copy"; clear: boolean }
  | { kind: "newline" }
  | { kind: "select"; move: Move }
  | { kind: "selectAll" }
  | { kind: "scroll"; by: "line" | "page" | "edge"; up: boolean };

const PASS: KeyAction = { kind: "pass" };

const mods = (event: Keys) => ({ ctrl: event.ctrlKey, shift: event.shiftKey, alt: event.altKey, meta: event.metaKey });
const exactly = (event: Keys, want: Partial<ReturnType<typeof mods>>) => {
  const have = mods(event);
  return (["ctrl", "shift", "alt", "meta"] as const).every((key) => have[key] === !!want[key]);
};
const letter = (event: Keys, wanted: string) =>
  /^[\x21-\x7e]$/.test(event.key) ? event.key.toUpperCase() === wanted : event.code === `Key${wanted}`;

const MOVES: Record<string, Move> = { ArrowLeft: "left", ArrowRight: "right", ArrowUp: "up", ArrowDown: "down", Home: "home", End: "end" };

/**
 * What a key does in Terminal. `selection` says whether text is selected now.
 * - Copy: Ctrl+Shift+C (Command-C on macOS), Ctrl+Insert, and on Windows
 *   Ctrl+C while text is selected (then the selection clears); Ctrl+C without
 *   a selection stays the agent's interrupt.
 * - New line: Ctrl+Enter and Shift+Enter send a line feed (Ctrl+J), which
 *   Claude Code and Codex take as a new line in their input; Enter submits.
 * - Selection: Shift with an arrow, Home or End extends a keyboard selection
 *   from the cursor; Ctrl+Shift+A (Command-A on macOS) selects everything.
 * - Scrolling: Shift+Page Up/Down by a page, Ctrl+Shift+Up/Down by a line,
 *   Ctrl+Shift+Home/End to the top or the bottom.
 */
export function terminalKey(event: Keys, on: Platform, selection: boolean): KeyAction {
  const mac = on === "macos";
  if (copyKey(event, on)) return { kind: "copy", clear: false };
  if (!mac && event.key === "Insert" && exactly(event, { ctrl: true })) return selection ? { kind: "copy", clear: false } : PASS;
  if (on === "windows" && selection && exactly(event, { ctrl: true }) && letter(event, "C")) return { kind: "copy", clear: true };
  if (event.key === "Enter" && (exactly(event, { ctrl: true }) || exactly(event, { shift: true }))) return { kind: "newline" };
  const move = MOVES[event.key];
  if (move && exactly(event, { shift: true })) return { kind: "select", move };
  if (letter(event, "A") && (mac ? exactly(event, { meta: true }) : exactly(event, { ctrl: true, shift: true }))) return { kind: "selectAll" };
  if ((event.key === "PageUp" || event.key === "PageDown") && exactly(event, { shift: true })) {
    return { kind: "scroll", by: "page", up: event.key === "PageUp" };
  }
  if (exactly(event, { ctrl: true, shift: true })) {
    if (event.key === "ArrowUp" || event.key === "ArrowDown") return { kind: "scroll", by: "line", up: event.key === "ArrowUp" };
    if (event.key === "Home" || event.key === "End") return { kind: "scroll", by: "edge", up: event.key === "Home" };
  }
  return PASS;
}

/** A cell of the buffer: column x (0 to cols) and buffer line y. */
export interface Cell { x: number; y: number }

/**
 * The keyboard selection's moving end after `move`: by a cell (wrapping across
 * lines), by a line, or to the start or the end of its line's text. It stays
 * inside the buffer.
 */
export function moveFocus(focus: Cell, move: Move, cols: number, lines: number, lineLength: (y: number) => number): Cell {
  const last = Math.max(0, lines - 1);
  const at = (offset: number): Cell => {
    const bounded = Math.max(0, Math.min(offset, lines * cols));
    return { x: bounded % cols, y: Math.min(last, Math.floor(bounded / cols)) };
  };
  switch (move) {
    case "left": return at(focus.y * cols + focus.x - 1);
    case "right": return at(focus.y * cols + focus.x + 1);
    case "up": return { x: focus.x, y: Math.max(0, focus.y - 1) };
    case "down": return { x: focus.x, y: Math.min(last, focus.y + 1) };
    case "home": return { x: 0, y: focus.y };
    case "end": return { x: Math.min(cols, Math.max(0, lineLength(focus.y))), y: focus.y };
  }
}

/** xterm's `select(column, row, length)` for the cells between both ends, or none when they meet. */
export function selectionRange(anchor: Cell, focus: Cell, cols: number): { column: number; row: number; length: number } | undefined {
  const a = anchor.y * cols + anchor.x, f = focus.y * cols + focus.x;
  if (a === f) return undefined;
  const start = Math.min(a, f);
  return { column: start % cols, row: Math.floor(start / cols), length: Math.abs(f - a) };
}
