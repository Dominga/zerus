import { tr } from "../../i18n";
import type { TeamMemberView, TeamTaskView, TeamView } from "../../types";

// What the Team tab shows of a team and its members, from the core's view
// (client/core/src/teams/view.rs), as Qt's TeamModel words it
// (tray/src/TeamModel.cpp: memberStatus, displayStatus, statusLabel). Pure,
// so the states are tested without a webview.

// Task 342 (owner, 2026-11-08): the plan's five task states are told apart by
// colour, so a tone names the state the plan CSS tints.
export type Tone = "neutral" | "active" | "assigned" | "done" | "review" | "cancelled" | "muted" | "dim" | "warn" | "danger" | "planning";

/** Task #347: the member card's state colour family, as the owner's legend:
 * working and building are blue, waiting is amber, needs answer/blocked/failed
 * red, idle/done grey, paused/offline muted (the card adds the hatch). */
export type MemberStateColor = "info" | "warn" | "danger" | "muted" | "dim";

export function memberStateColor(kind: string): MemberStateColor {
  if (["working", "sent", "submitted", "in_review", "in_test", "planning"].includes(kind)) return "info";
  if (["stuck", "no_report", "input_blocked", "asking"].includes(kind)) return "warn";
  if (["answer", "failed", "unconfirmed"].includes(kind)) return "danger";
  if (["paused", "offline", "removed"].includes(kind)) return "muted";
  return "dim";
}

/** What the session list says about a member's session (Qt's Teams::Live). */
export interface MemberLive {
  /** Its computer does not answer, or the team saw it stopped. */
  offline: boolean;
  /** A question or approval waits for the owner. */
  attention: boolean;
  working: boolean;
}

export const NO_LIVE: MemberLive = { offline: false, attention: false, working: false };

export interface MemberStatus {
  /** working, sent, submitted, idle, offline, answer, paused, failed, stuck, not_created... */
  kind: string;
  /** "Working on #3 Saved cards API". */
  text: string;
  /** "#3", "Offline". */
  short: string;
  tone: Tone;
  /** The task it names, 0 for none. */
  task: number;
  /** A problem the owner should look at (the row is tinted, the team may be Stuck). */
  problem: boolean;
  icon: string;
}

/** The attempt states of a task that is still with its member. */
const OPEN_ATTEMPT = new Set(["delivering", "delivered", "unconfirmed"]);

export const finished = (task: TeamTaskView) => task.status === "accepted" || task.status === "cancelled";

/** The task a member works on now: its open attempt. */
export function currentTask(team: TeamView, member: TeamMemberView): TeamTaskView | undefined {
  const open = (task: TeamTaskView) => task.assignee === member.id && !finished(task) && OPEN_ATTEMPT.has(task.attempt_state);
  const first = member.queue.length ? team.tasks.find((task) => task.id === member.queue[0] && open(task)) : undefined;
  return first ?? team.tasks.find(open);
}

const ICONS: Record<Tone, string> = {
  neutral: "circle", active: "play", assigned: "arrow-right", done: "check", review: "eye", cancelled: "x",
  muted: "pause", dim: "circle", warn: "circle-question-mark", danger: "circle-alert", planning: "pencil",
};

function make(kind: string, text: string, short: string, tone: Tone, task = 0, problem = false, icon?: string): MemberStatus {
  return { kind, text, short, tone, task, problem, icon: icon ?? ICONS[tone] };
}

