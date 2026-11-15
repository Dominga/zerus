import { tr } from "../../i18n";
import type { TeamCall } from "../../store/teams";
import type { MemberView, TaskView, TeamView } from "./types";

// The Team tab's action vocabulary: the calls the views can send and the
// words of their buttons. The outcome is never decided here: the store sends
// one intent and the core answers done, failed, uncertain, confirm or pending.

/** One intent's call: the person's request id, the team and its revision. */
export function teamCall(view: TeamView, requestId: string, kind: string, fields: Record<string, unknown> = {}): TeamCall {
  return { team: view.bar.id, team_name: view.bar.name, expected_revision: view.revision, request_id: requestId, action: { kind, ...fields } };
}

export interface BarAction {
  id: "pause" | "resume" | "stop" | "archive";
  text: string;
  danger?: boolean;
  disabled?: boolean;
}

// Every action is offered only in the states where the store accepts it; the
// rules are the store's operations (src/teams/ops on integrate/teams):
// team.rs pause, resume, stop and archive; queue.rs admit, hold and release;
// decisions.rs accept, return and cancel; members.rs pause, resume, stop and
// archive. Task and member actions need an active team (common.rs active: a
// running or paused team) and a live task (live_node: not accepted or
// cancelled). The store's refusal still decides what the rows cannot show: an
// unsettled decision, a person's earlier stop of a member, the queue's
// capacity.

/** A team the store takes task and member actions on: running or paused. */
export const activeTeam = (status: string) => status === "running" || status === "paused";
/** A task the store still decides: not accepted and not cancelled. */
export const liveTask = (state: string) => state !== "accepted" && state !== "cancelled";

/** The team's own operations: pause a running team, resume a paused one,
 * stop an active one, archive a stopped or finished one. */
export function barActions(view: TeamView): BarAction[] {
  const status = view.bar.status;
  const actions: BarAction[] = [];
  if (status === "planning" && view.planning?.known) actions.push({ id: view.planning.paused ? "resume" : "pause", text: view.planning.paused ? tr("Resume planning") : tr("Pause planning") });
  if (status === "running") actions.push({ id: "pause", text: tr("Pause") });
  if (status === "paused") actions.push({ id: "resume", text: tr("Resume") });
  if (activeTeam(status) || status === "planning") actions.push({ id: "stop", text: tr("Stop"), danger: true });
  if (status === "stopped" || status === "finished") actions.push({ id: "archive", text: tr("Archive"), danger: true });
  return actions;
}

export interface TaskButton {
  id: "admit" | "accept" | "return" | "hold" | "release" | "cancel";
  text: string;
  danger?: boolean;
  /** The button asks for a reason before it sends. */
  reason?: boolean;
  /** Why the store would refuse it now: the button is disabled and says so. */
  blocked?: string;
  /** What sending it does now, said beside the open button. */
  note?: string;
}

/** The decisions the store refuses while a previous one has not settled
 * (ops common.rs undecided: accept, return, reassign, cancel, route,
 * release). */
const WAIT_FOR_DECISION = new Set<TaskButton["id"]>(["accept", "return", "release", "cancel"]);

/** The decisions that replace a pending Return or Reassign that has not
 * started (zerus-63rru.43; Reassign is not offered here). An acceptance is
 * never replaced, and a release waits like before. */
const REPLACING = new Set<TaskButton["id"]>(["return", "cancel"]);

/** The line beside a decision that replaces the pending one, naming its kind. */
function replacesPending(kind: string): string {
  switch (kind) {
    case "return": return tr("A new decision here replaces the pending return.");
    case "reassign": return tr("A new decision here replaces the pending reassignment.");
    default: return tr("A new decision here replaces the pending decision (%1).", kind);
  }
}

/** A card's decisions, by the team's status and the card's state: admit a
 * task not admitted yet; accept a reported one; return a reported one that
 * has a holder; hold one not held and release one held; cancel any live one.
 * What the row says about the store's other conditions disables and explains
 * a decision before it is sent (zerus-63rru.29): a pending decision blocks
 * the decisions that wait for it, and a holder of nobody blocks Return. A row
 * from an older hgs without a holder keeps the implementer, who reported it. */
export function taskButtons(card: TaskView, teamStatus: string): TaskButton[] {
  if (!activeTeam(teamStatus) || !liveTask(card.state)) return [];
  const buttons: TaskButton[] = [];
  const holder = card.holder === undefined ? card.implementer || null : card.holder;
  if (!card.admitted) buttons.push({ id: "admit", text: tr("Admit") });
  if (card.state === "reported") buttons.push({ id: "accept", text: tr("Accept") });
  if (card.state === "reported" && (holder || card.holder === null)) {
    buttons.push({ id: "return", text: tr("Return"), reason: true,
      ...(holder ? {} : { blocked: tr("Nobody holds this task's attempt, so there is nobody to return it to.") }) });
  }
  buttons.push(card.held ? { id: "release", text: tr("Release") } : { id: "hold", text: tr("Hold") });
  buttons.push({ id: "cancel", text: tr("Cancel task"), danger: true, reason: true });
  if (!card.pending_decision) return buttons;
  const waiting = tr("The task's previous decision has not settled yet.");
  // A pending decision that has not started can be replaced by a new Return
  // or Cancel (the row says so); otherwise, and when an older hgs does not
  // say, the four wait for it as the store requires.
  const replaces = card.pending_replaceable === true ? replacesPending(card.pending_decision) : undefined;
  return buttons.map((button) => {
    if (replaces && REPLACING.has(button.id)) return button.blocked ? button : { ...button, note: replaces };
    return WAIT_FOR_DECISION.has(button.id) ? { ...button, blocked: waiting } : button;
  });
}

/** A member's operations, by the team's status and the member's state: pause
 * an active member; resume a paused one that has a session; stop an active or
 * paused one the person has not stopped (the store refuses a second Stop);
 * remove any member that is not removed and is not the lead. Each id is the
 * action's kind as the core reads it (client/core teams/fixtures/action-kinds.json). */
export function memberActions(member: MemberView, teamStatus: string): { id: string; text: string; danger?: boolean; disabled?: boolean; reason?: string }[] {
  if (!activeTeam(teamStatus)) return [];
  const active = member.state === "active";
  const paused = member.state === "paused";
  return [
    ...(active ? [{ id: "pause_member", text: tr("Pause") }] : []),
    ...(paused && member.session ? [{ id: "resume_member", text: tr("Resume") }] : []),
    ...(active || paused ? [{ id: "stop_member", text: tr("Stop"), danger: true,
      ...(member.person_stopped ? { disabled: true, reason: tr("You stopped this member. Resume it to let it work again.") } : {}) }] : []),
    ...(member.state !== "removed" && !member.lead ? [{ id: "archive_member", text: tr("Remove"), danger: true }] : []),
  ];
}

export function stageName(kind: string): string {
  return tr(kind === "test" ? "test" : "review");
}
