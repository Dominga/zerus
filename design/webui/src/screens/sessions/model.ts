import { createMemo } from "solid-js";
import { catalog, collapsed, stateFilter, matchesFilter, search, machineFilter, projectFilter, expandedChildren, compactRows } from "../../state";
import { projects, machines } from "../../mock/data";
import type { Session, Subagent } from "../../mock/types";

// Flattened list model. Heights match tray/src/SessionCardDelegate.h
// sizeHint: group header 40, card 104 (compact 94), native child 64,
// collapsed (compact) row 40. Fixed heights per row type let the virtualizer
// place rows without measuring the DOM.

export type Row =
  | { type: "header"; id: string; project: string; total: number; attention: number; unread: number; working: number }
  | { type: "card"; id: string; session: Session; expandable: boolean; expanded: boolean; compactRow: boolean }
  | { type: "child"; id: string; parent: Session; child: Subagent; last: boolean };

export const ROW_HEIGHT = { header: 40, card: 104, compactCard: 94, child: 64, collapsedRow: 40 } as const;

export function rowHeight(row: Row, compactList: boolean): number {
  if (row.type === "header") return ROW_HEIGHT.header;
  if (row.type === "child") return ROW_HEIGHT.child;
  if (row.compactRow) return ROW_HEIGHT.collapsedRow;
  return compactList ? ROW_HEIGHT.compactCard : ROW_HEIGHT.card;
}

const machineByAlias = new Map(machines.map((m) => [m.alias, m]));
export const machineOf = (alias: string) => machineByAlias.get(alias);

export function sessionMatchesSearch(s: Session, q: string): boolean {
  if (!q) return true;
  const needle = q.toLowerCase();
  return s.name.toLowerCase().includes(needle) || s.summary.toLowerCase().includes(needle) || s.machine.includes(needle) || (s.branch ?? "").includes(needle);
}

export const listRows = createMemo<Row[]>(() => {
  const f = stateFilter();
  const q = search();
  const hosts = machineFilter();
  const projs = projectFilter();
  const groups = new Map<string, Session[]>();
  for (const s of catalog.rows) {
    if (!matchesFilter(s, f)) continue;
    if (hosts.length && !hosts.includes(s.machine)) continue;
    if (projs.length && !projs.includes(s.project)) continue;
    if (!sessionMatchesSearch(s, q)) continue;
    const list = groups.get(s.project);
    if (list) list.push(s);
    else groups.set(s.project, [s]);
  }
  const rows: Row[] = [];
  for (const p of projects) {
    const list = groups.get(p.id);
    if (!list?.length) continue;
    let attention = 0, unread = 0, working = 0;
    for (const s of list) {
      if (s.status === "attention") attention++;
      else if (s.unread) unread++;
      if (s.status === "working") working++;
    }
    rows.push({ type: "header", id: `h:${p.id}`, project: p.id, total: list.length, attention, unread, working });
    if (collapsed[p.id]) continue;
    for (const s of list) {
      const kids = s.subagents ?? [];
      const expanded = kids.length > 0 && !!expandedChildren[s.key];
      rows.push({ type: "card", id: s.key, session: s, expandable: kids.length > 0, expanded, compactRow: !!compactRows[s.key] });
      if (expanded) kids.forEach((child, i) => rows.push({ type: "child", id: `${s.key}/${child.id}`, parent: s, child, last: i === kids.length - 1 }));
    }
  }
  return rows;
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