/** One member's state as its row shows it (Qt's memberStatus). */
export function memberStatus(team: TeamView, member: TeamMemberView, live: MemberLive = NO_LIVE): MemberStatus {
  const session = !!member.session;
  // F5: nothing but the lead exists while the team is a draft.
  if (team.status === "draft") {
    return session ? make("planning", tr("Drafting the team"), tr("Drafting"), "planning")
      : make("not_created", tr("Created with the team"), tr("Draft"), "dim");
  }
  if (team.status === "planning") {
    return session ? make("planning", tr("Planning"), tr("Planning"), "planning")
      : make("not_created", tr("Created on approval"), tr("Planned"), "dim");
  }
  if (member.state === "removed") return make("removed", tr("Removed"), tr("Removed"), "dim");
  if (member.error) return make("failed", tr("Failed to start"), tr("Failed"), "danger", 0, true);
  if (!session) {
    return team.status === "running" || team.status === "paused"
      ? make("not_started", tr("Starts with its first task"), tr("Not started"), "dim")
      : make("not_started", tr("Not started"), tr("Not started"), "dim");
  }
  if (member.state === "paused") return make("paused", tr("Paused"), tr("Paused"), "muted");
  const current = currentTask(team, member);
  const task = current?.id ?? 0;
  if (live.offline) return make("offline", tr("Offline"), tr("Offline"), "danger", task, true, "wifi-off");
  if (live.attention || member.question) return make("answer", tr("Needs your answer"), tr("Answer"), "warn", task, true);
  if (current?.attempt_state === "unconfirmed") return make("unconfirmed", tr("Delivery unconfirmed"), tr("Unconfirmed"), "danger", task, true);
  // Task 315 (F9): Qt reads the open attempt's own no_report; the member's phase may still say working.
  if (member.phase === "no_report" || current?.no_report) return make("no_report", tr("No report"), tr("No report"), "warn", task, true, "message-square");
  if (member.phase === "asking") return make("asking", tr("Waiting for your answer"), tr("Answer"), "warn", task, true);
  // Task 98: its Terminal refuses hand-outs (a draft, a dialog or an unknown prompt).
  if ((member.input_refusals ?? 0) > 0) {
    const reason = member.input_reason || tr("a draft or dialog");
    return make("input_blocked", tr("Terminal refused %1 hand-outs: %2", member.input_refusals!, reason), tr("Blocked"), "warn", task, true, "terminal");
  }
  if (member.stuck) {
    const minutes = member.minutes_without_progress;
    return make("stuck", minutes > 0 ? tr("No progress for %1 min", minutes) : tr("Stuck"), tr("Stuck"), "warn", task, true, "hourglass");
  }
  if (current?.attempt_state === "delivering") return make("sent", tr("Sending #%1 %2", current.id, current.title), tr("Sending"), "assigned", task);
  // Task 355: the row's word, with the task number beside it in the card.
  if (current) return make("working", tr("Working on #%1 %2", current.id, current.title), tr("Working"), "active", task);
  const last = team.tasks.find((t) => t.assignee === member.id && (t.status === "submitted" || t.status === "in_review" || t.status === "in_test"));
  if (last?.status === "submitted") return make("submitted", tr("Submitted"), tr("Submitted"), "done", last.id);
  if (last) {
    const text = last.status === "in_test" ? tr("In test") : tr("In review");
    return make(last.status, text, text, "done", last.id);
  }
  if (team.status === "done") return make("done", tr("Done"), tr("Done"), "done");
  if (live.working || member.activity === "busy") return make("working", tr("Working"), tr("Working"), "active");
  return make("idle", tr("Idle"), tr("Idle"), "muted");
}

/**
 * Task 134: a member that has not read its team messages shows "unread: N";
 * the tooltip says how many are urgent and how it reads them. Null when it
 * has read everything.
 */
export function unreadBadge(member: TeamMemberView): { text: string; tip: string; urgent: boolean } | null {
  const count = member.unread ?? 0;
  if (count <= 0) return null;
  const urgent = member.unread_urgent ?? 0;
  return {
    text: tr("unread: %1", count),
    tip: urgent > 0
      ? tr("%1 unread team messages, %2 urgent; the member reads them with hgs team inbox", count, urgent)
      : tr("%1 unread team messages; the member reads them with hgs team inbox", count),
    urgent: urgent > 0,
  };
}

/**
 * The team's state as its badge shows it (Qt's displayStatus): its own
 * status, except that a running team where no open task can move without you
 * is Stuck.
 */
