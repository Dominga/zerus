import { describe, expect, it } from "vitest";
import {
  TeamActions, allDecided, barActions, barChips, discardConfirmation, leadStall, finishSummary, formAction, formText, moreItems, moveTargets,
  openQuestions, pauseConfirmation, planApproval, questionClosed, retryConfirmation, retryContinues, retryContinueText, sendBackMessage, stageName, stopConfirmation, subtreeImpact, subteamQuestion, taskButtons,
  leadMemberOf,
  type ActTask, type ActTeam,
} from "./actions";
import type { DecisionPort } from "./decisions";
import type { TeamCall, TeamChoice, TeamConfirmation } from "../../types";

// Task 40: every team bar action and task action of Qt (TeamWorkspace
// headerActionsContent, TeamPanel approvePlan/finishTeam, TeamTaskCard footer,
// More and forms) with Qt's confirmations, one path each.

function task(over: Partial<ActTask> & { id: number }): ActTask {
  return { title: `Task ${over.id}`, status: "pending", assignee: "api", phase: 1, deps: [], attempts: 0, ...over };
}

function team(over: Partial<ActTeam> = {}): ActTeam {
  return {
    id: "t1", name: "Checkout", status: "running", plan_mode: "standard", revision: 7, pending_phase: null, waiting_for_lead: false, settings: {},
    members: [
      { id: "lead", role: "Lead", agent: "claude", computer: "", lead: true, session: { name: "claude/x/lead" } },
      { id: "api", role: "Backend", agent: "codex", computer: "", lead: false, session: { name: "codex/x/api" } },
      { id: "qa", role: "Tester", agent: "kimi", computer: "studio", lead: false, session: null },
    ],
    tasks: [task({ id: 1, status: "in_progress", attempts: 1 }), task({ id: 2 })],
    phases: [{ n: 1, title: "Build", state: "running", status: "in_work" }, { n: 2, title: "Polish", state: "draft", status: "draft" }],
    ...over,
  };
}

interface Fake { port: DecisionPort; sent: TeamCall[]; asked: TeamConfirmation[]; yes: boolean; refuse?: string; unknown?: string }

/** The fake core: Archive, Cancel, Take over and Stop ask first unless confirmed (action.rs). */
function fake(): Fake {
  const f: Fake = {
    sent: [], asked: [], yes: true,
    port: {
      async send(call) {
        if (!call.confirmed && ["archive", "cancel", "take_over", "stop", "discard"].includes(call.action.kind))
          return { outcome: "confirm", confirmation: { name: `core-${call.action.kind}`, title: "Sure?", text: "Core asks", action: "Do it" } };
        f.sent.push(call);
        if (f.refuse) return { outcome: "failed", detail: f.refuse, request_id: "r" };
        if (f.unknown) { const id = f.unknown; f.unknown = undefined; return { outcome: "uncertain", detail: "hgs did not answer in time", request_id: id }; }
        return { outcome: "done", receipt: {}, request_id: call.request_id ?? `r${f.sent.length}` };
      },
      async confirm(confirmation) { f.asked.push(confirmation); return f.yes; },
      async read() { throw new Error("no read"); },
    },
  };
  return f;
}

const actionsOf = (f: Fake, t: ActTeam) => new TeamActions(f.port, () => ({ computer: "", team: t }));
const ids = (t: ActTeam, mode = "") => barActions(t, mode).map((action) => action.id);

