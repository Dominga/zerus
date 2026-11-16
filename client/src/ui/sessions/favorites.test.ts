import { describe, expect, it, vi } from "vitest";
import { toSession } from "../../store/catalog";
import type { ViewRow } from "../../types";
import { groupRows, parentIndex, type ListInput } from "./model";

vi.hoisted(() => vi.stubGlobal("location", new URL("http://localhost/")));
const MACHINE = "11111111-1111-4111-8111-111111111111";
const id = (n: number) => `00000000-0000-4000-8000-${String(n).padStart(12, "0")}`;
// Synthetic ViewRows, mapped by the production catalog writer.
const row = (n: number, parent?: number, extra: Partial<ViewRow> = {}) => toSession("", `run:${n}`, {
  name: `codex/project/session-${n}`, session_id: id(n), conversation_id: `conversation-${n}`,
  state: "running", status: "ready", parent: parent ? { machine: MACHINE, session_id: id(parent),
    name: `codex/project/session-${parent}`, conversation_id: `conversation-${parent}`, run_id: `run-${parent}`, state: "live" } : undefined, ...extra,
});
const lead = row(1, undefined, { state: "paused", status: "paused" });
const middle = row(2, 1), favorite = row(3, 2), sibling = row(4, 2), other = row(5);
function input(projectCollapsed = false, favorites = [favorite.key]): ListInput {
  const rows = [lead, middle, favorite, sibling];
  return { grouping: { loaded: true, ready: true, reason: null, default_project: "project", busy: false, status: "", status_error: false, conflicts: 0, headers: [
    { id: "project", name: "Project", color: null, palette: 0, collapsed: projectCollapsed, accessible: true, is_default: true },
    { id: "other", name: "Other", color: null, palette: 1, collapsed: false, accessible: true, is_default: false },
  ] }, members: new Map([["project", rows], ["other", [other]]]), visible: () => true, online: () => true,
    projects: [], expanded: () => false, compact: () => false, parentKey: parentIndex(rows, () => MACHINE),
    containers: true, machineOf: () => MACHINE, teamCollapsed: (key) => key === lead.key,
    favorite: (session) => favorites.includes(session.key),
  };
}
const keys = (arg: ListInput) => groupRows(arg).flatMap((r) => r.type === "card" ? [r.session.key] : []);

describe("favorite sessions stay in the existing hierarchy (zerus-ux18t)", () => {
  it("reveals only a favorite and its ancestor path below a paused collapsed coordinator", () => {
    expect(keys(input())).toEqual([lead.key, middle.key, favorite.key, other.key]);
    expect(groupRows(input()).find((r) => r.type === "card" && r.id === lead.key)).toMatchObject({ team: { collapsed: true, total: 3 } });
  });
  it("keeps project and parent context when both levels are collapsed, without expanding either", () => {
    const rows = groupRows(input(true));
    expect(rows.map((r) => r.id)).toEqual(["h:project", lead.key, middle.key, favorite.key, "h:other", other.key]);
    expect(rows[0]).toMatchObject({ collapsed: true, total: 4 });
    expect(rows.find((r) => r.id === favorite.key)).toMatchObject({ depth: 2, parentKey: middle.key });
    expect(rows.find((r) => r.id === favorite.key)).toMatchObject({ lastChild: true });
  });
  it("removing a favorite hides it immediately without revealing siblings", () => {
    expect(keys(input(true, []))).toEqual([other.key]);
    expect(keys(input(false, []))).toEqual([lead.key, other.key]);
  });
  it("never duplicates favorites when all groups are open", () => {
    const arg = input(); arg.teamCollapsed = () => false;
    expect(keys(arg)).toEqual([lead.key, middle.key, favorite.key, sibling.key, other.key]);
    expect(new Set(groupRows(arg).map((r) => r.id)).size).toBe(groupRows(arg).length);
  });
  it("keeps text/state/scope filtering authoritative and project counts unchanged", () => {
    const arg = input(true); arg.visible = (s) => s.key !== favorite.key;
    expect(keys(arg)).toEqual([other.key]);
    expect(groupRows(arg)[0]).toMatchObject({ total: 3 });
    arg.projects = ["other"];
    expect(groupRows(arg).map((r) => r.id)).toEqual(["h:other", other.key]);
  });
  it("keeps an ended parent presentational when the state filter hides it", () => {
    const arg = input(true); arg.visible = (s) => s.key !== lead.key;
    const rows = groupRows(arg);
    expect(rows.find((r) => r.type === "container")).toMatchObject({ container: { key: lead.key } });
    expect(keys(arg)).toEqual([middle.key, favorite.key, other.key]);
  });
  it("does not expand native internal subagents inside a collapsed project", () => {
    const arg = input(true, [middle.key]); arg.roster = (s) => s.key === middle.key ? [{ id: "native-child", name: "Native", summary: "", state: "ready", inactive: false }] : [];
    expect(groupRows(arg).some((r) => r.type === "child")).toBe(false);
    expect(keys(arg)).toEqual([lead.key, middle.key, other.key]);
  });
});
