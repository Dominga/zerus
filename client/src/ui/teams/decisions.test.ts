import { withdrawnRefusal } from "../../teamRefusals";
import { describe, expect, it } from "vitest";
import { decidedWaiting, leadToolMissing, toolMissingAction, TaskReportRead,
  ProposalCards, WaitingAnswer, WaitingNotices, acceptanceSummary, act, approvePhaseCall, noticeToken, proposalCards, proposalKindLabel, toolOffer,
  waitingActionLabel, waitingAge, waitingCount, waitingKindIcon, waitingKindLabel, deferredRequest, splitDeferred, approves, opensCard, type DecisionPort, type QuestionChoice,
  bannerHeadItem, digestGroups, stillBlocks,
} from "./decisions";
import { choiceButtons, confirmButtons, createConfirmer } from "../../store/teamConfirm";
import type { TeamAction, TeamCall, TeamConfirmation, TeamMemberView, TeamOutcome, TeamProposalView, TeamTaskView, TeamView, TeamWaitingItem } from "../../types";

it("an archived report loads on demand, exposes errors and drops late responses", async () => {
  const pending: { resolve: (value: import("../../types").TeamTaskDetailView) => void; reject: (error: Error) => void }[] = [];
  const ids: number[] = [];
  const reader = new TaskReportRead((_computer, _team, task) => {
    ids.push(task);
    return new Promise((resolve, reject) => pending.push({ resolve, reject }));
  }, () => {});
  expect(ids).toEqual([]);
  const first = reader.open("local", "team-1", 1);
  expect(reader.loading).toBe(true);
  const second = reader.open("local", "team-1", 2);
  pending[0].resolve({ task: task({ id: 1 }), report: "Stale", report_truncated: false });
  await first;
  expect(reader.detail).toBeNull();
  pending[1].reject(new Error("hgs: Archive unavailable"));
  await second;
  expect(reader.error).toBe("Archive unavailable");
  expect(reader.loading).toBe(false);
  const retry = reader.open("local", "team-1", 2);
  pending[2].resolve({ task: task({ id: 2 }), report: "Final report", report_truncated: false });
  await retry;
  expect(reader.detail?.report).toBe("Final report");
  const late = reader.open("local", "team-1", 3);
  reader.close();
  pending[3].resolve({ task: task({ id: 3 }), report: "After close", report_truncated: false });
  await late;
  expect(reader.detail).toBeNull();
  expect(reader.loading).toBe(false);
});

// Every owner decision path of Qt's tray/tests/test_teamsui_waiting.cpp, on
// the pure model the Team tab and Waiting for you draw (task 11). Each test
// names the Qt test it mirrors. The port is a fake: what was sent, what the
// owner was asked, and the team a read returns.

const session = (name: string) => ({ host: "", name, run_id: "r", conversation_id: "c" });

function member(over: Partial<TeamMemberView> & { id: string }): TeamMemberView {
  return {
    role: "Backend", agent: "claude", model: "", effort: "", account: "", computer: "", folder: "worktree", state: "active",
    activity: "idle", phase: "idle", error: "", lead: false, session: session(`claude/p/${over.id}`), live: true, stuck: false,
    question: false, queue: [], minutes_without_progress: 0, host_shared: false, ...over,
  };
}

function task(over: Partial<TeamTaskView> & { id: number }): TeamTaskView {
  return {
    title: `Task ${over.id}`, status: "pending", assignee: "backend", phase: 1, deps: [], estimate_minutes: 0, attempts: 0,
    attempt_state: "", decided_by: "", pinned: false, output: [], waits: "", ...over,
  };
}

const phase = (n: number, title: string, state: string) => ({ n, title, state, status: state, done: 0, total: 1, barrier: false, auto: false, acceptance: "", paused: false, health: "" });

function proposal(over: Partial<TeamProposalView> & { id: string; kind: string }): TeamProposalView {
  return { by: "Lead", at: 2000, reason: "", hash: `h${over.id.slice(1)}`, summary: over.kind, lines: [], change: null, member: null, ...over };
}

/** The permission question of tests/fixtures/desktop/hgs-team-member-question.json. */
const QUESTION = {
  answer: { allow: "1", deny: "2", question_id: "approval" }, asked_at: 1792990000, command: "cargo test --locked",
  command_source: "tool_input", question_hash: "6f2b0e3a", question_id: "claude-approval:6f2b0e3a", reason: "cargo test --locked", tool: "Bash",
};

/** Qt's answerTeam: proposals p1 and p2, addition add-1 to running phase 1, Backend's permission and phase 2 as a draft. */
function answerTeam(over: Partial<TeamView> = {}): TeamView {
  return {
    id: "team-1", name: "Live", status: "running", project: "", folder: "/work/p", goal: "", plan_mode: "standard", lead: "lead",
    revision: 1, created: 1000, updated: 1000, waiting_for_lead: false, pending_phase: null, settings: {},
    members: [member({ id: "lead", role: "Lead", lead: true }), member({ id: "backend", permission: QUESTION }), member({ id: "tester", role: "Tester" })],
    tasks: [task({ id: 1, title: "Saved cards API" }), task({ id: 2, title: "Release notes", phase: 2 }), task({ id: 4, title: "Unit tests", phase: 2, status: "in_progress" }),
      task({ id: 5, title: "Code review", phase: 3 })],
    phases: [phase(1, "Build", "approved"), phase(2, "Release", "draft"), phase(3, "Polish", "draft")],
    proposals: [
      proposal({ id: "p1", kind: "add-member", reason: "The UI needs a designer", member: { role: "Designer", agent: "claude", model: "sonnet" } }),
      proposal({ id: "p2", kind: "add-member", reason: "Docs need an owner", member: { role: "Writer", agent: "claude", model: "haiku" } }),
    ],
    additions: [{ id: "add-1", phase: 1, by: "Lead", request: "add the card form tests", tasks: [{ id: 3, title: "Card form tests" }] }],
    attention: [], validation: { errors: [], warnings: [] }, health: { critical_path: 0, parallelism: 0, warnings: [] },
    ready: [], cycle: [], draft: null, truncated: false, ...over,
  };
}

const NOW = 1_800_000_000;

function item(key: string, kind: string, action: string, title: string, since: number, detail: Record<string, unknown> = {}, team = "team-1", name = "Live"): TeamWaitingItem {
  const id = `${team}/${kind}/${key}`;
  return { id, kind, team, team_name: name, title, action, since, proposal: detail.proposal ?? null, detail: { id, kind, team, title, action, since, ...detail } };
}

/** Qt's answerItems. */
function answerItems(): Record<string, TeamWaitingItem> {
  return {
    proposal: item("proposal-p1", "owner_word_asked", "decide_proposal", "The lead proposes: add member", NOW - 60, { proposal: "p1" }),
    stale: item("proposal-p2", "owner_word_asked", "decide_proposal", "The lead proposes: add member", NOW - 70, { proposal: "p2" }),
    addition: item("addition-add-1", "owner_word_asked", "confirm_addition", "Adding 1 task to running phase 1 waits for your confirmation", NOW - 120, { phase: 1 }),
    permission: item("backend", "permission_escalated", "answer_permission", "Backend asks to run cargo test --locked", NOW - 180, { member: "backend" }),
    draft: item("2", "phase_draft", "approve_phase", "Phase 2 Release waits for your approval", NOW - 240, { phase: 2 }),
    report: item("1", "report_escalated", "decide_report", "Report on #1 Saved cards API from Backend waits for your decision", NOW - 300, { task: 1, member: "Backend" }),
    held: item("2", "task_waits_owner", "release_task", "#2 Release notes is held by you", NOW - 360, { task: 2 }),
  };
}

interface Fake {
  port: DecisionPort;
  sent: TeamCall[];
  asked: TeamConfirmation[];
  /** The owner's choice in the next confirmations. */
  yes: boolean;
  /** Runs while the owner reads a confirmation (the lead changes something). */
  meanwhile?: () => void;
  /** hgs refuses this operation kind with these words. */
  refuse: Map<string, string>;
  /** The decision a replay returns ("accepted", "rejected"). */
  status: string;
  team: TeamView;
  reads: number;
}

/** The fake core: approvals ask first (action.rs `confirmation`), everything else runs once. */
function fake(team: TeamView): Fake {
  const state: Fake = {
    sent: [], asked: [], yes: true, refuse: new Map(), status: "", team, reads: 0,
    port: {
      async send(call) {
        if (!call.confirmed && (call.action.kind === "approve" || call.action.kind === "finish"))
          return { outcome: "confirm", confirmation: { name: "teamApprovePhaseConfirm", title: "Approve phase?", text: "Hand phase 2's tasks to the members?", action: "Approve phase" } };
        state.sent.push(call);
        const refused = state.refuse.get(call.action.kind);
        if (refused) return { outcome: "failed", detail: refused, request_id: `r${state.sent.length}` };
        const accept = call.action.kind === "decide_proposal" ? call.action.accept : true;
        return { outcome: "done", receipt: { status: state.status || (accept ? "accepted" : "rejected") }, request_id: `r${state.sent.length}` };
      },
      async confirm(confirmation) {
        state.asked.push(confirmation);
        state.meanwhile?.();
        state.meanwhile = undefined;
        return state.yes;
      },
      async read() { state.reads += 1; return state.team; },
    },
  };
  return state;
}