describe("team bar", () => {
  it("shows Qt's actions for each status, in Qt's order", () => {
    expect(ids(team({ status: "planning" }))).toEqual(["resumeLimits", "discard", "sendBack", "approve"]);
    expect(ids(team())).toEqual(["resumeLimits", "pause", "stop"]);
    expect(ids(team({ status: "paused" }))).toEqual(["resumeLimits", "resume", "stop"]);
    expect(ids(team({ status: "done" }))).toEqual(["archive", "nextGoal"]);
    expect(ids(team({ status: "stopped" }))).toEqual(["archive"]);
    const decided = team({ tasks: [task({ id: 1, status: "accepted" }), task({ id: 2, status: "cancelled" })] });
    expect(allDecided(decided)).toBe(true);
    expect(ids(decided)).toEqual(["resumeLimits", "finish", "pause", "stop"]);
    // U59: a free team's empty queue is idle, not done.
    expect(allDecided({ ...decided, plan_mode: "free" })).toBe(false);
  });

  it("Approve & Run waits for tasks and a clean plan, and names free mode", () => {
    const planning = team({ status: "planning" });
    const approve = (t: ActTeam, mode = "") => barActions(t, mode).find((action) => action.id === "approve")!;
    expect(approve(planning)).toMatchObject({ text: "Approve & Run", enabled: true, primary: true, tip: "Create members and hand out ready tasks" });
    expect(approve(planning, "free").text).toBe("Approve & Run in free mode");
    expect(approve({ ...planning, tasks: [] })).toMatchObject({ enabled: false, tip: "The plan has no tasks yet" });
    expect(approve({ ...planning, validation: { errors: [{}] } } as ActTeam)).toMatchObject({ enabled: false, tip: "Fix the plan errors first" });
    expect(approve({ ...planning, widenings: [{ member: "api", from: "auto", to: "bypass" }] }).tip).toBe("A permission widening needs your confirmation");
  });

  it("shows the pending phase, Finish waiting and waiting for the lead as chips", () => {
    expect(barChips(team({ pending_phase: 2 }))).toEqual([{ id: "pendingPhase", phase: 2, text: "Phase 2 waits for you", tip: "Open the phase draft" }]);
    const two = team({ pending_phase: 2, phases: [...team().phases, { n: 3, title: "", state: "draft", status: "draft" }] });
    expect(barChips(two)[0].tip).toBe("phases 2, 3 pending approval");
    expect(barChips(team({ status: "planning", pending_phase: 2 }))).toEqual([]);
    const decided = team({ waiting_for_lead: true, tasks: [task({ id: 1, status: "accepted" })] });
    expect(barChips(decided).map((chip) => chip.text)).toEqual(["Waiting for you to finish", "waiting for the lead"]);
  });

  it("puts a stalled lead first on the bar, by its reason and count (task 95)", () => {
    const stall = (over: Record<string, unknown>) => ({ kind: "lead_stalled", member: "Lead", member_id: "lead", since: 100, minutes: 42,
      reason: "idle", items: 3, members: 2, round: 1, summary: "The lead stalled (it is idle): 3 decisions wait for it", ...over });
    const stalled = team({ pending_phase: 2, attention: [{ kind: "report", task: 1 }, stall({})] });
    expect(barChips(stalled).map((chip) => chip.id)).toEqual(["leadStalled", "pendingPhase"]);
    expect(barChips(stalled)[0]).toEqual({ id: "leadStalled", text: "Lead stalled: 3 decisions wait", tip: "The lead stalled (it is idle): 3 decisions wait for it" });
    expect(barChips(team({ attention: [stall({ reason: "exited", items: 1 })] }))[0].text).toBe("Lead stopped: 1 decision waits");
    expect(leadStall(team({ attention: [stall({ round: 2 })] }))).toMatchObject({ reason: "idle", items: 3, members: 2, round: 2 });
    // The lead moved: no item, no chip.
    expect(leadStall(team({ attention: [{ kind: "report", task: 1 }] }))).toBeNull();
    expect(barChips(team())).toEqual([]);
  });

  it("Approve & Run asks Qt's question, then sends approve once; Cancel sends nothing", async () => {
    const t = team({ status: "planning" });
    const { confirmation } = planApproval(t, "");
    expect(confirmation).toMatchObject({ name: "teamApproveConfirm", title: "Approve plan and run?", action: "Approve and run" });
    expect(confirmation.text).toBe("Approve the plan of Checkout and run it? It starts 1 member session: Tester (kimi on studio). Ready tasks are handed out at once.");
    expect(planApproval(t, "free").confirmation.text).toContain(" It runs in free planning: one open phase");
    const f = fake();
    f.yes = false;
    expect(await actionsOf(f, t).approve("free")).toBe("cancelled");
    expect(f.sent).toEqual([]);
    f.yes = true;
    await actionsOf(f, t).approve("free");
    expect(f.sent).toEqual([{ computer: "", team: "t1", team_name: "Checkout", confirmed: true, request_id: null, action: { kind: "approve", plan_mode: "free" } }]);
  });

  it("a widening asks its own confirmation and approves with confirm_widening", async () => {
    const t = team({ status: "planning", widenings: [{ member: "api", role: "Backend", from: "auto", to: "bypass" }] });
    const { confirmation, action } = planApproval(t, "");
    expect(confirmation).toMatchObject({ name: "teamWideningConfirm", title: "Widen permissions?", action: "Confirm and approve" });
    expect(confirmation.text).toContain("Backend: auto → bypass");
    expect(action).toEqual({ kind: "approve", confirm_widening: true });
  });

  it("Discard plan asks Qt's question; Send back words the coordinator's message", async () => {
    const t = team({ status: "planning" });
    expect(discardConfirmation(t)).toEqual({ name: "teamDiscardConfirm", title: "Discard plan?", text: "Discard the plan of Checkout? The team stops before any member starts.", action: "Discard plan" });
    const f = fake();
    f.yes = false;
    expect(await actionsOf(f, t).discard()).toBe("cancelled");
    f.yes = true;
    await actionsOf(f, t).discard();
    expect(f.sent.map((call) => [call.action, call.confirmed])).toEqual([[{ kind: "discard" }, true]]);
    expect(f.asked.map((asked) => asked.name)).toEqual(["teamDiscardConfirm", "teamDiscardConfirm"]);
    expect(sendBackMessage("  Split #3 ")).toBe("Your plan needs changes before I approve it: Split #3");
  });

  it("Pause asks only for a team tree; Resume goes at once", async () => {
    const f = fake();
    const alone = team();
    expect(pauseConfirmation(alone, subtreeImpact(alone, []))).toBeUndefined();
    await actionsOf(f, alone).pause(subtreeImpact(alone, []));
    expect(f.sent.at(-1)!.action).toEqual({ kind: "pause" });
    expect(f.asked).toEqual([]);
    const tree = team({ children: [{ team: "s1" }] });
    const impact = subtreeImpact(tree, [{ id: "s1", members: [{ session: { name: "a" } }, { session: null }] } as never]);
    expect(impact).toEqual({ teams: 1, sessions: 1 });
    expect(pauseConfirmation(tree, impact)).toMatchObject({ name: "teamPauseTreeConfirm", text: "Pause Checkout? It also pauses 1 sub-teams and 1 member sessions in them." });
    f.yes = false;
    expect(await actionsOf(f, tree).pause(impact)).toBe("cancelled");
    expect(f.sent).toHaveLength(1);
    await actionsOf(f, team({ status: "paused" })).resume();
    expect(f.sent.at(-1)!.action).toEqual({ kind: "resume" });
  });

  it("Stop asks with the sub-teams it reaches; Archive asks the core's question", async () => {
    const tree = team({ children: [{ team: "s1" }] });
    expect(stopConfirmation(tree, { teams: 1, sessions: 2 }).text).toBe(
      "Stop Checkout? Every open task is cancelled and no task is handed out again. Member sessions keep running; their current turns finish. It stops 1 sub-teams and affects 2 member sessions in them.");
    const f = fake();
    await actionsOf(f, tree).stop({ teams: 1, sessions: 2 });
    expect(f.sent.map((call) => [call.action.kind, call.confirmed])).toEqual([["stop", true]]);
    f.yes = false;
    expect(await actionsOf(f, team({ status: "done" })).archive()).toBe("cancelled");
    expect(f.asked.at(-1)!.name).toBe("core-archive");
    f.yes = true;
    await actionsOf(f, team({ status: "done" })).archive();
    expect(f.sent.at(-1)).toMatchObject({ action: { kind: "archive" }, confirmed: true });
  });

  it("Task 52: the impact counts the whole tree, through each listed team's parent", () => {
    const tree = team({ children: [{ team: "s1" }] });
    const session = (name: string) => ({ session: { name } });
    const list = [
      { id: "t1", members: [session("lead")], parent: "" },
      { id: "s1", members: [session("a"), { session: null }], parent: "t1" },
      { id: "s2", members: [session("b"), session("c")], parent: "s1" },
      { id: "s3", members: [session("d")], parent: "s2" },
      { id: "s4", members: [session("e")], parent: "t1" },
      { id: "other", members: [session("f")], parent: "" },
    ] as never[];
    // s1 from the team's record and its list entry (once), s4 from the list
    // only, s2 and s3 below them: four sub-teams, five sessions.
    expect(subtreeImpact(tree, list)).toEqual({ teams: 4, sessions: 5 });
    expect(stopConfirmation(tree, subtreeImpact(tree, list)).text).toContain("It stops 4 sub-teams and affects 5 member sessions in them.");
    // A cycle in broken records is walked once.
    const loop = [{ id: "s1", members: [], parent: "s2" }, { id: "s2", members: [], parent: "s1" }] as never[];
    expect(subtreeImpact(team({ children: [{ team: "s1" }] }), loop)).toEqual({ teams: 2, sessions: 0 });
  });

  it("Finish needs a summary of at most 4000 characters and sends it once", async () => {
    expect(finishSummary("")).toEqual({ count: "0 / 4000", ok: false });
    expect(finishSummary(" Shipped ")).toEqual({ count: "7 / 4000", ok: true });
    expect(finishSummary("x".repeat(4001)).ok).toBe(false);
    const f = fake();
    const decided = team({ tasks: [task({ id: 1, status: "accepted" })] });
    expect(await actionsOf(f, decided).finish("   ")).toBeUndefined();
    await actionsOf(f, decided).finish(" Shipped the API ");
    expect(f.sent).toEqual([{ computer: "", team: "t1", team_name: "Checkout", confirmed: true, request_id: null, action: { kind: "finish", summary: "Shipped the API" } }]);
  });

  it("Resume after limits switches only its own setting", async () => {
    expect(barActions(team({ settings: { resume_after_limits: true } }), "")[0]).toMatchObject({ id: "resumeLimits", checked: true });
    const f = fake();
    await actionsOf(f, team()).resumeAfterLimits(true);
    expect(f.sent[0].action).toEqual({ kind: "settings", settings: { resume_after_limits: true } });
  });

  it("a refusal says why in hgs's words; an unknown outcome is sent again with the same request id", async () => {
    const f = fake();
    f.refuse = "hgs: The team is not running";
    const actions = actionsOf(f, team({ status: "paused" }));
    await actions.resume();
    expect(actions.notice).toBe("The team is not running");
    f.refuse = undefined;
    f.unknown = "11111111-2222-4333-8444-555555555555";
    await actions.resume();
    expect(actions.notice).toContain("did not answer in time");
    await actions.resume();
    expect(f.sent.map((call) => call.request_id)).toEqual([null, null, "11111111-2222-4333-8444-555555555555"]);
    expect(actions.notice).toBe("");
  });
});

