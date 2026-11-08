import { tr, trNoop } from "../../i18n";
import { withdrawnRefusal } from "../../teamRefusals";
import { retryContinues, retryContinueText } from "./actions";
import type { ComputerId, TeamAction, TeamCall, TeamChoice, TeamConfirmation, TeamInboxItem, TeamMemberQuestion, TeamMemberView, TeamOutcome, TeamProposalView, TeamView, TeamWaitingItem } from "../../types";
import type { TeamTaskDetailView } from "../../types";

/** One selected report, with no per-task cache; stale completions are discarded. */
export class TaskReportRead {
  loading = false;
  error = "";
  detail: TeamTaskDetailView | null = null;
  private generation = 0;

  constructor(private read: (computer: ComputerId, team: string, task: number) => Promise<TeamTaskDetailView>, private changed: () => void) {}

  async open(computer: ComputerId, team: string, task: number) {
    const generation = ++this.generation;
    this.loading = true;
    this.error = "";
    this.detail = null;
    this.changed();
    try {
      const detail = await this.read(computer, team, task);
      if (generation !== this.generation) return;
      this.detail = detail;
    } catch (error) {
      if (generation !== this.generation) return;
      this.error = hgsWords(error instanceof Error ? error.message : String(error));
    } finally {
      if (generation === this.generation) {
        this.loading = false;
        this.changed();
      }
    }
  }

  close() {
    ++this.generation;
    this.loading = false;
    this.detail = null;
    this.error = "";
    this.changed();
  }
}

// The owner's decisions on a team (task 11), ported from Qt's
// TeamProposalCard.cpp, TeamWaitingAnswer.cpp, TeamWaitingPage.cpp and
// TeamWaitingBanner.cpp: the lead's proposal cards (Allow, Deny with a reason,
// a question's options), every item of Waiting for you answered in place, and
// the notices. Pure: the components hold one of these per card or row and draw
// what it says; the tests drive them through a fake port.
//
// Every call goes through the core's team actions (task 7): an action the
// core wants confirmed comes back as `confirm` and is sent again only after
// the owner's choice; a destructive card asks first with Cancel focused. A
// card or item that changed or was decided meanwhile sends nothing and says
// so; a refusal says why on the card, in hgs's words; an unknown outcome keeps
// its request id, so answering again is the same request.

/** What a decision needs from the window. */
export interface DecisionPort {
  /** Sends one team action (bridge.teamAction). */
  send(call: TeamCall): Promise<TeamOutcome>;
  /** Asks the owner; true only for the confirming button (Cancel is focused). */
  confirm(confirmation: TeamConfirmation): Promise<boolean>;
  /** Task 52: asks the owner to choose one answer; undefined is Cancel (Cancel is focused). */
  choose?(choice: TeamChoice): Promise<string | undefined>;
  /** Reads one team once (bridge.teamRead). */
  read(computer: ComputerId, team: string): Promise<TeamView>;
}

const str = (value: unknown) => (typeof value === "string" ? value : "");
const num = (value: unknown) => (typeof value === "number" && Number.isFinite(value) ? value : 0);
const obj = (value: unknown): Record<string, unknown> => (value && typeof value === "object" && !Array.isArray(value) ? value as Record<string, unknown> : {});
const arr = (value: unknown): unknown[] => (Array.isArray(value) ? value : []);

/** hgs's own words: without the "hgs: " prefix, or that it gave none. */
export function hgsWords(message: string): string {
  let why = message.trim();
  while (why.startsWith("hgs: ")) why = why.slice(5).trim();
  return why || tr("Zerus gave no reason");
}

/**
 * Sends an action once. A confirmation the core asks for is the owner's: Cancel
 * gives `cancelled` and sends nothing; the confirming button sends it again,
 * confirmed. `requestId` repeats an earlier request whose outcome was unknown.
 */
export async function act(port: Pick<DecisionPort, "send" | "confirm">, call: Omit<TeamCall, "confirmed">, requestId?: string): Promise<TeamOutcome | "cancelled"> {
  const sent: TeamCall = { ...call, confirmed: false, request_id: requestId ?? null };
  let outcome: TeamOutcome;
  try { outcome = await port.send(sent); } catch (reason) { return { outcome: "failed", detail: String(reason instanceof Error ? reason.message : reason), request_id: requestId ?? "" }; }
  if (outcome.outcome !== "confirm") return outcome;
  if (!await port.confirm(outcome.confirmation)) return "cancelled";
  try { return await port.send({ ...sent, confirmed: true }); } catch (reason) { return { outcome: "failed", detail: String(reason instanceof Error ? reason.message : reason), request_id: requestId ?? "" }; }
}

// ---------------------------------------------------------------------------
// Proposal cards (Qt TeamProposalCards)

const roleOf = (team: TeamView | undefined, id: string) => team?.members.find((member) => member.id === id)?.role || id;
const hasMember = (team: TeamView | undefined, id: string) => !!id && !!team?.members.some((member) => member.id === id);

/** The kind's pill: what it is in words, never the raw kind. */
export function proposalKindLabel(kind: string): string {
  switch (kind) {
    case "add-member": case "scale": return tr("Members");
    case "remove-member": return tr("Remove");
    case "swap-member": return tr("Swap");
    case "next-goal": return tr("Next goal");
    case "switch-plan-mode": return tr("Plan mode");
    case "working-mode": return tr("Working mode");
    case "add-to-phase": return tr("Tasks");
    case "cancel-tasks": return tr("Cancel");
    case "question": return tr("Question");
    case "switch-setting": case "settings": return tr("Setting");
    case "relaunch": return tr("Relaunch");
    case "drain": return tr("Drain");
    case "approve": case "approve-phase": return tr("Approval");
    case "discard-phase": case "return-to-draft": case "edit-draft": case "move-phase": case "unpause-phase": case "phase-acceptance":
    case "accept-phase": case "extend": return tr("Phase");
    case "iron-rules": return tr("Iron rules");
    case "install-tool": return tr("Tool");
    case "full-tests": return tr("Tests");
    default: return kind;
  }
}

/** "Designer (claude, sonnet, default account)": no dots as separators. */
function proposedMember(team: TeamView | undefined, member: Record<string, unknown>): string {
  const id = str(member.id);
  const role = hasMember(team, id) ? roleOf(team, id) : str(member.role) || id;
  const how = ["agent", "model", "effort", "account"].map((key) => str(member[key])).filter(Boolean);
  // A member without an account runs on its computer's default one.
  if (!str(member.account) && !("session" in member)) how.push(tr("default account"));
  return how.length ? `${role} (${how.join(", ")})` : role;
}

function proposedMembers(proposal: TeamProposalView): Record<string, unknown>[] {
  const members = arr(obj(proposal.change).members).map(obj);
  return members.length === 0 && proposal.member && typeof proposal.member === "object" ? [obj(proposal.member)] : members;
}

function phaseName(team: TeamView | undefined, n: number): string {
  const title = team?.phases.find((phase) => phase.n === n)?.title;
  return title ? tr("phase %1 (%2)", n, title) : tr("phase %1", n);
}

function taskStatus(status: string): string {
  switch (status) {
    case "pending": return tr("Pending");
    case "in_progress": return tr("In progress");
    case "submitted": return tr("Submitted");
    case "in_review": return tr("In review");
    case "in_test": return tr("In test");
    case "accepted": return tr("Accepted");
    case "rejected": return tr("Rejected");
    case "cancelled": return tr("Cancelled");
    case "draft": return tr("Draft");
    case "held": return tr("Held");
    default: return status;
  }
}

