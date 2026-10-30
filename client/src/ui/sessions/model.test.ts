import { describe, expect, it, vi } from "vitest";
import { createComputed, createMemo, createRoot } from "solid-js";
import { createStore } from "solid-js/store";
import { applyBatch, catalog, toSession } from "../../store/catalog";
import { applyGrouping } from "../../store/organization";
import { rowOut } from "../../store/testing";
import { groupMembers, groupRows, membersByGroup, projectChoices, summarize, type ListInput } from "./model";
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
  grouping: g, members, visible: () => true, online: () => true, projects: [], expanded: () => false, compact: () => false, ...extra,
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

  describe("a header's counters follow Qt's online rule (SessionsWindowRows.cpp:398-404)", () => {
    // "" is this computer, online; "b" is a peer that is not.
    const on = (n: number, extra: Partial<ViewRow> = {}) => session(n, extra);
    const off = (n: number, extra: Partial<ViewRow> = {}) =>
      toSession("b", `run:${n}`, { name: `claude/sample-project/s${n}`, machine: "b", status: "ready", ...extra });
    const online = (s: Session) => s.computer === "";
    const counts = (list: Session[]) => {
      const rows = groupRows(input(grouping([header("alpha"), header("ungrouped")]), new Map([["alpha", list]]), { online }));
      const h = rows[0];
      if (h?.type !== "header") throw new Error("the project has no header");
      return { attention: h.attention, unread: h.unread, working: h.working };
    };
    const asking = { status: "attention", needs_action: true, needs_attention: true } as const;

    it("counts a request only while its computer is online", () => {
      expect(counts([on(1, asking)])).toEqual({ attention: 1, unread: 0, working: 0 });
      expect(counts([off(1, asking)])).toEqual({ attention: 0, unread: 0, working: 0 });
    });

    it("counts an error that needs the person, as Qt's needsAction does", () => {
      expect(counts([on(1, { status: "error", needs_action: true, needs_attention: true })]).attention).toBe(1);
      expect(counts([off(1, { status: "error", needs_action: true, needs_attention: true })]).attention).toBe(0);
    });

    it("counts a subagent that asks only while online", () => {
      expect(counts([on(1, { status: "attention", child_attention: true, needs_attention: false })]).attention).toBe(1);
      expect(counts([off(1, { status: "attention", child_attention: true, needs_attention: false })]).attention).toBe(0);
    });

    it("counts Review later online and offline", () => {
      expect(counts([{ ...on(1), reviewLater: true }, { ...off(2), reviewLater: true }]).attention).toBe(2);
    });

    it("counts an unread reply as unread, not as needing a response", () => {
      const reply = { unread: true, needs_attention: true } as const;
      expect(counts([on(1, reply)])).toEqual({ attention: 0, unread: 1, working: 0 });
      // An offline computer's unread replies still count (visibleUnreadReply, HgsClient.h:104-105).
      expect(counts([off(1, reply)])).toEqual({ attention: 0, unread: 1, working: 0 });
    });

    it("shows an offline computer's unread reply even while its last reply was in progress", () => {
      const online = { ...on(1, { unread: false }), unreadReply: true, unreadCandidate: false };
      const offline = { ...off(2, { unread: false }), unreadReply: true, unreadCandidate: false };
      expect(counts([online])).toEqual({ attention: 0, unread: 0, working: 0 });
      expect(counts([offline])).toEqual({ attention: 0, unread: 1, working: 0 });
      expect(counts([{ ...offline, muted: true }])).toEqual({ attention: 0, unread: 0, working: 0 });
    });

    it("counts work and background jobs only while online", () => {
      expect(counts([on(1, { status: "working" }), on(2, { status: "background" })]).working).toBe(2);
      expect(counts([off(1, { status: "working" }), off(2, { status: "background" })]).working).toBe(0);
    });

    it("never counts a muted member as attention or unread, online or not", () => {
      const muted = { ...asking, unread: true, muted: true } as const;
      expect(counts([on(1, muted), { ...off(2, muted), reviewLater: true }])).toEqual({ attention: 0, unread: 0, working: 0 });
    });

    it("counts each member by its own computer when a project spans online and offline ones", () => {
      const list = [
        on(1, asking), off(2, asking),
        on(3, { status: "working" }), off(4, { status: "working" }),
        on(5, { unread: true, needs_attention: true }), off(6, { unread: true, needs_attention: true }),
        { ...off(7), reviewLater: true },
      ];
      expect(counts(list)).toEqual({ attention: 2, unread: 2, working: 1 });
    });
  });

  it("the footer counts the cards shown as Qt does: work while online, attention unless muted (SessionsWindowRows.cpp:468-474)", () => {
    const off = (n: number, extra: Partial<ViewRow> = {}) =>
      toSession("b", `run:${n}`, { name: `claude/sample-project/s${n}`, machine: "b", status: "ready", ...extra });
    const asking = { status: "attention", needs_action: true, needs_attention: true } as const;
    const cards = [
      session(1, { status: "working" }), session(2, { status: "background" }), off(3, { status: "working" }),
      session(4, asking), session(5, { ...asking, muted: true }), off(6, asking), { ...off(7), reviewLater: true },
    ];
    const rows = groupRows(input(grouping([header("alpha")]), new Map([["alpha", cards]])));
    expect(summarize(rows, (s) => s.computer === "")).toEqual({ shown: 7, working: 2, attention: 2 });
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