describe("task card", () => {
  const review = task({ id: 4, title: "API", status: "in_review", attempts: 1, pipeline: "code",
    pipeline_run: { name: "Code", accept: "lead", stages: ["review", "test"], open: { stage: 0, kind: "review", member: "qa", state: "delivered" } } });
  const buttons = (t: ActTask) => taskButtons(t).map((b) => b.id);

  it("offers Qt's footer for a stage, a report, open work and a held task", () => {
    expect(buttons(review)).toEqual(["acceptNow", "skip", "sendBack", "reject"]);
    expect(taskButtons(review)[1].text).toBe("Skip review…");
    expect(buttons(task({ id: 5, status: "submitted" }))).toEqual(["accept", "reject", "reassign"]);
    expect(buttons(task({ id: 5, status: "submitted", pipeline_run: { name: "Code", accept: "lead", stages: [], open: null } }))).toEqual(["accept", "reject", "sendBack", "reassign"]);
    expect(buttons(task({ id: 6 }))).toEqual(["hold", "reassign", "takeOver", "cancel"]);
    expect(buttons(task({ id: 6, held: true }))).toEqual(["release", "reassign", "takeOver", "cancel"]);
    expect(buttons(task({ id: 6, status: "in_progress", attempts: 1 }))).toEqual(["reassign", "takeOver", "cancel"]);
    expect(buttons(task({ id: 7, status: "accepted" }))).toEqual([]);
    expect(stageName("sub_lead")).toBe("Sub-lead");
  });

  it("More lists the rest, disabled with Qt's reasons, and the phases a task may move to", () => {
    const phases = [...team().phases, { n: 3, title: "Ship", state: "draft", status: "draft" }];
    const t = team({ phases, tasks: [task({ id: 1, phase: 3 }), task({ id: 2, deps: [{ task: 1, hard: true }] })] });
    const more = moreItems(t, t.tasks[1]);
    expect(more.map((entry) => [entry.id, entry.phase ?? null, entry.enabled])).toEqual([
      ["retry", null, true], ["hold", null, true], ["release", null, false], ["move", 2, false], ["move", 3, true],
      ["accept", null, false], ["reject", null, false], ["done", null, true]]);
    expect(more.find((entry) => entry.id === "move")).toMatchObject({ phase: 2, text: "Move to phase 2 Polish", why: "Depends on #1 in phase 3; move it first" });
    expect(more.find((entry) => entry.id === "accept")!.why).toBe("Nothing to accept yet: no report");
    const decided = moreItems(t, task({ id: 9, status: "accepted" }));
    expect(decided.find((entry) => entry.id === "retry")).toMatchObject({ enabled: false, why: "Task #9 is already decided" });
    expect(decided.find((entry) => entry.id === "cancel")).toMatchObject({ text: "Cancel task…", enabled: false });
    expect(moveTargets(t, 1)).toEqual([{ phase: 1, title: "Build", blocked: "" }, { phase: 2, title: "Polish", blocked: "" }]);
    // A started task does not move.
    expect(moveTargets(team(), 1)).toEqual([]);
  });

  it("Task #308/#451: proposes the project's working mode as it is now", async () => {
    const f = fake();
    await actionsOf(f, team()).proposeWorkingMode();
    // Task #451: never a cached preset: hgs re-reads the project's profile and
    // proposes the mode it selects now, so a stale block corrects itself.
    expect(f.sent.at(-1)?.action).toEqual({ kind: "propose_working_mode", preset: "auto", reason: "" });
    expect(f.asked.at(-1)).toMatchObject({ name: "teamWorkingModeConfirm" });
  });

  it("Task #337: keeps the team's settings without a confirmation", async () => {
    const f = fake();
    await actionsOf(f, team()).keepWorkingMode();
    expect(f.sent.at(-1)?.action).toEqual({ kind: "keep_working_mode" });
    // Nothing about the team changes: no owner dialog comes first.
    expect(f.asked).toHaveLength(0);
  });

  it("Task #296: finds the next team's lead once its list arrives", () => {
    const list = { teams: [{ id: "t9", members: [
      { id: "dev", role: "Backend", agent: "codex", state: "active", computer: "", lead: false, session: { host: "", name: "codex/x/dev", run_id: "r", conversation_id: "c" } },
      { id: "lead9", role: "Lead", agent: "claude", state: "active", computer: "", lead: true, session: { host: "", name: "claude/x/lead9", run_id: "r", conversation_id: "c" } },
    ] }] };
    expect(leadMemberOf(list, "t9")?.id).toBe("lead9");
    expect(leadMemberOf({ teams: [] }, "t9")).toBeUndefined();
    expect(leadMemberOf(undefined, "t9")).toBeUndefined();
  });

  it("Task 253: a retry that continues names the kept attempt", () => {
    expect(retryContinues({ status: "continue", task: 4, attempt: 2 })).toBe(2);
    expect(retryContinueText(2)).toBe("Continues attempt 2 - nothing was revoked");
    expect(retryContinues({ status: "pending", task: 4, attempt: 2 })).toBeNull();
    expect(retryContinues({ status: "continue" })).toBeNull();
  });

  it("each override form words Qt's question and sends its reason; no reason sends nothing", async () => {
    const t = team({ members: [...team().members, { id: "rev", role: "Reviewer", agent: "claude", computer: "", lead: false, session: null }] });
    expect(formText(t, review, "accept-now")).toMatchObject({ heading: "Accept #4 API now?", submit: "Accept now",
      what: "Ends the pipeline here; the Review and Test stages do not run. The feed records that you accepted it and why." });
    expect(formText(t, review, "skip")).toMatchObject({ heading: "Skip Review on #4 API?", submit: "Skip Review",
      what: "Tester stops; #4 moves to Test. The feed records that you skipped it and why." });
    expect(formText(t, review, "send-back")).toMatchObject({ heading: "Send #4 API back to Backend?",
      what: "The review stops; Backend gets a new attempt with your reason. It does not count as a round." });
    expect(formText(t, task({ id: 6 }), "hold").submit).toBe("Hold");
    expect(formText(t, task({ id: 6 }), "done").heading).toBe("Mark done manually");
    expect(formAction(review, "skip", "  ")).toBeUndefined();
    const f = fake();
    const actions = actionsOf(f, t);
    await actions.submit(review, "accept-now", "Reviewed by hand");
    await actions.submit(review, "skip", "Covered by CI");
    await actions.submit(review, "send-back", "Paging is missing");
    await actions.submit(review, "reject", "Wrong endpoint", true);
    await actions.submit(task({ id: 6 }), "hold", "After the release");
    await actions.submit(task({ id: 6 }), "done", "Done by hand");
    await actions.submit(task({ id: 6 }), "hold", "   ");
    expect(f.sent.map((call) => call.action)).toEqual([
      { kind: "accept", task: 4, reason: "Reviewed by hand" },
      { kind: "skip_stage", task: 4, reason: "Covered by CI" },
      { kind: "send_back", task: 4, reason: "Paging is missing" },
      { kind: "reject", task: 4, reason: "Wrong endpoint", hold: true },
      { kind: "hold", task: 6, reason: "After the release" },
      { kind: "accept", task: 6, reason: "Done by hand", manual: true },
    ]);
  });

  it("Accept, Release, Reassign and Move go at once; Move names the plan revision", async () => {
    const f = fake();
    const t = team();
    const actions = actionsOf(f, t);
    await actions.accept(task({ id: 5, status: "submitted" }));
    await actions.release(task({ id: 6, held: true }));
    await actions.reassign(task({ id: 6 }), "qa");
    await actions.move(task({ id: 2 }), 2);
    expect(f.sent.map((call) => call.action)).toEqual([
      { kind: "accept", task: 5 }, { kind: "release", task: 6 }, { kind: "reassign", task: 6, member: "qa" }, { kind: "move_task", task: 2, phase: 2, revision: 7 }]);
    expect(f.asked).toEqual([]);
  });

  it("Take over and Cancel ask the core's question; Retry asks Qt's, naming the attempt", async () => {
    const f = fake();
    const actions = actionsOf(f, team());
    f.yes = false;
    expect(await actions.takeOver(task({ id: 6 }))).toBe("cancelled");
    expect(await actions.cancel(task({ id: 6 }))).toBe("cancelled");
    expect(await actions.retry(task({ id: 1, status: "in_progress", attempts: 2 }))).toBe("cancelled");
    expect(f.sent).toEqual([]);
    expect(retryConfirmation(task({ id: 1, attempts: 2 }))).toEqual({ name: "teamRetryConfirm", title: "Retry task?", text: "Revoke attempt 2 and hand the task out again?", action: "Retry" });
    expect(f.asked.map((asked) => asked.name)).toEqual(["core-take_over", "core-cancel", "teamRetryConfirm"]);
    f.yes = true;
    await actions.takeOver(task({ id: 6 }));
    await actions.cancel(task({ id: 6 }));
    await actions.retry(task({ id: 1, status: "in_progress", attempts: 2 }));
    expect(f.sent.map((call) => [call.action.kind, call.confirmed])).toEqual([["take_over", true], ["cancel", true], ["retry", true]]);
    // A task never tried retries without asking.
    expect(retryConfirmation(task({ id: 2 }))).toBeUndefined();
  });
});