export function teamState(team: TeamView, live: (member: TeamMemberView) => MemberLive = () => NO_LIVE): string {
  if (team.status !== "running") return team.status;
  const ready = new Set(team.ready);
  let open = false;
  for (const task of team.tasks) {
    if (finished(task) || task.status === "draft") continue;
    open = true;
    // The lead decides a report; a stage works on it.
    if (task.status === "submitted" || task.status === "in_review" || task.status === "in_test") return team.status;
    const assignee = team.members.find((member) => member.id === task.assignee);
    const state = assignee ? live(assignee) : NO_LIVE;
    if (OPEN_ATTEMPT.has(task.attempt_state)) {
      const blocked = task.attempt_state === "unconfirmed" || state.offline || state.attention
        || !!assignee?.question || assignee?.phase === "no_report" || !!assignee?.stuck;
      if (!blocked) return team.status;
      continue;
    }
    // Taken over by you.
    if (!task.assignee && task.status === "in_progress") return team.status;
    if (ready.has(task.id) && assignee && !assignee.error && assignee.state !== "paused" && !state.offline) return team.status;
  }
  return open ? "stuck" : team.status;
}

/** The badge's text for a state. */
export function stateLabel(state: string): string {
  switch (state) {
    case "draft": return tr("Draft");
    case "planning": return tr("Planning");
    case "running": return tr("Running");
    case "paused": return tr("Paused");
    case "stuck": return tr("Stuck");
    case "done": return tr("Done");
    case "stopped": return tr("Stopped");
    case "archived": return tr("Archived");
    default: return state;
  }
}

/** The badge's tone for a state (HANDOFF §2 team badge). */
export function stateTone(state: string): Tone {
  switch (state) {
    case "draft": case "planning": return "planning";
    case "running": return "active";
    case "stuck": return "warn";
    case "done": return "done";
    case "stopped": return "danger";
    default: return "muted";
  }
}

export interface Progress { accepted: number; submitted: number; active: number; problems: number; total: number }

/** The tasks that count toward the team's progress: all but the cancelled ones. */
export function progress(team: TeamView, problemTasks: ReadonlySet<number> = new Set()): Progress {
  const counted = team.tasks.filter((task) => task.status !== "cancelled");
  return {
    accepted: counted.filter((task) => task.status === "accepted").length,
    submitted: counted.filter((task) => task.status === "submitted" || task.status === "in_review" || task.status === "in_test").length,
    active: counted.filter((task) => OPEN_ATTEMPT.has(task.attempt_state) && !finished(task)).length,
    problems: counted.filter((task) => problemTasks.has(task.id)).length,
    total: counted.length,
  };
}

/** A member's own tasks: accepted of those it was given (the row's progress bar). */
export function memberProgress(team: TeamView, member: TeamMemberView): { accepted: number; total: number } {
  const own = team.tasks.filter((task) => task.assignee === member.id && task.status !== "cancelled");
  return { accepted: own.filter((task) => task.status === "accepted").length, total: own.length };
}

/** The members a row lists: the lead first, then the others in the team's order; removed members leave the list. */
export function shownMembers(team: TeamView): TeamMemberView[] {
  const kept = team.members.filter((member) => member.state !== "removed");
  return [...kept.filter((member) => member.lead), ...kept.filter((member) => !member.lead)];
}

/** "12m", "3h 12m", "3d 4h", "2w 3d" (HANDOFF §14 fmtDur). */
export function duration(seconds: number): string {
  const minutes = Math.max(0, Math.floor(seconds / 60));
  if (minutes < 60) return tr("%1m", minutes);
  const hours = Math.floor(minutes / 60);
  if (hours < 24) return tr("%1h %2m", hours, minutes % 60);
  const days = Math.floor(hours / 24);
  if (days < 7) return tr("%1d %2h", days, hours % 24);
  return tr("%1w %2d", Math.floor(days / 7), days % 7);
}
