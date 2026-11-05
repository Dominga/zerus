import { tr } from "../../i18n";
import { act, hgsWords, type DecisionPort } from "./decisions";
import type { ComputerId, TeamAction, TeamConfirmation, TeamOutcome, TeamSummary } from "../../types";

// Task 40: the team bar's actions and the task card's actions, as Qt's
// TeamWorkspace (headerActionsContent, the bar chips), TeamPanel (approvePlan,
// finishTeam) and TeamTaskCard (the footer, More and the inline forms) offer
// them, with Qt's confirmations. Pure: the Team tab and the plan's task detail
// draw what these say; every call goes through the core's team actions.

/** The fields of a team these actions read (the core's TeamView; the plan view's own type has them too). */
export interface ActTeam {
  id: string; name: string; status: string; plan_mode: string; revision: number; pending_phase: number | null;
  waiting_for_lead: boolean; settings: Record<string, unknown>;
  members: { id: string; role: string; agent: string; computer: string; lead: boolean; session: { name: string } | null }[];
  tasks: ActTask[];
  phases: { n: number; title: string; state: string; status: string }[];
  widenings?: { member?: string; role?: string; from?: string; to?: string }[];
  children?: { team: string; name?: string; status?: string }[];
}

export interface ActTask {
  id: number; title: string; status: string; assignee: string; phase: number; deps: { task: number; hard: boolean }[];
  attempts: number; held?: boolean; pipeline?: string; subteam?: boolean;
  /** What it waits for, as hgs words it. */
  waits?: string;
  pipeline_run?: { name: string; accept: string; stages: string[]; open: { stage: number; kind: string; member: string; state: string } | null } | null;
}

const FINISHED = new Set(["accepted", "rejected", "cancelled"]);
export const finished = (task: Pick<ActTask, "status">) => FINISHED.has(task.status);
/** A task in one of its pipeline's stages (Qt Task::inStage). */
export const inStage = (task: Pick<ActTask, "status">) => task.status === "in_review" || task.status === "in_test";
const roleOf = (team: ActTeam, id: string) => team.members.find((member) => member.id === id)?.role || id;

/** U59: a free-mode team's empty queue is idle, not done (Qt allDecided). */
export function allDecided(team: ActTeam): boolean {
  if (team.tasks.length === 0 || team.plan_mode === "free") return false;
  return team.tasks.every(finished);
}

/** A stage kind in words (Qt stageName). */
export function stageName(kind: string): string {
  switch (kind) {
    case "review": return tr("Review");
    case "test": return tr("Test");
    case "command": return tr("Command");
    case "land": return tr("Land");
    case "implement": return tr("Implement");
    case "auto": return tr("Auto");
    case "sub_lead": return tr("Sub-lead");
    case "lead": return tr("Lead");
    default: return kind ? kind.charAt(0).toUpperCase() + kind.slice(1) : "";
  }
}

// ---------------------------------------------------------------------------
// The team bar

export interface BarChip { id: "pendingPhase" | "finishWaits" | "waitingForLead"; text: string; tip: string; phase?: number }

export interface BarAction {
  id: "resumeLimits" | "finish" | "discard" | "sendBack" | "approve" | "pause" | "resume" | "stop" | "archive";
  text: string; tip: string; primary: boolean; danger: boolean; enabled: boolean;
  /** The Resume after limits switch's state. */
  checked?: boolean;
}

/** The phase drafts that wait for the owner. */
const draftPhases = (team: ActTeam) => team.phases.filter((phase) => phase.state === "draft" || phase.status === "draft").map((phase) => phase.n);

/** The bar's chips (Qt TeamWorkspace header row): the pending phase, Finish waiting, waiting for the lead. */
export function barChips(team: ActTeam): BarChip[] {
  const chips: BarChip[] = [];
  if ((team.pending_phase ?? 0) > 0 && team.status !== "planning") {
    const drafts = draftPhases(team);
    chips.push({ id: "pendingPhase", phase: team.pending_phase!, text: tr("Phase %1 waits for you", team.pending_phase!),
      tip: drafts.length > 1 ? tr("phases %1 pending approval", drafts.join(", ")) : tr("Open the phase draft") });
  }
  if (allDecided(team) && (team.status === "running" || team.status === "paused"))
    chips.push({ id: "finishWaits", text: tr("Waiting for you to finish"), tip: tr("Every task is decided") });
  if (team.waiting_for_lead) chips.push({ id: "waitingForLead", text: tr("waiting for the lead"), tip: "" });
  return chips;
}

