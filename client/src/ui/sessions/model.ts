import { groupRows, type Row } from "./group-rows";
export { groupRows, type Row, type ListInput } from "./group-rows";
import { createMemo } from "solid-js";
import { archive, archiveRows, fleetComputers } from "../../store/archive";
import { catalog, computerFresh, orderedRows } from "../../store/catalog";
import { machineCards } from "../../store/machines";
import { archiveFilterCounts, archiveMachineChoices, archiveProjectChoices } from "./archive-filters";
import { defaultProject, grouping, groups } from "../../store/organization";
import { stateFilter, matchesFilter, search, machineFilter, projectFilter, expandedChildren, compactRows } from "../../store/ui";
import { withCreations } from "../../store/creations";
import { isWorking, needsAttention } from "../../store/attention";
import type { Session } from "./types";

// Flattened list model. Heights match tray/src/SessionCardDelegate.h
// sizeHint at the compact density, the only one in the web client: group
// header 40, card 94, native child 64, collapsed row 40. Fixed heights per row
// type let the virtualizer place rows without measuring the DOM.

export const ROW_HEIGHT = { header: 40, card: 94, child: 64, collapsedRow: 40 } as const;

/** In the strip every row keeps its height except compact rows, which show as tiles. */
export function rowHeight(row: Row, strip = false): number {
  if (row.type === "header") return ROW_HEIGHT.header;
  if (row.type === "child") return ROW_HEIGHT.child;
  if (row.type === "creation") return ROW_HEIGHT.card;
  if (row.compactRow && !strip) return ROW_HEIGHT.collapsedRow;
  return ROW_HEIGHT.card;
}

export function sessionMatchesSearch(s: Session, q: string): boolean {
  if (!q) return true;
  const needle = q.toLowerCase();
  return s.name.toLowerCase().includes(needle) || s.title.toLowerCase().includes(needle) || s.summary.toLowerCase().includes(needle)
    || s.machine.toLowerCase().includes(needle) || (s.branch ?? "").toLowerCase().includes(needle);
}

/**
 * Each project's rows in list order: by rank, ties in arrival order (the
 * sort is stable); a row without an assignment is in `fallback`. It reads
 * only row ids and assignments, so a status upsert does not run it again.
 */
export function groupMembers(order: readonly string[], rows: Record<string, Session>,
                             assigned: Record<string, { group: string; rank: number } | undefined>, fallback: string): Map<string, Session[]> {
  const map = new Map<string, Session[]>();
  const rank = new Map<string, number>();
  for (const id of order) {
    const s = rows[id];
    if (!s) continue;
    const a = assigned[id];
    rank.set(id, a?.rank ?? Number.MAX_SAFE_INTEGER);
    const group = a?.group ?? fallback;
    const list = map.get(group);
    if (list) list.push(s);
    else map.set(group, [s]);
  }
  for (const list of map.values()) list.sort((x, y) => rank.get(x.key)! - rank.get(y.key)!);
  return map;
}

/** The window's online state of a row (Qt's `entry.online`). */
const online = (s: Session) => computerFresh(s.computer);

/** The list's rows by project, from the row ids and the assignments only. */
export const membersByGroup = createMemo(() => groupMembers(catalog.order, catalog.rows, groups, defaultProject()));

export const listRows = createMemo<Row[]>(() => {
  if (stateFilter() === "archived") return archiveRows();
  const f = stateFilter();
  const q = search().trim();
  const hosts = machineFilter();
  const visible = (s: Session) => matchesFilter(s, f) && (!hosts.length || hosts.includes(s.machine)) && sessionMatchesSearch(s, q);
  const scope = {
    grouping: grouping(), members: membersByGroup(), visible, online, projects: projectFilter(),
    expanded: (k: string) => !!expandedChildren[k], compact: (k: string) => !!compactRows[k], filter: f, hosts, query: q,
  };
  const out = groupRows(scope);
  return withCreations(out, scope);
});

/** The list footer's counts of the cards shown (SessionsWindowRows.cpp:468-474). */
export function summarize(rows: readonly Row[], up: (s: Session) => boolean) {
  let shown = 0, working = 0, attention = 0;
  for (const r of rows) {
    if (r.type !== "card") continue;
    shown++;
    const s = r.session;
    if (isWorking(s, up(s))) working++;
    if (!s.muted && needsAttention(s, up(s))) attention++;
  }
  return { shown, working, attention };
}

export const shownSummary = createMemo(() => summarize(listRows(), online));

const archivedCounts = createMemo(() => archiveFilterCounts(archive.rows(), archive.includes, machineFilter(), projectFilter()));

/** Machines with their session counts, in first-seen order (the machine filter's choices). */
export const machineChoices = createMemo(() => {
  if (stateFilter() === "archived") return archiveMachineChoices(fleetComputers(catalog.computers, orderedRows(), machineCards), archivedCounts().machine);
  const seen = new Map<string, { id: string; label: string; count: number; computer: string }>();
  for (const s of orderedRows()) {
    const m = seen.get(s.machine) ?? { id: s.machine, label: s.computer === "" ? `${s.machine} (this machine)` : s.machine, count: 0, computer: s.computer };
    m.count++;
    seen.set(s.machine, m);
  }
  return [...seen.values()];
});

/** Projects with members and their member counts, in the organization's order (the project filter's choices, by id). */
export const projectChoices = createMemo(() => {
  const g = grouping();
  if (stateFilter() === "archived") return archiveProjectChoices(g, archivedCounts().project, projectFilter());
  const members = membersByGroup();
  if (!g.loaded) return [];
  return g.headers.filter((h) => members.get(h.id)?.length).map((h) => ({ id: h.id, label: h.name, count: members.get(h.id)!.length }));
});
