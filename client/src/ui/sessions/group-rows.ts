import { countedHeaderAttention, isWorking, needsAttention, quietAttention, visibleUnreadReply } from "../../store/attention";
import type { CreationCard, GroupingOut } from "../../types";
import type { Session, Subagent } from "./types";
import { currentChildren } from "./subagents";
import { sessionIs } from "../../store/teamsView";
import type { TeamSummary } from "../../types";

export type Row =
  /** `project` is the project's id; `name` and the colour come from the organization's header. */
  | { type: "header"; id: string; project: string; name: string; color: string | null; palette: number; collapsed: boolean; total: number; attention: number; unread: number; working: number }
  | { type: "card"; id: string; session: Session; expandable: boolean; expanded: boolean; compactRow: boolean;
      /** Nesting under parent sessions (zerus-sa9qn.2) and under a team lead: 0 at top level. */
      depth?: number;
      /** The nested card's parent row key and whether it is its parent's last shown child. */
      parentKey?: string; lastChild?: boolean;
      /** A parent's child sessions: all descendants in this group and those working. */
      team?: Team;
      /** A child whose parent is not shown above it: the parent's name. */
      fromParent?: { name: string; key?: string };
      /** A Teams lead's team (its strip); a member under its lead is `teamFirst` and `teamLast` at the ends. */
      teamSummary?: TeamSummary; teamFirst?: boolean; teamLast?: boolean }
  | { type: "child"; id: string; parent: Session; child: Subagent; first: boolean; last: boolean }
  | { type: "creation"; id: string; creation: CreationCard };

/**
 * A parent's child sessions: all, working, quiet requests its children hold
 * for it (`asking`) and requests only the person can answer (`needsYou`).
 * Informational: it never raises the parent's own attention (spec 2.3).
 */
export interface Team { total: number; working: number; collapsed: boolean; asking?: number; needsYou?: number; errors?: number }

/** Nesting stops after this many levels; deeper children start at top level (spec 1.4). */
export const MAX_DEPTH = 8;

export interface ListInput {
  grouping: GroupingOut;
  /** Each project's rows in list order (groupMembers). */
  members: Map<string, Session[]>;
  /** The state, machine and search filters. */
  visible: (s: Session) => boolean;
  /** Whether some filter or search is active: an empty project is then always hidden (SessionsWindowRows.cpp:559). */
  filtering?: boolean;
  /** "Hide projects without active sessions" (SettingsPage.h:232), on by default. */
  hideEmpty?: boolean;
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
  /** The Teams team a row belongs to, when its computer's team list is loaded. */
  teamOf?: (s: Session) => TeamSummary | undefined;
  /** Whether this Teams team's members are folded under their lead (workspace/collapsedTeams). */
  teamFolded?: (team: string) => boolean;
}

/** Qt's grouped rows (SessionsWindowRows.cpp:289-338, header counts 393-411). */
/**
 * A parent's team with its shown native subagents counted in (zerus-sa9qn.16):
 * they add to all and working. Their requests and errors are the parent's
 * own and already show on its card, so they are not counted twice.
 */
export function withSubagents(team: Team | undefined, kids: readonly Subagent[], collapsed: boolean): Team | undefined {
  if (!kids.length) return team;
  return {
    ...(team ?? { asking: 0, needsYou: 0, errors: 0 }),
    total: (team?.total ?? 0) + kids.length,
    working: (team?.working ?? 0) + kids.filter((child) => child.state === "working").length,
    collapsed: team?.collapsed ?? collapsed,
  };
}