/** The bar's actions in Qt's order (headerActionsContent), for the team's status. */
export function barActions(team: ActTeam, planMode: string): BarAction[] {
  const out: BarAction[] = [];
  const action = (a: Omit<BarAction, "primary" | "danger" | "enabled"> & Partial<BarAction>): void => {
    out.push({ primary: false, danger: false, enabled: true, ...a });
  };
  const status = team.status;
  if (status === "planning" || status === "running" || status === "paused") {
    const on = team.settings.resume_after_limits === true;
    action({ id: "resumeLimits", text: tr("Resume after limits"), checked: on,
      tip: on ? tr("On: a member stopped by a usage limit with a known reset time continues after the reset")
        : tr("Off: a member stopped by a usage limit waits for you or the lead") });
  }
  if (allDecided(team) && (status === "running" || status === "paused"))
    action({ id: "finish", text: tr("Finish…"), tip: tr("Every task is decided"), primary: true });
  if (status === "planning") {
    action({ id: "discard", text: tr("Discard plan…"), tip: tr("Drop the draft"), danger: true });
    action({ id: "sendBack", text: tr("Send back"), tip: tr("Ask the coordinator to change the plan") });
    const errors = (team as { validation?: { errors: unknown[] } }).validation?.errors.length ?? 0;
    const enabled = errors === 0 && team.tasks.length > 0;
    action({ id: "approve", text: planMode === "free" ? tr("Approve & Run in free mode") : tr("Approve & Run"), primary: true, enabled,
      tip: !enabled ? (team.tasks.length === 0 ? tr("The plan has no tasks yet") : tr("Fix the plan errors first"))
        : (team.widenings?.length ?? 0) > 0 ? tr("A permission widening needs your confirmation") : tr("Create members and hand out ready tasks") });
  } else if (status === "running") {
    action({ id: "pause", text: tr("Pause"), tip: tr("Running turns finish; nothing new is handed out") });
  } else if (status === "paused") {
    action({ id: "resume", text: tr("Resume"), tip: tr("Hand out tasks again"), primary: true });
  }
  if (status === "running" || status === "paused") action({ id: "stop", text: tr("Stop…"), tip: tr("Cancel every open task"), danger: true });
  if (status === "done" || status === "stopped") action({ id: "archive", text: tr("Archive"), tip: tr("Archive the team") });
  return out;
}

/** G1: the sub-teams a tree action reaches and their member sessions (direct sub-teams, as the list shows them). */
export function subtreeImpact(team: ActTeam, list: readonly Pick<TeamSummary, "id" | "members">[]): { teams: number; sessions: number } {
  const children = team.children ?? [];
  let sessions = 0;
  for (const child of children) sessions += list.find((found) => found.id === child.team)?.members.filter((member) => member.session).length ?? 0;
  return { teams: children.length, sessions };
}

/** B5/N17/U59: Approve & Run's question (Qt planApproval), and what it sends. */
export function planApproval(team: ActTeam, mode: string): { confirmation: TeamConfirmation; action: TeamAction } {
  const sessions = team.members.filter((member) => !member.lead && !member.session).map((member) => {
    const role = member.role || member.id;
    return member.computer ? tr("%1 (%2 on %3)", role, member.agent, member.computer) : tr("%1 (%2)", role, member.agent);
  });
  const starts = sessions.length === 0 ? tr("Every member has a session already; none starts.")
    : sessions.length === 1 ? tr("It starts 1 member session: %1.", sessions[0])
    : tr("It starts %1 member sessions: %2.", sessions.length, sessions.join(", "));
  const runs = mode === "free" ? tr(" It runs in free planning: one open phase that takes tasks as they come until you close it.")
    : mode === "standard" ? tr(" It runs in standard planning.") : "";
  const widenings = team.widenings ?? [];
  const action: TeamAction = { kind: "approve", ...(mode ? { plan_mode: mode } : {}), ...(widenings.length ? { confirm_widening: true } : {}) };
  if (widenings.length) {
    const lines = widenings.map((widening) => tr("%1: %2 → %3", widening.role || widening.member || "", widening.from ?? "", widening.to ?? ""));
    return { action, confirmation: { name: "teamWideningConfirm", title: tr("Widen permissions?"), action: tr("Confirm and approve"),
      text: tr("This plan widens the permissions of its members beyond the template:\n\n%1\n\nApprove it only if they need it.", lines.join("\n")) + "\n\n" + starts + runs } };
  }
  return { action, confirmation: { name: "teamApproveConfirm", title: tr("Approve plan and run?"), action: tr("Approve and run"),
    text: tr("Approve the plan of %1 and run it? %2 Ready tasks are handed out at once.", team.name, starts) + runs } };
}

