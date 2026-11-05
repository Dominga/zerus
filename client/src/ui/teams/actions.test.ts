import { describe, expect, it } from "vitest";
import {
  TeamActions, allDecided, barActions, barChips, discardConfirmation, finishSummary, formAction, formText, moreItems, moveTargets,
  openQuestions, pauseConfirmation, planApproval, questionClosed, retryConfirmation, sendBackMessage, stageName, stopConfirmation, subtreeImpact, taskButtons,
  type ActTask, type ActTeam,
} from "./actions";
import type { DecisionPort } from "./decisions";
import type { TeamCall, TeamConfirmation } from "../../types";

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
    expect(ids(team({ status: "done" }))).toEqual(["archive"]);
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