export function groupRows(input: ListInput): Row[] {
  const out: Row[] = [];
  // A Teams team's members are not separate list rows: they sit under their
  // lead (Qt's SessionRoles::TeamMember tree). Only a visible lead takes them,
  // so a member filtered into view on its own still lists normally.
  const everyone = [...input.members.values()].flat();
  const teamFor = new Map<string, TeamSummary>();
  for (const s of everyone) {
    const team = input.teamOf?.(s);
    if (team) teamFor.set(s.key, team);
  }
  const leadOf = new Map<string, Session>();
  const membersOf = new Map<string, Session[]>();
  for (const team of new Set(teamFor.values())) {
    const leadMember = team.members.find((m) => m.lead);
    const lead = leadMember ? everyone.find((s) => sessionIs(leadMember.session, s.computer, s)) : undefined;
    if (!lead) continue;
    leadOf.set(team.id, lead);
    membersOf.set(team.id, team.members.filter((m) => !m.lead)
      .map((m) => everyone.find((s) => sessionIs(m.session, s.computer, s)))
      .filter((s): s is Session => !!s));
  }
  const nested = new Set<string>();
  for (const [team, lead] of leadOf) {
    if (!teamFor.has(lead.key)) continue;
    const visibleLead = [...input.members.values()].some((list) => list.some((s) => s.key === lead.key && input.visible(s)));
    if (visibleLead) for (const member of membersOf.get(team) ?? []) nested.add(member.key);
  }
  const card = (s: Session, extra: Partial<Extract<Row, { type: "card" }>> = {}) => {
      const current = currentChildren(s, input.online(s));
      const roster = input.children === false ? [] : (input.roster?.(s) ?? s.subagents ?? []);
      const kids = roster.filter((child) =>
        input.includeCompleted
        || (input.selected?.key === s.key && input.selected.subagent === child.id
          && (!input.selected.conversation || input.selected.conversation === (s.conversationId ?? "")))
        || (!child.group && input.childDraft?.(s, child))
        || child.inactive === false).map((child): Subagent => !current && child.state === "working" ? { ...child, state: "unknown" } : child);
      // One counter and one toggle for a parent's subagents and child
      // sessions (owner, 2026-11-07, zerus-sa9qn.16): the toggle shows or
      // hides both, and is remembered across relaunches (zerus-sa9qn.15).
      const team = withSubagents(extra.team, kids, !!input.teamCollapsed?.(s.key));
      const expanded = kids.length > 0 && !team?.collapsed;
      const changed = kids.length !== roster.length || kids.some((child, index) => child !== roster[index]);
      const shown = changed ? { ...s, subagents: kids } : s;
      const summary = teamFor.get(s.key);
      const teamMembers = summary ? membersOf.get(summary.id) ?? [] : [];
      const isLead = !!summary && leadOf.get(summary.id)?.key === s.key;
      const membersShown = isLead && teamMembers.length > 0 && !(input.teamFolded?.(summary!.id) ?? false);
      out.push({ type: "card", id: s.key, session: shown, expandable: isLead && teamMembers.length > 0, expanded: expanded || membersShown, compactRow: input.compact(s.key), ...extra, team,
                 teamSummary: isLead ? summary : undefined });
      if (expanded) kids.forEach((child, i) => out.push({ type: "child", id: `${s.key}/${child.id}`, parent: shown, child, first: i === 0, last: i === kids.length - 1 }));
      if (membersShown) {
        teamMembers.forEach((member, i) => out.push({
          type: "card", id: member.key, session: member, expandable: false, expanded: false,
          compactRow: false, depth: (extra.depth ?? 0) + 1, teamFirst: i === 0, teamLast: i === teamMembers.length - 1,
        }));
      }
  };
  // One project group's visible cards in rank order, child sessions nested
  // under a parent shown in the same list. Nesting never hides a row: a loop
  // or a parent elsewhere leaves the card at top level. A Teams member
  // listed under its lead is not a card of its own.
  const cards = (members: readonly Session[]) => {
    const list = members.filter((s) => !nested.has(s.key));
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
      if (!parent) continue;
      const siblings = children.get(parent);
      if (siblings) siblings.push(s);
      else children.set(parent, [s]);
    }
    // One depth-first pass from each root in rank order; a loop's first card
    // in rank order becomes its root.
    interface Node { s: Session; depth: number; parent?: string; last: boolean }
    const nodes: Node[] = [];
    const seen = new Set<string>();
    const walk = (root: Session) => {
      const stack: Node[] = [{ s: root, depth: 0, last: true }];
      while (stack.length) {
        const node = stack.pop()!;
        if (seen.has(node.s.key)) continue;
        seen.add(node.s.key);
        nodes.push(node);
        if (node.depth >= MAX_DEPTH) continue;
        const kids = (children.get(node.s.key) ?? []).filter((c) => !seen.has(c.key));
        for (let i = kids.length - 1; i >= 0; i--) stack.push({ s: kids[i], depth: node.depth + 1, parent: node.s.key, last: i === kids.length - 1 });
      }
    };
    // Cards below a session without a shown parent; the rest sit in a loop.
    const rooted = new Set<string>();
    for (const s of list) {
      if (parentOf.has(s.key)) continue;
      const stack = [s];
      while (stack.length) {
        const at = stack.pop()!;
        if (rooted.has(at.key)) continue;
        rooted.add(at.key);
        stack.push(...(children.get(at.key) ?? []));
      }
    }
    // In rank order: a real root, or the first card of a loop, starts a walk.
    for (const s of list) if (!seen.has(s.key) && (!parentOf.has(s.key) || !rooted.has(s.key))) walk(s);
    // Below the depth bound a card starts at top level again; nothing is hidden.
    for (const s of list) if (!seen.has(s.key)) walk(s);
    // Each parent's descendants, summed once from the leaves up (linear).
    const teams = new Map<string, Required<Omit<Team, "collapsed">>>();
    for (let i = nodes.length - 1; i >= 0; i--) {
      const { s, parent } = nodes[i];
      if (!parent) continue;
      const below = teams.get(s.key), sum = teams.get(parent) ?? { total: 0, working: 0, asking: 0, needsYou: 0, errors: 0 };
      const up = input.online(s);
      // A quiet request waits for the parent; any other one needs the person (spec 2.3).
      const asks = up && s.status === "attention" && s.state !== "archived";
      sum.total += 1 + (below?.total ?? 0);
      sum.working += (isWorking(s, up) ? 1 : 0) + (below?.working ?? 0);
      sum.asking += (asks && s.quiet ? 1 : 0) + (below?.asking ?? 0);
      sum.errors += (up && s.status === "error" && s.state !== "archived" ? 1 : 0) + (below?.errors ?? 0);
      sum.needsYou += (!s.muted && (asks ? !s.quiet : !s.quiet && needsAttention(s, up)) ? 1 : 0) + (below?.needsYou ?? 0);
      teams.set(parent, sum);
    }
    const hidden = new Set<string>();
    for (const { s, depth, parent, last } of nodes) {
      // Hidden with a collapsed parent; never promoted to top level.
      if (parent && hidden.has(parent)) { hidden.add(s.key); continue; }
      const counts = teams.get(s.key);
      const collapsed = !!counts && !!input.teamCollapsed?.(s.key);
      if (collapsed) hidden.add(s.key);
      const from = !parent && s.parent ? { fromParent: { name: s.parent.name, key: input.parentKey?.(s) } } : {};
      card(s, { depth, parentKey: parent, lastChild: parent ? last : undefined, team: counts && { ...counts, collapsed }, ...from });
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
    // A project without visible members stays hidden while filtering, when
    // the owner asked to hide empty projects (the default), and when the
    // organization cannot reach it (SessionsWindowRows.cpp:559, 323).
    if (!list.length && (input.filtering || input.hideEmpty !== false || !h.accessible)) continue;
    // The counters cover members the filters hide; muted members never count as attention or unread (398-404).
    // A computer that is not online counts only Review later as attention and nothing as working.
    let attention = 0, unread = 0, working = 0;
    for (const s of all) {
      const up = input.online(s);
      if (countedHeaderAttention(s, up)) attention++;
      // An offline computer's unread replies count too (visibleUnreadReply, HgsClient.h:104-105).
      if (!quietAttention(s) && visibleUnreadReply(s, up)) unread++;
      if (isWorking(s, up)) working++;
    }
    out.push({ type: "header", id: `h:${h.id}`, project: h.id, name: h.name, color: h.color, palette: h.palette,
               collapsed: h.collapsed, total: list.length, attention, unread, working });
    if (!h.collapsed) cards(list);
  }
  return out;
}
