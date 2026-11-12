import { tr } from "../../i18n";
import type { AccountsView, TeamMemberView, TeamTaskView } from "../../types";
import { accountNameForId } from "../provider-selection";

// Task 199 (owner, 2026-11-07: Tauri parity with Qt): the side column's
// members as Qt's TeamWorkspace draws them: the tree of sub-leads and their
// reports, the member menu with its enable rules and tooltips, the account a
// session runs on, the room for more members and the focus filter.

type Member = Pick<TeamMemberView, "id" | "role" | "agent" | "state" | "error" | "lead" | "session"> &
  Partial<Pick<TeamMemberView, "parent" | "sub" | "draining" | "account" | "session_account" | "computer">>;
type Task = Pick<TeamTaskView, "id" | "assignee" | "status">;
export interface MembersTeam { status: string; lead: string; members: Member[]; tasks: Task[]; settings: Record<string, unknown> }

const finished = (task: Task) => ["accepted", "rejected", "cancelled", "done"].includes(task.status);
const active = (status: string) => ["planning", "running", "paused"].includes(status);
const member = (team: MembersTeam, id: string) => team.members.find((found) => found.id === id);

/** Qt Team::reviewerOf: its parent when that is a sub-lead, else the lead; none for the lead. */
export function reviewerOf(team: MembersTeam, id: string): string {
  if (id === team.lead) return "";
  const parent = member(team, member(team, id)?.parent ?? "");
  return parent && parent.sub && parent.id !== id && parent.id !== team.lead ? parent.id : team.lead;
}

/** The members who report to `id` (the lead's reports are everyone not under a sub-lead). */
export function reportsOf(team: MembersTeam, id: string): Member[] {
  return team.members.filter((found) => found.id !== team.lead && reviewerOf(team, found.id) === id);
}

export interface MemberNode { member: Member; depth: number; last: boolean; hidden: boolean }

/**
 * Spec 5.6: the lead, then each member under its reviewer, depth first; a
 * folded sub-lead hides its reports. A broken or cyclic chain ends at the lead.
 */
export function memberTree(team: MembersTeam, folded: ReadonlySet<string> = new Set()): MemberNode[] {
  const out: MemberNode[] = [];
  const seen = new Set<string>();
  const walk = (id: string, depth: number, last: boolean, hidden: boolean) => {
    const found = member(team, id);
    if (!found || seen.has(id)) return;
    seen.add(id);
    out.push({ member: found, depth, last, hidden });
    const children = team.members.filter((child) => child.id !== team.lead && !seen.has(child.id) && reviewerOf(team, child.id) === id);
    children.forEach((child, index) => walk(child.id, depth + 1, index === children.length - 1, hidden || folded.has(id)));
  };
  walk(team.lead, 0, true, false);
  // Whatever the walk did not reach (no lead record, a cycle) still shows.
  for (const left of team.members) if (!seen.has(left.id)) { seen.add(left.id); out.push({ member: left, depth: 1, last: true, hidden: false }); }
  return out;
}

/** Qt scaleRoom: the cap counts the members besides the lead; no cap is room for ten. */
export function scaleRoom(team: MembersTeam): number {
  const max = Number(team.settings.max_members ?? 0);
  if (!Number.isFinite(max) || max <= 0) return 10;
  return max - team.members.filter((found) => found.state !== "removed" && found.id !== team.lead).length;
}

/** U54: "", "native" and "native-<agent>" are the agent's default account. */
export const accountOf = (agent: string, account: string) => (!account || account === "native" || account.startsWith("native-") ? `native-${agent}` : account);
/** The account its session runs on; before it has one, the configured one. */
export const memberAccount = (found: Member) =>
  accountOf(found.agent, found.session && found.session_account != null ? found.session_account : found.account ?? "");
/** Its Accounts label, else the configured label map or the recorded profile id. */
export function accountLabel(found: Member, labels: Record<string, string> = {}, view?: Pick<AccountsView, "accounts">): string {
  const id = memberAccount(found);
  const resolved = view && accountNameForId(view, found.computer ?? "", found.agent, id);
  return labels[id] || resolved || (id.startsWith("native-") ? tr("Default account") : id);
}
/** U53: only Claude and Codex change account in place. */
export const movableAgent = (agent: string) => agent === "claude" || agent === "codex";
/** The members a move from `from` takes: of this agent with a local session, the lead too (the server's rule). */
export function moveAffected(team: MembersTeam, agent: string, from: string): string[] {
  const account = accountOf(agent, from);
  return team.members.filter((found) => found.agent === agent && found.session && !found.session.host && memberAccount(found) === account).map((found) => found.id);
}

/** The open tasks of a member, as Qt words Remove's tooltip. */
export const openTasks = (team: MembersTeam, id: string) => team.tasks.filter((task) => task.assignee === id && !finished(task)).map((task) => task.id);

/** A member added to a running team waits for its first task: no session, nothing open, no error. */
export function waitingForTask(team: MembersTeam, found: Member): boolean {
  return !found.lead && !found.session && openTasks(team, found.id).length === 0 && !found.error && (team.status === "running" || team.status === "paused");
}