describe("Task 52: a delegated task's sub-team", () => {
  const delegated = task({ id: 4, status: "in_progress", attempts: 1, subteam: true, subteam_team: "child-team" });
  const withChoice = (f: Fake, answers: (string | undefined)[]) => {
    const asked: TeamChoice[] = [];
    f.port.choose = async (choice) => { asked.push(choice); return answers.shift(); };
    return asked;
  };

  it("Cancel asks first, then whether the sub-team stops, and sends the answer once", async () => {
    const f = fake();
    const asked = withChoice(f, ["stop"]);
    await actionsOf(f, team({ tasks: [delegated] })).cancel(delegated);
    expect(f.asked.map((confirmation) => confirmation.name)).toEqual(["teamCancelTaskConfirm"]);
    expect(asked).toEqual([subteamQuestion(delegated, "Cancel")]);
    expect(asked[0]).toMatchObject({ title: "Cancel task #4?", text: "Task #4 has the sub-team child-team. Stop that sub-team too, or keep it running?" });
    expect(asked[0].choices.map((choice) => choice.id)).toEqual(["stop", "keep"]);
    expect(f.sent.map((call) => [call.action, call.confirmed])).toEqual([[{ kind: "cancel", task: 4, subteam: "stop" }, true]]);
  });

  it("Reassign asks about the sub-team; Cancel in that question sends nothing", async () => {
    const f = fake();
    const asked = withChoice(f, ["keep", undefined]);
    await actionsOf(f, team({ tasks: [delegated] })).reassign(delegated, "qa");
    expect(asked[0].title).toBe("Reassign task #4?");
    expect(f.sent.at(-1)!.action).toEqual({ kind: "reassign", task: 4, member: "qa", subteam: "keep" });
    expect(await actionsOf(f, team({ tasks: [delegated] })).reassign(delegated, "qa")).toBe("cancelled");
    expect(f.sent).toHaveLength(1);
    // Declining Cancel's own question asks nothing more.
    f.yes = false;
    expect(await actionsOf(f, team({ tasks: [delegated] })).cancel(delegated)).toBe("cancelled");
    expect(asked).toHaveLength(2);
  });

  it("a task without a sub-team asks nothing more; a sub-team without its id keeps it, as Qt", async () => {
    const f = fake();
    const asked = withChoice(f, []);
    const plain = task({ id: 5 });
    await actionsOf(f, team({ tasks: [plain] })).reassign(plain, "qa");
    expect(f.sent.at(-1)!.action).toEqual({ kind: "reassign", task: 5, member: "qa" });
    const unnamed = task({ id: 6, subteam: true });
    expect(subteamQuestion(unnamed, "Cancel")).toBeUndefined();
    await actionsOf(f, team({ tasks: [unnamed] })).cancel(unnamed);
    expect(f.sent.at(-1)!.action).toEqual({ kind: "cancel", task: 6, subteam: "keep" });
    expect(asked).toEqual([]);
  });
});

