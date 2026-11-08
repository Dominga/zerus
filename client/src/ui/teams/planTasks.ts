// Task 197: what the plan's Graph, Board and Layers say about each task, as
// Qt's TeamWorkspace does (tray/src/TeamModel.cpp taskStatus and problems,
// TeamWorkspace.cpp column, activityLabel, waitLabel, TeamWidgets.h taskTime).
// Pure, so the wording is tested without a webview. The Tauri UI never
// separates values with dots: Qt's dotted "attempt 2, idle" uses a comma here.
import { tr } from "../../i18n";
import type { TeamMemberView, TeamTaskView, TeamView } from "../../types";
import type { MemberStatus, Tone } from "./team-model";

export interface TaskState {
  /** "In progress", "Delivery unconfirmed", "Draft"... */
  text: string;
  tone: Tone;
  /** A problem of its own (the delivery is unconfirmed, no report came). */
  problem: boolean;
}

/** One task's state as Qt's taskStatus words it. */
export function taskState(task: TeamTaskView): TaskState {
  const open = ["delivering", "delivered", "unconfirmed"].includes(task.attempt_state);
  const make = (text: string, tone: Tone, problem = false): TaskState => ({ text, tone, problem });
  if (open && task.attempt_state === "unconfirmed") return make(tr("Delivery unconfirmed"), "danger", true);
  if (open && task.no_report) return make(tr("No report"), "warn", true);
  if (task.held) return make(tr("Held"), "muted");
  switch (task.status) {
    case "draft": return make(tr("Draft"), "dim");
    case "pending": return make(tr("Pending"), "muted");
    case "assigned": return make(tr("Assigned"), "assigned");
    case "in_progress": return make(tr("In progress"), "active");
    case "submitted": return make(tr("Submitted"), "done");
    case "in_review": return make(tr("In review"), "active");
    case "in_test": return make(tr("In test"), "active");
    case "accepted": return make(tr("Accepted"), "done");
    case "rejected": return make(tr("Rejected"), "danger");
    case "cancelled": return make(tr("Cancelled"), "dim");
    default: return make(task.status || tr("Pending"), "muted");
  }
}

export interface TaskProblem { tone: Tone; text: string }

/**
 * Qt's problemTasks and problemTexts: the tasks the members' problems name
 * (an offline member, a question, no report, a refused Terminal, no
 * progress...), with the reason; then each task's own problem.
 */
export function problemTasks(team: TeamView, statuses: ReadonlyMap<string, MemberStatus>): Map<number, TaskProblem> {
  const found = new Map<number, TaskProblem>();
  for (const member of team.members) {
    const status = statuses.get(member.id);
    if (!status?.problem || status.task <= 0 || found.has(status.task)) continue;
    found.set(status.task, { tone: status.tone, text: status.text });
  }
  for (const task of team.tasks) {
    const state = taskState(task);
    if (state.problem && !found.has(task.id)) found.set(task.id, { tone: state.tone, text: state.text });
  }
  return found;
}

/** The Board's columns, in order (Qt boardColumns). */
export type BoardKey = "pending" | "active" | "problems" | "submitted" | "in_review" | "in_test" | "accepted" | "decided";

/**
 * A task's column, by what it needs (Qt column): a problem first; Accepted
 * only for an accepted task, a rejected or cancelled one under Decided; a
 * draft, a pending or an unknown status under Pending, so no task vanishes
 * from the board (an assigned one is Active).
 */
export function boardColumn(task: TeamTaskView, problem: boolean): BoardKey {
  if (problem) return "problems";
  if (task.status === "accepted") return "accepted";
  if (task.status === "rejected" || task.status === "cancelled") return "decided";
  if (task.status === "submitted") return "submitted";
  if (task.status === "in_review" || task.status === "in_test") return task.status;
  if (task.status === "assigned" || task.status === "in_progress") return "active";
  return "pending";
}

/** Every phase shows the same columns, so they line up: In review and In test
 * once the team uses pipelines, Decided once any task is decided. */
export function boardKeys(team: TeamView, problems: ReadonlyMap<number, unknown>): BoardKey[] {
  const keys: BoardKey[] = ["pending", "active", "problems", "submitted", "accepted"];
  const pipelines = team.tasks.some((task) => !!task.pipeline_run || (!!task.pipeline && task.pipeline !== "none"));
  if (pipelines) keys.splice(4, 0, "in_review", "in_test");
  if (team.tasks.some((task) => boardColumn(task, problems.has(task.id)) === "decided")) keys.push("decided");
  return keys;
}

export function boardTitle(key: BoardKey): string {
  switch (key) {
    case "pending": return tr("Pending");
    case "active": return tr("Active");
    case "problems": return tr("Problems");
    case "submitted": return tr("Submitted");
    case "in_review": return tr("In review");
    case "in_test": return tr("In test");
    case "accepted": return tr("Accepted");
    default: return tr("Decided");
  }
}

/** What an open task waits for (Qt waitLabel): "after #1, #4", "waits for you". */
export function waitText(task: TeamTaskView): string {
  const on = task.wait_on ?? [];
  if (!task.wait_for || on.length === 0) return "";
  if (task.wait_for === "deps") return tr("after %1", on.map((item) => (item.startsWith("#") ? item : `#${item}`)).join(", "));
  if (task.wait_for === "owner") return tr("waits for you");
  return tr("waits for %1", on.join(", "));
}

