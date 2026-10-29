import { describe, expect, it } from "vitest";
import { createRoot, createSignal } from "solid-js";
import { createStore } from "solid-js/store";
import {
  createOutputFollow, createProcesses, groupItem, nextSelection, processesTab, processTarget, runningCommandsEntry,
  trackFollow, trackShown, visibleRows, type TreeKey,
} from "./processes";
import type { ProcessesView, ProcessRow, ProcessTarget } from "../types";
import type { Session } from "../ui/sessions/types";

const order = ["p1", "p2", "p3", "p4"];

describe("process selection (QTreeWidget ExtendedSelection)", () => {
  it("a click selects one, Ctrl/Cmd toggles, Shift extends from the current row", () => {
    expect(nextSelection(order, ["p1"], "p1", "p3", { toggle: false, range: false })).toEqual({ selected: ["p3"], current: "p3" });
    expect(nextSelection(order, ["p1"], "p1", "p3", { toggle: true, range: false })).toEqual({ selected: ["p1", "p3"], current: "p3" });
    expect(nextSelection(order, ["p1", "p3"], "p3", "p3", { toggle: true, range: false })).toEqual({ selected: ["p1"], current: "p3" });
    expect(nextSelection(order, ["p2"], "p2", "p4", { toggle: false, range: true })).toEqual({ selected: ["p2", "p3", "p4"], current: "p4" });
  });

  it("keeps tree order, ranges upwards too, and treats Shift without a current row as a click", () => {
    expect(nextSelection(order, ["p4"], "p4", "p1", { toggle: true, range: false }).selected).toEqual(["p1", "p4"]);
    expect(nextSelection(order, ["p4"], "p4", "p2", { toggle: false, range: true })).toEqual({ selected: ["p2", "p3", "p4"], current: "p2" });
    expect(nextSelection(order, [], null, "p3", { toggle: false, range: true })).toEqual({ selected: ["p3"], current: "p3" });
  });
});

const row = (id: string): ProcessRow => ({ id, command: id, command_full: id, owner: "Agent", status: "running", tooltip: "", tone: "warning", duration: "1s" });
const view = (change: Partial<ProcessesView> = {}): ProcessesView => ({
  key: "host\nname", summary: "", summary_tooltip: "", notes: "",
  groups: [
    { group: "live", title: "Live processes", rows: [row("p1"), row("p2")] },
    { group: "recorded", title: "Recorded commands / unconfirmed", rows: [row("p3")] },
    { group: "finished", title: "Finished", rows: [row("p4")] },
  ],
  selected: ["p1"], current: "p1", stop: { label: "Stop selected", enabled: true, tooltip: "" }, stop_notice: "", busy: false,
  failed: null, detail: null, count: 2, ...change,
});

function harness() {
  const calls: string[] = [];
  let clock = 0;
  const store = createRoot(() => createProcesses({
    select: (key, ids, current) => { calls.push(`select ${JSON.stringify([key, ids, current])}`); },
    stop: (key, ids) => { calls.push(`stop ${JSON.stringify([key, ids])}`); },
  }, () => clock));
  const menus: string[] = [];
  const opened = (v: ProcessesView) => { menus.push(`${v.key}:${v.selected.join(",")}`); };
  return { store, calls, menus, opened, advance: (ms: number) => { clock += ms; } };
}
const select = (key: string, ids: string[], current: string | null) => `select ${JSON.stringify([key, ids, current])}`;