export function discardConfirmation(team: ActTeam): TeamConfirmation {
  return { name: "teamDiscardConfirm", title: tr("Discard plan?"), text: tr("Discard the plan of %1? The team stops before any member starts.", team.name), action: tr("Discard plan") };
}

/** Pause asks only when it reaches sub-teams (Qt teamPauseTreeConfirm). */
export function pauseConfirmation(team: ActTeam, impact: { teams: number; sessions: number }): TeamConfirmation | undefined {
  if (impact.teams === 0) return undefined;
  return { name: "teamPauseTreeConfirm", title: tr("Pause the team tree?"), action: tr("Pause"),
    text: tr("Pause %1? It also pauses %2 sub-teams and %3 member sessions in them.", team.name, impact.teams, impact.sessions) };
}

export function stopConfirmation(team: ActTeam, impact: { teams: number; sessions: number }): TeamConfirmation {
  let text = tr("Stop %1? Every open task is cancelled and no task is handed out again. Member sessions keep running; their current turns finish.", team.name);
  if (impact.teams > 0) text += tr(" It stops %1 sub-teams and affects %2 member sessions in them.", impact.teams, impact.sessions);
  return { name: "teamStopConfirm", title: tr("Stop team?"), text, action: tr("Stop team") };
}

/** Finish's summary: required, at most 4000 characters (Qt teamFinishCount). */
export function finishSummary(text: string): { count: string; ok: boolean } {
  const characters = [...text.trim()].length;
  return { count: tr("%1 / 4000", characters), ok: characters > 0 && characters <= 4000 };
}

/** Send back's message to the coordinator (Qt teamWorkspaceSendBack). */
export const sendBackMessage = (comment: string) => tr("Your plan needs changes before I approve it: %1", comment.trim());

// ---------------------------------------------------------------------------
// The task card

export type TaskForm = "accept-now" | "skip" | "send-back" | "reject" | "hold" | "done";

export interface TaskButton {
  id: "acceptNow" | "skip" | "sendBack" | "reject" | "accept" | "reassign" | "release" | "hold" | "takeOver" | "cancel";
  text: string; primary: boolean; danger: boolean;
  /** The inline form it opens, if it is one. */
  form?: TaskForm;
}

export interface MoreItem {
  id: "retry" | "hold" | "release" | "move" | "accept" | "reject" | "reassign" | "takeOver" | "cancel" | "done";
  text: string; enabled: boolean; why: string; phase?: number; form?: TaskForm;
}

/** A task that started: an attempt was made or it left Pending (Qt taskStarted). */
const started = (task: ActTask) => task.attempts > 0 || !["pending", "draft"].includes(task.status);

/** U58: the phases a task that has not started may move to, and why one may not (Qt moveTargets). */
export function moveTargets(team: ActTeam, id: number): { phase: number; title: string; blocked: string }[] {
  const task = team.tasks.find((found) => found.id === id);
  if (!task || finished(task) || started(task)) return [];
  if (!["planning", "running", "paused"].includes(team.status)) return [];
  const phases = [...new Set([1, ...team.phases.filter((phase) => phase.n > 1 && phase.state !== "discarded").map((phase) => phase.n)])].sort((a, b) => a - b);
  return phases.filter((n) => n !== task.phase).map((n) => {
    const dep = task.deps.map((found) => team.tasks.find((t) => t.id === found.task)).find((found) => found && found.phase > n);
    return { phase: n, title: team.phases.find((phase) => phase.n === n)?.title ?? "",
      blocked: dep ? tr("Depends on #%1 in phase %2; move it first", dep.id, dep.phase) : "" };
  });
}