/** The small activity label (Qt activityLabel): "review", "attempt 2, idle",
 * "starting", "accepted", "after #3"... */
export function activityText(task: TeamTaskView, working: boolean): string {
  if (task.status === "submitted") return tr("review");
  if (task.pipeline_run?.open) return tr("attempt %1", Math.max(1, task.attempts));
  if (task.status === "cancelled") return tr("cancelled");
  if (task.status === "in_progress" || task.status === "assigned") {
    const state = task.attempts === 0 || task.status === "assigned" ? tr("starting")
      : !task.attempt_begun && !working ? tr("starting")
      : working ? tr("working") : tr("idle");
    return task.attempts > 1 ? tr("attempt %1, %2", task.attempts, state) : state;
  }
  if (task.held) return tr("held");
  if (task.status === "accepted") return tr("accepted");
  if (task.status === "rejected") return tr("rejected");
  if (task.status === "draft") return tr("draft");
  if (task.status === "pending") {
    const wait = waitText(task);
    if (task.attempts > 0) return wait ? tr("returned, %1", wait) : tr("returned");
    return wait || tr("pending");
  }
  return "";
}

export interface TaskTime { text: string; over: boolean }

/** "<1m", "12m", "1h 05m" (Qt workedText). */
export function workedText(seconds: number): string {
  if (seconds < 60) return tr("<1m");
  const minutes = Math.floor(seconds / 60);
  if (minutes < 60) return tr("%1m", minutes);
  return tr("%1h %2m", Math.floor(minutes / 60), String(minutes % 60).padStart(2, "0"));
}

/**
 * A task's time against its estimate (Qt estimateTime and taskTime): "~45
 * min" before it works, "32 of ~45 min" while it works and after, "no
 * estimate yet" without one; `over` once the time spent passed the estimate.
 * An older hgs sends no estimate: the plain working time.
 */
export function taskTime(task: TeamTaskView, now: number): TaskTime {
  let seconds = 0;
  let spent = true;
  if ((task.running_since ?? 0) > 0) seconds = Math.max(0, now - task.running_since!);
  else if ((task.worked_s ?? 0) > 0) seconds = task.worked_s!;
  else if (task.estimate_spent != null && task.estimate_spent >= 0) seconds = task.estimate_spent * 60;
  else spent = false;
  if (!task.estimate_source) return { text: spent && seconds > 0 ? workedText(seconds) : "", over: false };
  const minutes = Math.floor((seconds + 30) / 60);
  if (task.estimate_minutes > 0) {
    const wanted = Math.round(task.estimate_minutes);
    if (!spent) return { text: tr("~%1 min", wanted), over: false };
    return { text: tr("%1 of ~%2 min", minutes, wanted), over: minutes > wanted };
  }
  return { text: spent ? tr("%1 min, no estimate yet", minutes) : tr("no estimate yet"), over: false };
}

/** A card's time line: the activity, then the time ("after #3 ~45 min",
 * "attempt 2, idle 12 of ~30 min", "accepted 40 of ~45 min"). */
export function timeLine(task: TeamTaskView, working: boolean, now: number): { activity: string; time: TaskTime } {
  return { activity: activityText(task, working), time: taskTime(task, now) };
}

/** The toolbar's state filter (Qt m_filter). */
export type PlanFilter = "all" | "active" | "problems";

const ACTIVE = new Set(["assigned", "in_progress", "submitted", "in_review", "in_test"]);

/** Whether a task matches the state filter and the member filter (Qt node.match). */
export function matches(task: TeamTaskView, filter: PlanFilter, problems: ReadonlyMap<number, unknown>, member: string, planning: boolean): boolean {
  const state = filter === "all" || planning || (filter === "active" && ACTIVE.has(task.status)) || (filter === "problems" && problems.has(task.id));
  return state && (!member || task.assignee === member);
}

const DECIDED = new Set(["accepted", "cancelled", "rejected"]);

/** A phase whose tasks are all decided (Qt finishedPhases). */
export function phaseFinished(tasks: readonly TeamTaskView[]): boolean {
  return tasks.length > 0 && tasks.every((task) => DECIDED.has(task.status));
}

/** The status mark's icon (Qt kindOfTask), by the task's state. */
export function statusIcon(task: TeamTaskView, problem: boolean): string {
  if (problem) return "circle-alert";
  switch (task.status) {
    case "accepted": return "check";
    case "submitted": return "inbox";
    case "in_review": return "eye";
    case "in_test": return "activity";
    case "in_progress": return "play";
    case "assigned": return "arrow-right";
    case "rejected": return "x";
    case "cancelled": return "circle-stop";
    case "draft": return "pencil";
    default: return task.held ? "pause" : "circle";
  }
}

/** The role a card's chip names (Qt assigneeRole): the member's role, "You"
 * for a task the owner took over, the assignee's id else. */
export function assigneeRole(team: TeamView, task: TeamTaskView): string {
  if (!task.assignee) return task.status === "in_progress" ? tr("You") : tr("Unassigned");
  const member: TeamMemberView | undefined = team.members.find((candidate) => candidate.id === task.assignee);
  return member?.role || task.assignee;
}

/** The member's agent, for its small logo. */
export function assigneeAgent(team: TeamView, task: TeamTaskView): string {
  return team.members.find((candidate) => candidate.id === task.assignee)?.agent ?? "";
}