describe("the Processes store", () => {
  it("ranges over the rows that show: a collapsed group's rows are never selected (Qt's QTreeView)", () => {
    // Finished starts collapsed: its row p4 does not show.
    expect(visibleRows(view(), { finished: true })).toEqual(["p1", "p2", "p3"]);
    const h = harness();
    h.store.apply(view());
    h.store.toggleGroup("recorded");
    h.store.click("p4", { toggle: false, range: true });
    expect(h.calls.at(-1)).toBe(select("host\nname", ["p4"], "p4"));
    h.store.toggleGroup("finished");
    h.store.click("p4", { toggle: false, range: true });
    expect(h.calls.at(-1)).toBe(select("host\nname", ["p1", "p2", "p4"], "p4"));
  });

  it("sends a selection and a stop for the view they were made in", () => {
    const h = harness();
    h.store.click("p2", { toggle: false, range: false });
    expect(h.calls).toEqual([]);
    h.store.apply(view());
    h.store.click("p3", { toggle: false, range: true });
    h.store.stop();
    expect(h.calls).toEqual([select("host\nname", ["p1", "p2", "p3"], "p3"), `stop ${JSON.stringify(["host\nname", ["p1"]])}`]);
  });

  it("stops nothing while Stop selected is disabled", () => {
    const h = harness();
    h.store.apply(view({ stop: { label: "Stop selected", enabled: false, tooltip: "Nothing selected can be stopped." } }));
    h.store.stop();
    expect(h.calls).toEqual([]);
  });

  it("a menu's Terminate stops only while the view and its selection are the ones it opened on", () => {
    const h = harness();
    h.store.apply(view({ selected: ["p1", "p2"] }));
    h.store.stopIfStill("other\nview", ["p1", "p2"]);
    h.store.stopIfStill("host\nname", ["p1"]);
    h.store.stopIfStill("host\nname", ["p2", "p1"]);
    expect(h.calls).toEqual([]);
    h.store.stopIfStill("host\nname", ["p1", "p2"]);
    expect(h.calls).toEqual([`stop ${JSON.stringify(["host\nname", ["p1", "p2"]])}`]);
  });

  it("opens a selected row's menu at once", () => {
    const h = harness();
    h.store.apply(view({ selected: ["p1", "p2"] }));
    h.store.requestMenu("p2", h.opened);
    expect(h.menus).toEqual(["host\nname:p1,p2"]);
    expect(h.calls).toEqual([]);
  });

  it("selects an unselected row alone first, and opens its menu once the same view shows it", () => {
    const h = harness();
    h.store.apply(view());
    h.store.requestMenu("p2", h.opened);
    expect(h.calls).toEqual([select("host\nname", ["p2"], "p2")]);
    expect(h.menus).toEqual([]);
    // Not yet that selection alone.
    h.store.apply(view({ selected: ["p1", "p2"] }));
    expect(h.menus).toEqual([]);
    h.store.apply(view({ selected: ["p2"], current: "p2" }));
    expect(h.menus).toEqual(["host\nname:p2"]);
    // Once only.
    h.store.apply(view({ selected: ["p2"], current: "p2" }));
    expect(h.menus).toEqual(["host\nname:p2"]);
  });

  it("drops a waiting menu when the tab shows another session's view, or after its wait", () => {
    const h = harness();
    h.store.apply(view());
    h.store.requestMenu("p2", h.opened);
    h.store.apply(view({ key: "host\nother", selected: ["p2"], current: "p2" }));
    expect(h.menus).toEqual([]);
    h.store.apply(view({ selected: ["p2"], current: "p2" }));
    expect(h.menus, "the session switch dropped it").toEqual([]);
    h.store.apply(view());
    h.store.requestMenu("p3", h.opened);
    h.advance(1001);
    h.store.apply(view({ selected: ["p3"], current: "p3" }));
    expect(h.menus).toEqual([]);
  });
});