/** The card's head line: what hgs wrote for an owner-gated operation, else the change in words. */
export function proposalText(team: TeamView | undefined, proposal: TeamProposalView): string {
  const change = obj(proposal.change);
  if ("op" in change && proposal.summary && proposal.summary !== proposal.kind) return proposal.summary;
  if ("op" in change && str(change.summary)) return str(change.summary);
  const member = obj(proposal.member);
  const id = str(member.id);
  const role = hasMember(team, id) ? roleOf(team, id) : str(member.role) || id;
  const how = ["agent", "model", "effort", "account"].map((key) => str(member[key])).filter(Boolean);
  switch (proposal.kind) {
    case "add-member": {
      const members = proposedMembers(proposal);
      if (members.length > 1) return tr("Add %1 members", members.length);
      return tr("Add %1", proposedMember(team, members[0] ?? member));
    }
    case "remove-member": return tr("Remove %1", role);
    case "swap-member": {
      const members = proposedMembers(proposal);
      if (members.length > 1) return tr("Swap %1 members", members.length);
      const session = str(obj(member.session).name);
      if (session) return tr("Swap %1 to the session %2", role, session);
      return how.length ? tr("Swap %1 to %2", role, how.join(", ")) : tr("Swap %1 to a new session with the same settings", role);
    }
    case "next-goal": return tr("Next goal: %1", (proposal.goal ?? "").split("\n")[0].slice(0, 120));
    case "switch-plan-mode": {
      // Task #58: a card that names a phase switches that phase only.
      const phase = num(change.phase);
      if (phase > 0) return proposal.mode === "standard" ? tr("Switch phase %1 to standard planning", phase) : tr("Switch phase %1 to free planning", phase);
      return proposal.mode === "standard" ? tr("Switch to standard planning") : tr("Switch to free planning");
    }
    case "working-mode": return tr("Switch to %1", str(change.preset) || tr("the proposed working mode"));
    case "add-to-phase": {
      const count = arr(change.tasks).length;
      const phase = phaseName(team, num(change.phase));
      return count === 1 ? tr("Add 1 task to %1", phase) : tr("Add %1 tasks to %2", count, phase);
    }
    case "cancel-tasks": {
      const count = arr(change.tasks).length;
      return count === 1 ? tr("Cancel 1 task") : tr("Cancel %1 tasks", count);
    }
    case "discard-phase": return tr("Discard %1", phaseName(team, num(change.phase)));
    case "question": return str(change.question);
    case "install-tool": {
      const tool = str(change.tool) || tr("a tool");
      const why = str(change.why) || proposal.reason;
      return why ? tr("Install %1: %2", tool, why) : tr("Install %1", tool);
    }
    case "switch-setting": {
      const on = change.value === true;
      if (str(change.setting) === "build_broker") return on ? tr("Turn on the build broker") : tr("Turn off the build broker");
      return on ? tr("Turn on %1", str(change.setting)) : tr("Turn off %1", str(change.setting));
    }
    default: return proposal.summary || proposal.kind;
  }
}

/** The card's change lines, one per member, task or setting it touches. */
export function proposalLines(team: TeamView | undefined, proposal: TeamProposalView): string[] {
  const change = obj(proposal.change);
  if ("op" in change) return proposal.lines.length ? [...proposal.lines] : arr(change.lines).map(str).filter(Boolean);
  const taskLine = (id: number) => {
    const task = team?.tasks.find((found) => found.id === id);
    return task ? tr("#%1 %2 (%3)", id, task.title, taskStatus(task.status)) : `#${id}`;
  };
  const lines: string[] = [];
  switch (proposal.kind) {
    case "add-member": {
      const members = proposedMembers(proposal);
      if (members.length > 1) for (const member of members) lines.push(proposedMember(team, member));
      break;
    }
    case "remove-member": lines.push(tr("Its session keeps running; it gets no more tasks.")); break;
    case "swap-member": {
      const members = proposedMembers(proposal);
      if (members.length > 1) for (const member of members) {
        const how = ["agent", "model", "effort", "account"].map((key) => str(member[key])).filter(Boolean);
        if ("prompt" in member) how.push(tr("new prompt"));
        const role = roleOf(team, str(member.id));
        lines.push(how.length ? `${role}: ${how.join(", ")}` : tr("%1: a new session with the same settings", role));
      }
      lines.push(tr("Each open task goes to the new session with what was done so far; the old session is paused and kept under Saved as <name>-replaced."));
      break;
    }
    case "next-goal": lines.push((proposal.goal ?? "").slice(0, 600)); break;
    case "install-tool": {
      // The command and folder are drawn as the card's own line and code block.
      const project = str(change.project);
      if (project) lines.push(tr("Project: %1", project));
      if (str(change.saves)) lines.push(tr("It saves %1", str(change.saves)));
      break;
    }
    case "working-mode":
      for (const reason of arr(change.reasons)) lines.push(str(reason));
      break;
    case "add-to-phase":
      for (const value of arr(change.tasks)) {
        const task = obj(value);
        const title = str(task.title) || tr("Untitled task");
        const assignee = str(task.assignee);
        let line = assignee ? tr("%1, for %2", title, roleOf(team, assignee)) : title;
        const estimate = num(task.estimate_minutes);
        if (estimate > 0) line += tr(", ~%1 min", Math.round(estimate));
        lines.push(line);
      }
      break;
    case "cancel-tasks": for (const value of arr(change.tasks)) lines.push(taskLine(num(value))); break;
    case "discard-phase": {
      const n = num(change.phase);
      for (const task of team?.tasks ?? []) if (task.phase === n) lines.push(taskLine(task.id));
      if (!lines.length) lines.push(tr("It has no tasks."));
      break;
    }
    case "switch-setting":
      if (str(change.setting) === "build_broker") {
        for (const value of arr(change.shared)) {
          const shared = obj(value);
          lines.push(tr("%1 share %2", arr(shared.members).map((member) => roleOf(team, str(member))).join(", "), str(shared.folder)));
        }
        if (change.value === true) lines.push(tr("Each member builds its own snapshot of the folder, one build at a time per build folder; an unchanged tree builds once, and a failure in another member's file goes to that member."));
      }
      break;
  }
  return lines;
}

/** One proposal as its card shows it. */
export interface ProposalCard {
  id: string; hash: string; kind: string; by: string; reason: string; at: number; what: string; lines: string[];
  /** A question's options and its recommended one. */
  options: string[]; recommended: string;
  /** Task 251: an install-tool card's exact command and folder (empty otherwise). */
  command: string; folder: string;
}

export function proposalCards(team: TeamView | undefined): ProposalCard[] {
  return (team?.proposals ?? []).map((proposal) => {
    const change = obj(proposal.change);
    return {
      id: proposal.id, hash: proposal.hash, kind: proposal.kind, by: proposal.by || tr("The lead"), reason: proposal.reason, at: proposal.at,
      what: proposalText(team, proposal), lines: proposalLines(team, proposal),
      options: proposal.kind === "question" ? arr(change.options).map(str).filter(Boolean) : [],
      recommended: proposal.kind === "question" ? str(change.recommended) : "",
      command: proposal.kind === "install-tool" ? arr(change.command).map(str).filter(Boolean).join(" ") : "",
      folder: proposal.kind === "install-tool" ? str(change.folder) : "",
    };
  });
}

