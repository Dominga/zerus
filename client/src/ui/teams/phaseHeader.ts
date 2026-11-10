import { tr } from "../../i18n";
import type { TeamPhaseView } from "../../types";

// Task 196 (owner, 2026-11-07: Tauri parity with Qt): what a phase header
// shows and offers, by the phase's state, as Qt's TeamWorkspace::phaseScroll
// (tray/src/TeamWorkspace.cpp) does: the state's buttons, the phase menu, the
// notes ("accepted by Auto", "Healthy N/M", "Starts after", the send-back,
// who proposed it, the pause, the lead's changes), the collapsed one-line
// summary and the counts, which leave cancelled tasks out.

export interface HeaderTask {
  id: number; title: string; status: string; phase: number; attempts: number;
  waits?: string; held?: boolean; no_report?: boolean;
}
/** A phase as the header reads it: its number and state, and whatever else the view has. */
export type HeaderPhase = Pick<TeamPhaseView, "n" | "state"> & Partial<TeamPhaseView>;
export interface HeaderTeam {
  status: string;
  phases: HeaderPhase[];
  tasks: HeaderTask[];
  free_phase?: number | null;
}

export type PhaseButton = "reject" | "accept" | "undo" | "send_back" | "accept_phase" | "add_task";
export type PhaseMenuId = "edit" | "move_earlier" | "move_later" | "pause" | "unpause" | "barrier" | "owner_acceptance" | "return_to_draft" | "discard";
export interface PhaseMenuItem { id: PhaseMenuId; enabled: boolean; checked?: boolean }
export interface PhaseNote { id: string; text: string; tone: "dim" | "warn" | "accent"; tip?: string }
export interface PhaseHeader {
  state: string;
  buttons: PhaseButton[];
  menu: PhaseMenuItem[];
  notes: PhaseNote[];
  /** Collapsed: the phase's tasks in one line (Qt teamPhaseSummary). */
  summary: string;
  /** Accepted of the tasks it still owes: a cancelled task is not owed (owner, 2026-10-28). */
  done: number;
  total: number;
}

const tasksOf = (team: HeaderTeam, n: number) => team.tasks.filter((task) => task.phase === n);

/** A task whose work has started: it was handed out, or left draft and pending. */
export function taskStarted(task: HeaderTask): boolean {
  return task.attempts > 0 || !["draft", "pending"].includes(task.status);
}

/** U63: an accepted phase (it has a record: never the plan's phase 1) none of whose work has started. */
export function acceptedUnstarted(team: HeaderTeam, n: number): boolean {
  const phase = team.phases.find((found) => found.n === n);
  if (!phase || n <= 1 || !["queued", "running"].includes(phase.state)) return false;
  return !tasksOf(team, n).some(taskStarted);
}

/** F10, U63: the phases Move earlier and Move later reorder: drafts and accepted unstarted ones. */
export function movablePhases(team: HeaderTeam): number[] {
  if (["done", "stopped", "archived"].includes(team.status)) return [];
  return team.phases
    .filter((phase) => phase.n > 1 && (phase.state === "draft" || phase.state === "rejected" || acceptedUnstarted(team, phase.n)))
    .map((phase) => phase.n)
    .sort((a, b) => a - b);
}

/** Where Move earlier or Move later sends a phase: the neighbour among the movable ones. */
export function moveTarget(team: HeaderTeam, n: number, later: boolean): { to: number; position: "before" | "after" } | undefined {
  const list = movablePhases(team);
  const at = list.indexOf(n);
  if (at < 0) return undefined;
  const to = list[later ? at + 1 : at - 1];
  return to === undefined ? undefined : { to, position: later ? "after" : "before" };
}

/** Qt's "Paused by you", "Paused by the lead", "Paused by NAME", with the reason. */
export function pauseText(phase: HeaderPhase): string {
  const by = phase.pause_by ?? "";
  const who = phase.pause_owner ? tr("Paused by you") : !by || by === "Lead" ? tr("Paused by the lead") : tr("Paused by %1", by);
  const reason = (phase.pause_reason ?? "").trim();
  return reason ? tr("%1: %2", who, reason) : who;
}

function hhmm(seconds: number): string {
  const at = new Date(seconds * 1000);
  return `${String(at.getHours()).padStart(2, "0")}:${String(at.getMinutes()).padStart(2, "0")}`;
}

/** A task that needs attention, as the plan marks it: it waits, is held or got no report. */
const problem = (task: HeaderTask) => !!task.waits?.trim() || !!task.held || !!task.no_report;