describe("member questions (task 55)", () => {
  const asks = task({ id: 15, assignee: "qa", status: "in_progress", attempts: 1, questions: [{ id: "q1", text: "Keep the old field?", at: 1720 }] });

  it("the owner answers a member's question once, as hgs team reply", async () => {
    const f = fake();
    const t = team({ tasks: [asks] });
    expect(openQuestions(t).map((entry) => [entry.task.id, entry.question.id])).toEqual([[15, "q1"]]);
    const actions = actionsOf(f, t);
    expect(await actions.reply(asks, "q1", "   ")).toBeUndefined();
    // Two clicks while the first is on its way: one reply.
    const first = actions.reply(asks, "q1", " Yes, keep it. ");
    const second = actions.reply(asks, "q1", "Yes, keep it.");
    await Promise.all([first, second]);
    expect(f.sent.map((call) => [call.team, call.action])).toEqual([["t1", { kind: "reply", question: "q1", text: "Yes, keep it." }]]);
    expect(f.asked).toEqual([]);
  });

  it("a refusal says why; a decided task's question says it is closed and sends nothing", async () => {
    const f = fake();
    f.refuse = "hgs: Question q1 was already answered";
    const actions = actionsOf(f, team({ tasks: [asks] }));
    await actions.reply(asks, "q1", "Yes");
    expect(actions.notice).toBe("Question q1 was already answered");
    const decided = { ...asks, status: "cancelled" };
    expect(questionClosed(decided)).toBe("Task #15 is decided, so this question is closed; nobody can answer it any more.");
    expect(questionClosed(asks)).toBe("");
    const before = f.sent.length;
    expect(await actions.reply(decided, "q1", "Yes")).toBeUndefined();
    expect(f.sent).toHaveLength(before);
  });
});

