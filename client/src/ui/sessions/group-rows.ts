import { headerAttention, isWorking, visibleUnreadReply } from "../../store/attention";
import type { CreationCard, GroupingOut } from "../../types";
import type { Session, Subagent } from "./types";
import { currentChildren } from "./subagents";

export type Row =
  /** `project` is the project's id; `name` and the colour come from the organization's header. */
  | { type: "header"; id: string; project: string; name: string; color: string | null; palette: number; collapsed: boolean; total: number; attention: number; unread: number; working: number }
  | { type: "card"; id: string; session: Session; expandable: boolean; expanded: boolean; compactRow: boolean;
      /** Nesting under parent sessions (zerus-sa9qn.2): 0 at top level. */
      depth?: number;
      /** The nested card's parent row key and whether it is its parent's last shown child. */
      parentKey?: string; lastChild?: boolean;
      /** A parent's child sessions: all descendants in this group and those working. */
      team?: Team;
      /** A child whose parent is not shown above it: the parent's name. */
      fromParent?: { name: string; key?: string } }
  | { type: "child"; id: string; parent: Session; child: Subagent; first: boolean; last: boolean }
  | { type: "creation"; id: string; creation: CreationCard };

export interface Team { total: number; working: number; collapsed: boolean }

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
  /** Archives never expand recorded subagents. */
  children?: boolean;
  includeCompleted?: boolean;
  selected?: { key?: string; subagent?: string; conversation?: string };
  roster?: (s: Session) => Subagent[];
  childDraft?: (parent: Session, child: Subagent) => boolean;
  /** The listed row key of a session's parent session, when the catalog has it. */
  parentKey?: (s: Session) => string | undefined;
  /** A parent whose child sessions are collapsed. */
  teamCollapsed?: (key: string) => boolean;
}

/** Qt's grouped rows (SessionsWindowRows.cpp:289-338, header counts 393-411). */
export function groupRows(input: ListInput): Row[] {
  const out: Row[] = [];
  const card = (s: Session, extra: Partial<Extract<Row, { type: "card" }>> = {}) => {
      const current = currentChildren(s, input.online(s));
      const roster = input.children === false ? [] : (input.roster?.(s) ?? s.subagents ?? []);
      const kids = roster.filter((child) =>
        input.includeCompleted
        || (input.selected?.key === s.key && input.selected.subagent === child.id
          && (!input.selected.conversation || input.selected.conversation === (s.conversationId ?? "")))
        || (!child.group && input.childDraft?.(s, child))
        || child.inactive === false).map((child): Subagent => !current && child.state === "working" ? { ...child, state: "unknown" } : child);
      const expanded = kids.length > 0 && input.expanded(s.key);
      const changed = kids.length !== roster.length || kids.some((child, index) => child !== roster[index]);
      const shown = changed ? { ...s, subagents: kids } : s;
      out.push({ type: "card", id: s.key, session: shown, expandable: kids.length > 0, expanded, compactRow: input.compact(s.key), ...extra });
      if (expanded) kids.forEach((child, i) => out.push({ type: "child", id: `${s.key}/${child.id}`, parent: shown, child, first: i === 0, last: i === kids.length - 1 }));
  };
  // One project group's visible cards in rank order, child sessions nested
  // under a parent shown in the same list. Nesting never hides a row: a loop
  // or a parent elsewhere leaves the card at top level.
  const cards = (list: readonly Session[]) => {
    const parentOf = new Map<string, string>();
    if (input.parentKey) {
      const shown = new Set(list.map((s) => s.key));
      for (const s of list) {
        const parent = input.parentKey(s);
        if (parent && parent !== s.key && shown.has(parent)) parentOf.set(s.key, parent);
      }
    }
    if (!parentOf.size) {
      for (const s of list) {
        const from = s.parent && input.parentKey ? input.parentKey(s) : undefined;
        card(s, s.parent ? { fromParent: { name: s.parent.name, key: from } } : {});
      }
      return;
    }
    const children = new Map<string, Session[]>();
    for (const s of list) {
      const parent = parentOf.get(s.key);
      if (parent) children.set(parent, [...(children.get(parent) ?? []), s]);
    }
    // A chain that loops back never reaches a root: its first card in rank order becomes one.
    const roots = list.filter((s) => {
      const seen = new Set<string>([s.key]);
      for (let at = parentOf.get(s.key); at; at = parentOf.get(at)) {
        if (seen.has(at)) return true;
        seen.add(at);
      }
      return !parentOf.has(s.key);
    });
    const team = (key: string, seen: Set<string>): Omit<Team, "collapsed"> => {
      let total = 0, working = 0;
      const stack = [...(children.get(key) ?? [])];
      while (stack.length) {
        const s = stack.pop()!;
        if (seen.has(s.key)) continue;
        seen.add(s.key); total++;
        if (isWorking(s, input.online(s))) working++;
        stack.push(...(children.get(s.key) ?? []));
      }
      return { total, working };
    };
    const placed = new Set<string>();
    for (const root of roots) {
      if (placed.has(root.key)) continue;
      const stack: { s: Session; depth: number; parent?: string; last: boolean }[] = [{ s: root, depth: 0, last: true }];
      while (stack.length) {
        const { s, depth, parent, last } = stack.pop()!;
        if (placed.has(s.key)) continue;
        placed.add(s.key);
        const kids = (children.get(s.key) ?? []).filter((c) => !placed.has(c.key));
        const counts = kids.length ? team(s.key, new Set([s.key])) : undefined;
        const collapsed = !!counts && !!input.teamCollapsed?.(s.key);
        const from = !parent && s.parent ? { fromParent: { name: s.parent.name, key: input.parentKey?.(s) } } : {};
        card(s, { depth, parentKey: parent, lastChild: parent ? last : undefined, team: counts && { ...counts, collapsed }, ...from });
        if (collapsed) {
          // Hidden with their parent; never promoted to top level.
          const hide = [...kids];
          while (hide.length) { const k = hide.pop()!; if (placed.has(k.key)) continue; placed.add(k.key); hide.push(...(children.get(k.key) ?? [])); }
          continue;
        }
        for (let i = kids.length - 1; i >= 0; i--) stack.push({ s: kids[i], depth: depth + 1, parent: s.key, last: i === kids.length - 1 });
      }
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
      if (!s.muted && visibleUnreadReply(s, up)) unread++;
      if (isWorking(s, up)) working++;
    }
    out.push({ type: "header", id: `h:${h.id}`, project: h.id, name: h.name, color: h.color, palette: h.palette,
               collapsed: h.collapsed, total: list.length, attention, unread, working });
    if (!h.collapsed) cards(list);
  }
  return out;
}