export function phaseHeader(team: HeaderTeam, phase: HeaderPhase): PhaseHeader {
  const n = phase.n;
  const state = phase.state;
  const tasks = tasksOf(team, n);
  const owed = tasks.filter((task) => task.status !== "cancelled");
  const done = owed.filter((task) => task.status === "accepted" || task.status === "done").length;
  const planning = team.status === "planning";
  const freeOpen = team.free_phase === n;
  const buttons: PhaseButton[] = [];
  const menu: PhaseMenuItem[] = [];
  const notes: PhaseNote[] = [];
  const movable = movablePhases(team);
  const at = movable.indexOf(n);
  const moves = (): PhaseMenuItem[] => [
    { id: "move_earlier", enabled: at > 0 },
    { id: "move_later", enabled: at >= 0 && at + 1 < movable.length },
  ];
  const acceptance: PhaseMenuItem = { id: "owner_acceptance", enabled: n > 1, checked: !!phase.owner_accepts };
  // Owner, 2026-11-11: the barrier decides whether an approved phase starts
  // beside the phases before it, so the owner sees it and can remove it (the
  // lead set it and the owner had no control over it at all).
  const barrier: PhaseMenuItem = { id: "barrier", enabled: n > 1, checked: !!phase.barrier };
  // U61: started ahead, or accepted by Auto.
  if (state === "draft" && phase.auto && phase.start_ahead) notes.push({ id: "start_ahead", text: tr("started ahead by the lead"), tone: "accent" });
  else if (!freeOpen && phase.accepted_by_auto) notes.push({ id: "by_auto", text: tr("accepted by Auto"), tone: "dim", tip: tr("Auto accepted this phase when its first task started") });
  if ((state === "draft" || state === "rejected") && phase.added_by) {
    const when = phase.added_at ? ` ${hhmm(phase.added_at)}` : "";
    notes.push({ id: "proposed", text: tr("proposed by %1", phase.added_by) + when, tone: "dim" });
  } else if (state === "running" || state === "queued") {
    const healthy = owed.filter((task) => !problem(task)).length;
    notes.push({ id: "health", text: tr("Healthy %1/%2", healthy, owed.length), tone: healthy === owed.length ? "accent" : "warn", tip: phase.health || undefined });
  }
  if (phase.paused && !freeOpen) notes.push({ id: "paused", text: pauseText(phase), tone: "warn", tip: tr("Approved; it will not start until the pause is removed") });
  if (["draft", "rejected", "queued", "running"].includes(state) && !freeOpen && (phase.changes ?? 0) > 0) {
    const count = phase.changes ?? 0;
    notes.push({ id: "changes", text: count === 1 ? tr("1 change from the lead") : tr("%1 changes from the lead", count), tone: "dim" });
  }
  if (state === "draft" && !planning) {
    buttons.push("reject", "accept");
    menu.push(...moves(), barrier, acceptance, { id: "discard", enabled: true });
  } else if (state === "rejected") {
    notes.push({ id: "sent_back", text: tr("Sent back: “%1”", phase.rejection ?? ""), tone: "warn", tip: phase.rejection });
    if (!phase.reworked) buttons.push("undo");
  } else if (state === "awaiting_acceptance" && !planning) {
    buttons.push("send_back", "accept_phase");
  } else if (state === "running" && ["running", "paused"].includes(team.status)) {
    buttons.push("add_task");
  } else if (state === "queued") {
    notes.push({ id: "queued", text: phase.waiting || tr("Starts after phase %1", n - 1), tone: "dim" });
  }
  if ((state === "queued" || state === "running") && !planning && !freeOpen) {
    const unstarted = acceptedUnstarted(team, n);
    menu.push({ id: "edit", enabled: unstarted }, ...moves());
    menu.push(phase.paused ? { id: "unpause", enabled: true } : { id: "pause", enabled: unstarted });
    menu.push(barrier, acceptance, { id: "return_to_draft", enabled: unstarted }, { id: "discard", enabled: unstarted });
  }
  const summary = tasks.map((task) => `#${task.id} ${task.title}`).join(", ");
  return { state, buttons, menu, notes, summary, done, total: owed.length };
}

/** The menu item's words. */
export function menuText(id: PhaseMenuId): string {
  switch (id) {
    case "edit": return tr("Edit phase…");
    case "move_earlier": return tr("Move earlier");
    case "move_later": return tr("Move later");
    case "pause": return tr("Pause phase…");
    case "unpause": return tr("Remove pause");
    case "barrier": return tr("Waits for earlier phases (barrier)");
    case "owner_acceptance": return tr("Requires owner acceptance");
    case "return_to_draft": return tr("Return to draft…");
    case "discard": return tr("Discard phase…");
  }
}

/** The button's words and tooltip. */
export function buttonText(id: PhaseButton): { text: string; tip: string } {
  switch (id) {
    case "reject": return { text: tr("Reject…"), tip: tr("Send the draft back to the lead with what should change") };
    case "accept": return { text: tr("Accept"), tip: tr("Hand the phase's tasks out; a barrier phase waits for the phases before it") };
    case "undo": return { text: tr("Undo"), tip: tr("Take the send-back back; the draft waits for you again") };
    case "send_back": return { text: tr("Send back…"), tip: tr("The lead reworks the phase with your comment") };
    case "accept_phase": return { text: tr("Accept phase"), tip: tr("Accept the phase's result; it becomes Done") };
    case "add_task": return { text: tr("Add task…"), tip: tr("Add a task to this running phase: Zerus warns first, then you confirm") };
  }
}
