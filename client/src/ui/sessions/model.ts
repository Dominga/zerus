import { createMemo } from "solid-js";
import { archiveRows } from "../../store/archive";
import { catalog, computerFresh, orderedRows } from "../../store/catalog";
import { defaultProject, grouping, groups } from "../../store/organization";
import { stateFilter, matchesFilter, search, machineFilter, projectFilter, expandedChildren, compactRows } from "../../store/ui";
import { headerAttention, isWorking, needsAttention } from "../../store/attention";
import type { GroupingOut } from "../../types";
import type { Session, Subagent } from "./types";

// Flattened list model. Heights match tray/src/SessionCardDelegate.h
// sizeHint at the compact density, the only one in the web client: group
// header 40, card 94, native child 64, collapsed row 40. Fixed heights per row
// type let the virtualizer place rows without measuring the DOM.

export type Row =
  /** `project` is the project's id; `name` and the colour come from the organization's header. */
  | { type: "header"; id: string; project: string; name: string; color: string | null; palette: number; collapsed: boolean; total: number; attention: number; unread: number; working: number }
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

export interface ListInput {
  grouping: GroupingOut;
  /** Each project's rows in list order (groupMembers). */
  members: Map<string, Session[]>;
  /** The state, machine and search filters. */
  visible: (s: Session) => boolean;
  /** The window's online state of a row's computer (Qt's `entry.online`). */
  online: (s: Session) => boolean;
  /** The project filter's project ids; empty is every project. */
  projects: readonly string[];
  expanded: (key: string) => boolean;
  compact: (key: string) => boolean;
}

/** Qt's grouped rows (SessionsWindowRows.cpp:289-338, header counts 393-411). */
export function groupRows(input: ListInput): Row[] {
  const out: Row[] = [];
  const cards = (list: readonly Session[]) => {
    for (const s of list) {
      const kids = s.subagents ?? [];
      const expanded = kids.length > 0 && input.expanded(s.key);
      out.push({ type: "card", id: s.key, session: s, expandable: kids.length > 0, expanded, compactRow: input.compact(s.key) });
      if (expanded) kids.forEach((child, i) => out.push({ type: "child", id: `${s.key}/${child.id}`, parent: s, child, first: i === 0, last: i === kids.length - 1 }));
    }
  };
  const wanted = (id: string) => !input.projects.length || input.projects.includes(id);
  const headers = input.grouping.loaded ? input.grouping.headers : [];
  if (headers.length <= 1) {
    // One project, or none presented yet: no header, and nothing collapses (293, 324, 326).
    for (const [id, list] of input.members) if (wanted(id)) cards(list.filter(input.visible));
    return out;
  }
  for (const h of headers) {
    if (!wanted(h.id)) continue;
    const all = input.members.get(h.id) ?? [];
    const list = all.filter(input.visible);
    // A project without visible members stays hidden (323; "Hide projects without active sessions" defaults on, 109).
    if (!list.length) continue;
    // The counters cover members the filters hide; muted members never count as attention or unread (398-404).
    // A computer that is not online counts only Review later as attention and nothing as working.
    let attention = 0, unread = 0, working = 0;
    for (const s of all) {
      const up = input.online(s);
      if (!s.muted && headerAttention(s, up)) attention++;
      // An offline computer's unread replies count too (visibleUnreadReply, HgsClient.h:104-105).
      if (!s.muted && s.unread) unread++;
      if (isWorking(s, up)) working++;
    }
    out.push({ type: "header", id: `h:${h.id}`, project: h.id, name: h.name, color: h.color, palette: h.palette,
               collapsed: h.collapsed, total: list.length, attention, unread, working });
    if (!h.collapsed) cards(list);
  }
  return out;
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
  return groupRows({
    grouping: grouping(), members: membersByGroup(), visible, online, projects: projectFilter(),
    expanded: (k) => !!expandedChildren[k], compact: (k) => !!compactRows[k],
  });
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

/** Projects with members and their member counts, in the organization's order (the project filter's choices, by id). */
export const projectChoices = createMemo(() => {
  const g = grouping();
  const members = membersByGroup();
  if (!g.loaded) return [];
  return g.headers.filter((h) => members.get(h.id)?.length).map((h) => ({ id: h.id, label: h.name, count: members.get(h.id)!.length }));
});