/** Allow of these kinds asks first: they remove, replace or cancel what exists. */
export function proposalAsk(card: ProposalCard): TeamConfirmation | undefined {
  switch (card.kind) {
    case "remove-member":
      return { name: "teamProposalConfirm", title: tr("Remove member?"), text: tr("%1? Its session keeps running; it gets no more tasks.", card.what), action: tr("Remove") };
    case "swap-member":
      return { name: "teamProposalConfirm", title: tr("Swap member?"), action: tr("Swap"),
        text: tr("%1? Each open task goes to the new session with what was done so far; the old session is paused and kept under Saved as <name>-replaced.", card.what) };
    case "cancel-tasks":
      return { name: "teamProposalConfirm", title: tr("Cancel tasks?"), text: `${card.what}?\n${card.lines.join("\n")}`, action: tr("Cancel tasks") };
    case "discard-phase":
      return { name: "teamProposalConfirm", title: tr("Discard phase?"), text: tr("%1? Its tasks are cancelled and the later phases close up.", card.what), action: tr("Discard") };
    case "drain":
      return { name: "teamProposalConfirm", title: tr("Drain member?"), text: `${card.what}? ${card.lines.join(" ")}`, action: tr("Drain") };
    default: return undefined;
  }
}

const sameCard = (a: ProposalCard, b: ProposalCard) => a.hash === b.hash && a.what === b.what && a.lines.join("\n") === b.lines.join("\n");

/** One card as it is drawn. */
export interface CardState {
  card: ProposalCard;
  /** Its proposal is gone; it stays to say what happened until dismissed. */
  gone: boolean;
  notice: string; reason: string; sending: boolean;
  /** A question's chosen option, and whether Other (the owner's words) is chosen. */
  picked: string; own: boolean;
}

/**
 * The open proposals of one team as cards (Qt TeamProposalCards): the lead's
 * conversation shows them all; a Waiting for you row shows `only` its own.
 */
export class ProposalCards {
  private teamId = "";
  private views: ProposalCard[] = [];
  private stale = new Map<string, ProposalCard>();
  private notices = new Map<string, string>();
  private reasons = new Map<string, string>();
  private sending = new Set<string>();
  /** Task 107: decided, confirmed by hgs: hidden at once, until the store no longer lists them. */
  private decided = new Set<string>();
  private picked = new Map<string, string>();
  private ownWords = new Set<string>();
  private uncertain = new Map<string, string>();
  private asking = false;

  constructor(
    private port: DecisionPort,
    private where: { computer: ComputerId; teamName: () => string },
    /** The team as the window has it now: read again before anything is sent. */
    private current: () => TeamView | undefined | Promise<TeamView | undefined>,
    private changed: () => void = () => {},
    private only?: string,
  ) {}

  /** Takes the team as read; the cards of decided proposals go (those that say something stay). */
  setTeam(team: TeamView | undefined) {
    const id = team?.id ?? "";
    let views = proposalCards(team);
    if (this.only) views = views.filter((view) => view.id === this.only);
    if (id !== this.teamId) { this.stale.clear(); this.notices.clear(); this.reasons.clear(); this.sending.clear(); this.uncertain.clear(); this.decided.clear(); }
    const open = new Set(views.map((view) => view.id));
    for (const sent of [...this.sending]) if (!open.has(sent)) this.sending.delete(sent);
    for (const done of [...this.decided]) if (!open.has(done)) this.decided.delete(done);
    // A decided card stays hidden while an older read still lists it.
    views = views.filter((view) => !this.decided.has(view.id));
    for (const view of this.views) if (!open.has(view.id) && this.notices.has(view.id)) this.stale.set(view.id, view);
    for (const key of [...this.stale.keys()]) if (open.has(key)) this.stale.delete(key);
    for (const key of [...this.reasons.keys()]) if (!open.has(key)) this.reasons.delete(key);
    for (const key of [...this.notices.keys()]) if (!open.has(key) && !this.stale.has(key)) this.notices.delete(key);
    this.teamId = id;
    this.views = views;
    if (!this.asking) this.changed();
  }

  get team() { return this.teamId; }

  /** Task 107: hgs confirmed this card's decision: it leaves at once. */
  private confirmed(id: string) {
    this.sending.delete(id);
    this.decided.add(id);
    this.views = this.views.filter((view) => view.id !== id);
  }

  cards(): CardState[] {
    const state = (card: ProposalCard, gone: boolean): CardState => ({
      card, gone, notice: this.notices.get(card.id) ?? "", reason: this.reasons.get(card.id) ?? "", sending: this.sending.has(card.id),
      picked: this.picked.get(card.id) ?? "", own: card.options.length === 0 || this.ownWords.has(card.id),
    });
    return [...this.views.map((card) => state(card, false)), ...[...this.stale.values()].map((card) => state(card, true))];
  }

  setReason(id: string, text: string) { this.reasons.set(id, text); this.changed(); }
  pick(id: string, option: string) { this.picked.set(id, option); this.ownWords.delete(id); this.changed(); }
  chooseOwnWords(id: string) { this.ownWords.add(id); this.changed(); }
  dismiss(id: string) { this.stale.delete(id); this.notices.delete(id); this.changed(); }

  private async now(): Promise<TeamView | undefined> {
    const team = await this.current();
    if (team && team.id === this.teamId) this.setTeam(team);
    return team;
  }

  private async send(id: string, action: TeamAction): Promise<TeamOutcome | "cancelled"> {
    this.sending.add(id);
    this.notices.delete(id);
    this.changed();
    const outcome = await act(this.port, { computer: this.where.computer, team: this.teamId, team_name: this.where.teamName(), action }, this.uncertain.get(id));
    if (outcome === "cancelled" || outcome.outcome !== "done") this.sending.delete(id);
    if (outcome !== "cancelled" && outcome.outcome === "uncertain") this.uncertain.set(id, outcome.request_id);
    else this.uncertain.delete(id);
    return outcome;
  }