/** The footer's buttons for the task's status (Qt TeamTaskCard footer). */
export function taskButtons(task: ActTask): TaskButton[] {
  const decided = finished(task);
  const open = !decided && task.status !== "draft";
  const b = (id: TaskButton["id"], text: string, primary = false, danger = false, form?: TaskForm): TaskButton => ({ id, text, primary, danger, form });
  if (inStage(task)) {
    const stage = task.pipeline_run?.open;
    return [
      b("acceptNow", tr("Accept now…"), true, false, "accept-now"),
      ...(stage ? [b("skip", tr("Skip %1…", stageName(stage.kind).toLowerCase()), false, false, "skip")] : []),
      b("sendBack", tr("Send back…"), false, false, "send-back"),
      b("reject", tr("Reject…"), false, true, "reject"),
    ];
  }
  if (task.status === "submitted") {
    return [
      b("accept", tr("Accept"), true),
      b("reject", tr("Reject with comment…"), false, false, "reject"),
      ...(task.pipeline_run ? [b("sendBack", tr("Send back…"), false, false, "send-back")] : []),
      b("reassign", tr("Reassign…")),
    ];
  }
  if (open) {
    return [
      ...(task.held ? [b("release", tr("Release"), true)] : task.status === "pending" ? [b("hold", tr("Hold…"), false, false, "hold")] : []),
      b("reassign", tr("Reassign…")),
      b("takeOver", tr("Take over")),
      b("cancel", tr("Cancel"), false, true),
    ];
  }
  return [];
}

/** More: the rest, disabled with the reason they do not apply (Qt teamTaskMoreMenu). */
export function moreItems(team: ActTeam, task: ActTask): MoreItem[] {
  const decided = finished(task);
  const open = !decided && task.status !== "draft";
  const submitted = task.status === "submitted";
  const decidedWhy = tr("Task #%1 is already decided", task.id);
  const items: MoreItem[] = [
    { id: "retry", text: tr("Retry"), enabled: open, why: decided ? decidedWhy : tr("Nothing to retry yet") },
    { id: "hold", text: tr("Hold…"), enabled: !decided && task.status === "pending" && !task.held, form: "hold",
      why: task.held ? tr("Already held") : decided ? decidedWhy : tr("Only a pending task can be held") },
    { id: "release", text: tr("Release"), enabled: !!task.held, why: task.held ? "" : tr("Not held") },
  ];
  for (const target of moveTargets(team, task.id))
    items.push({ id: "move", phase: target.phase, enabled: !target.blocked, why: target.blocked,
      text: target.title ? tr("Move to phase %1 %2", target.phase, target.title) : tr("Move to phase %1", target.phase) });
  if (!submitted && !inStage(task)) {
    items.push({ id: "accept", text: tr("Accept"), enabled: false, why: decided ? decidedWhy : tr("Nothing to accept yet: no report") });
    items.push({ id: "reject", text: tr("Reject with comment…"), enabled: false, why: decided ? decidedWhy : tr("Nothing to reject yet: no report") });
  }
  if (submitted || decided || inStage(task)) {
    items.push({ id: "reassign", text: tr("Reassign…"), enabled: open, why: decidedWhy });
    items.push({ id: "takeOver", text: tr("Take over"), enabled: open, why: decidedWhy });
    items.push({ id: "cancel", text: tr("Cancel task…"), enabled: open, why: decidedWhy });
  }
  items.push({ id: "done", text: tr("Mark done manually…"), enabled: !decided && task.status !== "draft", form: "done",
    why: decided ? decidedWhy : tr("Approve the plan first") });
  return items;
}