export type MemberMenuId = "open" | "message" | "iron_rules" | "pause" | "resume" | "relaunch" | "scale" | "drain" | "undrain"
  | "remove" | "reports_to" | "sub_lead" | "swap" | "move_account";
export interface MemberMenuItem { id: MemberMenuId; text: string; enabled: boolean; tip?: string; choices?: { id: string; text: string; checked: boolean }[] }

/** Qt TeamWorkspace::memberMenu, item by item. */
export function memberMenu(team: MembersTeam, found: Member, moving = false): MemberMenuItem[] {
  const role = found.role || found.id;
  const items: MemberMenuItem[] = [
    { id: "open", text: tr("Open session"), enabled: !!found.session },
    { id: "message", text: tr("Message…"), enabled: !!found.session },
    { id: "iron_rules", text: tr("Iron rules..."), enabled: true },
  ];
  if (!found.lead) {
    if (found.state === "paused") items.push({ id: "resume", text: tr("Resume member"), enabled: true });
    else items.push({ id: "pause", text: tr("Pause member"), enabled: true });
    const failed = !found.session && !!found.error;
    items.push({ id: "relaunch", text: tr("Relaunch"), enabled: failed, tip: failed ? undefined : tr("Starts automatically with its first task") });
    if (active(team.status)) {
      const room = scaleRoom(team);
      const max = Number(team.settings.max_members ?? 0);
      items.push({ id: "scale", text: tr("Add more %1…", role), enabled: room > 0,
        tip: room > 0 ? tr("Adds members like this one: the same role, agent and model") : tr("The team has its %1 members; raise its limit first", max) });
      items.push(found.draining
        ? { id: "undrain", text: tr("Cancel drain"), enabled: true, tip: tr("It takes tasks again") }
        : { id: "drain", text: tr("Drain…"), enabled: true, tip: tr("Finish the open work, then leave") });
    }
    const open = openTasks(team, found.id);
    const reports = reportsOf(team, found.id).length > 0;
    items.push({ id: "remove", text: tr("Remove from team…"), enabled: open.length === 0 && !reports,
      tip: open.length ? tr("Has open tasks %1 — reassign or cancel them first", open.map((id) => `#${id}`).join(", "))
        : reports ? tr("Move its reports to another lead first") : undefined });
  }
  if (active(team.status)) {
    // Spec 5.6: the lead or a sub-lead, never itself or one below it.
    const below = new Set([found.id]);
    for (const node of memberTree(team)) if (below.has(reviewerOf(team, node.member.id))) below.add(node.member.id);
    const current = reviewerOf(team, found.id);
    const choices = memberTree(team).map((node) => node.member)
      .filter((boss) => (boss.lead || boss.sub) && !below.has(boss.id))
      .map((boss) => ({ id: boss.id, text: boss.lead ? tr("%1 (coordinator)", boss.role || boss.id) : tr("%1 (sub-lead)", boss.role || boss.id), checked: boss.id === current }));
    items.push({ id: "reports_to", text: tr("Reports to…"), enabled: !found.lead, tip: found.lead ? tr("The coordinator reports to you") : undefined, choices });
    const reports = reportsOf(team, found.id).length > 0;
    items.push({ id: "sub_lead", text: found.sub ? tr("Stop being a sub-lead") : tr("Make sub-lead"), enabled: !found.lead && !(found.sub && reports),
      tip: found.lead ? tr("Already the coordinator") : found.sub && reports ? tr("Move its reports to another lead first") : undefined });
    items.push({ id: "swap", text: tr("Swap session, agent or model…"), enabled: true,
      tip: tr("Keeps its role, tasks, folder and branch; an open task goes to the new session with what was done so far") });
    const remote = !!found.session?.host;
    const movable = movableAgent(found.agent) && !remote && !!found.session;
    items.push({ id: "move_account", text: tr("Move to another account…"), enabled: movable && !moving,
      tip: moving ? tr("A move is running; wait for it")
        : !movableAgent(found.agent) ? tr("Only Claude and Codex members change account in place")
        : remote ? tr("Its session runs on another computer")
        : !found.session ? tr("It has no session yet; its account applies when it starts")
        : tr("Moves every %1 member on its account to another one, the lead included", found.agent) });
  }
  return items;
}

/** "Only X's tasks": the member's tasks and the edges between them. */
export function focusedTasks<T extends Task>(tasks: T[], id: string | null): T[] {
  return id ? tasks.filter((task) => task.assignee === id) : tasks;
}

/** H1: the usage limit's line: the reset time when known, and whether Zerus continues it. */
export function limitText(found: Pick<TeamMemberView, "limited" | "limit_resume">, format: (at: number) => string): string {
  const limit = found.limited;
  if (!limit) return "";
  if (!limit.resets_at) return tr("Usage limit, reset unknown");
  return found.limit_resume ? tr("Usage limit: continues after the reset (%1)", format(limit.resets_at)) : tr("Usage limit: %1", format(limit.resets_at));
}
