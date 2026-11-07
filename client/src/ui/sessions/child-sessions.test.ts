import { describe, expect, it, vi } from "vitest";
import { toSession } from "../../store/catalog";
import { groupRows, parentIndex, type ListInput, type Row } from "./model";
import type { GroupingOut, ParentLinkOut, ViewRow } from "../../types";
import type { Session } from "./types";

// The list model's module reads the page's query (store/ui.ts); the tests run without a page.
vi.hoisted(() => { vi.stubGlobal("location", new URL("http://localhost/")); });

const MACHINE = "11111111-1111-4111-8111-111111111111";
const id = (n: number) => `00000000-0000-4000-8000-${String(n).padStart(12, "0")}`;
const link = (n: number, extra: Partial<ParentLinkOut> = {}): ParentLinkOut =>
  ({ machine: MACHINE, session_id: id(n), name: `claude/p/s${n}`, conversation_id: `c${n}`, run_id: `r${n}`, state: "live", ...extra });
const session = (n: number, parent?: number, extra: Partial<ViewRow> = {}): Session =>
  toSession("", `run:${n}`, { name: `claude/p/s${n}`, machine: "workstation", status: "ready", session_id: id(n),
    conversation_id: `c${n}`, parent: parent === undefined ? undefined : link(parent), ...extra });
const header = (hid: string) => ({ id: hid, name: hid, color: null, accessible: true, collapsed: false, is_default: hid === "ungrouped", palette: 0 });
const grouping = (headers: ReturnType<typeof header>[]): GroupingOut => ({
  loaded: true, default_project: "ungrouped", headers, ready: true, reason: null, busy: false, status: "", status_error: false, conflicts: 0,
});
const rows = (list: Session[], extra: Partial<ListInput> = {}, all: Session[] = list) => groupRows({
  grouping: grouping([header("alpha")]), members: new Map([["alpha", list]]), visible: () => true, online: () => true, projects: [],
  expanded: () => false, compact: () => false, parentKey: parentIndex(all, () => MACHINE), ...extra,
});
const cards = (out: Row[]) => out.filter((r): r is Extract<Row, { type: "card" }> => r.type === "card");
const shape = (out: Row[]) => cards(out).map((r) => `${"  ".repeat(r.depth ?? 0)}${r.session.name.slice(-2)}`);

describe("child sessions in the list", () => {
  it("nests children under their parent in rank order, several levels deep", () => {
    const out = rows([session(1), session(2, 1), session(3), session(4, 2), session(5, 1)]);
    expect(shape(out)).toEqual(["s1", "  s2", "    s4", "  s5", "s3"]);
    const lead = cards(out)[0];
    expect(lead.team).toEqual({ total: 3, working: 0, asking: 0, needsYou: 0, collapsed: false });
    expect(cards(out)[1]).toMatchObject({ parentKey: lead.session.key, lastChild: false });
    expect(cards(out)[3]).toMatchObject({ parentKey: lead.session.key, lastChild: true });
  });

  it("counts working descendants and collapses a parent's children without promoting them", () => {
    const out = rows([session(1), session(2, 1, { status: "working" }), session(3, 2)], { teamCollapsed: (k) => k === "\nrun:1" });
    expect(shape(out)).toEqual(["s1"]);
    expect(cards(out)[0].team).toEqual({ total: 2, working: 1, asking: 0, needsYou: 0, collapsed: true });
  });

  it("tells a parent which children wait for its answer and which need the person (zerus-sa9qn.3)", () => {
    const out = rows([session(1), session(2, 1, { status: "attention", quiet: true }), session(3, 1, { status: "attention" }),
      session(4, 2, { status: "attention", quiet: true }), session(5, 1, { status: "working" })]);
    expect(cards(out)[0].team).toEqual({ total: 4, working: 1, asking: 2, needsYou: 1, collapsed: false });
  });

  it("keeps a child at top level, naming its parent, when the parent is elsewhere or not listed", () => {
    const parent = session(1), child = session(2, 1), orphan = session(3, 9);
    const out = rows([child, orphan], {}, [parent, child, orphan]);
    expect(shape(out)).toEqual(["s2", "s3"]);
    expect(cards(out)[0].fromParent).toEqual({ name: "claude/p/s1", key: parent.key });
    expect(cards(out)[1].fromParent).toEqual({ name: "claude/p/s9", key: undefined });
  });

  it("never hides a session in a parent loop", () => {
    const out = rows([session(1, 2), session(2, 1), session(3, 3)]);
    expect(shape(out).map((s) => s.trim()).sort()).toEqual(["s1", "s2", "s3"]);
    expect(shape(out)[0]).toBe("s1");
  });

  it("handles a 20,000-session chain in one pass, nesting at most eight levels (zerus-x94c7)", () => {
    const chain = Array.from({ length: 20000 }, (_, i) => session(i + 1, i ? i : undefined));
    const started = performance.now();
    const out = cards(rows(chain));
    expect(performance.now() - started).toBeLessThan(3000);
    expect(out).toHaveLength(20000);
    expect(Math.max(...out.map((r) => r.depth ?? 0))).toBe(8);
    expect(out[0].team?.total).toBe(8);
    // The ninth level starts at top level again, naming its parent.
    expect(out[9]).toMatchObject({ depth: 0, fromParent: { name: "claude/p/s9" } });
  });

  it("handles a 40,000-child star in one pass (zerus-x94c7)", () => {
    const star = Array.from({ length: 40000 }, (_, i) => session(i + 1, i ? 1 : undefined));
    const started = performance.now();
    const out = cards(rows(star));
    expect(performance.now() - started).toBeLessThan(3000);
    expect(out).toHaveLength(40000);
    expect(out[0].team?.total).toBe(39999);
  });

  it("matches a parent by machine and session ID, never by name", () => {
    const parent = session(1), other = toSession("peer", "run:1", { name: "claude/p/s1", session_id: id(7) });
    const find = parentIndex([parent, other], (computer) => (computer === "" ? MACHINE : "99999999-9999-4999-8999-999999999999"));
    expect(find(session(2, 1))).toBe(parent.key);
    expect(find(session(2, 7))).toBeUndefined();
    expect(find(session(2, undefined, { parent: link(1, { machine: "99999999-9999-4999-8999-999999999999" }) }))).toBeUndefined();
  });

  it("falls back to the conversation only for a parent listed without a session ID", () => {
    const old = session(1, undefined, { session_id: undefined });
    const find = parentIndex([old], () => MACHINE);
    expect(find(session(2, undefined, { parent: link(1, { session_id: "" }) }))).toBe(old.key);
    expect(find(session(2, 1))).toBe(old.key);
    const identified = session(1);
    expect(parentIndex([identified], () => MACHINE)(session(2, undefined, { parent: link(5, { conversation_id: "c1" }) }))).toBeUndefined();
  });
});
