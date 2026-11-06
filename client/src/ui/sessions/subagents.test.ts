import { describe, expect, it } from "vitest";
import { toSession } from "../../store/catalog";
import { groupRows, type ListInput } from "./group-rows";
import { childInactive, childRows, rosterFor } from "./subagents";
import type { GroupingOut } from "../../types";

const grouping: GroupingOut = { loaded: true, default_project: "ungrouped", headers: [], ready: true, reason: null, busy: false, status: "", status_error: false, conflicts: 0 };
const input = (extra: Partial<ListInput> = {}): ListInput => ({ grouping, members: new Map(), visible: () => true, online: () => true, projects: [], expanded: () => true, compact: () => false, ...extra });
const parent = () => toSession("", "run:1", { name: "dsh/example/review", conversation_id: "conv", updated_at: 10, subagents: {
  working: { label: "Worker", state: "working", detail: "Checking now" },
  done: { name: "Finished", state: "ready", task: "Earlier result" },
} });

describe("native child presentation", () => {
  it("keeps unchanged current and non-current cards by identity, including absent child metadata", () => {
    for (const state of ["ready", "archived", "paused", "stopped"] as const) {
      for (const online of [true, false]) {
        for (const processState of ["live", "exited"] as const) {
          for (const subagents of [undefined, [], childRows({ ready: { state: "ready" }, unknown: { state: "unknown" } })]) {
            const row = { ...parent(), state, processState, subagents };
            const result = groupRows(input({ members: new Map([["ungrouped", [row]]]), online: () => online, includeCompleted: true }));
            const card = result.find((item) => item.type === "card");
            expect(card?.type === "card" && card.session).toBe(row);
          }
        }
      }
    }
    const archived = { ...parent(), state: "archived" as const };
    const cards = groupRows(input({ members: new Map([["ungrouped", [archived]]]), children: false }));
    expect(cards[0].type === "card" && cards[0].session).toBe(archived);
  });
  it("copies only when a child is hidden or a non-current working child becomes unknown", () => {
    const row = parent();
    for (const online of [true, false]) {
      const result = groupRows(input({ members: new Map([["ungrouped", [row]]]), online: () => online }));
      const card = result.find((item) => item.type === "card");
      expect(card?.type === "card" && card.session).not.toBe(row);
      expect(card?.type === "card" && card.session.subagents?.map((child) => child.state)).toEqual([online ? "working" : "unknown"]);
    }
    const working = { ...row, subagents: row.subagents?.filter((child) => child.state === "working") };
    const result = groupRows(input({ members: new Map([["ungrouped", [working]]]), online: () => false }));
    expect(result[0].type === "card" && result[0].session).not.toBe(working);
    expect(working.subagents?.[0].state).toBe("working");
  });
  it("shows ended and unknown children without manufacturing working or completion", () => {
    const rows = childRows({ ended: { state: "ended", completion_source: "original_process_exit" }, unknown: { state: "working", display_state: "unknown" } });
    expect(rows.map((row) => [row.id, row.state, row.inactive])).toEqual([["ended", "ended", true], ["unknown", "unknown", false]]);
    const row = parent();
    const result = groupRows(input({ members: new Map([["ungrouped", [row]]]), online: () => false }));
    expect(result.filter((item) => item.type === "child").map((item) => item.type === "child" && item.child.state)).toEqual(["unknown"]);
  });
  it("uses stable IDs, labels, states and bounded inactive evidence", () => {
    const rows = childRows({ done: { name: "Finished", state: "ready", task: "Earlier result" }, working: { label: "Worker", display_state: "approval" }, group: { group: true, active_count: 0 } }, true);
    expect(rows.map((row) => [row.id, row.name, row.state, row.summary, row.inactive])).toEqual([
      ["done", "Finished", "ready", "Earlier result", true],
      ["group", "group", "ready", "", true],
      ["working", "Worker", "attention", "", false],
    ]);
    expect(childInactive({ group: true, active_count: 1 }, true)).toBe(false);
    expect(childInactive({ group: true, active_count: 0 }, false)).toBe(false);
    expect(childRows({ empty: { label: "", name: "Not used", detail: "", task: "Not shown" } })[0]).toMatchObject({ id: "empty", name: "empty", summary: "" });
  });

  it("uses only a same-conversation fresh inspection roster", () => {
    const row = parent();
    expect(rosterFor(row, { conversation_id: "conv", last_event_at: 10, subagents: { fresh: { state: "working" } } }).map((child) => child.id)).toEqual(["fresh"]);
    expect(rosterFor(row, { conversation_id: "conv", last_event_at: 9, subagents: { stale: { state: "working" } } }).map((child) => child.id)).toEqual(["done", "working"]);
    expect(rosterFor(row, { conversation_id: "other", last_event_at: 11, subagents: { other: { state: "working" } } }).map((child) => child.id)).toEqual(["done", "working"]);
  });

  it("retains selected and drafted completed children and honors includeCompleted", () => {
    const row = parent();
    const members = new Map([["ungrouped", [row]]]);
    const ids = (extra: Partial<ListInput>) => groupRows(input({ ...extra, members })).flatMap((item) => item.type === "child" ? [item.child.id] : []);
    expect(ids({})).toEqual(["working"]);
    expect(ids({ selected: { key: row.key, subagent: "done", conversation: "conv" } })).toEqual(["done", "working"]);
    expect(ids({ childDraft: (_parent, child) => child.id === "done" })).toEqual(["done", "working"]);
    expect(ids({ includeCompleted: true })).toEqual(["done", "working"]);
  });
});