  /** Allow or Deny: a destructive Allow asks first; a changed or decided proposal sends nothing. */
  async answer(id: string, allow: boolean): Promise<TeamOutcome | "cancelled" | "stale" | undefined> {
    const view = this.views.find((card) => card.id === id);
    if (!view || this.sending.has(id) || this.asking) return undefined;
    if (allow) {
      const ask = proposalAsk(view);
      if (ask) {
        this.asking = true;
        let yes = false;
        try { yes = await this.port.confirm(ask); } finally { this.asking = false; }
        if (!yes) { this.changed(); return "cancelled"; }
      }
    }
    await this.now();
    const now = this.views.find((card) => card.id === id);
    if (!now || !sameCard(now, view)) {
      if (!now) { this.stale.set(id, view); this.notices.set(id, tr("This proposal was decided or withdrawn meanwhile; nothing was sent.")); }
      else this.notices.set(id, tr("The lead changed this proposal while you answered; nothing was sent. Read it again and answer."));
      this.changed();
      return "stale";
    }
    const reason = (this.reasons.get(id) ?? "").trim();
    const action: TeamAction = { kind: "decide_proposal", proposal: id, accept: allow, expected_hash: view.hash, ...(!allow && reason ? { reason } : {}) };
    const outcome = await this.send(id, action);
    if (outcome !== "cancelled") {
      // Task 111: hgs withdrew it (it can no longer apply): it leaves at once.
      if (outcome.outcome === "failed" && withdrawnRefusal(outcome.detail)) this.confirmed(id);
      else if (outcome.outcome === "failed") {
        // #754: a refusal says why on the card, in the server's words; the card stays to answer again.
        this.notices.set(id, allow ? tr("Allow was refused, nothing was applied: %1", hgsWords(outcome.detail))
          : tr("Deny was refused, nothing was decided: %1", hgsWords(outcome.detail)));
      } else if (outcome.outcome === "uncertain") {
        this.notices.set(id, tr("It may have been decided: %1", hgsWords(outcome.detail)));
      } else if (outcome.outcome === "done") {
        // A replay returns the stored decision: it was decided before.
        const status = str(obj(outcome.receipt).status);
        const wanted = allow ? "accepted" : "rejected";
        if ((status === "accepted" || status === "rejected") && status !== wanted)
          this.notices.set(id, status === "accepted" ? tr("It was already allowed; your Deny changed nothing.") : tr("It was already denied; your Allow changed nothing."));
        else this.confirmed(id);
      }
    }
    this.changed();
    return outcome;
  }

  /** A question proposal: the chosen option or the owner's words, sent with its hash. */
  async answerQuestion(id: string): Promise<TeamOutcome | "cancelled" | "stale" | undefined> {
    const view = this.views.find((card) => card.id === id);
    if (!view || this.sending.has(id) || this.asking) return undefined;
    const own = view.options.length === 0 || this.ownWords.has(id);
    const answer = (own ? this.reasons.get(id) ?? "" : this.picked.get(id) ?? "").trim();
    if (!answer) return undefined;
    await this.now();
    const now = this.views.find((card) => card.id === id);
    if (!now || now.hash !== view.hash) {
      if (!now) this.stale.set(id, view);
      this.notices.set(id, tr("The lead changed or withdrew this question; nothing was sent."));
      this.changed();
      return "stale";
    }
    const outcome = await this.send(id, { kind: "decide_proposal", proposal: id, accept: true, expected_hash: view.hash, answer });
    if (outcome !== "cancelled" && outcome.outcome === "failed" && withdrawnRefusal(outcome.detail)) this.confirmed(id);
    else if (outcome !== "cancelled" && outcome.outcome === "failed")
      this.notices.set(id, outcome.detail.includes("changed") ? tr("The question changed; nothing was sent. Read it again and answer.") : hgsWords(outcome.detail));
    else if (outcome !== "cancelled" && outcome.outcome === "done") this.confirmed(id);
    this.changed();
    return outcome;
  }
}

// ---------------------------------------------------------------------------
// Waiting for you (Qt TeamWaitingPage, TeamWaitingBanner, TeamWaitingAnswer)

/**
 * Task 107: the items of Waiting for you a confirmed action decided, as Qt's
 * TeamWaitingFeed::decidedIds matches them: a proposal by its id, a draft or
 * a waiting phase by its number, a held or reported task by its id. Another
 * team's items, and actions that name none of these, decide nothing.
 */
export function decidedWaiting(items: readonly TeamWaitingItem[], call: Pick<TeamCall, "team" | "action">): string[] {
  const action = call.action as { kind: string; proposal?: string; phase?: number | null; task?: number };
  const phase = action.phase ?? 0, task = action.task ?? 0;
  return items.filter((item) => {
    if (!call.team || item.team !== call.team) return false;
    const fields = waitingFields(item);
    switch (action.kind) {
      case "decide_proposal": return !!action.proposal && (fields.proposal === action.proposal || item.id === `${call.team}/owner_word_asked/proposal-${action.proposal}`);
      case "approve": return item.kind === "phase_draft" && (phase > 0 ? fields.phase === phase : true);
      case "accept_phase": case "send_back_phase": return item.kind === "phase_acceptance" && phase > 0 && fields.phase === phase;
      case "release": return item.kind === "task_waits_owner" && task > 0 && fields.task === task;
      case "resume": return item.kind === "team_paused";   // task 112
      case "accept": case "reject": case "send_back": return item.action === "decide_report" && task > 0 && fields.task === task;
      default: return false;
    }
  }).map((item) => item.id);
}

/** Task 108/109: a drive low on space; its row opens Settings > Build folders. */
export const opensDisk = (item: Pick<TeamWaitingItem, "action">) => item.action === "open_disk";

/** Task 112: a paused team's item resumes the team from Waiting for you. */
export const resumesTeam = (item: Pick<TeamWaitingItem, "action">) => item.action === "resume_team";

/** The item's kind in words. */
export function waitingKindLabel(item: Pick<TeamWaitingItem, "kind" | "action">): string {
  if (item.action === "decide_proposal") return tr("Proposal");
  switch (item.kind) {
    case "phase_draft": return tr("Phase draft");
    case "plan_draft": return tr("Plan");
    case "report_escalated": return tr("Report");
    case "permission_escalated": return tr("Permission");
    case "owner_word_asked": return tr("Your word");
    case "task_waits_owner": return tr("Held task");
    case "team_finished": return tr("All decided");
    case "tool_missing": return tr("Tool");
    case "test_full_run": return tr("Full test run");
    case "test_quarantine": return tr("Flaky tests");
    case "phase_acceptance": return tr("Phase acceptance");
    case "lead_stalled": return tr("Lead stalled");
    case "deferred": return tr("Deferred");
    case "disk_low": return tr("Disk space");
    case "team_paused": return tr("Team paused");
    // Task 186: an urgent team message the working lead has not received.
    case "lead_message_undelivered": return tr("Urgent message");
    // Task 245 (hgs task 162): the lead's plan changes inside the approved goal, each revertible.
    case "lead_changes": return tr("Lead's changes");
    // Task 304 (hgs #286): a task wait the lead left unresolved, escalated once.
    case "task_wait_escalated": return tr("Lead has not acted");
    default: return item.kind;
  }
}

/** Task 304 (hgs #286): one kind of the lead's changes, its count and its newest few. */
export interface DigestGroup { kind: string; label: string; count: number; newest: string[] }

const GROUP_LABELS: Record<string, string> = { added: trNoop("added"), moved: trNoop("moved"), cancelled: trNoop("cancelled"), "re-issued": trNoop("re-issued"), changed: trNoop("changed") };

/** The lead-changes digest's groups (`groups`): "2 added", "1 moved"... with their newest summaries. */
export function digestGroups(item: Pick<TeamWaitingItem, "kind" | "detail">): DigestGroup[] {
  if (item.kind !== "lead_changes") return [];
  const groups = (item.detail as { groups?: unknown } | null | undefined)?.groups;
  if (!Array.isArray(groups)) return [];
  return groups.flatMap((group: { kind?: unknown; count?: unknown; newest?: unknown }) => {
    const kind = typeof group?.kind === "string" ? group.kind : "";
    const count = typeof group?.count === "number" ? group.count : 0;
    if (!kind || count <= 0) return [];
    const newest = Array.isArray(group.newest) ? group.newest.flatMap((change: { summary?: unknown }) => (typeof change?.summary === "string" ? [change.summary] : [])) : [];
    return [{ kind, label: tr("%1 %2", count, tr(GROUP_LABELS[kind] ?? GROUP_LABELS.changed)), count, newest }];
  });
}

