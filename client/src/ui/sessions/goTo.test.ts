import { describe, expect, it } from "vitest";
import { parseSequence } from "../../store/shortcuts";
import { goToDetails, goToFrame, goToKey, goToKindLabel, goToMessageRows, goToScore, rankGoTo, recentSessionIds, rememberRecentSession, visibleGoToRows, GoToKind, type GoToItem, type GoToKeyState } from "./goTo";

const item = (kind: GoToKind, id: string, title = id, recency = -1): GoToItem => ({ kind, id, host: "", title, detail: "", recency });
const press = (key: string, code: string, extra: Partial<KeyboardEvent> = {}) => ({ key, code, altKey: false, ctrlKey: false, metaKey: false, shiftKey: false, ...extra });

describe("Go to matching and ordering", () => {
  it("labels kinds and matches each detail part independently", () => {
    expect([GoToKind.Session, GoToKind.Project, GoToKind.Account, GoToKind.Machine, GoToKind.Section, GoToKind.Command].map(goToKindLabel))
      .toEqual(["SESSION", "PROJECT", "ACCOUNT", "MACHINE", "SECTION", "COMMAND"]);
    const row = { ...item(GoToKind.Session, "session"), detail: ["Project", "workstation"] };
    expect(goToDetails(row.detail)).toEqual(["Project", "workstation"]);
    expect(goToDetails("Section")).toEqual(["Section"]);
    expect(rankGoTo([row], "workstation").map((entry) => entry.id)).toEqual(["session"]);
  });

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

// Task 423: the title-bar search opens in place and finds sessions by name and
// ID and messages, as its placeholder says.
describe("the title-bar search (task 423)", () => {
  it("lies over the search field: its left edge and top, at least its width, inside the window", () => {
    expect(goToFrame({ left: 300, top: 6, width: 340, height: 28 }, { width: 1400, height: 900 })).toEqual({ left: 300, top: 6, width: 560, height: 28 });
    expect(goToFrame({ left: 300, top: 6, width: 700, height: 28 }, { width: 1400, height: 900 }).width).toBe(700);
    // Near the right edge it moves left to stay inside the window.
    expect(goToFrame({ left: 1000, top: 6, width: 340, height: 28 }, { width: 1400, height: 900 })).toMatchObject({ left: 1400 - 8 - 560, width: 560 });
    // A window narrower than the minimum keeps a margin on both sides.
    expect(goToFrame({ left: 100, top: 6, width: 200, height: 28 }, { width: 400, height: 900 })).toMatchObject({ left: 8, width: 384 });
  });

  it("finds a session by its ID or name, not only its title", () => {
    const items: GoToItem[] = [
      { kind: GoToKind.Session, id: "a", host: "", title: "Checkout", detail: "", recency: -1, keys: ["claude/shop/checkout", "conv-7f3a2c91-0b44"] },
      { kind: GoToKind.Session, id: "b", host: "", title: "Docs", detail: "", recency: -1, keys: ["claude/shop/docs", "conv-1111"] },
    ];
    expect(rankGoTo(items, "7f3a2c91").map((item) => item.id)).toEqual(["a"]);
    expect(rankGoTo(items, "CONV-1111").map((item) => item.id)).toEqual(["b"]);
    expect(rankGoTo(items, "shop/docs").map((item) => item.id)).toEqual(["b"]);
    // Commands only after ">": IDs never pull sessions in.
    expect(rankGoTo(items, "> conv").filter((item) => item.kind === GoToKind.Session)).toEqual([]);
  });

  it("lists a message hit as a row of its live session, and skips archived and foreign hits", () => {
    const session = { key: "\nrun:r1", catalogKey: "run:r1", computer: "", name: "claude/shop/checkout", title: "Checkout", runId: "r1", conversationId: "c1" } as never;
    const hit = (extra: Record<string, unknown>) => ({ computer: "", machine: "This computer",
      hit: { name: "claude/shop/checkout", run_id: "r1", conversation_id: "c1", archive_id: "", message_id: "m1", at: 1, role: "assistant",
        snippet: "the refund webhook retries twice", event: {} as never, content_truncated: false, session: null, group: "", ...extra } });
    const rows = goToMessageRows([hit({}), hit({ archive_id: "z9", message_id: "m2" }), hit({ name: "other", message_id: "m3" })],
      (_computer, found) => (found.name === "claude/shop/checkout" ? session : undefined));
    expect(rows.map((row) => [row.item.kind, row.item.title, row.item.detail])).toEqual([[GoToKind.Message, "Checkout", ["the refund webhook retries twice", "This computer"]]]);
    expect(goToKindLabel(GoToKind.Message)).toBe("MESSAGE");
  });
});