describe("the tree's keyboard (QTreeView ExtendedSelection)", () => {
  const key = (k: string, mods: Partial<TreeKey> = {}): TreeKey => ({ key: k, shift: false, toggle: false, ...mods });

  it("moves through the items that show, selecting each row alone", () => {
    const h = harness();
    h.store.apply(view());
    // The current row p1 has the focus first.
    expect(h.store.focus()?.id).toBe("p1");
    h.store.key(key("ArrowDown"), () => {});
    expect(h.calls.at(-1)).toBe(select("host\nname", ["p2"], "p2"));
    h.store.key(key("ArrowDown"), () => {});
    expect(h.store.focus()?.id, "a group header takes the focus, not the selection").toBe(groupItem("recorded"));
    h.store.key(key("End"), () => {});
    expect(h.store.focus()?.id, "Finished is collapsed: its row does not show").toBe(groupItem("finished"));
    h.store.key(key("Home"), () => {});
    expect(h.store.focus()?.id).toBe(groupItem("live"));
  });

  it("extends with Shift, moves only the current row with Ctrl/Cmd, and selects or toggles with Space", () => {
    const h = harness();
    h.store.apply(view());
    h.store.key(key("ArrowDown", { shift: true }), () => {});
    expect(h.calls.at(-1)).toBe(select("host\nname", ["p1", "p2"], "p2"));
    // The hub answers; Ctrl/Cmd moves only the current row, the selection stays.
    h.store.apply(view({ selected: ["p1", "p2"], current: "p2" }));
    h.store.key(key("ArrowUp", { toggle: true }), () => {});
    expect(h.calls.at(-1)).toBe(select("host\nname", ["p1", "p2"], "p1"));
    h.store.apply(view({ selected: ["p1", "p2"], current: "p1" }));
    // Ctrl+Space toggles the focused row; Space selects it alone.
    h.store.key(key(" ", { toggle: true }), () => {});
    expect(h.calls.at(-1)).toBe(select("host\nname", ["p2"], "p1"));
    h.store.key(key(" "), () => {});
    expect(h.calls.at(-1)).toBe(select("host\nname", ["p1"], "p1"));
  });

  it("collapses and expands groups with Left and Right, and moves from a row to its group", () => {
    const h = harness();
    h.store.apply(view());
    h.store.key(key("ArrowLeft"), () => {});
    expect(h.store.focus()?.id).toBe(groupItem("live"));
    h.store.key(key("ArrowLeft"), () => {});
    expect(h.store.collapsed().live).toBe(true);
    h.store.key(key("ArrowRight"), () => {});
    expect(h.store.collapsed().live).toBe(false);
    h.store.key(key("ArrowRight"), () => {});
    expect(h.store.focus()?.id).toBe("p1");
  });

  it("opens a row's menu with the Menu key or Shift+F10, and leaves F10 alone", () => {
    const h = harness();
    h.store.apply(view());
    const asked: string[] = [];
    expect(h.store.key(key("ContextMenu"), (id) => asked.push(id))).toBe(true);
    expect(h.store.key(key("F10", { shift: true }), (id) => asked.push(id))).toBe(true);
    expect(h.store.key(key("F10"), (id) => asked.push(id))).toBe(false);
    expect(h.store.key(key("a"), (id) => asked.push(id))).toBe(false);
    expect(asked).toEqual(["p1", "p1"]);
  });
});

describe("the output pane", () => {
  it("keeps its end while it was there, and a new current process starts at its end", () => {
    const f = createOutputFollow();
    f.process("p1");
    expect(f.stick()).toBe(true);
    f.scrolled(false);
    f.process("p1");
    expect(f.stick(), "the person scrolled up in the same process").toBe(false);
    f.process("p2");
    expect(f.stick()).toBe(true);
  });
});

const session = (change: Partial<Session> = {}): Session => ({
  key: "\nrun:a", catalogKey: "run:a", computer: "", name: "claude/sample-project/review", rawMachine: "workstation", machine: "workstation",
  runId: "run-a", conversationId: "conv-a", state: "running", processState: "running", status: "background", ackStatus: "background",
  archiveId: undefined, ...change,
}) as Session;

