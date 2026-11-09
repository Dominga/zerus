import { headerAttention, isWorking, visibleUnreadReply } from "../../store/attention";
import type { CreationCard, GroupingOut } from "../../types";
import type { Session, Subagent } from "./types";
import { currentChildren } from "./subagents";
import { sessionIs } from "../../store/teamsView";
import type { TeamSummary } from "../../types";

export type Row =
  /** `project` is the project's id; `name` and the colour come from the organization's header. */
  | { type: "header"; id: string; project: string; name: string; color: string | null; palette: number; collapsed: boolean; total: number; attention: number; unread: number; working: number }
  | { type: "card"; id: string; session: Session; expandable: boolean; expanded: boolean; compactRow: boolean;
      /** A team member under its lead: 22 px per level, a connector when it follows a lead. */
      depth?: number; team?: TeamSummary; teamFirst?: boolean; teamLast?: boolean }
  | { type: "child"; id: string; parent: Session; child: Subagent; first: boolean; last: boolean }
  | { type: "creation"; id: string; creation: CreationCard };

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
  /** The team a row belongs to, when its computer's team list is loaded. */
  teamOf?: (s: Session) => TeamSummary | undefined;
  /** Whether this team's members are folded (workspace/collapsedTeams). */
  teamCollapsed?: (team: string) => boolean;
}

/** Qt's grouped rows (SessionsWindowRows.cpp:289-338, header counts 393-411). */
export function groupRows(input: ListInput): Row[] {
  const out: Row[] = [];
  // A team's members are not separate list rows: they sit under their lead
  // (Qt's SessionRoles::TeamMember tree). Only a visible lead takes them, so a
  // member filtered into view on its own still lists normally.
  const all = [...input.members.values()].flat();
  const teamFor = new Map<string, TeamSummary>();
  for (const s of all) {
    const team = input.teamOf?.(s);
    if (team) teamFor.set(s.key, team);
  }
  const leadOf = new Map<string, Session>();
  const membersOf = new Map<string, Session[]>();
  for (const team of new Set(teamFor.values())) {
    const leadMember = team.members.find((m) => m.lead);
    const lead = leadMember ? all.find((s) => sessionIs(leadMember.session, s.computer, s)) : undefined;
    if (!lead) continue;
    leadOf.set(team.id, lead);
    membersOf.set(team.id, team.members.filter((m) => !m.lead)
      .map((m) => all.find((s) => sessionIs(m.session, s.computer, s)))
      .filter((s): s is Session => !!s));
  }
  /**
   * Task 431: a team member is folded under its lead only in the project folder
   * that actually lists that lead. Before, a lead visible in any project folded
   * its members everywhere, so a folder whose lead sits in another project (or is
   * filtered out there) kept its header and hid every session behind it.
   */
  const nested = (list: readonly Session[], key: string): boolean => {
    const team = teamFor.get(key);
    if (!team) return false;
    const lead = leadOf.get(team.id);
    if (!lead || !list.some((s) => s.key === lead.key && input.visible(s))) return false;
    return (membersOf.get(team.id) ?? []).some((member) => member.key === key);
  };
  const cards = (list: readonly Session[], depth = 0) => {
    for (const s of list) {
      if (nested(list, s.key)) continue;
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
      const team = teamFor.get(s.key);
      // Task 431: a team tree belongs to the folder that lists its lead; members
      // filed in another project draw in their own folder instead of vanishing.
      const teamMembers = team ? (membersOf.get(team.id) ?? []).filter((m) => list.some((s) => s.key === m.key)) : [];
      const teamExpanded = teamMembers.length > 0 && !(input.teamCollapsed?.(team!.id) ?? false);
      const isLead = !!team && leadOf.get(team.id)?.key === s.key;
      out.push({ type: "card", id: s.key, session: shown, expandable: kids.length > 0 || (isLead && teamMembers.length > 0),
                 expanded: expanded || teamExpanded, compactRow: input.compact(s.key), depth,
                 team: isLead ? team : undefined });
      if (expanded) kids.forEach((child, i) => out.push({ type: "child", id: `${s.key}/${child.id}`, parent: shown, child, first: i === 0, last: i === kids.length - 1 }));
      if (isLead && teamExpanded) {
        teamMembers.forEach((member, i) => out.push({
          type: "card", id: member.key, session: member, expandable: false, expanded: false,
          compactRow: false, depth: depth + 1, teamFirst: i === 0, teamLast: i === teamMembers.length - 1,
        }));
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
    // A project without visible members stays hidden while filtering, when
    // the owner asked to hide empty projects (the default), and when the
    // organization cannot reach it (SessionsWindowRows.cpp:559, 323).
    if (!list.length && (input.filtering || input.hideEmpty !== false || !h.accessible)) continue;
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