/**
 * Task 304 (hgs #286): the item the strip's header names. The lead-changes
 * digest has its own row, so the header names the newest other item; with
 * only digests it names none.
 */
export function bannerHeadItem(items: readonly TeamWaitingItem[]): TeamWaitingItem | undefined {
  return items.find((item) => item.kind !== "lead_changes");
}

/** Task 304 (hgs #246): what a dismissed row still blocks (`blocks`), when it blocks work. */
export function stillBlocks(item: Pick<TeamWaitingItem, "detail">): string {
  const detail = item.detail as { blocking?: unknown; blocks?: unknown } | null | undefined;
  return detail?.blocking === true && typeof detail.blocks === "string" ? detail.blocks : "";
}

/** The item's icon (ui/lib/icons names). */
export function waitingKindIcon(kind: string): string {
  switch (kind) {
    case "phase_draft": return "pencil";
    case "plan_draft": return "eye";
    case "report_escalated": return "message-square";
    case "permission_escalated": return "shield-check";
    case "owner_word_asked": return "key-round";
    case "task_waits_owner": return "clock";
    case "team_finished": case "phase_acceptance": return "check";
    case "tool_missing": return "terminal";
    case "test_full_run": return "refresh-cw";
    case "lead_stalled": return "triangle-alert";
    case "deferred": return "clock";
    case "disk_low": return "hard-drive";
    case "team_paused": return "pause";
    case "lead_message_undelivered": return "message-square";
    case "lead_changes": return "pencil";
    case "task_wait_escalated": return "clock";
    default: return "triangle-alert";
  }
}

/** Opens a card to answer in place. */
export const opensCard = (action: string) =>
  ["decide_proposal", "confirm_addition", "answer_permission", "decide_tool", "accept_phase", "answer_deferred",
    // Task 116: every kind that waits for the owner is answered in its row, the
    // same controls as its home view, not only in the Team tab.
    "decide_report", "release_task", "retry_task", "finish"].includes(action);
/** Approves with the team view's question. */
export const approves = (action: string) => action === "approve_phase" || action === "approve_plan";

/** The banner's action word for an item. */
export function waitingActionLabel(action: string): string {
  if (approves(action)) return tr("Approve…");
  if (action === "decide_report") return tr("Accept…");
  if (opensCard(action)) return tr("Answer");
  return tr("Open team");
}

/** The fields an answer names, from the item as hgs lists it. */
export function waitingFields(item: TeamWaitingItem) {
  const detail = obj(item.detail);
  return {
    phase: num(detail.phase), task: num(detail.task), member: str(detail.member), memberId: str(detail.member_id),
    proposal: str(item.proposal) || str(detail.proposal), offer: str(detail.offer), tool: str(detail.tool), why: str(detail.why),
    saves: str(detail.saves), command: str(detail.command), state: str(detail.state), output: str(detail.output),
    summary: detail.summary && typeof detail.summary === "object" ? obj(detail.summary) : undefined,
  };
}

/** A phase acceptance's summary: "3 accepted, 1 rejected, 0 cancelled; tests pass; 42 min". */
export function acceptanceSummary(summary: Record<string, unknown> | undefined): string {
  if (!summary) return "";
  const parts = [[tr("%1 accepted", num(summary.accepted)), tr("%1 rejected", num(summary.rejected)), tr("%1 cancelled", num(summary.cancelled))].join(", ")];
  const tests = str(summary.tests).trim();
  if (tests) parts.push(tests);
  const minutes = Math.round(num(summary.minutes));
  if (minutes > 0) parts.push(tr("%1 min", minutes));
  return parts.join("; ");
}

/** The id of an addition from its item ("…/owner_word_asked/addition-<id>"). */
export function additionId(item: TeamWaitingItem): string {
  const at = item.id.indexOf("/owner_word_asked/addition-");
  return at < 0 ? "" : item.id.slice(at + "/owner_word_asked/addition-".length);
}

/** The member an item names: by id, else by role. */
export function memberOf(team: TeamView, item: TeamWaitingItem): TeamMemberView | undefined {
  const fields = waitingFields(item);
  const id = fields.memberId || fields.member;
  return team.members.find((member) => member.id === id) ?? team.members.find((member) => member.role === fields.member);
}

/** A member's question that waits for an answer, if it does. */
export function openQuestion(member: TeamMemberView | undefined): TeamMemberQuestion | undefined {
  const question = member?.permission;
  if (!question || typeof question !== "object") return undefined;
  if (num(question.answered_at) > 0 || (typeof question.answer === "string" && question.answer)) return undefined;
  const permission = !!(question.tool || question.command || question.reason);
  return permission || (question.questions?.length ?? 0) > 0 ? question : undefined;
}

export const questionHash = (question: TeamMemberQuestion) => question.question_hash || question.expected_question_hash || "";

/** The card's own ids for Allow and Deny; undefined when hgs sent none. */
export function nativeAnswer(question: TeamMemberQuestion): { question_id: string; allow: string; deny: string } | undefined {
  const answer = obj(question.answer);
  const native = { question_id: str(answer.question_id), allow: str(answer.allow), deny: str(answer.deny) };
  return native.question_id && native.allow && native.deny ? native : undefined;
}

/** The phase approval's question, as the team view asks it (Qt Teams::phaseApproval, plain case). */
export function phaseApproval(team: TeamView, phase: number): TeamConfirmation {
  const count = team.tasks.filter((task) => task.phase === phase).length;
  return { name: "teamApprovePhaseConfirm", title: tr("Approve phase?"), text: tr("Hand phase %1's %2 tasks to the members?", phase, count), action: tr("Approve phase") };
}

/**
 * The plan's Approve on a draft phase (the Team tab): the same call Waiting
 * for you makes, so the core's confirmation is asked in the app's own dialog
 * and only a yes sends it again, confirmed.
 */
export function approvePhaseCall(
  port: Pick<DecisionPort, "send" | "confirm">,
  where: { computer: ComputerId; team: string; teamName: string },
  phase: number,
): Promise<TeamOutcome | "cancelled"> {
  return act(port, {
    computer: where.computer,
    team: where.team,
    team_name: where.teamName,
    action: { kind: "approve", phase },
  });
}