function cardsOf(f: Fake) {
  const cards = new ProposalCards(f.port, { computer: "", teamName: () => f.team.name }, () => f.team);
  cards.setTeam(f.team);
  return cards;
}
const card = (cards: ProposalCards, id: string) => cards.cards().find((state) => state.card.id === id);

describe("proposal cards", () => {
  it("proposalsAreAcceptedOrRejectedOnce: Deny sends its reason with the hash; a swap and a remove ask first", async () => {
    const f = fake(answerTeam({
      proposals: [
        proposal({ id: "p1", kind: "add-member", reason: "The UI needs a designer", member: { role: "Designer", agent: "claude", model: "sonnet" } }),
        proposal({ id: "p2", kind: "swap-member", member: { id: "backend", agent: "codex", model: "gpt-5.5" } }),
        proposal({ id: "p3", kind: "remove-member", member: { id: "tester" } }),
      ],
    }));
    const cards = cardsOf(f);
    expect(cards.cards().map((state) => state.card.what)).toEqual(["Add Designer (claude, sonnet, default account)", "Swap Backend to codex, gpt-5.5", "Remove Tester"]);
    expect(card(cards, "p3")!.card.hash).toBe("h3");
    expect(card(cards, "p1")!.card.by).toBe("Lead");
    // A typed reason survives a new read; Deny sends it with the hash.
    cards.setReason("p1", "Not before the release");
    cards.setTeam(f.team);
    expect(card(cards, "p1")!.reason).toBe("Not before the release");
    await cards.answer("p1", false);
    expect(f.sent.map((call) => call.action)).toEqual([{ kind: "decide_proposal", proposal: "p1", accept: false, expected_hash: "h1", reason: "Not before the release" }]);
    expect(f.sent[0].team).toBe("team-1");
    // A swap asks first: Cancel sends nothing; the confirmation sends once, without a reason.
    f.yes = false;
    expect(await cards.answer("p2", true)).toBe("cancelled");
    expect(f.asked.at(-1)!.title).toBe("Swap member?");
    expect(f.sent).toHaveLength(1);
    expect(card(cards, "p2")!.sending).toBe(false);
    f.yes = true;
    await cards.answer("p2", true);
    expect(f.sent).toHaveLength(2);
    expect(f.sent[1].action).toEqual({ kind: "decide_proposal", proposal: "p2", accept: true, expected_hash: "h2" });
    // Task 107: confirmed by hgs, the card leaves at once; it cannot be sent twice.
    expect(card(cards, "p2")).toBeUndefined();
    expect(await cards.answer("p2", true)).toBeUndefined();
    expect(f.sent).toHaveLength(2);
    // A remove asks too.
    await cards.answer("p3", true);
    expect(f.asked.at(-1)!.title).toBe("Remove member?");
    expect(f.sent.at(-1)!.action).toMatchObject({ proposal: "p3", expected_hash: "h3", accept: true });
    // Once the store has taken them, their cards are gone.
    f.team = { ...f.team, proposals: [] };
    cards.setTeam(f.team);
    expect(cards.cards()).toEqual([]);
  });

  it("proposalCardsNameTheChangeAndAStaleCardSendsNothing: each kind's change, a change meanwhile, a refusal and an earlier decision", async () => {
    const designer = { id: "designer", role: "Designer", agent: "claude", model: "sonnet" };
    const writer = { id: "writer", role: "Writer", agent: "codex" };
    const f = fake(answerTeam({
      phases: [phase(1, "Build", "approved"), phase(2, "QA fixes", "approved"), phase(3, "Polish", "draft")],
      proposals: [
        proposal({ id: "p1", kind: "add-member", change: { members: [designer, writer] } }),
        proposal({ id: "p2", kind: "add-to-phase", change: { phase: 2, tasks: [{ title: "Saved card list", assignee: "backend" }, { title: "Empty state" }] } }),
        proposal({ id: "p3", kind: "cancel-tasks", change: { tasks: [4] } }),
        proposal({ id: "p4", kind: "discard-phase", change: { phase: 3 } }),
      ],
    }));
    const cards = cardsOf(f);
    expect(card(cards, "p1")!.card.what).toBe("Add 2 members");
    expect(card(cards, "p1")!.card.lines).toEqual(["Designer (claude, sonnet, default account)", "Writer (codex, default account)"]);
    expect(card(cards, "p2")!.card.what).toBe("Add 2 tasks to phase 2 (QA fixes)");
    expect(card(cards, "p2")!.card.lines).toEqual(["Saved card list, for Backend", "Empty state"]);
    expect(card(cards, "p3")!.card.what).toBe("Cancel 1 task");
    expect(card(cards, "p3")!.card.lines[0]).toMatch(/^#4 Unit tests \(/);
    expect(card(cards, "p4")!.card.what).toBe("Discard phase 3 (Polish)");
    expect(card(cards, "p4")!.card.lines[0]).toMatch(/^#5 Code review \(/);
    // The lead changes p3 while the owner reads the confirmation: nothing is sent.
    f.meanwhile = () => {
      f.team = { ...f.team, proposals: f.team.proposals.map((p) => (p.id === "p3" ? { ...p, hash: "h3b", change: { tasks: [4, 5] } } : p)) };
    };
    expect(await cards.answer("p3", true)).toBe("stale");
    expect(card(cards, "p3")!.notice).toContain("changed this proposal");
    expect(f.sent).toEqual([]);
    expect(card(cards, "p3")!.card.hash).toBe("h3b");
    expect(card(cards, "p3")!.card.what).toBe("Cancel 2 tasks");
    // A server refusal stays on the card; nothing applied, the card answers again.
    f.refuse.set("decide_proposal", "hgs: The proposal changed; reload the card and answer again with its hash");
    await cards.answer("p2", true);
    expect(f.sent[0].action).toMatchObject({ proposal: "p2", expected_hash: "h2" });
    expect(card(cards, "p2")!.notice).toBe("Allow was refused, nothing was applied: The proposal changed; reload the card and answer again with its hash");
    expect(card(cards, "p2")!.sending).toBe(false);
    f.refuse.clear();
    // Discard asks; an earlier decision (a replay) shows on its card.
    f.status = "rejected";
    await cards.answer("p4", true);
    expect(f.asked.at(-1)!.title).toBe("Discard phase?");
    expect(f.sent.at(-1)!.action).toMatchObject({ proposal: "p4", expected_hash: "h4" });
    expect(card(cards, "p4")!.notice).toContain("already denied");
    // Decided: the card stays to say so, without answers, until dismissed.
    f.team = { ...f.team, proposals: f.team.proposals.filter((p) => p.id !== "p4") };
    cards.setTeam(f.team);
    expect(card(cards, "p4")).toMatchObject({ gone: true });
    cards.dismiss("p4");
    expect(card(cards, "p4")).toBeUndefined();
  });

  it("everyOwnerGateIsACardAnsweredWithAClick: hgs's summary and lines, the kind in words, a drain asks, Deny rejects", async () => {
    const kinds: [string, string, string, string[]][] = [
      ["relaunch", "Relaunch", "Relaunch Backend", []],
      ["scale", "Members", "Add 2 × Tester", ["claude"]],
      ["drain", "Drain", "Drain Tester", ["It finishes its open work, takes nothing new, then its session is paused and it leaves the team."]],
      ["approve", "Approval", "Approve the plan and run it", ["5 tasks, 2 members"]],
      ["approve-phase", "Approval", "Approve phase 2 (QA fixes)", []],
      ["return-to-draft", "Phase", "Return phase 2 (QA fixes) to a draft", []],
      ["edit-draft", "Phase", "Change phase 3 (Polish)", ["1 task changes"]],
      ["move-phase", "Phase", "Move phase 3 (Polish) before 2", []],
      ["unpause-phase", "Phase", "Remove the pause of phase 2 (QA fixes)", []],
      ["phase-acceptance", "Phase", "phase 3 (Polish) is accepted by owner", []],
      ["accept-phase", "Phase", "Accept phase 2 (QA fixes)", []],
      ["extend", "Phase", "Add a phase of 1 tasks", ["Load test"]],
      ["settings", "Setting", "Change the team's settings", ["max_members: 6 → 8"]],
      ["switch-setting", "Setting", "Change the team's settings", ["full_tests: \"ask\" → \"allowed\""]],
      ["full-tests", "Tests", "Allow one full test run for v1.2", []],
      ["iron-rules", "Iron rules", "Set the project's iron rules (proj-1)", ["Never push"]],
    ];
    const proposals = kinds.map(([kind, , summary, lines], n) => proposal({
      id: `p${n + 1}`, kind, summary, lines, change: { op: kind === "switch-setting" ? "settings" : kind, input: {}, summary, lines },
    }));
    proposals.push(proposal({ id: "p99", kind: "settings", summary: "Change the team's settings", lines: ["max_members: 6 → 3"], change: { op: "settings", summary: "Change the team's settings", lines: ["max_members: 6 → 3"] } }));
    const f = fake(answerTeam({ proposals }));
    const cards = cardsOf(f);
    kinds.forEach(([kind, label, summary, lines], n) => {
      const state = card(cards, `p${n + 1}`)!;
      expect(state.card.kind).toBe(kind);
      expect(state.card.what).toBe(summary);
      expect(state.card.lines).toEqual(lines);
      expect(proposalKindLabel(kind)).toBe(label);
      expect(state.card.options).toEqual([]);   // no typed phrase: Allow and Deny are the answer
    });
    for (const [n, [kind]] of kinds.entries()) {
      const id = `p${n + 1}`;
      const before = f.sent.length;
      if (kind === "drain") {
        f.yes = false;
        expect(await cards.answer(id, true)).toBe("cancelled");
        expect(f.sent).toHaveLength(before);
        expect(card(cards, id)!.sending).toBe(false);
        f.yes = true;
      }
      await cards.answer(id, true);
      expect(f.sent).toHaveLength(before + 1);
      expect(f.sent.at(-1)!.action).toEqual({ kind: "decide_proposal", proposal: id, accept: true, expected_hash: `h${n + 1}` });
      expect(f.sent.at(-1)!.team).toBe("team-1");
      // Task 107: confirmed by hgs, the card leaves at once.
      expect(card(cards, id)).toBeUndefined();
    }
    expect(f.asked.map((asked) => asked.title)).toEqual(["Drain member?", "Drain member?"]);
    cards.setReason("p99", "keep six");
    await cards.answer("p99", false);
    expect(f.sent.at(-1)!.action).toEqual({ kind: "decide_proposal", proposal: "p99", accept: false, expected_hash: "h99", reason: "keep six" });
  });

  it("everyProposalKindTheCoreOpensRendersAndDecides: the hgs team propose kinds, the owner operations and a phase card", async () => {
    // src/state/teams/ops.rs:5144-5149 and 5607-5629: every kind a lead may
    // propose, with the change hgs stores; op cards carry their own summary
    // and lines, the other kinds the client words from the change.
    const note = "Each open task goes to the new session with what was done so far; the old session is paused and kept under Saved as <name>-replaced.";
    const broker = "Each member builds its own snapshot of the folder, one build at a time per build folder; an unchanged tree builds once, and a failure in another member's file goes to that member.";
    const rows: { kind: string; what: string; lines: string[]; change?: unknown; member?: unknown; goal?: string; mode?: string;
      question?: boolean; options?: string[]; recommended?: string }[] = [
      { kind: "add-member", what: "Add Designer (claude, sonnet, default account)", lines: [],
        change: { members: [{ id: "designer", role: "Designer", agent: "claude", model: "sonnet" }] } },
      { kind: "remove-member", what: "Remove Tester", lines: ["Its session keeps running; it gets no more tasks."], member: { id: "tester" } },
      { kind: "swap-member", what: "Swap Backend to codex, gpt-5.5", lines: [note],
        change: { members: [{ id: "backend", agent: "codex", model: "gpt-5.5" }] }, member: { id: "backend", agent: "codex", model: "gpt-5.5" } },
      { kind: "next-goal", what: "Next goal: Ship v2", lines: ["Ship v2\nthen rest"], goal: "Ship v2\nthen rest" },
      { kind: "switch-plan-mode", what: "Switch to free planning", lines: [], mode: "free" },
      { kind: "switch-plan-mode", what: "Switch phase 2 to standard planning", lines: [], mode: "standard", change: { phase: 2 } },
      { kind: "add-to-phase", what: "Add 1 task to phase 2 (Release)", lines: ["Late fix, for Backend, ~30 min"],
        change: { phase: 2, tasks: [{ title: "Late fix", assignee: "backend", estimate_minutes: 30 }] } },
      { kind: "cancel-tasks", what: "Cancel 1 task", lines: ["#4 Unit tests (In progress)"], change: { tasks: [4] } },
      { kind: "discard-phase", what: "Discard phase 3 (Polish)", lines: ["#5 Code review (Pending)"], change: { phase: 3 } },
      { kind: "question", what: "Which cache size?", lines: [], options: ["10 GB", "20 GB"], recommended: "20 GB", question: true,
        change: { question: "Which cache size?", options: ["10 GB", "20 GB"], recommended: "20 GB" } },
      { kind: "switch-setting", what: "Turn on the build broker", lines: ["Backend share /work/p", broker],
        change: { setting: "build_broker", value: true, shared: [{ folder: "/work/p", members: ["backend"] }] } },
      { kind: "switch-setting", what: "Team limits", lines: ["max_members: 6 → 8"],
        change: { op: "settings", setting: "max_members", value: 8, summary: "Team limits", lines: ["max_members: 6 → 8"] } },
      { kind: "iron-rules", what: "Set the project's iron rules (p)", lines: ["Never push"],
        change: { op: "iron-rules", input: { scope: "project", project: "p" }, summary: "Set the project's iron rules (p)", lines: ["Never push"] } },
      { kind: "relaunch", what: "Relaunch Backend", lines: [], change: { op: "relaunch", input: { member: "backend" }, summary: "Relaunch Backend", lines: [] } },
      { kind: "scale", what: "Add 2 × Tester", lines: ["claude"], change: { op: "scale", summary: "Add 2 × Tester", lines: ["claude"] } },
      { kind: "drain", what: "Drain Tester", lines: ["It finishes its open work, takes nothing new, then its session is paused and it leaves the team."],
        change: { op: "drain", summary: "Drain Tester", lines: ["It finishes its open work, takes nothing new, then its session is paused and it leaves the team."] } },
      { kind: "approve", what: "Approve the plan and run it", lines: ["5 tasks, 2 members"], change: { op: "approve", summary: "Approve the plan and run it", lines: ["5 tasks, 2 members"] } },
      { kind: "approve-phase", what: "Approve phase 2 (Release)", lines: [], change: { op: "approve-phase", summary: "Approve phase 2 (Release)", lines: [] } },
      { kind: "return-to-draft", what: "Return phase 2 (Release) to a draft", lines: [], change: { op: "return-to-draft", summary: "Return phase 2 (Release) to a draft", lines: [] } },
      { kind: "edit-draft", what: "Change phase 3 (Polish)", lines: ["1 task changes"], change: { op: "edit-draft", summary: "Change phase 3 (Polish)", lines: ["1 task changes"] } },
      { kind: "move-phase", what: "Move phase 3 (Polish) before 2", lines: [], change: { op: "move-phase", summary: "Move phase 3 (Polish) before 2", lines: [] } },
      { kind: "unpause-phase", what: "Remove the pause of phase 2 (Release)", lines: [], change: { op: "unpause-phase", summary: "Remove the pause of phase 2 (Release)", lines: [] } },
      { kind: "phase-acceptance", what: "phase 3 (Polish) is accepted by owner", lines: [], change: { op: "phase-acceptance", summary: "phase 3 (Polish) is accepted by owner", lines: [] } },
      { kind: "accept-phase", what: "Accept phase 2 (Release)", lines: [], change: { op: "accept-phase", summary: "Accept phase 2 (Release)", lines: [] } },
      { kind: "extend", what: "Add a phase of 1 tasks", lines: ["Load test"], change: { op: "extend", summary: "Add a phase of 1 tasks", lines: ["Load test"] } },
      { kind: "settings", what: "Change the team's settings", lines: ["max_members: 6 → 8"], change: { op: "settings", summary: "Change the team's settings", lines: ["max_members: 6 → 8"] } },
      { kind: "full-tests", what: "Allow one full test run for v1.2", lines: [], change: { op: "full-tests", summary: "Allow one full test run for v1.2", lines: [] } },
      { kind: "rights", what: "Give Backend the rights profile-x, allow", lines: ["Zerus relaunches it in its conversation with these rights."],
        change: { op: "rights", summary: "Give Backend the rights profile-x, allow", lines: ["Zerus relaunches it in its conversation with these rights."] } },
      { kind: "switch-memory-mode", what: "Switch the team's memory from plain to team", lines: ["The team keeps its own memory."],
        change: { op: "switch-memory-mode", summary: "Switch the team's memory from plain to team", lines: ["The team keeps its own memory."] } },
      { kind: "scratch-rule", what: "Let the lead's scratch teams in /work/p start at once, the folder trusted for claude",
        lines: ["Scratch teams a lead creates there start without your per-plan approval; you can revoke the rule in Settings > Teams (hgs team scratch-rules)."],
        change: { op: "scratch-rule", summary: "Let the lead's scratch teams in /work/p start at once, the folder trusted for claude",
          lines: ["Scratch teams a lead creates there start without your per-plan approval; you can revoke the rule in Settings > Teams (hgs team scratch-rules)."] } },
      // Task 251: the lead's install-tool card names the tool and why, its
      // exact command and the folder.
      { kind: "install-tool", what: "Install cmake: the build needs it", lines: ["Project: demo", "It saves 10 min"],
        change: { tool: "cmake", why: "the build needs it", saves: "10 min", command: ["apt", "install", "cmake"], folder: "/work/demo", project: "demo" } },
    ];
    const proposals = rows.map((row, n) => proposal({ id: `p${n + 1}`, kind: row.kind, summary: row.what, lines: row.lines,
      change: row.change ?? null, member: row.member ?? null, goal: row.goal ?? "", mode: row.mode ?? "" }));
    const f = fake(answerTeam({ proposals }));
    const cards = cardsOf(f);
    rows.forEach((row, n) => {
      const state = card(cards, `p${n + 1}`)!;
      expect(state.card.kind, `p${n + 1}`).toBe(row.kind);
      expect(state.card.what, `p${n + 1} ${row.kind}`).toBe(row.what);
      expect(state.card.lines, `p${n + 1} ${row.kind}`).toEqual(row.lines);
      expect(state.card.hash).toBe(`h${n + 1}`);
      expect(state.card.options).toEqual(row.options ?? []);
      expect(state.card.recommended).toBe(row.recommended ?? "");
    });
    // Task 251: the install card carries the exact command and folder apart
    // from its lines, and Allow decides it like any proposal.
    const installer = rows.findIndex((row) => row.kind === "install-tool") + 1;
    expect(card(cards, `p${installer}`)!.card.command).toBe("apt install cmake");
    expect(card(cards, `p${installer}`)!.card.folder).toBe("/work/demo");
    // Allow is one decide-proposal bound to the card's hash, for every kind.
    for (const [n, row] of rows.entries()) {
      if (row.question) continue;
      const before = f.sent.length;
      await cards.answer(`p${n + 1}`, true);
      expect(f.sent, row.kind).toHaveLength(before + 1);
      expect(f.sent.at(-1)!.action).toEqual({ kind: "decide_proposal", proposal: `p${n + 1}`, accept: true, expected_hash: `h${n + 1}` });
      expect(card(cards, `p${n + 1}`)).toBeUndefined();
    }
    expect(f.asked.map((asked) => asked.title)).toEqual(["Remove member?", "Swap member?", "Cancel tasks?", "Discard phase?", "Drain member?"]);
    // A question is answered with the picked option, the recommendation marked on its card.
    const question = rows.findIndex((row) => row.question) + 1;
    cards.pick(`p${question}`, "20 GB");
    await cards.answerQuestion(`p${question}`);
    expect(f.sent.at(-1)!.action).toEqual({ kind: "decide_proposal", proposal: `p${question}`, accept: true, expected_hash: `h${question}`, answer: "20 GB" });
  });

  it("task251: a tool_missing lead item names the tool, why, command, folder and offer", () => {
    const item = {
      kind: "tool_missing", member: "Dev", task: 7, since: 1,
      summary: "Dev's run needs cmake in demo: the build needs it",
      command: '{"request_id":"r","team":"t","offer":"offer-1","allow":false,"reason":"We work without cmake for now"} | hgs team decide-tool --json',
      detail: "Project: demo (/work/demo).\nThe install command: apt install cmake\nAsk the owner to install it: {\"offer\":\"offer-1\"} | hgs team propose --json",
    };
    expect(leadToolMissing(item)).toEqual({
      who: "Dev", tool: "cmake", project: "demo", why: "the build needs it", folder: "/work/demo",
      install: "apt install cmake", offer: "offer-1", denyCommand: item.command,
      proposeCommand: "{\"offer\":\"offer-1\"}",
    });
    expect(leadToolMissing({ ...item, kind: "report" })).toBeUndefined();
    const tool = leadToolMissing(item)!;
    expect(toolMissingAction(tool, "deny", "  no time  ", item.summary)).toEqual({ kind: "decide_tool", offer: "offer-1", allow: false, reason: "no time" });
    expect(toolMissingAction(tool, "deny", "   ", item.summary)).toBeUndefined();
    expect(toolMissingAction(tool, "ask", "", item.summary)).toEqual({ kind: "propose", proposal_kind: "install-tool", reason: item.summary, offer: "offer-1" });
  });

  it("aRefusedDecisionSaysWhyOnItsCard: in the team view and in Waiting for you; a later Deny goes through and the row leaves", async () => {    const words = "Task #3 depends on #9, which is neither in the plan nor among the new tasks";
    const refused = `Allow was refused, nothing was applied: ${words}`;
    const f = fake(answerTeam());
    f.refuse.set("decide_proposal", `hgs: ${words}`);
    const cards = cardsOf(f);
    await cards.answer("p1", true);
    expect(f.sent).toHaveLength(1);
    expect(card(cards, "p1")).toMatchObject({ notice: refused, sending: false });
    // A new read keeps it saying so.
    cards.setTeam(f.team);
    expect(card(cards, "p1")!.notice).toBe(refused);
    // Waiting for you: the same card in its row.
    const answered: string[] = [];
    const row = new WaitingAnswer(answerItems().proposal, f.port, "", () => {}, (done) => answered.push(done.id));
    await row.open();
    await row.decide(true);
    expect(f.sent).toHaveLength(2);
    expect(card(row.proposals!, "p1")).toMatchObject({ notice: refused, sending: false });
    expect(row.done).toBe(false);
    expect(answered).toEqual([]);
    f.refuse.clear();
    row.proposals!.setReason("p1", "Renumber it first");
    await row.decide(false);
    expect(f.sent.at(-1)!.action).toMatchObject({ accept: false, reason: "Renumber it first" });
    expect(row.done).toBe(true);
    expect(answered).toEqual(["team-1/owner_word_asked/proposal-p1"]);
  });
});

describe("Waiting for you", () => {
  /** Qt's waitingKinds: seven kinds from six teams. */
  const kinds = () => [
    item("plan", "plan_draft", "approve_plan", "The plan waits for your approval", NOW - 120, {}, "team-plan", "Onboarding"),
    item("2", "phase_draft", "approve_phase", "Phase 2 QA fixes waits for your approval", NOW - 300, { phase: 2 }, "team-1", "Checkout"),
    item("Tester", "permission_escalated", "answer_permission", "Tester waits for your answer: Bash npm run e2e", NOW - 900, { member: "Tester" }, "team-3", "Mobile sync"),
    item("proposal-p3", "owner_word_asked", "decide_proposal", "The lead proposes: add member", NOW - 1200, { proposal: "p3" }, "team-2", "Search index"),
    item("4", "report_escalated", "decide_report", "Report on #4 Ranking from Backend waits for your decision", NOW - 2400, { task: 4, member: "Backend" }, "team-2", "Search index"),
    item("7", "task_waits_owner", "release_task", "#7 Publish the guide is held by you: after the release", NOW - 7200, { task: 7 }, "team-4", "Docs site"),
    item("finish", "team_finished", "finish", "All 12 tasks decided: waits for you to finish", NOW - 86400, {}, "team-5", "Release 0.43"),
  ];

  it("waitingPageListsEveryKindAcrossTeams: each kind in words, its answer in place, the count by teams", () => {
    const items = kinds();
    expect(items.map(waitingKindLabel)).toEqual(["Plan", "Phase draft", "Permission", "Proposal", "Report", "Held task", "All decided"]);
    expect(items.map((found) => (approves(found.action) ? "Approve…" : opensCard(found.action) ? "Answer" : ""))).toEqual(["Approve…", "Approve…", "Answer", "Answer", "Answer", "Answer", "Answer"]);
    expect(items.map((found) => waitingActionLabel(found.action))).toEqual(["Approve…", "Approve…", "Answer", "Answer", "Accept…", "Answer", "Answer"]);
    expect(waitingCount(items, 7)).toBe("7 items in 6 teams");
    expect(waitingCount([], 0)).toBe("Nothing waits for you");
    expect(waitingAge(NOW - 300, NOW)).toBe("5 min ago");
  });

  it("waitingBannerFoldsOpensAndHides: one notice per item, the rest folded, never repeated", () => {
    const items = kinds();
    const notices = new WaitingNotices();
    expect(notices.observe([items[1]])).toEqual([{ token: "zerus-team-attention:team-1:waiting:team-1/phase_draft/2", title: "Checkout waits for you", body: items[1].title, team: "team-1" }]);
    const raised = notices.observe(items);
    expect(raised).toHaveLength(3);
    expect(raised.slice(0, 2).map((notice) => notice.body)).toEqual([items[0].title, items[2].title]);
    expect(raised[2].body).toBe("4 more items wait for you");
    expect(raised.some((notice) => notice.token.includes("phase_draft/2"))).toBe(false);
    expect(notices.observe(items)).toEqual([]);
    // A new window keeps what was announced.
    expect(new WaitingNotices(notices.ids()).observe(items)).toEqual([]);
  });

  it("waitingItemsAreAnsweredInPlace: the proposal, the permission, the addition and the phase draft, each once; a stale item sends nothing", async () => {
    const f = fake(answerTeam());
    const items = answerItems();
    const answered: string[] = [];
    const answer = (found: TeamWaitingItem) => new WaitingAnswer(found, f.port, "", () => {}, (done) => answered.push(done.id));
    // Task 116: a report and a held task are answered in their row too, like
    // every other kind that waits for the owner.
    expect([items.report, items.held].map((found) => opensCard(found.action) || approves(found.action))).toEqual([true, true]);
    // The proposal: Deny with its reason, bound to its hash.
    const proposalRow = answer(items.proposal);
    await proposalRow.open();
    proposalRow.proposals!.setReason("p1", "Not this week");
    await proposalRow.decide(false);
    expect(f.sent[0]).toMatchObject({ team: "team-1", action: { kind: "decide_proposal", proposal: "p1", accept: false, expected_hash: "h1", reason: "Not this week" } });
    expect(answered).toEqual([items.proposal.id]);
    // The permission: Allow with the question's id, hash and the card's own answer ids.
    const permission = answer(items.permission);
    await permission.open();
    expect(permission.question()!.command).toBe("cargo test --locked");
    await permission.answerPermission(true);
    expect(f.sent[1].action).toEqual({ kind: "answer", member: "backend", question_id: "claude-approval:6f2b0e3a", expected_question_hash: "6f2b0e3a",
      answers: [{ question_id: "approval", selected_option_ids: ["1"], text: "" }] });
    // The addition: Add wants the typed word.
    const addition = answer(items.addition);
    await addition.open();
    expect(addition.addition()!.tasks!.map((found) => found.title)).toEqual(["Card form tests"]);
    expect(await addition.answerAddition(true, "ad")).toBeUndefined();
    await addition.answerAddition(true, "add");
    expect(f.sent[2].action).toEqual({ kind: "add_to_phase", phase: 1, confirm: "add-1", confirmation: "add" });
    // The phase draft: Approve… asks the team view's question; Cancel sends nothing.
    const draft = answer(items.draft);
    f.yes = false;
    expect(await draft.approve()).toBe("cancelled");
    expect(f.asked.at(-1)!.name).toBe("teamApprovePhaseConfirm");
    expect(f.sent).toHaveLength(3);
    expect(draft.done).toBe(false);
    f.yes = true;
    await draft.approve();
    expect(f.sent[3]).toMatchObject({ confirmed: true, action: { kind: "approve", phase: 2 } });
    expect(answered).toEqual([items.proposal.id, items.permission.id, items.addition.id, items.draft.id]);
    // A stale item: p2 was decided after its card opened; Allow says so and sends nothing.
    const stale = answer(items.stale);
    await stale.open();
    f.team = { ...f.team, proposals: f.team.proposals.filter((p) => p.id !== "p2") };
    await stale.decide(true);
    expect(stale.notice).toContain("nothing was sent");
    expect(f.sent).toHaveLength(4);
    expect(stale.done).toBe(false);
  });

  it("everyKindIsAnsweredInItsRow: a report, a held task, a retry and the end answer where they wait (task 116)", async () => {
    const team = answerTeam({
      tasks: [
        task({ id: 1, title: "Saved cards API", status: "submitted" }),
        task({ id: 2, title: "Release notes", held: true }),
        task({ id: 5, title: "Code review", status: "pending", waits: "hgs: unknown model gpt-5.5" }),
        task({ id: 6, title: "Docs", status: "accepted" }),
      ],
    });
    const f = fake(team);
    const answered: string[] = [];
    const answer = (found: TeamWaitingItem) => new WaitingAnswer(found, f.port, "", () => {}, (done) => answered.push(done.id));
    // The report: the same Accept and Reject its team card has.
    const report = answer(item("1", "report_escalated", "decide_report", "Report on #1 Saved cards API", NOW - 300, { task: 1, member: "Backend" }));
    await report.open();
    await report.acceptReport();
    expect(f.sent.at(-1)!.action).toEqual({ kind: "accept", task: 1 });
    const reject = answer(item("1b", "report_escalated", "decide_report", "Report on #1 Saved cards API", NOW - 300, { task: 1, member: "Backend" }));
    await reject.open();
    await reject.rejectReport("Fix the flaky test first");
    expect(f.sent.at(-1)!.action).toEqual({ kind: "reject", task: 1, reason: "Fix the flaky test first" });
    // A held task: Release; a step Zerus could not take: Retry.
    const held = answer(item("2", "task_waits_owner", "release_task", "#2 Release notes is held by you", NOW - 360, { task: 2 }));
    await held.open();
    await held.release();
    expect(f.sent.at(-1)!.action).toEqual({ kind: "release", task: 2 });
    const retry = answer(item("5", "task_waits_owner", "retry_task", "#5 Code review waits: hgs: unknown model", NOW - 400, { task: 5 }));
    await retry.open();
    await retry.retry();
    expect(f.sent.at(-1)!.action).toEqual({ kind: "retry", task: 5 });
    // The end of the team: its summary reaches the lead, after the confirmation.
    const finish = answer(item("finish", "team_finished", "finish", "All 4 tasks decided: waits for you to finish", NOW - 900, {}));
    await finish.open();
    await finish.finishTeam("  ");
    expect(f.sent).toHaveLength(4);
    await finish.finishTeam("Shipped 0.43");
    expect(f.sent.at(-1)!.action).toEqual({ kind: "finish", summary: "Shipped 0.43" });
    expect(answered).toEqual([report.item.id, reject.item.id, held.item.id, retry.item.id, finish.item.id]);
    // Each is an answer in its row, never only an Open team: the strip and the
    // page use the same entries.
    for (const found of [report, reject, held, retry, finish]) expect(opensCard(found.item.action)).toBe(true);
  });

  it("aDeferredRequestIsAnsweredWithoutOpeningTheTeam", () => {
    const found = item("d1", "deferred", "answer_deferred", "answered safely", NOW - 60,
      { deferred: "d1", request: "permission", text: "rm -rf build", session: "codex/x/dev", outcome: "denied" });
    expect(splitDeferred([found]).deferred.map((entry) => entry.id)).toEqual([found.id]);
    expect(opensCard(found.action)).toBe(true);
    // The tool offer is answered in its row too, with or without a team.
    expect(opensCard("decide_tool")).toBe(true);
  });


  it("waitingAdditionCancelDeclinesIt: Cancel declines the addition with its id", async () => {
    const f = fake(answerTeam());
    const answered: string[] = [];
    const addition = new WaitingAnswer(answerItems().addition, f.port, "", () => {}, (done) => answered.push(done.id));
    await addition.open();
    await addition.answerAddition(false);
    expect(f.sent).toHaveLength(1);
    expect(f.sent[0].action).toEqual({ kind: "add_to_phase", phase: 1, confirm: "add-1", decline: true });
    expect(answered).toEqual(["team-1/owner_word_asked/addition-add-1"]);
  });

  it("answers a deferred request with one message from its own card (task 97)", async () => {
    const permission = item("ab12cd34ef56ab78", "deferred", "answer_deferred", "Deferred permission of Backend: Bash: cargo test --locked", NOW - 60,
      { deferred: "ab12cd34ef56ab78", request: "permission", text: "Bash: cargo test --locked", session: "codex/x/dev",
        outcome: "Zerus denied it for now", questions: [] }, "team-1", "Live");
    expect(waitingKindLabel(permission)).toBe("Deferred");
    expect(waitingKindIcon("deferred")).toBe("clock");
    expect(opensCard(permission.action)).toBe(true);
    expect(deferredRequest(permission)).toMatchObject({ id: "ab12cd34ef56ab78", permission: true, text: "Bash: cargo test --locked", outcome: "Zerus denied it for now" });
    const f = fake(answerTeam({}));
    const row = new WaitingAnswer(permission, f.port, "", () => {}, () => {});
    await row.open();
    expect(f.sent).toEqual([]);
    await row.answerDeferred("allow");
    expect(f.sent[0].action).toEqual({ kind: "answer_deferred", id: "ab12cd34ef56ab78", decision: "allow" });
    // A question takes words; an empty answer sends nothing.
    const question = item("cd34", "deferred", "answer_deferred", "Deferred question of solo", NOW - 30,
      { deferred: "cd34", request: "question", text: "Which storage?", questions: [{ id: "q", question: "Which storage?", options: [{ id: "a", label: "SQLite" }] }] }, "", "claude/p/solo");
    expect(deferredRequest(question).questions[0].options[0].label).toBe("SQLite");
    const g = fake(answerTeam({}));
    const asked = new WaitingAnswer(question, g.port, "", () => {}, () => {});
    expect(await asked.answerDeferred("answer", "  ")).toBeUndefined();
    await asked.answerDeferred("answer", "SQLite");
    expect(g.sent.map((sent) => sent.action)).toEqual([{ kind: "answer_deferred", id: "cd34", decision: "answer", text: "SQLite" }]);
    // Waiting for you lists them under Deferred.
    const split = splitDeferred([question, item("1", "phase_draft", "approve_phase", "Phase 1", NOW)]);
    expect([split.waiting.length, split.deferred.map((found) => found.id)]).toEqual([1, [question.id]]);
  });

  it("names a stalled lead and only opens its team (task 95)", () => {
    const stalled = item("Lead", "lead_stalled", "open_lead", "The lead stalled (it is idle): 3 decisions wait for it", NOW - 60, { round: 2 });
    expect(waitingKindLabel(stalled)).toBe("Lead stalled");
    expect(waitingKindIcon("lead_stalled")).toBe("triangle-alert");
    expect(opensCard(stalled.action)).toBe(false);
    expect(waitingActionLabel(stalled.action)).toBe("Open team");
    expect(waitingActionLabel("resume_lead")).toBe("Open team");
  });

  it("aPhaseIsAcceptedOrSentBackFromWaitingForYou: Send back wants a comment, Accept phase, and a phase accepted meanwhile sends nothing", async () => {
    const waiting = answerTeam({ phases: [{ ...phase(1, "Build", "awaiting_acceptance"), acceptance: "owner" }, phase(2, "Release", "draft")] });
    const f = fake(waiting);
    const accepted = item("1", "phase_acceptance", "accept_phase", "Phase 1 is ready for your acceptance", NOW - 60,
      { phase: 1, summary: { accepted: 3, rejected: 1, cancelled: 0, tests: "tests pass", minutes: 42 } });
    expect(waitingKindLabel(accepted)).toBe("Phase acceptance");
    // Task 245: the lead's plan changes have their own label and icon.
    expect([waitingKindLabel({ kind: "lead_changes", action: "open_team" }), waitingKindIcon("lead_changes")]).toEqual(["Lead's changes", "pencil"]);
    expect(acceptanceSummary(accepted.detail!.summary as Record<string, unknown>)).toBe("3 accepted, 1 rejected, 0 cancelled; tests pass; 42 min");
    const answered: string[] = [];
    let row = new WaitingAnswer(accepted, f.port, "", () => {}, (done) => answered.push(done.id));
    await row.open();
    expect(row.notice).toBe("");
    expect(await row.sendBack("  ")).toBeUndefined();
    await row.sendBack("Add the migration steps to the notes");
    expect(f.sent[0].action).toEqual({ kind: "send_back_phase", phase: 1, comment: "Add the migration steps to the notes" });
    expect(answered).toEqual([accepted.id]);
    // The lead reworked it and it waits again: Accept phase.
    row = new WaitingAnswer(accepted, f.port, "", () => {}, (done) => answered.push(done.id));
    await row.open();
    await row.acceptPhase();
    expect(f.sent[1].action).toEqual({ kind: "accept_phase", phase: 1 });
    expect(answered).toEqual([accepted.id, accepted.id]);
    // A phase accepted meanwhile sends nothing.
    f.team = answerTeam({ phases: [phase(1, "Build", "done"), phase(2, "Release", "draft")] });
    row = new WaitingAnswer(accepted, f.port, "", () => {}, (done) => answered.push(done.id));
    await row.open();
    expect(row.notice).toContain("nothing was sent");
    expect(await row.acceptPhase()).toBe("stale");
    expect(f.sent).toHaveLength(2);
  });

  it("waitingBannerAnswersInPlaceAndWithdrawsTheNotice: Deny with a reason, and the notice of the answered item goes", async () => {
    const f = fake(answerTeam());
    const items = answerItems();
    const notices = new WaitingNotices();
    notices.observe([items.permission, items.held]);
    const withdrawn: string[] = [];
    const row = new WaitingAnswer(items.permission, f.port, "", () => {}, (done) => withdrawn.push(...notices.answered(done)));
    await row.open();
    await row.answerPermission(false, "Not on main");
    expect(f.sent[0].action).toEqual({ kind: "answer", member: "backend", question_id: "claude-approval:6f2b0e3a", expected_question_hash: "6f2b0e3a",
      answers: [{ question_id: "approval", selected_option_ids: ["2"], text: "Not on main" }] });
    expect(withdrawn).toEqual([noticeToken(items.permission)]);
  });

  it("aLeadsQuestionIsAnsweredFromWaitingForYou: the session's options or Other, and a question proposal's recommended option", async () => {
    const lead = member({ id: "lead", role: "Lead", lead: true, permission: {
      question_id: "toolu_cache", question_hash: "hash-cache", tool: "Build cache", command: "", reason: "", asked_at: NOW - 30,
      questions: [{ id: "q_0", header: "Build cache", question: "Install the shared build cache?", multi_select: false, allow_other: true,
        options: [{ id: "1", label: "Yes, on D: (Recommended)" }, { id: "2", label: "No" }] }],
    } });
    const team = answerTeam({
      members: [lead, member({ id: "backend" })],
      proposals: [proposal({ id: "p3", kind: "question", hash: "h3", change: { question: "Which cache size?", options: ["10 GB", "20 GB"], recommended: "20 GB" } })],
    });
    const f = fake(team);
    const answered: string[] = [];
    const asked = item("Lead", "permission_escalated", "answer_permission", "lead (Lead) waits for your answer: Install the shared build cache?", NOW - 30, { member: "Lead", member_id: "lead" });
    const row = new WaitingAnswer(asked, f.port, "", () => {}, (done) => answered.push(done.id));
    await row.open();
    expect(row.question()!.questions![0].question).toBe("Install the shared build cache?");
    // Other without words is not an answer yet.
    const choices = new Map<string, QuestionChoice>([["q_0", { chosen: new Set(), other: true, text: "" }]]);
    expect(await row.answerQuestion(choices)).toBeUndefined();
    choices.set("q_0", { chosen: new Set(), other: true, text: "Yes, on D:, but 30 GB" });
    await row.answerQuestion(choices);
    expect(f.sent[0].action).toEqual({ kind: "answer", member: "lead", question_id: "toolu_cache", expected_question_hash: "hash-cache",
      answers: [{ question_id: "q_0", selected_option_ids: [], text: "Yes, on D:, but 30 GB" }] });
    // The proposal: its options, the recommended one marked; the chosen one sent with its hash.
    const proposalItem = item("proposal-p3", "owner_word_asked", "decide_proposal", "The lead asks: Which cache size?", NOW - 20, { proposal: "p3" });
    const question = new WaitingAnswer(proposalItem, f.port, "", () => {}, (done) => answered.push(done.id));
    await question.open();
    const shown = card(question.proposals!, "p3")!;
    expect(shown.card.options).toEqual(["10 GB", "20 GB"]);
    expect(shown.card.recommended).toBe("20 GB");
    expect(proposalCards(team)[0].what).toBe("Which cache size?");
    question.proposals!.pick("p3", "20 GB");
    await question.decideQuestion();
    expect(f.sent[1].action).toEqual({ kind: "decide_proposal", proposal: "p3", accept: true, expected_hash: "h3", answer: "20 GB" });
    expect(answered).toEqual([asked.id, proposalItem.id]);
  });

  it("waitingToolOffersAreAnsweredInPlace: why, what it saves and the command; Deny with a reason, a failed install retries, no manager no Allow", async () => {
    const tool = (offer: string, name: string, state: string, command: string): TeamWaitingItem => ({
      id: `tools/${offer}`, kind: "tool_missing", team: "", team_name: "Zerus", title: `Zerus needs ${name}: the shared build cache`, action: "decide_tool",
      since: NOW - 30, proposal: null, detail: { offer, tool: name, why: "the shared build cache ([build_cache]) for Rust",
        saves: "rebuilding the same crates in every worktree of the team", command, state, output: state === "failed" ? "No package found matching input criteria." : "" },
    });
    const a1 = tool("a1", "sccache", "offered", "winget install --id Mozilla.sccache --exact --scope user --silent");
    const b2 = tool("b2", "ccache", "failed", "winget install --id Ccache.Ccache --exact --scope user --silent");
    const c3 = tool("c3", "gcovr", "offered", "");
    expect(waitingCount([a1, b2, c3], 3)).toBe("3 items");
    expect(waitingKindLabel(a1)).toBe("Tool");
    expect(toolOffer(a1)).toMatchObject({ head: "Install sccache on this computer?", command: "winget install --id Mozilla.sccache --exact --scope user --silent", canAllow: true });
    expect(toolOffer(c3).canAllow).toBe(false);
    expect(toolOffer(c3).command).toContain("No package manager");
    expect(toolOffer(b2)).toMatchObject({ head: "Installing ccache failed", allow: "Retry install" });
    expect(toolOffer(b2).output).toContain("No package found");
    const f = fake(answerTeam());
    const answered: string[] = [];
    const row = (found: TeamWaitingItem) => new WaitingAnswer(found, f.port, "", () => {}, (done) => answered.push(done.id));
    await row(a1).decideTool(false, "Not on this laptop");
    expect(f.sent[0]).toMatchObject({ team: "", action: { kind: "decide_tool", offer: "a1", allow: false, reason: "Not on this laptop" } });
    expect(f.reads).toBe(0);   // an offer belongs to no team: nothing to read
    await row(b2).decideTool(true);
    expect(f.sent[1].action).toEqual({ kind: "decide_tool", offer: "b2", allow: true });
    expect(await row(c3).decideTool(true)).toBeUndefined();
    expect(answered).toEqual(["tools/a1", "tools/b2"]);
  });

  it("permissionAttentionNamesTheMemberAndAnswers: no answer ids from this hgs, nothing is sent", async () => {
    const f = fake(answerTeam({ members: [member({ id: "lead", role: "Lead", lead: true }), member({ id: "backend", permission: { ...QUESTION, answer: undefined } })] }));
    const row = new WaitingAnswer(answerItems().permission, f.port, "", () => {});
    await row.open();
    expect(await row.answerPermission(true)).toBeUndefined();
    expect(f.sent).toEqual([]);
    // An answered question no longer waits: opening it says so.
    const answered = fake(answerTeam({ members: [member({ id: "backend", permission: { ...QUESTION, answered_at: NOW } })] }));
    const gone = new WaitingAnswer(answerItems().permission, answered.port, "", () => {});
    await gone.open();
    expect(gone.notice).toContain("nothing was sent");
  });
});

describe("confirmations", () => {
  it("destructive actions confirm with the safe choice focused: Cancel is the default, Escape cancels", async () => {
    const confirmation = { name: "teamStopConfirm", title: "Stop team?", text: "Stop Live?", action: "Stop team" };
    expect(confirmButtons(confirmation)).toEqual([
      { id: "confirm", text: "Stop team", danger: true, initial: false },
      { id: "cancel", text: "Cancel", danger: false, initial: true },
    ]);
    const confirmer = createConfirmer();
    const first = confirmer.ask(confirmation);
    const second = confirmer.ask({ ...confirmation, title: "Archive team?" });
    expect(confirmer.asking()!.confirmation.title).toBe("Stop team?");
    expect(confirmer.asking()!.buttons.find((button) => button.initial)!.id).toBe("cancel");
    confirmer.key("Enter");   // Enter takes the focused button: Cancel
    expect(await first).toBe(false);
    expect(confirmer.asking()!.confirmation.title).toBe("Archive team?");
    confirmer.key("Escape");
    expect(await second).toBe(false);
    expect(confirmer.asking()).toBeUndefined();
    const third = confirmer.ask(confirmation);
    confirmer.choose("confirm");
    expect(await third).toBe(true);
  });

  it("Task 52: a choice offers its answers with Cancel last and focused; Enter and Escape cancel", async () => {
    const choice = { name: "teamSubteamChoice", title: "Cancel task #4?", text: "Stop it too?", choices: [{ id: "stop", text: "Stop sub-team", danger: true }, { id: "keep", text: "Keep it running" }] };
    expect(choiceButtons(choice)).toEqual([
      { id: "stop", text: "Stop sub-team", danger: true, initial: false },
      { id: "keep", text: "Keep it running", danger: false, initial: false },
      { id: "cancel", text: "Cancel", danger: false, initial: true },
    ]);
    const confirmer = createConfirmer();
    const first = confirmer.askChoice(choice);
    expect(confirmer.asking()!.confirmation.title).toBe("Cancel task #4?");
    confirmer.key("Enter");
    expect(await first).toBeUndefined();
    const second = confirmer.askChoice(choice);
    confirmer.key("Escape");
    expect(await second).toBeUndefined();
    const third = confirmer.askChoice(choice);
    confirmer.choose("keep");
    expect(await third).toBe("keep");
    // An answer the question did not offer is Cancel.
    const fourth = confirmer.askChoice(choice);
    confirmer.choose("confirm");
    expect(await fourth).toBeUndefined();
  });

  it("an unknown outcome keeps its request id; the next answer is the same request", async () => {
    const sent: TeamCall[] = [];
    let unknown = true;
    const port: DecisionPort = {
      async send(call) {
        sent.push(call);
        if (unknown) { unknown = false; return { outcome: "uncertain", detail: "hgs did not answer in time", request_id: "11111111-2222-4333-8444-555555555555" }; }
        return { outcome: "done", receipt: {}, request_id: call.request_id ?? "" };
      },
      async confirm() { return true; },
      async read() { return answerTeam(); },
    };
    const answered: string[] = [];
    const row = new WaitingAnswer(answerItems().held, port, "", () => {}, (done) => answered.push(done.id));
    await row.release();
    expect(row.notice).toContain("did not answer in time");
    expect(row.done).toBe(false);
    await row.release();
    expect(sent.map((call) => call.request_id)).toEqual([null, "11111111-2222-4333-8444-555555555555"]);
    expect(answered).toEqual([answerItems().held.id]);
    // act() itself: a refused call that never ran is failed, never thrown.
    const broken: DecisionPort = { ...port, async send() { throw new Error("Teams need the desktop app."); } };
    expect(await act(broken, { computer: "", team: "team-1", team_name: "Live", action: { kind: "finish", summary: "Done" } })).toMatchObject({ outcome: "failed" });
  });
});

describe("the Team tab's draft-phase approval", () => {
  const confirmation: TeamConfirmation = {
    name: "teamApprovePhaseConfirm", title: "Approve phase?",
    text: "Hand phase 2's 3 tasks to the members?", action: "Approve phase",
  };
  function fake(yes: boolean) {
    const sent: TeamCall[] = [], asked: TeamConfirmation[] = [];
    const port: Pick<DecisionPort, "send" | "confirm"> = {
      async send(call: TeamCall): Promise<TeamOutcome> {
        sent.push(call);
        if (!call.confirmed) return { outcome: "confirm", confirmation };
        return { outcome: "done", receipt: { status: "approved" }, request_id: "r1" };
      },
      async confirm(askedFor: TeamConfirmation) {
        asked.push(askedFor);
        return yes;
      },
    };
    return { sent, asked, port };
  }

  it("Cancel sends nothing after the core's question", async () => {
    const f = fake(false);
    expect(await approvePhaseCall(f.port, { computer: "", team: "t", teamName: "Team" }, 2)).toBe("cancelled");
    // The dialog shows the core's own text; the first call went out alone.
    expect(f.asked).toEqual([confirmation]);
    expect(f.sent).toHaveLength(1);
    expect(f.sent[0]).toMatchObject({ confirmed: false, action: { kind: "approve", phase: 2 } });
  });

  it("the confirm button sends the call again, confirmed", async () => {
    const f = fake(true);
    expect(await approvePhaseCall(f.port, { computer: "", team: "t", teamName: "Team" }, 2)).toMatchObject({ outcome: "done" });
    expect(f.asked).toEqual([confirmation]);
    expect(f.sent).toHaveLength(2);
    expect(f.sent[1]).toMatchObject({ confirmed: true, action: { kind: "approve", phase: 2 } });
  });
});

describe("reports", () => {
  it("an escalated report is accepted or rejected with a comment; a missing one takes the last reply", async () => {
    const { ReportDecision, reportRows, reportLine } = await import("./decisions");
    const team = answerTeam({
      attention: [{ kind: "report", task: 1, member: "Backend", since: NOW - 600, minutes: 10 }],
      tasks: [task({ id: 1, title: "Saved cards API", status: "submitted", attempt_id: "a1" }),
        task({ id: 2, title: "Release notes", status: "in_progress", attempt_id: "a7", no_report: true })],
    });
    const rows = reportRows(team);
    expect(rows.map(reportLine)).toEqual(["Report on #1 Saved cards API from Backend waits for your decision", "#2 Release notes: Backend got no report in"]);
    const f = fake(team);
    const decision = new ReportDecision(f.port, { computer: "", team: "team-1", teamName: "Live" });
    await decision.accept(rows[0]);
    expect(await decision.reject(rows[0], " ")).toBeUndefined();
    await decision.reject(rows[0], "The API misses paging");
    await decision.useLastReply(rows[1]);
    expect(f.sent.map((call) => call.action)).toEqual([
      { kind: "accept", task: 1 }, { kind: "reject", task: 1, reason: "The API misses paging" }, { kind: "use_last_reply", task: 2, attempt: "a7" },
    ]);
    f.refuse.set("accept", "hgs: Task #1 was decided meanwhile");
    await decision.accept(rows[0]);
    expect(decision.notice).toBe("Task #1 was decided meanwhile");
  });
});

describe("a confirmed decision leaves at once (task 107)", () => {
  it("Allow takes the card out before any read; a refused one keeps it with why", async () => {
    const f = fake(answerTeam({
      proposals: [
        proposal({ id: "p1", kind: "add-member", member: { role: "Designer", agent: "claude", model: "sonnet" } }),
        proposal({ id: "p2", kind: "add-member", member: { role: "Writer", agent: "claude", model: "haiku" } }),
      ],
    }));
    const cards = cardsOf(f);
    expect(cards.cards().map((state) => state.card.id)).toEqual(["p1", "p2"]);
    await cards.answer("p1", true);
    expect(card(cards, "p1")).toBeUndefined();
    // An older read still listing p1 does not bring the card back.
    cards.setTeam(f.team);
    expect(card(cards, "p1")).toBeUndefined();
    f.refuse.set("decide_proposal", "hgs: The plan changed since the warning");
    await cards.answer("p2", true);
    expect(card(cards, "p2")?.sending).toBe(false);
    expect(card(cards, "p2")?.notice).toContain("The plan changed since the warning");
  });

  it("a card hgs withdrew (task 111) leaves at once when its Allow is refused for it", async () => {
    const f = fake(answerTeam({
      proposals: [proposal({ id: "p3", kind: "add-member", member: { role: "Designer", agent: "claude", model: "sonnet" } })],
    }));
    const cards = cardsOf(f);
    f.refuse.set("decide_proposal", "hgs: This proposal can no longer apply (Phase 2 is closed (done); propose the work in a new phase with extend instead); it was withdrawn and the lead is told why");
    await cards.answer("p3", true);
    expect(card(cards, "p3")).toBeUndefined();
    cards.setTeam(f.team);
    expect(card(cards, "p3")).toBeUndefined();
    expect(withdrawnRefusal("hgs: The plan changed since the warning")).toBe(false);
  });

  it("matches what a confirmed action decided in Waiting for you, as Qt does", () => {
    const item = (id: string, kind: string, action: string, detail: Record<string, unknown> = {}, proposal: unknown = null, team = "t1") =>
      ({ id: `${team}/${kind}/${id}`, kind, team, team_name: "Live", title: id, action, since: 1, proposal, detail });
    const items = [
      item("proposal-p1", "owner_word_asked", "decide_proposal", {}, "p1"),
      item("proposal-p2", "owner_word_asked", "decide_proposal", {}, "p2"),
      item("2", "phase_draft", "approve_phase", { phase: 2 }),
      item("3", "phase_acceptance", "accept_phase", { phase: 3 }),
      item("1", "report_escalated", "decide_report", { task: 1 }),
      item("2", "task_waits_owner", "release_task", { task: 2 }),
      item("proposal-p1", "owner_word_asked", "decide_proposal", {}, "p1", "t2"),
    ];
    const decided = (action: TeamAction, team = "t1") => decidedWaiting(items, { team, action });
    expect(decided({ kind: "decide_proposal", proposal: "p1", accept: true, expected_hash: "h1" })).toEqual(["t1/owner_word_asked/proposal-p1"]);
    expect(decided({ kind: "approve", phase: 2 })).toEqual(["t1/phase_draft/2"]);
    expect(decided({ kind: "accept_phase", phase: 3 })).toEqual(["t1/phase_acceptance/3"]);
    expect(decided({ kind: "accept", task: 1 })).toEqual(["t1/report_escalated/1"]);
    expect(decided({ kind: "release", task: 2 })).toEqual(["t1/task_waits_owner/2"]);
    expect(decided({ kind: "hold", task: 2, reason: "later" } as TeamAction)).toEqual([]);
    expect(decided({ kind: "accept_phase", phase: 9 })).toEqual([]);
    expect(decided({ kind: "decide_proposal", proposal: "p1", accept: true, expected_hash: "h1" }, "t3")).toEqual([]);
  });
});

// Task 198 (W3): Allow and Deny never fail silently: a card without the
// question's hash or its answer ids says why (Qt TeamWaitingAnswer C4).
describe("a permission that cannot be answered says why (task 198)", () => {
  it("names the missing hash, the missing answer ids, or nothing when it can answer", async () => {
    const noIds = fake(answerTeam({ members: [member({ id: "lead", role: "Lead", lead: true }), member({ id: "backend", permission: { ...QUESTION, answer: undefined } })] }));
    const row = new WaitingAnswer(answerItems().permission, noIds.port, "", () => {});
    await row.open();
    expect(row.permissionBlocked()).toBe("This hgs sends no answer ids for the question; answer it in the session.");
    expect(await row.answerPermission(true)).toBeUndefined();
    expect(noIds.sent).toEqual([]);
    expect(row.notice).toBe("This hgs sends no answer ids for the question; answer it in the session.");
    const noHash = fake(answerTeam({ members: [member({ id: "lead", role: "Lead", lead: true }), member({ id: "backend", permission: { ...QUESTION, question_hash: "" } })] }));
    const hashless = new WaitingAnswer(answerItems().permission, noHash.port, "", () => {});
    await hashless.open();
    expect(hashless.permissionBlocked()).toBe("This hgs sends no question hash in status; answer it in the session.");
    const fine = fake(answerTeam());
    const ok = new WaitingAnswer(answerItems().permission, fine.port, "", () => {});
    await ok.open();
    expect(ok.permissionBlocked()).toBe("");
  });
});

// Task 304 (hgs #246, #286): the escalated wait, the grouped digest, the strip header and a dismissed row's blocks.
describe("Waiting rows of tasks 246 and 286 (task 304)", () => {
  const base = { team: "t1", team_name: "Orbit", action: "open_team", since: 1_800_000_000, proposal: null } as const;
  it("labels the escalated wait, which names the lead", () => {
    const escalated = { ...base, id: "t1/task_wait_escalated/escalated-4", kind: "task_wait_escalated", title: "#4 API still waits after 60 min: unknown model (Lead has not acted)" };
    expect([waitingKindLabel(escalated), waitingKindIcon("task_wait_escalated")]).toEqual(["Lead has not acted", "clock"]);
  });
  it("reads the digest's groups: count and kind, the newest summaries", () => {
    const digest = { ...base, id: "t1/lead_changes/digest", kind: "lead_changes", title: "The lead changed the plan 4 times", detail: { groups: [
      { kind: "added", count: 2, newest: [{ summary: "added #9 API tests" }, { summary: "added #10 docs" }] },
      { kind: "moved", count: 1, newest: [{ summary: "moved #3 to phase 2" }] },
      { kind: "odd", count: 1, newest: [] }, { kind: "", count: 3 }, { kind: "cancelled", count: 0 },
    ] } };
    expect(digestGroups(digest)).toEqual([
      { kind: "added", label: "2 added", count: 2, newest: ["added #9 API tests", "added #10 docs"] },
      { kind: "moved", label: "1 moved", count: 1, newest: ["moved #3 to phase 2"] },
      { kind: "odd", label: "1 changed", count: 1, newest: [] },
    ]);
    expect(digestGroups({ ...digest, kind: "team_paused" })).toEqual([]);
    expect(digestGroups({ ...digest, detail: null })).toEqual([]);
  });
  it("the strip header names the newest item that is not the digest", () => {
    const digest = { ...base, id: "d", kind: "lead_changes", title: "The lead changed the plan" };
    const paused = { ...base, id: "p", kind: "team_paused", title: "Team paused" };
    expect(bannerHeadItem([digest, paused])?.id).toBe("p");
    expect(bannerHeadItem([digest])).toBeUndefined();
  });
  it("a dismissed decision says what it still blocks", () => {
    expect(stillBlocks({ detail: { blocking: true, blocks: "Orbit/backend" } })).toBe("Orbit/backend");
    expect(stillBlocks({ detail: { blocks: "Orbit" } })).toBe("");
    expect(stillBlocks({ detail: null })).toBe("");
  });
});
