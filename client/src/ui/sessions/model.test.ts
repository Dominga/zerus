import { describe, expect, it, vi } from "vitest";
import { createComputed, createMemo, createRoot } from "solid-js";
import { createStore } from "solid-js/store";
import { applyBatch, catalog, toSession } from "../../store/catalog";
import { applyGrouping } from "../../store/organization";
import { rowOut } from "../../store/testing";
import { groupMembers, groupRows, membersByGroup, projectChoices, type ListInput } from "./model";
import { emptyBatch, type Batch, type GroupingOut, type ViewRow } from "../../types";
import type { Session } from "./types";

// The list model's module reads the page's query (store/ui.ts); the tests run without a page.
vi.hoisted(() => { vi.stubGlobal("location", new URL("http://localhost/")); });

const session = (n: number, extra: Partial<ViewRow> = {}): Session =>
  toSession("", `run:${n}`, { name: `claude/sample-project/s${n}`, machine: "workstation", status: "ready", ...extra });
const header = (id: string, collapsed = false) => ({ id, name: id.toUpperCase(), color: null, accessible: true, collapsed, is_default: id === "ungrouped", palette: 0 });
const grouping = (headers: ReturnType<typeof header>[]): GroupingOut => ({
  loaded: true, default_project: "ungrouped", headers, ready: true, reason: null, busy: false, status: "", status_error: false, conflicts: 0,
});
const input = (g: GroupingOut, members: Map<string, Session[]>, extra: Partial<ListInput> = {}): ListInput => ({
  grouping: g, members, visible: () => true, projects: [], expanded: () => false, compact: () => false, ...extra,
});
const batchOf = (part: Partial<Batch>): Batch => ({ ...emptyBatch(), ...part });
const shape = (rows: ReturnType<typeof groupRows>) => rows.map((r) => (r.type === "header" ? `#${r.project}` : r.type === "card" ? r.session.name.slice(-2) : "child"));

describe("grouped rows", () => {
  it("one project shows no header and nothing collapses", () => {
    const members = new Map([["alpha", [session(1), session(2)]]]);
    expect(shape(groupRows(input(grouping([header("alpha", true)]), members)))).toEqual(["s1", "s2"]);
    // The project filter still applies without headers.
    expect(shape(groupRows(input(grouping([header("alpha")]), members, { projects: ["other"] })))).toEqual([]);
  });

  it("follows the organization's order, hides empty projects and collapses only members", () => {
    const members = new Map([["beta", [session(3)]], ["alpha", [session(1), session(2)]]]);
    const g = grouping([header("alpha"), header("ungrouped"), header("beta", true)]);
    expect(shape(groupRows(input(g, members)))).toEqual(["#alpha", "s1", "s2", "#beta"]);
    expect(shape(groupRows(input(g, members, { projects: ["beta"] })))).toEqual(["#beta"]);
  });

  it("counts every member of a project in its header, never muted ones", () => {
    // s1 both needs attention and has an unread reply: it counts in both (Qt counts them separately).
    const members = new Map([["alpha", [session(1, { status: "attention", unread: true }), session(2, { unread: true }), session(3, { status: "working" }), session(4, { status: "attention", muted: true })]], ["ungrouped", [session(5)]]]);
    const rows = groupRows(input(grouping([header("alpha"), header("ungrouped")]), members, { visible: (s) => s.name.endsWith("s3") || s.name.endsWith("s5") }));
    const alpha = rows[0];
    expect(alpha.type === "header" && [alpha.total, alpha.attention, alpha.unread, alpha.working]).toEqual([1, 1, 2, 1]);
  });

  it("orders members by rank, keeps arrival order for ties and puts unassigned rows in the default project", () => {
    const all = [session(1), session(2), session(3), session(4)];
    const rows = Object.fromEntries(all.map((s) => [s.key, s]));
    const members = groupMembers(all.map((s) => s.key), rows, {
      "\nrun:1": { group: "alpha", rank: 3 }, "\nrun:2": { group: "alpha", rank: 1 }, "\nrun:3": { group: "alpha", rank: 1 },
    }, "ungrouped");
    expect(members.get("alpha")?.map((s) => s.name.slice(-2))).toEqual(["s2", "s3", "s1"]);
    expect(members.get("ungrouped")?.map((s) => s.name.slice(-2))).toEqual(["s4"]);
  });

  it("does not regroup when only a row's status changes", () => createRoot((dispose) => {
    const [rows, setRows] = createStore<Record<string, Session>>({ "\nrun:1": session(1) });
    const [order] = createStore(["\nrun:1"]);
    let runs = 0;
    const members = createMemo(() => { runs++; return groupMembers(order, rows, {}, "ungrouped"); });
    members();
    setRows("\nrun:1", "status", "working");
    members();
    expect(runs).toBe(1);
    dispose();
  }));

  it("the list's members do not regroup on a status-only batch", () => createRoot((dispose) => {
    applyBatch(batchOf({ upserts: [rowOut("", 1), rowOut("", 2)] }));
    applyGrouping(batchOf({ groups: [{ computer: "", key: "run:1", group: "alpha", rank: 2 }, { computer: "", key: "run:2", group: "alpha", rank: 1 }] }));
    let runs = 0;
    createComputed(() => { membersByGroup(); runs++; });
    runs = 0;
    applyBatch(batchOf({ upserts: [rowOut("", 1, { status: "working", phase: "tool" })] }));
    expect(runs).toBe(0);
    expect(membersByGroup().get("alpha")?.map((s) => s.key)).toEqual(["\nrun:2", "\nrun:1"]);
    // A new assignment does regroup.
    applyGrouping(batchOf({ groups: [{ computer: "", key: "run:1", group: "alpha", rank: 0 }] }));
    expect(runs).toBe(1);
    expect(membersByGroup().get("alpha")?.map((s) => s.key)).toEqual(["\nrun:1", "\nrun:2"]);
    dispose();
  }));

  it("offers the projects with members in the organization's order, by id with the header's name", () => createRoot((dispose) => {
    applyBatch(batchOf({ upserts: [rowOut("", 11), rowOut("", 12), rowOut("", 13)] }));
    const all = catalog.order.map((id) => id.slice(1));
    applyGrouping(batchOf({
      grouping: grouping([header("beta"), header("ungrouped"), header("alpha"), header("gamma")]),
      groups: all.map((key, i) => ({ computer: "", key, group: key === "run:13" ? "alpha" : "beta", rank: i })),
    }));
    // The empty Inbox and gamma are left out.
    expect(projectChoices()).toEqual([
      { id: "beta", label: "BETA", count: all.length - 1 },
      { id: "alpha", label: "ALPHA", count: 1 },
    ]);
    dispose();
  }));
});
