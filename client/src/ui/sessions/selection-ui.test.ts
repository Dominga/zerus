import { describe, expect, it, vi } from "vitest";
import { ARCHIVE_ACTIONS_BOUND, barTools, countable, selectionMenu, splitTools, SELECTION_ICONS } from "./selection-ui";
import { SELECTION_ACTIONS, type SelectionAction } from "./selection";
import { resolveIcon } from "../lib/icons";

const counts = (c: Partial<Record<SelectionAction, number>>) => (a: SelectionAction) => c[a] ?? 0;

describe("Move to project for a selection (zerus-5wmh.18.1)", () => {
  const counts = (n: number) => (a: SelectionAction) => (a === "move" ? n : 0);
  it("is enabled while project editing is ready and no lifecycle batch runs", () => {
    const [move] = barTools(counts(3), 3, false, true, undefined);
    expect([move.action, move.enabled, move.hint]).toEqual(["move", true, "Move to project — 3 of 3 selected sessions"]);
  });
  it("waits for a running lifecycle batch, then for project sync, with the reason as its hint", () => {
    const [running] = barTools(counts(3), 3, true, true, undefined);
    expect([running.enabled, running.hint]).toEqual([false, "Wait for the current action to finish."]);
    const [waiting] = barTools(counts(3), 3, false, true, "Project sync is starting.");
    expect([waiting.enabled, waiting.hint]).toEqual([false, "Project sync is starting."]);
    const entry = selectionMenu(counts(3), 3, true, () => {}, () => {}, true, undefined).find((e) => "label" in e && e.label.startsWith("Move to project")) as { disabled?: boolean; hint?: string };
    expect([entry.disabled, entry.hint]).toEqual([true, "Wait for the current action to finish."]);
    const ready = selectionMenu(counts(3), 3, false, () => {}, () => {}, true, undefined).find((e) => "label" in e && e.label.startsWith("Move to project")) as { disabled?: boolean; hint?: string };
    expect([ready.disabled, ready.hint]).toEqual([false, undefined]);
  });
});

describe("barTools", () => {
  it("shows attention before row presentation, with each tool's count and hint", () => {
    const tools = barTools(counts({ terminate: 1, compact: 3, pause: 2, mark_read: 4 }), 5, false);
    expect(tools.map((t) => t.action)).toEqual(["mark_read", "compact", "pause", "terminate"]);
    expect(tools[1].text).toBe("Compact session rows (3)");
    expect(tools[1].hint).toBe("Compact session rows — 3 of 5 selected sessions");
    expect(tools[3].text).toBe("Terminate (1)");
    expect(tools.every((t) => !/\.|Esc/.test(t.hint.replace("sessions", "")))).toBe(true);
  });
  it("keeps the order of the actions", () => {
    const all = Object.fromEntries(SELECTION_ACTIONS.map((a) => [a, 1]));
    expect(barTools(counts(all), 2, false).map((t) => t.action)).toEqual(["mark_read", "review_later", "mute", "compact", "expand", "pause", "resume", "archive", "move", "forget", "terminate"]);
  });
  it("offers Mute notifications while any selected session is unmuted, else Unmute (zerus-qhtxf)", () => {
    const mixed = barTools(counts({ mute: 2, unmute: 1 }), 3, false, true);
    expect(mixed.map((t) => [t.action, t.text, t.icon])).toEqual([["mute", "Mute notifications (2)", "bell-off"]]);
    const muted = barTools(counts({ unmute: 3 }), 3, false, false);
    expect(muted.map((t) => [t.action, t.text, t.icon])).toEqual([["unmute", "Unmute notifications (3)", "bell"]]);
    expect(barTools(counts({}), 3, false, true)).toEqual([]);
    expect(barTools(counts({ mute: 1 }), 3, true, true)[0].enabled).toBe(false);
    const menu = (c: Partial<Record<SelectionAction, number>>, unmuted: boolean) => selectionMenu(counts(c), 3, false, () => {}, () => {}, unmuted).map((e) => ("label" in e ? e.label : ""));
    expect(menu({ mute: 2, unmute: 1 }, true)).toContain("Mute notifications (2)");
    expect(menu({ mute: 2, unmute: 1 }, true).some((l) => l.startsWith("Unmute"))).toBe(false);
    expect(menu({ unmute: 3 }, false)).toContain("Unmute notifications (3)");
  });
  it("takes the direction from every selected session, whether or not its setting can change now (zerus-qhtxf.1)", () => {
    // One unmuted row that cannot change (offline, a conflict or unsupported) and one muted row that can:
    // the selection is not all muted, so Unmute is never offered and Mute has nothing to do.
    expect(barTools(counts({ unmute: 1 }), 2, false, true)).toEqual([]);
    expect(selectionMenu(counts({ unmute: 1 }), 2, false, () => {}, () => {}, true).some((e) => "label" in e && e.label.startsWith("Unmute"))).toBe(false);
    // Every selected row muted: Unmute for the rows that can change.
    expect(barTools(counts({ unmute: 1 }), 2, false, false).map((t) => t.text)).toEqual(["Unmute notifications (1)"]);
  });
  it("disables writes while a batch runs but not compact or expand", () => {
    const tools = barTools(counts({ compact: 1, expand: 1, mark_read: 1, pause: 1, forget: 1 }), 2, true);
    expect(Object.fromEntries(tools.map((t) => [t.action, t.enabled]))).toEqual({ compact: true, expand: true, mark_read: false, pause: false, forget: false });
    expect(barTools(counts({ pause: 1 }), 2, false)[0].enabled).toBe(true);
  });
  it("shows Move to project with its count, enabled while project editing is ready (zerus-5wmh.18.1)", () => {
    const [move] = barTools(counts({ move: 3 }), 3, false);
    expect(move.text).toBe("Move to project (3)");
    expect(move.enabled).toBe(true);
    expect(move.hint).toBe("Move to project — 3 of 3 selected sessions");
  });
  it("disables Move to project with the organization's own reason", () => {
    const [move] = barTools(counts({ move: 3 }), 3, false, true, "Projects are being read.");
    expect(move.enabled).toBe(false);
    expect(move.hint).toBe("Projects are being read.");
  });
  it("says one selected session in English, and more than one in the plural (zerus-wi352)", () => {
    expect(barTools(counts({ pause: 1 }), 1, false)[0].hint).toBe("Pause — 1 of 1 selected session");
    expect(barTools(counts({ pause: 1 }), 2, false)[0].hint).toBe("Pause — 1 of 2 selected sessions");
    expect(selectionMenu(counts({ pause: 1 }), 1, false, () => {}, () => {})[0]).toEqual({ heading: "1 selected session" });
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
    const move = items[2] as { disabled?: boolean; hint?: string; action(): void };
    expect(move.disabled).toBe(false);
    expect(move.hint).toBeUndefined();
    move.action();
    expect(run).toHaveBeenCalledWith("move");
    run.mockClear();
    const refused = selectionMenu(counts({ move: 4 }), 4, false, run, deselect, true, "Projects are being read.")[1] as { disabled?: boolean; hint?: string };
    expect(refused.disabled).toBe(true);
    expect(refused.hint).toBe("Projects are being read.");
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
