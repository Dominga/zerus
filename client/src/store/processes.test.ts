import { describe, expect, it } from "vitest";
import { createRoot } from "solid-js";
import { createProcesses, nextSelection, processTarget, rowOrder, runningCommandsEntry } from "./processes";
import type { ProcessesView, ProcessRow } from "../types";
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
  groups: [{ group: "live", title: "Live processes", rows: [row("p1"), row("p2")] }, { group: "finished", title: "Finished", rows: [row("p3")] }],
  selected: ["p1"], current: "p1", stop: { label: "Stop selected", enabled: true, tooltip: "" }, stop_notice: "", busy: false,
  failed: null, detail: null, count: 2, ...change,
});

describe("the Processes store", () => {
  function harness() {
    const calls: string[] = [];
    const store = createRoot(() => createProcesses({
      select: (key, ids, current) => { calls.push(`select ${JSON.stringify([key, ids, current])}`); },
      stop: (key, ids) => { calls.push(`stop ${JSON.stringify([key, ids])}`); },
    }));
    return { store, calls };
  }

  it("orders rows as the tree lists them, Finished included", () => {
    expect(rowOrder(view())).toEqual(["p1", "p2", "p3"]);
  });

  it("sends a selection and a stop for the view they were made in", () => {
    const h = harness();
    h.store.click("p2", { toggle: false, range: false });
    expect(h.calls).toEqual([]);
    h.store.apply(view());
    h.store.click("p3", { toggle: false, range: true });
    h.store.stop();
    expect(h.calls).toEqual([`select ${JSON.stringify(["host\nname", ["p1", "p2", "p3"], "p3"])}`, `stop ${JSON.stringify(["host\nname", ["p1"]])}`]);
  });

  it("stops nothing while Stop selected is disabled", () => {
    const h = harness();
    h.store.apply(view({ stop: { label: "Stop selected", enabled: false, tooltip: "Nothing selected can be stopped." } }));
    h.store.stop();
    expect(h.calls).toEqual([]);
  });

  it("a right click selects an unselected row alone, and leaves a selected one", () => {
    const h = harness();
    h.store.apply(view({ selected: ["p1", "p2"] }));
    h.store.contextOn("p2");
    h.store.contextOn("p3");
    expect(h.calls).toEqual([`select ${JSON.stringify(["host\nname", ["p3"], "p3"])}`]);
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
});

describe("Show running commands (SessionsWindow.cpp:5297-5299)", () => {
  const open = () => {};
  it("is listed for a session with background work, enabled while online and Processes is on", () => {
    expect(runningCommandsEntry(session(), true, true, open)).toMatchObject({ label: "Show running commands", disabled: false, hint: "Follow this session's background work in Processes" });
    expect(runningCommandsEntry(session(), false, true, open)?.disabled).toBe(true);
    expect(runningCommandsEntry(session(), true, false, open)).toMatchObject({ disabled: true, hint: "Enable Settings > Processes to inspect running commands" });
  });

  it("counts background work under an acknowledged attention, and lists nothing otherwise", () => {
    expect(runningCommandsEntry(session({ status: "attention", ackStatus: "background" }), true, true, open)).toBeDefined();
    expect(runningCommandsEntry(session({ status: "working", ackStatus: "working" }), true, true, open)).toBeUndefined();
    expect(runningCommandsEntry(session({ state: "archived" }), true, true, open)).toBeUndefined();
  });
});
