import { describe, expect, it, vi } from "vitest";
import { ARCHIVE_ACTIONS_BOUND, barTools, countable, selectionMenu, splitTools, SELECTION_ICONS } from "./selection-ui";
import { MOVE_LATER, SELECTION_ACTIONS, type SelectionAction } from "./selection";
import { resolveIcon } from "../lib/icons";

const counts = (c: Partial<Record<SelectionAction, number>>) => (a: SelectionAction) => c[a] ?? 0;

describe("barTools", () => {
  it("shows only tools with a count, in Qt's order, with Qt's texts and hints", () => {
    const tools = barTools(counts({ terminate: 1, compact: 3, pause: 2, mark_read: 4 }), 5, false);
    expect(tools.map((t) => t.action)).toEqual(["compact", "mark_read", "pause", "terminate"]);
    expect(tools[0].text).toBe("Compact session rows (3)");
    expect(tools[0].hint).toBe("Compact session rows — 3 of 5 selected sessions");
    expect(tools[3].text).toBe("Terminate (1)");
    expect(tools.every((t) => !/\.|Esc/.test(t.hint.replace("sessions", "")))).toBe(true);
  });
  it("keeps the order of all ten actions", () => {
    const all = Object.fromEntries(SELECTION_ACTIONS.map((a) => [a, 1]));
    expect(barTools(counts(all), 2, false).map((t) => t.action)).toEqual(["compact", "expand", "mark_read", "review_later", "pause", "resume", "archive", "move", "forget", "terminate"]);
  });
  it("disables writes while a batch runs but not compact or expand", () => {
    const tools = barTools(counts({ compact: 1, expand: 1, mark_read: 1, pause: 1, forget: 1 }), 2, true);
    expect(Object.fromEntries(tools.map((t) => [t.action, t.enabled]))).toEqual({ compact: true, expand: true, mark_read: false, pause: false, forget: false });
    expect(barTools(counts({ pause: 1 }), 2, false)[0].enabled).toBe(true);
  });
  it("shows Move to project with its count, disabled, with the later hint", () => {
    const [move] = barTools(counts({ move: 3 }), 3, false);
    expect(move.text).toBe("Move to project (3)");
    expect(move.enabled).toBe(false);
    expect(move.hint).toBe(MOVE_LATER);
  });
  it("marks Forget and Terminate dangerous and gives every action a known icon", () => {
    const all = Object.fromEntries(SELECTION_ACTIONS.map((a) => [a, 1]));
    expect(barTools(counts(all), 2, false).filter((t) => t.danger).map((t) => t.action)).toEqual(["forget", "terminate"]);
    for (const a of SELECTION_ACTIONS) expect(resolveIcon(SELECTION_ICONS[a]).html, a).not.toBe("");
  });
});

describe("splitTools", () => {
  it("keeps all when they fit and moves the tail under the overflow button otherwise", () => {
    expect(splitTools([1, 2, 3], 3)).toEqual({ shown: [1, 2, 3], more: [] });
    expect(splitTools([1, 2, 3, 4, 5], 3)).toEqual({ shown: [1, 2], more: [3, 4, 5] });
    expect(splitTools([1, 2], 0)).toEqual({ shown: [], more: [1, 2] });
  });
});

describe("countable", () => {
  it("counts archived Resume and Forget now that the archive calls are bound, and not without them", () => {
    expect(ARCHIVE_ACTIONS_BOUND).toBe(true);
    expect(countable({ state: "archived" }, "resume", true)).toBe(true);
    expect(countable({ state: "archived" }, "forget", true)).toBe(true);
    expect(countable({ state: "archived" }, "resume", true, false)).toBe(false);
    expect(countable({ state: "archived" }, "forget", true, false)).toBe(false);
    expect(countable({ state: "archived" }, "mark_read", true)).toBe(true);
    expect(countable({ state: "paused" }, "resume", true)).toBe(true);
    expect(countable({ state: "paused" }, "resume", false)).toBe(false);
    expect(countable({ state: "archived" }, "resume", true, true)).toBe(true);
  });
});

describe("selectionMenu", () => {
  it("has the section, counted actions, a separator and Deselect all", () => {
    const run = vi.fn(), deselect = vi.fn();
    const items = selectionMenu(counts({ pause: 2, move: 4, forget: 1 }), 4, false, run, deselect);
    expect(items[0]).toEqual({ heading: "4 selected sessions" });
    const labels = items.map((e) => ("label" in e ? e.label : "separator" in e ? "-" : e.heading));
    expect(labels).toEqual(["4 selected sessions", "Pause (2)", "Move to project (4)", "Forget (1)", "-", "Deselect all"]);
    const move = items[2] as { disabled?: boolean; hint?: string };
    expect(move.disabled).toBe(true);
    expect(move.hint).toBe(MOVE_LATER);
    (items[1] as { action(): void }).action();
    expect(run).toHaveBeenCalledWith("pause");
    (items[5] as { action(): void }).action();
    expect(deselect).toHaveBeenCalled();
  });
  it("disables writes while a batch runs", () => {
    const items = selectionMenu(counts({ compact: 1, pause: 1 }), 2, true, () => {}, () => {});
    expect((items[1] as { disabled?: boolean }).disabled).toBe(false);
    expect((items[2] as { disabled?: boolean }).disabled).toBe(true);
  });
});

describe("visibleSelected", () => {
  const card = (key: string) => ({ type: "card", id: key, session: { key } as never, expandable: false, expanded: false, compactRow: false }) as never;
  it("counts only the listed card rows whose key is selected", async () => {
    const { visibleSelected } = await import("./selection-ui");
    const listed = [{ type: "header", id: "h:p" } as never, card("a"), card("b"), { type: "child", id: "a/c" } as never];
    // "gone" vanished from the catalog, "hidden" is filtered out of the list, "a/c" is a child row.
    const sel = { a: true, b: false, gone: true, hidden: true, "a/c": true };
    expect(visibleSelected(listed, sel).map((s) => s.key)).toEqual(["a"]);
  });
});