/** Why an item no longer waits, read from the team as it is now; "" while it still does. */
export function staleReason(item: TeamWaitingItem, team: TeamView, shown?: { question?: string; hash?: string; titles?: string[] }): string {
  const fields = waitingFields(item);
  switch (item.action) {
    case "decide_proposal":
      return team.proposals.some((proposal) => proposal.id === fields.proposal) ? "" : tr("This proposal was decided or withdrawn meanwhile; nothing was sent.");
    case "confirm_addition": {
      const addition = (team.additions ?? []).find((found) => found.id === additionId(item));
      const titles = (addition?.tasks ?? []).map((task) => task.title ?? "");
      return addition && (!shown?.titles || shown.titles.join("\n") === titles.join("\n")) ? ""
        : tr("This addition was confirmed, replaced or expired meanwhile; nothing was sent.");
    }
    case "answer_permission": {
      const question = openQuestion(memberOf(team, item));
      return question && (!shown?.question || (question.question_id === shown.question && questionHash(question) === shown.hash)) ? ""
        : tr("The member's question was answered or changed meanwhile; nothing was sent.");
    }
    case "approve_phase": {
      const phase = team.phases.find((found) => found.n === fields.phase);
      return team.status !== "planning" && phase && (phase.state === "draft" || phase.status === "draft") ? ""
        : tr("Phase %1 no longer waits for your approval; nothing was sent.", fields.phase);
    }
    case "accept_phase": {
      const phase = team.phases.find((found) => found.n === fields.phase);
      return phase && (phase.state === "awaiting_acceptance" || phase.status === "awaiting_acceptance") ? ""
        : tr("Phase %1 no longer waits for your acceptance; nothing was sent.", fields.phase);
    }
    case "approve_plan":
      return team.status === "planning" ? "" : tr("The plan no longer waits for your approval; nothing was sent.");
    // Task 116: the report, the held task and the team's end are answered in
    // their row too, and say when they were decided meanwhile.
    case "decide_report": {
      const task = team.tasks.find((found) => found.id === fields.task);
      return task && task.status === "submitted" ? "" : tr("That report was decided meanwhile; nothing was sent.");
    }
    case "release_task": {
      const task = team.tasks.find((found) => found.id === fields.task);
      return task && task.held ? "" : tr("That task is no longer held; nothing was sent.");
    }
    case "retry_task": {
      const task = team.tasks.find((found) => found.id === fields.task);
      return task && task.waits ? "" : tr("That task no longer waits; nothing was sent.");
    }
    default: return "";
  }
}

/** One question of a member's session card, with the owner's choice. */
export interface QuestionChoice { chosen: Set<string>; other: boolean; text: string }

/** The session card's answer: each question's chosen option ids and, with Other, its text; undefined until each is answered. */
export function questionAnswers(question: TeamMemberQuestion, choices: Map<string, QuestionChoice>): unknown[] | undefined {
  const questions = question.questions ?? [];
  if (!questions.length) return undefined;
  const result: unknown[] = [];
  for (const asked of questions) {
    const choice = choices.get(asked.id) ?? { chosen: new Set<string>(), other: false, text: "" };
    const selected = (asked.options ?? []).map((option) => option.id).filter((id) => choice.chosen.has(id));
    const text = choice.other ? choice.text.trim() : "";
    if ((selected.length === 0 && !text) || (choice.other && !text)) return undefined;
    result.push({ question_id: asked.id, selected_option_ids: selected, text });
  }
  return result;
}

/**
 * One item answered in place (Qt TeamWaitingAnswer): it reads its team first,
 * says when the item no longer waits, and sends the item's own operation once.
 * `done` is set once hgs took the answer: the row leaves at once.
 */
export class WaitingAnswer {
  team: TeamView | undefined;
  notice = "";
  reading = false;
  sending = false;
  done = false;
  private uncertain?: string;
  private shown: { question?: string; hash?: string; titles?: string[] } = {};
  readonly proposals?: ProposalCards;

  constructor(readonly item: TeamWaitingItem, private port: DecisionPort, private computer: ComputerId,
    private changed: () => void = () => {}, private answered: (item: TeamWaitingItem) => void = () => {}) {
    if (item.action === "decide_proposal") {
      this.proposals = new ProposalCards(port, { computer, teamName: () => item.team_name },
        async () => (await this.read()) ?? undefined, () => this.changed(), waitingFields(item).proposal);
    }
  }

  get busy() { return this.reading || this.sending; }

  /** Reads the team; null when it could not be read (the notice says so). */
  async read(): Promise<TeamView | null> {
    this.reading = true;
    this.changed();
    try {
      this.team = await this.port.read(this.computer, this.item.team);
      this.proposals?.setTeam(this.team);
      return this.team;
    } catch (reason) {
      this.notice = tr("The team was not read, so nothing was sent: %1", hgsWords(String(reason instanceof Error ? reason.message : reason)));
      return null;
    } finally {
      this.reading = false;
      this.changed();
    }
  }

  /** Opens the card: the team as it is now, and what the owner sees of it. */
  async open() {
    if (this.item.action === "decide_tool" || this.item.action === "answer_deferred" || !opensCard(this.item.action)) return;
    const team = await this.read();
    if (!team) return;
    const stale = staleReason(this.item, team);
    if (stale) { this.notice = stale; this.changed(); return; }
    const question = openQuestion(memberOf(team, this.item));
    if (question) this.shown = { ...this.shown, question: question.question_id, hash: questionHash(question) };
    const addition = (team.additions ?? []).find((found) => found.id === additionId(this.item));
    if (addition) this.shown = { ...this.shown, titles: (addition.tasks ?? []).map((task) => task.title ?? "") };
    this.changed();
  }

  /** What the open card shows: the addition, the member's question. */
  addition() { return (this.team?.additions ?? []).find((found) => found.id === additionId(this.item)); }
  question() { return this.team ? openQuestion(memberOf(this.team, this.item)) : undefined; }
  member() { return this.team ? memberOf(this.team, this.item) : undefined; }

  private finish() {
    this.done = true;
    this.changed();
    this.answered(this.item);
  }

  /** Reads again, checks the item still waits, then sends. */
  private async send(action: TeamAction, team = true): Promise<TeamOutcome | "cancelled" | "stale" | undefined> {
    if (this.busy || this.done) return undefined;
    this.notice = "";
    if (team) {
      const now = await this.read();
      if (!now) return undefined;
      const stale = staleReason(this.item, now, this.shown);
      if (stale) { this.notice = stale; this.changed(); return "stale"; }
    }
    this.sending = true;
    this.changed();
    const outcome = await act(this.port, { computer: this.computer, team: this.item.team, team_name: this.item.team_name, action }, this.uncertain);
    this.sending = false;
    if (outcome === "cancelled") { this.changed(); return outcome; }
    if (outcome.outcome === "done") {
      // Task #253: a retry on the task a member is working on keeps attempt K
      // and says so; the row stays, since nothing was revoked.
      const continued = action.kind === "retry" ? retryContinues(outcome.receipt) : null;
      if (continued !== null) { this.notice = retryContinueText(continued); this.changed(); return outcome; }
      this.uncertain = undefined; this.finish(); return outcome;
    }
    if (outcome.outcome === "uncertain") { this.uncertain = outcome.request_id; this.notice = tr("It may have been sent: %1", hgsWords(outcome.detail)); }
    else { this.uncertain = undefined; this.notice = hgsWords("detail" in outcome ? outcome.detail : ""); }
    this.changed();
    return outcome;
  }

  /** Approve…: the plan or the phase, with the team view's question (the core's confirmation). */
  approve() {
    const fields = waitingFields(this.item);
    return this.send(this.item.action === "approve_phase" ? { kind: "approve", phase: fields.phase } : { kind: "approve" });
  }

  /**
   * Task 198 (W3): why Allow and Deny cannot answer, or "" (Qt
   * TeamWaitingAnswer C4): without the question's hash, or without the card's
   * answer ids, nothing could be sent, so the buttons say so instead.
   */
  permissionBlocked(): string {
    const question = this.question();
    if (!question || !this.member()) return tr("The question is no longer shown; answer it in the session.");
    if (!question.question_id || !questionHash(question)) return tr("This hgs sends no question hash in status; answer it in the session.");
    if (!nativeAnswer(question)) return tr("This hgs sends no answer ids for the question; answer it in the session.");
    return "";
  }