/** An inline form's words (Qt TeamTaskCard's override and inline forms). */
export function formText(team: ActTeam, task: ActTask, form: TaskForm): { heading: string; what: string; placeholder: string; submit: string } {
  const ref = `#${task.id} ${task.title}`;
  const run = task.pipeline_run;
  const open = inStage(task) ? run?.open ?? null : null;
  const stage = open ? stageName(open.kind) : "";
  switch (form) {
    case "accept-now": {
      const rest = open && run ? run.stages.slice(open.stage).map(stageName) : [];
      return { heading: tr("Accept %1 now?", ref), placeholder: tr("Why accept it now?"), submit: tr("Accept now"),
        what: rest.length === 0 ? tr("Accepts the report; the feed records that you accepted it and why.")
          : rest.length === 1 ? tr("Ends the pipeline here; the %1 stage does not run. The feed records that you accepted it and why.", rest[0])
          : tr("Ends the pipeline here; the %1 stages do not run. The feed records that you accepted it and why.", rest.join(tr(" and "))) };
    }
    case "skip": {
      const next = open ? open.stage + 1 : 0;
      const then = run && next < run.stages.length ? tr("moves to %1", stageName(run.stages[next]))
        : run?.accept === "auto" ? tr("is accepted by the pipeline") : tr("goes to %1 to accept", stageName(run?.accept === "sub_lead" ? "sub_lead" : "lead"));
      const who = open?.member ? roleOf(team, open.member) : tr("The stage");
      return { heading: tr("Skip %1 on %2?", stage, ref), what: tr("%1 stops; #%2 %3. The feed records that you skipped it and why.", who, task.id, then),
        placeholder: tr("Why skip the %1?", stage.toLowerCase()), submit: tr("Skip %1", stage) };
    }
    case "send-back": {
      const implementer = roleOf(team, task.assignee);
      return { heading: tr("Send %1 back to %2?", ref, implementer), placeholder: tr("What must change?"), submit: tr("Send back"),
        what: open ? tr("The %1 stops; %2 gets a new attempt with your reason. It does not count as a round.", stage.toLowerCase(), implementer)
          : tr("%1 gets a new attempt with your reason. It does not count as a round.", implementer) };
    }
    case "reject":
      return { heading: tr("Reject with comment"), what: tr("The comment goes to the member with the next attempt."), placeholder: tr("What must change?"), submit: tr("Reject & send back") };
    case "hold":
      return { heading: tr("Hold task"), what: tr("The task stays with its member and is not handed out until you release it."), placeholder: tr("Why does it wait?"), submit: tr("Hold") };
    case "done":
      return { heading: tr("Mark done manually"), what: tr("The task counts as accepted without a report; the reason stays in the feed."), placeholder: tr("Why is it done?"), submit: tr("Mark done") };
  }
}

/** The action a submitted form sends (a reason is required). */
export function formAction(task: ActTask, form: TaskForm, reason: string, returnLater = false): TeamAction | undefined {
  const why = reason.trim();
  if (!why) return undefined;
  switch (form) {
    case "accept-now": return { kind: "accept", task: task.id, reason: why };
    case "skip": return { kind: "skip_stage", task: task.id, reason: why };
    case "send-back": return { kind: "send_back", task: task.id, reason: why };
    case "reject": return { kind: "reject", task: task.id, reason: why, ...(returnLater ? { hold: true } : {}) };
    case "hold": return { kind: "hold", task: task.id, reason: why };
    case "done": return { kind: "accept", task: task.id, reason: why, manual: true };
  }
}

/** Retry asks first, naming the attempt it revokes (Qt teamRetryConfirm). */
export function retryConfirmation(task: ActTask): TeamConfirmation | undefined {
  if (task.attempts === 0) return undefined;
  return { name: "teamRetryConfirm", title: tr("Retry task?"), text: tr("Revoke attempt %1 and hand the task out again?", task.attempts), action: tr("Retry") };
}

// ---------------------------------------------------------------------------
// Running them

/**
 * Runs the team's actions through the core, once each. A confirmation the
 * webview words (Qt's text) is asked here and the call goes confirmed; the
 * core asks for its own (Stop, Archive, Cancel, Take over, Discard) through
 * the same dialog. A refusal or an unknown outcome says why in `notice`; an
 * unknown outcome keeps its request id, so sending again is the same request.
 */
export class TeamActions {
  notice = "";
  busy = false;
  private uncertain = new Map<string, string>();

  constructor(private port: DecisionPort, private where: () => { computer: ComputerId; team: ActTeam },
    private changed: () => void = () => {}) {}