describe("Task 196: the phase header's actions", () => {
  const phased = () => team({
    tasks: [task({ id: 1, status: "in_progress", attempts: 1 }), task({ id: 2, phase: 2 }), task({ id: 3, phase: 3 }), task({ id: 4, phase: 3 })],
    phases: [
      { n: 1, title: "Build", state: "running", status: "in_work" },
      { n: 2, title: "Polish", state: "queued", status: "queued" },
      { n: 3, title: "Docs", state: "draft", status: "draft" },
    ],
  });

  it("Discard names the phase's tasks and whether it was accepted; Move sends the neighbour", async () => {
    const f = fake();
    const model = actionsOf(f, phased());
    await model.discardPhase(2);
    await model.movePhase(3, false);
    await model.movePhase(3, true);
    expect(f.sent.map((call) => call.action)).toEqual([
      { kind: "discard_phase", phase: 2, tasks: 1, accepted: true },
      { kind: "move_phase", phase: 3, to: 2, position: "before" },
    ]);
  });

  it("Reject and Send back need a comment; Reject names the revision", async () => {
    const f = fake();
    const model = actionsOf(f, phased());
    await model.rejectPhase(3, "  ");
    await model.sendBackPhase(2, "");
    await model.rejectPhase(3, " Split it ");
    expect(f.sent.map((call) => call.action)).toEqual([{ kind: "reject_phase", phase: 3, comment: "Split it", revision: 7 }]);
  });

  it("Edit phase sends only what changed; a dependency drag rewrites the task's deps", async () => {
    const f = fake();
    const model = actionsOf(f, phased());
    await model.editPhase(3, undefined, []);
    await model.linkDraft({ ...task({ id: 4, phase: 3 }), deps: [{ task: 2, hard: false }] }, 3, true);
    expect(f.sent.map((call) => call.action)).toEqual([
      { kind: "edit_draft", phase: 3, revision: 7, tasks_update: [{ id: 4, deps: [{ task: 2, kind: "soft" }, { task: 3, kind: "hard" }] }] },
    ]);
  });

  // Owner, 2026-11-11: the owner switches a phase's barrier from its menu.
  it("The barrier switch sends only the barrier, at the revision read", async () => {
    const f = fake();
    const model = actionsOf(f, phased());
    await model.phaseBarrier(2, false);
    expect(f.sent.map((call) => call.action)).toEqual([{ kind: "edit_draft", phase: 2, revision: 7, barrier: false }]);
  });

  it("Add task asks after hgs's warning and confirms or declines the pending addition", async () => {
    for (const yes of [true, false]) {
      const f = fake();
      f.yes = yes;
      const send = f.port.send;
      f.port.send = async (call) => {
        const outcome = await send(call);
        const action = call.action as { kind: string; tasks?: unknown[] };
        if (outcome.outcome === "done" && action.kind === "add_to_phase" && action.tasks)
          return { ...outcome, receipt: { status: "warning", pending: "add-9", warning: "Phase 1 is running." } };
        return outcome;
      };
      await actionsOf(f, phased()).addTasks(1, [{ title: "Docs", assignee: "api" }]);
      expect(f.asked.map((asked) => asked.text)).toEqual(["Phase 1 is running."]);
      expect(f.sent.map((call) => call.action)).toEqual([
        { kind: "add_to_phase", phase: 1, tasks: [{ title: "Docs", assignee: "api" }] },
        yes ? { kind: "add_to_phase", phase: 1, confirm: "add-9", confirmation: "add" } : { kind: "add_to_phase", phase: 1, confirm: "add-9", decline: true },
      ]);
    }
  });

  it("a plan edit names the revision the owner saw", async () => {
    const f = fake();
    await actionsOf(f, team({ status: "planning" })).planEdit({ edit: "link", dependency: 1, task: 2, add: true });
    expect(f.sent.map((call) => call.action)).toEqual([{ kind: "plan_edit", revision: 7, change: { edit: "link", dependency: 1, task: 2, add: true } }]);
  });
});