  /** A permission request: Allow or Deny with the card's own ids and the question's hash. */
  answerPermission(allow: boolean, reason = "") {
    const question = this.question();
    const member = this.member();
    const native = question && nativeAnswer(question);
    // Never silent: a click that cannot answer says why on the card.
    const blocked = this.permissionBlocked();
    if (blocked || !question || !member || !native) { this.notice = blocked; this.changed(); return Promise.resolve(undefined); }
    const text = allow ? "" : reason.trim();
    return this.send({ kind: "answer", member: member.id, question_id: question.question_id ?? "", expected_question_hash: questionHash(question),
      answers: [{ question_id: native.question_id, selected_option_ids: [allow ? native.allow : native.deny], text }] });
  }

  /** A member's (or the lead's) own question: the session card's answers. */
  answerQuestion(choices: Map<string, QuestionChoice>) {
    const question = this.question();
    const member = this.member();
    const answers = question && questionAnswers(question, choices);
    if (!question || !member || !answers) return Promise.resolve(undefined);
    return this.send({ kind: "answer", member: member.id, question_id: question.question_id ?? "", expected_question_hash: questionHash(question), answers });
  }

  /** An addition: Add with the typed word, or Cancel, which declines it. */
  answerAddition(add: boolean, typed = "") {
    const phase = waitingFields(this.item).phase || this.addition()?.phase || 0;
    const confirm = additionId(this.item);
    if (add && typed.trim() !== "add") return Promise.resolve(undefined);
    return this.send(add ? { kind: "add_to_phase", phase, confirm, confirmation: typed.trim() } : { kind: "add_to_phase", phase, confirm, decline: true });
  }

  /** A phase that waits for the owner's acceptance: Accept phase, or Send back with a comment. */
  acceptPhase() { return this.send({ kind: "accept_phase", phase: waitingFields(this.item).phase }); }
  sendBack(comment: string) {
    if (!comment.trim()) return Promise.resolve(undefined);
    return this.send({ kind: "send_back_phase", phase: waitingFields(this.item).phase, comment: comment.trim() });
  }

  /** A missing tool: Allow installs it with the command shown; Deny is remembered with its reason. No team. */
  decideTool(allow: boolean, reason = "") {
    const fields = waitingFields(this.item);
    if (allow && !fields.command) return Promise.resolve(undefined);
    return this.send({ kind: "decide_tool", offer: fields.offer, allow, ...(!allow && reason.trim() ? { reason: reason.trim() } : {}) }, false);
  }

  /**
   * Task 97: the owner's answer to a deferred request: Allow or Deny a
   * permission, or the owner's words to a question. hgs sends the session one
   * message; the tool is never replayed.
   */
  answerDeferred(decision: "allow" | "deny" | "answer", text = "") {
    const id = deferredRequest(this.item).id;
    if (!id || (decision === "answer" && !text.trim())) return Promise.resolve(undefined);
    return this.send({ kind: "answer_deferred", id, decision, ...(text.trim() ? { text: text.trim() } : {}) }, false);
  }

  /** A held task, a step Zerus could not take, a finished team. */
  release() { return this.send({ kind: "release", task: waitingFields(this.item).task }, false); }
  retry() { return this.send({ kind: "retry", task: waitingFields(this.item).task }, false); }
  /** Finish needs the owner's summary for the lead (hgs requires one). */
  finishTeam(summary: string) { return summary.trim() ? this.send({ kind: "finish", summary: summary.trim() }, false) : Promise.resolve(undefined); }

  /**
   * Task 116: a report the lead escalated, answered where it waits: Accept, or
   * Reject with the reason the member receives. The team is read again first,
   * so a report decided meanwhile sends nothing.
   */
  acceptReport(reason = "") {
    const fields = waitingFields(this.item);
    return this.send({ kind: "accept", task: fields.task, ...(reason.trim() ? { reason: reason.trim() } : {}) });
  }
  rejectReport(reason: string) {
    const fields = waitingFields(this.item);
    return reason.trim() ? this.send({ kind: "reject", task: fields.task, reason: reason.trim() }) : Promise.resolve(undefined);
  }

  /** A proposal answered in its row: once hgs took it, the row leaves. */
  async decide(allow: boolean) {
    const outcome = await this.proposals?.answer(waitingFields(this.item).proposal, allow);
    if (outcome && outcome !== "cancelled" && outcome !== "stale" && outcome.outcome === "done") this.finish();
    else if (outcome === "stale") { this.notice = tr("This proposal was decided or withdrawn meanwhile; nothing was sent."); this.changed(); }
    return outcome;
  }

  async decideQuestion() {
    const outcome = await this.proposals?.answerQuestion(waitingFields(this.item).proposal);
    if (outcome && outcome !== "cancelled" && outcome !== "stale" && outcome.outcome === "done") this.finish();
    return outcome;
  }
}

/** The tool offer's card texts. */
/** Task 97: a deferred request as its card shows it. */
export interface DeferredRequest {
  id: string; permission: boolean; text: string; session: string; outcome: string;
  questions: { id: string; question: string; options: { id: string; label: string }[] }[];
}

export function deferredRequest(item: TeamWaitingItem): DeferredRequest {
  const detail = obj(item.detail);
  const questions = Array.isArray(detail.questions) ? detail.questions : [];
  return {
    id: str(detail.deferred), permission: str(detail.request) === "permission", text: str(detail.text), session: str(detail.session),
    outcome: str(detail.outcome),
    questions: questions.map((raw) => {
      const asked = obj(raw);
      const options = Array.isArray(asked.options) ? asked.options : [];
      return { id: str(asked.id), question: str(asked.question),
        options: options.map((option) => ({ id: str(obj(option).id), label: str(obj(option).label) })) };
    }),
  };
}

/** Task 97: Waiting for you > Deferred: the requests Zerus answered safely for the absent owner. */
export function splitDeferred(items: readonly TeamWaitingItem[]) {
  return { waiting: items.filter((item) => item.kind !== "deferred"), deferred: items.filter((item) => item.kind === "deferred") };
}

export function toolOffer(item: TeamWaitingItem) {
  const fields = waitingFields(item);
  const failed = fields.state === "failed";
  return {
    head: failed ? tr("Installing %1 failed", fields.tool) : tr("Install %1 on this computer?", fields.tool),
    why: fields.why, saves: fields.saves, output: failed ? fields.output : "",
    command: fields.command || tr("No package manager on this computer installs it; install it yourself."),
    allow: failed ? tr("Retry install") : tr("Allow"), canAllow: !!fields.command,
  };
}

// ---------------------------------------------------------------------------
// The list and its notices

/** "7 items in 6 teams"; a tool offer belongs to no team. */
export function waitingCount(items: readonly TeamWaitingItem[], total: number): string {
  if (total === 0) return tr("Nothing waits for you");
  const teams = new Set(items.filter((item) => item.team).map((item) => item.team)).size;
  const count = total === 1 ? tr("1 item") : tr("%1 items", total);
  if (teams === 0) return count;
  return teams === 1 ? tr("%1 in 1 team", count) : tr("%1 in %2 teams", count, teams);
}

/** "5 min ago": the item's age, without dots. */
export function waitingAge(since: number, now = Date.now() / 1000): string {
  const minutes = Math.max(0, Math.floor((now - since) / 60));
  if (minutes < 1) return tr("just now");
  if (minutes < 60) return tr("%1 min ago", minutes);
  const hours = Math.floor(minutes / 60);
  if (hours < 24) return tr("%1 h ago", hours);
  return tr("%1 d ago", Math.floor(hours / 24));
}

