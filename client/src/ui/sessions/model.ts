import { createMemo } from "solid-js";
import { archiveRows } from "../../store/archive";
import { orderedRows } from "../../store/catalog";
import { collapsed, stateFilter, matchesFilter, search, machineFilter, projectFilter, expandedChildren, compactRows, projectOrder } from "../../store/ui";
import type { Session, Subagent } from "./types";

// Flattened list model. Heights match tray/src/SessionCardDelegate.h
// sizeHint at the compact density, the only one in the web client: group
// header 40, card 94, native child 64, collapsed row 40. Fixed heights per row
// type let the virtualizer place rows without measuring the DOM.

export type Row =
  | { type: "header"; id: string; project: string; total: number; attention: number; unread: number; working: number }
  | { type: "card"; id: string; session: Session; expandable: boolean; expanded: boolean; compactRow: boolean }
  | { type: "child"; id: string; parent: Session; child: Subagent; first: boolean; last: boolean };

export const ROW_HEIGHT = { header: 40, card: 94, child: 64, collapsedRow: 40 } as const;

/** In the strip every row keeps its height except compact rows, which show as tiles. */
export function rowHeight(row: Row, strip = false): number {
  if (row.type === "header") return ROW_HEIGHT.header;
  if (row.type === "child") return ROW_HEIGHT.child;
  if (row.compactRow && !strip) return ROW_HEIGHT.collapsedRow;
  return ROW_HEIGHT.card;
}

export function sessionMatchesSearch(s: Session, q: string): boolean {
  if (!q) return true;
  const needle = q.toLowerCase();
  return s.name.toLowerCase().includes(needle) || s.title.toLowerCase().includes(needle) || s.summary.toLowerCase().includes(needle)
    || s.machine.toLowerCase().includes(needle) || (s.branch ?? "").toLowerCase().includes(needle);
}

export const listRows = createMemo<Row[]>(() => {
  if (stateFilter() === "archived") return archiveRows();
  const f = stateFilter();
  const q = search().trim();
  const hosts = machineFilter();
  const projs = projectFilter();
  const rows = orderedRows();
  const groups = new Map<string, Session[]>();
  for (const s of rows) {
    if (!matchesFilter(s, f)) continue;
    if (hosts.length && !hosts.includes(s.machine)) continue;
    if (projs.length && !projs.includes(s.project)) continue;
    if (!sessionMatchesSearch(s, q)) continue;
    const list = groups.get(s.project);
    if (list) list.push(s);
    else groups.set(s.project, [s]);
  }
  // Header counters cover every session of the project, including ones the
  // filters hide; muted sessions never count as attention or unread.
  const all = new Map<string, { attention: number; unread: number; working: number }>();
  for (const s of rows) {
    const c = all.get(s.project) ?? { attention: 0, unread: 0, working: 0 };
    if (!s.muted && s.status === "attention") c.attention++;
    else if (!s.muted && s.unread) c.unread++;
    if (s.status === "working" || s.status === "background") c.working++;
    all.set(s.project, c);
  }
  const out: Row[] = [];
  for (const project of projectOrder()) {
    const list = groups.get(project);
    if (!list?.length) continue;
    const { attention, unread, working } = all.get(project)!;
    out.push({ type: "header", id: `h:${project}`, project, total: list.length, attention, unread, working });
    if (collapsed[project]) continue;
    for (const s of list) {
      const kids = s.subagents ?? [];
      const expanded = kids.length > 0 && !!expandedChildren[s.key];
      out.push({ type: "card", id: s.key, session: s, expandable: kids.length > 0, expanded, compactRow: !!compactRows[s.key] });
      if (expanded) kids.forEach((child, i) => out.push({ type: "child", id: `${s.key}/${child.id}`, parent: s, child, first: i === 0, last: i === kids.length - 1 }));
    }
  }
  return out;
});

export const shownSummary = createMemo(() => {
  let shown = 0, working = 0, attention = 0;
  for (const r of listRows()) {
    if (r.type !== "card") continue;
    shown++;
    if (r.session.status === "working") working++;
    if (r.session.status === "attention") attention++;
  }
  return { shown, working, attention };
});

/** Machines with their session counts, in first-seen order (the machine filter's choices). */
export const machineChoices = createMemo(() => {
  const seen = new Map<string, { id: string; label: string; count: number; computer: string }>();
  for (const s of orderedRows()) {
    const m = seen.get(s.machine) ?? { id: s.machine, label: s.computer === "" ? `${s.machine} (this machine)` : s.machine, count: 0, computer: s.computer };
    m.count++;
    seen.set(s.machine, m);
  }
  return [...seen.values()];
});

/** Projects with their session counts, in list order (the project filter's choices). */
export const projectChoices = createMemo(() => {
  const counts = new Map<string, number>();
  for (const s of orderedRows()) counts.set(s.project, (counts.get(s.project) ?? 0) + 1);
  return projectOrder().filter((p) => counts.has(p)).map((p) => ({ id: p, label: p, count: counts.get(p)! }));
});