describe("Task 199: the member menu and card", () => {
  it("sends each member action once with its member", async () => {
    const f = fake();
    const model = actionsOf(f, team());
    await model.pauseMember("api");
    await model.drain("api", "Backend", true);
    await model.reportsTo("api", "lead");
    await model.subLead("qa", true);
    await model.scale({ role: "Backend", agent: "codex", count: 2 });
    await model.swap("api", { model: "opus" });
    await model.moveAccount("claude", "native-claude", "work");
    await model.ironRules("api", ["Run the tests"]);
    await model.forceTest("client-core");
    expect(f.sent.map((call) => call.action)).toEqual([
      { kind: "pause_member", member: "api" },
      { kind: "drain", member: "api", role: "Backend", cancel: true },
      { kind: "reports_to", member: "api", parent: "lead" },
      { kind: "sub_lead", member: "qa", sub: true },
      { kind: "scale", role: "Backend", agent: "codex", count: 2 },
      { kind: "swap_member", member: "api", model: "opus" },
      { kind: "move_account", provider: "claude", from_account: "native-claude", to_account: "work" },
      { kind: "settings", settings: {}, members_update: [{ id: "api", iron_rules: ["Run the tests"] }] },
      { kind: "force_test", suite: "client-core" },
    ]);
  });

  it("answers a member's permission with the card's own ids, a deny with its reason, and nothing without ids", async () => {
    const f = fake();
    const model = actionsOf(f, team());
    const question = { question_id: "claude-approval:ab", question_hash: "ab", answer: { question_id: "approval", allow: "1", deny: "3" } };
    await model.answerMember("api", question, true);
    await model.answerMember("api", question, false, " Not in prod ");
    await model.answerMember("api", { question_id: "x" }, true);
    expect(f.sent.map((call) => call.action)).toEqual([
      { kind: "answer", member: "api", question_id: "claude-approval:ab", expected_question_hash: "ab", answers: [{ question_id: "approval", selected_option_ids: ["1"], text: "" }] },
      { kind: "answer", member: "api", question_id: "claude-approval:ab", expected_question_hash: "ab", answers: [{ question_id: "approval", selected_option_ids: ["3"], text: "Not in prod" }] },
    ]);
  });
});