describe("the followed session (SessionsWindow.cpp:3783-3784)", () => {
  const facts = { online: true, generation: "gen-1", inspectFailed: false };

  it("names the row and its details' host generation", () => {
    expect(processTarget(session(), facts)).toEqual({
      computer: "", key: "run:a", name: "claude/sample-project/review", run_id: "run-a", conversation_id: "conv-a",
      archive_id: null, machine: "workstation", online: true, live: true, generation: "gen-1",
    });
  });

  it("is recorded offline, archived, paused, after the process exited or after an inspect error", () => {
    expect(processTarget(session(), { ...facts, online: false }).live).toBe(false);
    expect(processTarget(session({ state: "archived", archiveId: "a-1" }), facts)).toMatchObject({ live: false, archive_id: "a-1" });
    expect(processTarget(session({ state: "paused" }), facts).live).toBe(false);
    expect(processTarget(session({ processState: "exited" }), facts).live).toBe(false);
    expect(processTarget(session(), { ...facts, inspectFailed: true }).live).toBe(false);
  });

  it("follows only while Processes is on, and again on every change of the row", () => {
    const followed: (ProcessTarget | null)[] = [];
    const [enabled, setEnabled] = createSignal(false);
    const [row, setRow] = createStore(session());
    const [selected, setSelected] = createSignal<Session | undefined>(row);
    const dispose = createRoot((d) => {
      trackFollow({ enabled, selected, facts: () => facts, follow: (t) => followed.push(t) });
      return d;
    });
    expect(followed).toEqual([null]);
    setEnabled(true);
    expect(followed.at(-1)).toMatchObject({ key: "run:a", live: true });
    // The same row changes: the hub hears it again (a failed follow follows again).
    setRow("processState", "exited");
    expect(followed.at(-1)).toMatchObject({ key: "run:a", live: false });
    expect(followed.length).toBe(3);
    setSelected(undefined);
    expect(followed.at(-1)).toBeNull();
    setSelected(row);
    setEnabled(false);
    expect(followed.at(-1)).toBeNull();
    dispose();
  });

  it("says the tab shows only with Processes on, a session, the Sessions page, the tab and a shown window", () => {
    const shown: boolean[] = [];
    const [enabled, setEnabled] = createSignal(true);
    const [page, setPage] = createSignal("sessions");
    const [tab, setTab] = createSignal("processes");
    const [hidden, setHidden] = createSignal(false);
    const [selected, setSelected] = createSignal(true);
    const dispose = createRoot((d) => {
      trackShown({ enabled, selected, page, tab, hidden, shown: (on) => shown.push(on) });
      return d;
    });
    expect(shown.at(-1)).toBe(true);
    for (const [off, on] of [[() => setEnabled(false), () => setEnabled(true)], [() => setPage("settings"), () => setPage("sessions")],
      [() => setTab("activity"), () => setTab("processes")], [() => setHidden(true), () => setHidden(false)], [() => setSelected(false), () => setSelected(true)]]) {
      off();
      expect(shown.at(-1)).toBe(false);
      on();
      expect(shown.at(-1)).toBe(true);
    }
    dispose();
  });
});

describe("the Processes tab entry (SessionsWindow.cpp:1330, 3786-3787)", () => {
  it("shows only with the setting, and counts the hub's processes", () => {
    expect(processesTab(false, 3).show).toBe(false);
    expect(processesTab(true, 0)).toEqual({ id: "processes", label: "Processes", show: true });
    expect(processesTab(true, 3).label).toBe("Processes (3)");
  });
});

describe("Show running commands (SessionsWindow.cpp:5297-5299, 5463-5465)", () => {
  const yes = () => true, no = () => false;

  it("is listed for a session with background work, enabled while online and Processes is on", () => {
    expect(runningCommandsEntry(session(), yes, yes, () => {})).toMatchObject({ label: "Show running commands", disabled: false, hint: "Follow this session's background work in Processes" });
    expect(runningCommandsEntry(session(), no, yes, () => {})?.disabled).toBe(true);
    expect(runningCommandsEntry(session(), yes, no, () => {})).toMatchObject({ disabled: true, hint: "Enable Settings > Processes to inspect running commands" });
  });

  it("counts background work under an acknowledged attention, and lists nothing otherwise", () => {
    expect(runningCommandsEntry(session({ status: "attention", ackStatus: "background" }), yes, yes, () => {})).toBeDefined();
    expect(runningCommandsEntry(session({ status: "working", ackStatus: "working" }), yes, yes, () => {})).toBeUndefined();
    expect(runningCommandsEntry(session({ state: "archived" }), yes, yes, () => {})).toBeUndefined();
  });

  it("opens the tab when chosen, checking online and the setting again then", () => {
    let online = true, opened = 0;
    const entry = runningCommandsEntry(session(), () => online, yes, () => { opened++; })!;
    entry.action!();
    expect(opened).toBe(1);
    online = false;
    entry.action!();
    expect(opened, "the computer went offline after the menu opened").toBe(1);
  });
});