  private async run(key: string, action: TeamAction, ask?: TeamConfirmation): Promise<TeamOutcome | "cancelled" | undefined> {
    if (this.busy) return undefined;
    if (ask && !await this.port.confirm(ask)) return "cancelled";
    const { computer, team } = this.where();
    this.busy = true;
    this.notice = "";
    this.changed();
    const call = { computer, team: team.id, team_name: team.name, action };
    const outcome = ask ? await this.confirmed(call, this.uncertain.get(key)) : await act(this.port, call, this.uncertain.get(key));
    this.busy = false;
    if (outcome !== "cancelled") {
      if (outcome.outcome === "uncertain") { this.uncertain.set(key, outcome.request_id); this.notice = tr("It may have been sent: %1", hgsWords(outcome.detail)); }
      else this.uncertain.delete(key);
      if (outcome.outcome === "failed") this.notice = hgsWords(outcome.detail);
    }
    this.changed();
    return outcome;
  }

  /** Sends a call the owner already confirmed here. */
  private async confirmed(call: { computer: ComputerId; team: string; team_name: string; action: TeamAction }, requestId?: string): Promise<TeamOutcome> {
    try { return await this.port.send({ ...call, confirmed: true, request_id: requestId ?? null }); }
    catch (reason) { return { outcome: "failed", detail: String(reason instanceof Error ? reason.message : reason), request_id: requestId ?? "" }; }
  }

  // The team bar.
  approve(mode: string) { const { action, confirmation } = planApproval(this.where().team, mode); return this.run("approve", action, confirmation); }
  discard() { return this.run("discard", { kind: "discard" }, discardConfirmation(this.where().team)); }
  pause(impact: { teams: number; sessions: number }) { return this.run("pause", { kind: "pause" }, pauseConfirmation(this.where().team, impact)); }
  resume() { return this.run("resume", { kind: "resume" }); }
  stop(impact: { teams: number; sessions: number }) { return this.run("stop", { kind: "stop" }, stopConfirmation(this.where().team, impact)); }
  archive() { return this.run("archive", { kind: "archive" }); }
  /** Finish: the dialog with its summary is the confirmation. */
  async finish(summary: string) {
    if (!finishSummary(summary).ok) return undefined;
    const { computer, team } = this.where();
    if (this.busy) return undefined;
    this.busy = true; this.notice = ""; this.changed();
    const outcome = await this.confirmed({ computer, team: team.id, team_name: team.name, action: { kind: "finish", summary: summary.trim() } }, this.uncertain.get("finish"));
    this.busy = false;
    if (outcome.outcome === "uncertain") this.uncertain.set("finish", outcome.request_id); else this.uncertain.delete("finish");
    if (outcome.outcome === "failed" || outcome.outcome === "uncertain") this.notice = hgsWords(outcome.detail);
    this.changed();
    return outcome;
  }
  /** N33: only the switch's own field; the server merges it. */
  resumeAfterLimits(on: boolean) { return this.run("limits", { kind: "settings", settings: { resume_after_limits: on } }); }

  // The task card.
  accept(task: ActTask) { return this.run(`accept:${task.id}`, { kind: "accept", task: task.id }); }
  release(task: ActTask) { return this.run(`release:${task.id}`, { kind: "release", task: task.id }); }
  reassign(task: ActTask, member: string) { return this.run(`reassign:${task.id}`, { kind: "reassign", task: task.id, member }); }
  takeOver(task: ActTask) { return this.run(`take-over:${task.id}`, { kind: "take_over", task: task.id }); }
  cancel(task: ActTask) { return this.run(`cancel:${task.id}`, { kind: "cancel", task: task.id }); }
  retry(task: ActTask) { return this.run(`retry:${task.id}`, { kind: "retry", task: task.id }, retryConfirmation(task)); }
  move(task: ActTask, phase: number) { return this.run(`move:${task.id}`, { kind: "move_task", task: task.id, phase, revision: this.where().team.revision }); }
  /** An inline form: nothing is sent without a reason. */
  submit(task: ActTask, form: TaskForm, reason: string, returnLater = false) {
    const action = formAction(task, form, reason, returnLater);
    return action ? this.run(`${form}:${task.id}`, action) : Promise.resolve(undefined);
  }
}