describe("Task 225: the dialogs Qt kept and Tauri lost", () => {
  const finding = {
    id: "f1", from_task: "2", severity: "critical", title: "Crash on empty cart", file: "src/cart.ts", line: 12,
    scenario: "Opening an empty cart crashes", task: "4", state: "open", by: "Lead", at: 1, note: "", proposal: "", phase: 2,
  };

  it("Findings names every phase finding action, asks first, and sends the answer", async () => {
    const t = team({ findings: { items: [finding] } });
    expect(barActions(t, "").map((action) => action.id)).toContain("findings");
    const f = fake();
    await actionsOf(f, t).finding("f1", "fixed", undefined, 4);
    expect(f.asked).toEqual([{ name: "teamFindingActionConfirm", title: "Fixed", text: "Fixed finding \"Crash on empty cart\"?", action: "Fixed" }]);
    expect(f.sent.at(-1)?.action).toEqual({ kind: "finding", finding: "f1", action: "fixed", task: 4 });
  });

  it("Replan sends the proposal without another confirmation", async () => {
    const f = fake();
    await actionsOf(f, team({ findings: { items: [finding] } })).finding("f1", "replan", "Split phase 2");
    expect(f.asked).toEqual([]);
    expect(f.sent.at(-1)?.action).toEqual({ kind: "finding", finding: "f1", action: "replan", proposal: "Split phase 2" });
  });

  it("Next goal is a Done team's action and sends the goal and name", async () => {
    const t = team({ status: "done", next: "" });
    expect(barActions(t, "").map((action) => action.id)).toContain("nextGoal");
    expect(barActions(team({ status: "done", next: "child" }), "").map((action) => action.id)).not.toContain("nextGoal");
    const f = fake();
    await actionsOf(f, t).nextGoal("Ship the next goal", "Orbit 2");
    expect(f.sent.at(-1)?.action).toEqual({ kind: "next_goal", goal: "Ship the next goal", name: "Orbit 2" });
  });

  it("Delegate creates a sub-team from the task", async () => {
    const f = fake();
    await actionsOf(f, team()).createSubteam("Cart fixes", 4);
    expect(f.sent.at(-1)?.action).toEqual({ kind: "create", name: "Cart fixes", parent: { team: "t1", task: 4 } });
  });
});

describe("Task #415: a No report task", () => {
  it("turns the attempt's last reply into the report without a dialog", async () => {
    const f = fake();
    const t = task({ id: 5, status: "in_progress", attempts: 1, no_report: true, attempt_id: "a7" });
    await actionsOf(f, team({ tasks: [t] })).useLastReply(t);
    expect(f.sent.at(-1)?.action).toEqual({ kind: "use_last_reply", task: 5, attempt: "a7" });
    expect(f.asked).toHaveLength(0);
  });

  it("sends nothing when the task has no attempt", async () => {
    const f = fake();
    const t = task({ id: 5, no_report: true });
    expect(await actionsOf(f, team()).useLastReply(t)).toBe("failed");
    expect(f.sent).toHaveLength(0);
  });
});