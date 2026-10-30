import { describe, expect, it } from "vitest";
import { parseSequence } from "../../store/shortcuts";
import { goToKey, goToScore, rankGoTo, recentSessionIds, rememberRecentSession, visibleGoToRows, GoToKind, type GoToItem, type GoToKeyState } from "./goTo";

const item = (kind: GoToKind, id: string, title = id, recency = -1): GoToItem => ({ kind, id, host: "", title, detail: "", recency });
const press = (key: string, code: string, extra: Partial<KeyboardEvent> = {}) => ({ key, code, altKey: false, ctrlKey: false, metaKey: false, shiftKey: false, ...extra });

describe("Go to matching and ordering", () => {
  it("scores subsequence, prefix and word starts like Qt", () => {
    expect(goToScore("", "anything")).toBe(0);
    expect(goToScore("abc", "a1b2c")).toBeGreaterThan(0);
    expect(goToScore("abc", "a1b2")).toBe(-1);
    expect(goToScore("ns", "New session")).toBe(157);
    expect(goToScore("ns", "New session")).toBeGreaterThan(goToScore("ns", "Newsletter session x"));
    expect(goToScore("ab", "a/b")).toBeGreaterThan(goToScore("ab", "1ab"));
  });

  it("uses recency only for an empty query and kinds and stable order for ties", () => {
    const rows = [item(GoToKind.Command, "command"), item(GoToKind.Session, "older", "older", 4), item(GoToKind.Session, "recent", "recent", 0), item(GoToKind.Project, "project")];
    expect(rankGoTo(rows, "").map((row) => row.id)).toEqual(["recent", "older", "command", "project"]);
    expect(rankGoTo(rows, ">").map((row) => row.id)).toEqual(["command"]);
    expect(rankGoTo(rows, "older").map((row) => row.id)).toEqual(["older"]);
  });

  it("keeps twelve visible rows and moves the window with the selection", () => {
    const rows = Array.from({ length: 30 }, (_, i) => item(GoToKind.Session, `s${i}`));
    expect(visibleGoToRows(rows, 0).rows.map((row) => row.id)).toEqual(rows.slice(0, 12).map((row) => row.id));
    expect(visibleGoToRows(rows, 17).start).toBe(6);
    expect(visibleGoToRows(rows, 29).start).toBe(18);
    expect(visibleGoToRows(rows, 29).rows).toHaveLength(12);
  });

  it("deduplicates and caps recency at twenty", () => {
    for (let i = 0; i < 25; i++) rememberRecentSession(`run-${i}`);
    rememberRecentSession("run-20");
    const ids = recentSessionIds();
    expect(ids).toHaveLength(20);
    expect(ids[0]).toBe("run-20");
    expect(new Set(ids).size).toBe(20);
    expect(ids.includes("run-0")).toBe(false);
  });
});

describe("Go to keyboard flow", () => {
  it("navigates, activates and cancels", () => {
    const state: GoToKeyState = { typed: [] };
    expect(goToKey(press("ArrowDown", "ArrowDown"), state, [])).toBe("down");
    expect(goToKey(press("ArrowUp", "ArrowUp"), state, [])).toBe("up");
    expect(goToKey(press("PageDown", "PageDown"), state, [])).toBe("pageDown");
    expect(goToKey(press("PageUp", "PageUp"), state, [])).toBe("pageUp");
    expect(goToKey(press("Enter", "Enter"), state, [])).toBe("activate");
    expect(goToKey(press("Escape", "Escape"), state, [])).toBe("close");
    expect(goToKey(press("Enter", "Enter", { isComposing: true }), state, [])).toBe("none");
  });

  it("closes on the current Go to chord and on a rebound sequence", () => {
    const state: GoToKeyState = { typed: [] };
    expect(goToKey(press("k", "KeyK", { ctrlKey: true }), state, [parseSequence("Ctrl+K")!])).toBe("close");
    expect(goToKey(press("j", "KeyJ", { ctrlKey: true, altKey: true }), state, [parseSequence("Ctrl+Alt+J")!])).toBe("close");
    expect(goToKey(press("x", "KeyX", { ctrlKey: true }), state, [parseSequence("Ctrl+X, Ctrl+S")!])).toBe("hold");
    expect(goToKey(press("s", "KeyS", { ctrlKey: true }), state, [parseSequence("Ctrl+X, Ctrl+S")!])).toBe("close");
  });
});