/** The banner shows this many rows when opened. */
export const BANNER_ROWS = 5;

/** A notification of one waiting item (or of several, folded). */
export interface WaitingNotice { token: string; title: string; body: string; team: string }

export const noticeToken = (item: Pick<TeamWaitingItem, "team" | "id">) => `zerus-team-attention:${item.team}:waiting:${item.id}`;

/**
 * Notifies each item once (Qt TeamWaitingNotices): the first two new ones on
 * their own, the rest folded into one line; an answered item's notice is
 * withdrawn. `known` survives a new window.
 */
export class WaitingNotices {
  private known: Set<string>;
  constructor(known: Iterable<string> = []) { this.known = new Set(known); }

  observe(items: readonly TeamWaitingItem[]): WaitingNotice[] {
    const fresh = items.filter((item) => !this.known.has(item.id));
    for (const item of fresh) this.known.add(item.id);
    const raised: WaitingNotice[] = fresh.slice(0, 2).map((item) => ({ token: noticeToken(item), title: tr("%1 waits for you", item.team_name || tr("Zerus")), body: item.title, team: item.team }));
    if (fresh.length > 2) {
      const rest = fresh.length - 2;
      raised.push({ token: `zerus-team-attention:waiting:more:${fresh[2].id}`, title: tr("Zerus"), body: rest === 1 ? tr("1 more item waits for you") : tr("%1 more items wait for you", rest), team: "" });
    }
    return raised;
  }

  /** An answered item: the notices to withdraw. */
  answered(item: TeamWaitingItem): string[] { return [noticeToken(item)]; }

  /** What was announced (for a new window). */
  ids(): string[] { return [...this.known]; }
}

// ---------------------------------------------------------------------------
// Reports in the Team tab (Qt TeamPanel's attention and problem actions)

/** A report that waits for the owner, or an attempt whose member got no report in. */
export interface ReportRow { task: number; title: string; member: string; kind: "report" | "no_report"; attempt: string }

/** The reports the owner decides: the escalated ones (Accept, Reject with a comment) and the missing ones (Use last reply). */
export function reportRows(team: TeamView): ReportRow[] {
  const rows: ReportRow[] = [];
  const seen = new Set<number>();
  for (const value of team.attention) {
    const attention = obj(value);
    if (str(attention.kind) !== "report") continue;
    const id = num(attention.task);
    const found = team.tasks.find((task) => task.id === id);
    if (!found || seen.has(id)) continue;
    seen.add(id);
    rows.push({ task: id, title: found.title, member: str(attention.member) || roleOf(team, found.assignee), kind: "report", attempt: found.attempt_id ?? "" });
  }
  for (const found of team.tasks) {
    if (seen.has(found.id) || !found.no_report || !found.attempt_id || found.status !== "in_progress") continue;
    rows.push({ task: found.id, title: found.title, member: roleOf(team, found.assignee), kind: "no_report", attempt: found.attempt_id });
  }
  return rows;
}

/** The report row's line. */
export const reportLine = (row: ReportRow) => (row.kind === "report"
  ? tr("Report on #%1 %2 from %3 waits for your decision", row.task, row.title, row.member)
  : tr("#%1 %2: %3 got no report in", row.task, row.title, row.member));

/** Accept, Reject with the member's comment, or Use last reply as report: each once. */
export class ReportDecision {
  notice = "";
  sending = false;
  private uncertain?: string;
  constructor(private port: DecisionPort, private where: { computer: ComputerId; team: string; teamName: string }, private changed: () => void = () => {}) {}

  private async send(action: TeamAction) {
    if (this.sending) return undefined;
    this.sending = true;
    this.notice = "";
    this.changed();
    const outcome = await act(this.port, { computer: this.where.computer, team: this.where.team, team_name: this.where.teamName, action }, this.uncertain);
    this.sending = false;
    if (outcome !== "cancelled") {
      this.uncertain = outcome.outcome === "uncertain" ? outcome.request_id : undefined;
      if (outcome.outcome === "failed") this.notice = hgsWords(outcome.detail);
      if (outcome.outcome === "uncertain") this.notice = tr("It may have been sent: %1", hgsWords(outcome.detail));
    }
    this.changed();
    return outcome;
  }

  accept(row: ReportRow) { return this.send({ kind: "accept", task: row.task }); }
  reject(row: ReportRow, comment: string) { return comment.trim() ? this.send({ kind: "reject", task: row.task, reason: comment.trim() }) : Promise.resolve(undefined); }
  useLastReply(row: ReportRow) { return this.send({ kind: "use_last_reply", task: row.task, attempt: row.attempt }); }
}

// ---------------------------------------------------------------------------
// Task 251: the lead's tool_missing inbox item

/** The fields hgs puts in a lead's tool_missing item, parsed from its strings. */
export interface LeadToolMissing {
  who: string;
  tool: string;
  project: string;
  why: string;
  folder: string;
  install: string;
  offer: string;
  /** The deny command hgs lists for the item (its offer is read from it). */
  denyCommand: string;
  /** The propose command hgs lists in the detail (the lead's Ask the owner). */
  proposeCommand: string;
}

/**
 * `tool_missing` as hgs words it: the summary is "<who>'s run needs <tool> in
 * <project>: <why>"; the detail names the project and folder, the exact install
 * command and the propose command, whose JSON carries the offer id (as does
 * the deny command).
 */
export function leadToolMissing(item: Pick<TeamInboxItem, "kind" | "member" | "summary" | "command" | "detail">): LeadToolMissing | undefined {
  if (item.kind !== "tool_missing") return undefined;
  const summary = str(item.summary);
  const detail = typeof item.detail === "string" ? item.detail : "";
  const hit = /^(.*?)'s run needs (.+?) in (.+?): (.*)$/.exec(summary);
  const offer = /"offer"\s*:\s*"([^"]+)"/.exec(item.command) ?? /"offer"\s*:\s*"([^"]+)"/.exec(detail);
  const project = /Project: (.+?) \(/.exec(detail)?.[1] ?? hit?.[3] ?? "";
  return {
    who: hit?.[1] ?? item.member,
    tool: hit?.[2] ?? "",
    project,
    why: hit?.[4] ?? "",
    folder: /Project: .*? \(([^)]*)\)/.exec(detail)?.[1] ?? "",
    install: /The install command: (.+)/.exec(detail)?.[1]?.trim() ?? "",
    offer: offer?.[1] ?? "",
    denyCommand: item.command,
    proposeCommand: /Ask the owner to install it: (.*?)(?: \| hgs team propose|$)/s.exec(detail)?.[1]?.trim() ?? "",
  };
}

/** Task 251: the lead's answer to a tool_missing offer: Deny with its reason, or ask the owner. */
export function toolMissingAction(tool: LeadToolMissing, answer: "deny" | "ask", reason: string, summary: string): TeamAction | undefined {
  if (!tool.offer) return undefined;
  if (answer === "deny") {
    const why = reason.trim();
    return why ? { kind: "decide_tool", offer: tool.offer, allow: false, reason: why } : undefined;
  }
  return { kind: "propose", proposal_kind: "install-tool", reason: summary, offer: tool.offer };
}
