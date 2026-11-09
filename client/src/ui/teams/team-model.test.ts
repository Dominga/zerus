import { describe, expect, it } from "vitest";
import { currentTask, duration, memberProgress, memberStatus, progress, shownMembers, stateLabel, stateTone, teamState, unreadBadge, type MemberLive } from "./team-model";
import { roleColor } from "./roles";
import type { TeamMemberView, TeamTaskView, TeamView } from "../../types";

// The Team tab's member and team states (task 9), as Qt's TeamModel words them
// for the prototype's Planning, Running, Stuck and Done.

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
    title: `Task ${over.id}`, status: "pending", assignee: "dev", phase: 1, deps: [], estimate_minutes: 0, attempts: 0,
    attempt_state: "", decided_by: "", pinned: false, output: [], waits: "", ...over,
  };
}

function team(over: Partial<TeamView> = {}): TeamView {
  return {
    id: "t1", name: "Checkout", status: "running", project: "", folder: "/work/p", goal: "", plan_mode: "standard", lead: "lead",
    revision: 1, created: 1000, updated: 1000, waiting_for_lead: false, pending_phase: null, settings: {},
    members: [member({ id: "lead", role: "Lead", lead: true }), member({ id: "dev" }), member({ id: "qa", role: "Tester", session: null, state: "not_created" })],
    tasks: [], phases: [], proposals: [], attention: [], validation: { errors: [], warnings: [] },
    health: { critical_path: 0, parallelism: 0, warnings: [] }, ready: [], cycle: [], draft: null, truncated: false, ...over,
  };
}

const LIVE = (over: Partial<MemberLive> = {}): MemberLive => ({ offline: false, attention: false, working: false, ...over });
const of = (t: TeamView, id: string) => t.members.find((m) => m.id === id)!;

describe("member states", () => {
  it("Planning: the lead plans, a member is created on approval", () => {
    const planning = team({ status: "planning" });
    expect(memberStatus(planning, of(planning, "lead"))).toMatchObject({ kind: "planning", text: "Planning", tone: "planning", problem: false });
    expect(memberStatus(planning, of(planning, "qa"))).toMatchObject({ kind: "not_created", text: "Created on approval", short: "Planned", tone: "dim" });
    const draft = team({ status: "draft" });
    expect(memberStatus(draft, of(draft, "lead")).text).toBe("Drafting the team");
    expect(memberStatus(draft, of(draft, "qa")).text).toBe("Created with the team");
  });

  it("a Terminal that refuses hand-outs names the count and hgs's reason (task 98)", () => {
    const t = team();
    const dev = { ...of(t, "dev"), input_refusals: 4, input_reason: "terminal has a draft, dialog, or unrecognized prompt", input_raised: true };
    expect(memberStatus(t, dev)).toMatchObject({ kind: "input_blocked", short: "Blocked", tone: "warn", problem: true, icon: "terminal",
      text: "Terminal refused 4 hand-outs: terminal has a draft, dialog, or unrecognized prompt" });
    expect(memberStatus(t, { ...dev, input_refusals: 0 }).kind).toBe("idle");
  });

  it("a member with unread team messages shows unread: N, urgent ones marked (task 134)", () => {
    expect(unreadBadge(member({ id: "dev" }))).toBeNull();
    expect(unreadBadge(member({ id: "dev", unread: 0 }))).toBeNull();
    expect(unreadBadge(member({ id: "dev", unread: 3 }))).toEqual({ text: "unread: 3", urgent: false,
      tip: "3 unread team messages; the member reads them with hgs team inbox" });
    expect(unreadBadge(member({ id: "dev", unread: 2, unread_urgent: 1 }))).toMatchObject({ urgent: true,
      tip: "2 unread team messages, 1 urgent; the member reads them with hgs team inbox" });
  });

  it("Running: working names its task, sending, submitted, idle and a member that starts later", () => {
    const t = team({ tasks: [task({ id: 3, title: "Saved cards API", status: "in_progress", attempt_state: "delivered", attempts: 1 })] });
    const dev = { ...of(t, "dev"), queue: [3] };
    expect(memberStatus(t, dev)).toMatchObject({ kind: "working", text: "Working on #3 Saved cards API", short: "Working", tone: "active", task: 3 });
    expect(currentTask(t, dev)?.id).toBe(3);
    const sending = team({ tasks: [task({ id: 4, status: "assigned", attempt_state: "delivering" })] });
    expect(memberStatus(sending, of(sending, "dev"))).toMatchObject({ kind: "sent", text: "Sending #4 Task 4", tone: "assigned" });
    const submitted = team({ tasks: [task({ id: 5, status: "submitted", attempt_state: "reported" })] });
    expect(memberStatus(submitted, of(submitted, "dev"))).toMatchObject({ kind: "submitted", tone: "done", task: 5 });
    expect(memberStatus(team(), of(team(), "dev"))).toMatchObject({ kind: "idle", text: "Idle", tone: "muted" });
    expect(memberStatus(team(), of(team(), "dev"), LIVE({ working: true })).kind).toBe("working");
    expect(memberStatus(team(), of(team(), "qa"))).toMatchObject({ kind: "not_started", text: "Starts with its first task" });
  });

  it("problems: offline, an answer, an unconfirmed delivery, no report, a failed start, stuck", () => {
    const open = team({ tasks: [task({ id: 6, status: "in_progress", attempt_state: "delivered" })] });
    const dev = of(open, "dev");
    expect(memberStatus(open, dev, LIVE({ offline: true }))).toMatchObject({ kind: "offline", tone: "danger", problem: true, task: 6, icon: "wifi-off" });
    expect(memberStatus(open, dev, LIVE({ attention: true }))).toMatchObject({ kind: "answer", text: "Needs your answer", tone: "warn", problem: true });
    expect(memberStatus(open, { ...dev, question: true }).kind).toBe("answer");
    const unconfirmed = team({ tasks: [task({ id: 6, status: "in_progress", attempt_state: "unconfirmed" })] });
    expect(memberStatus(unconfirmed, of(unconfirmed, "dev"))).toMatchObject({ kind: "unconfirmed", tone: "danger", problem: true });
    expect(memberStatus(open, { ...dev, phase: "no_report" })).toMatchObject({ kind: "no_report", tone: "warn", problem: true });
    // Task 315 (205 F9): the open attempt got no report while the member's phase still says it works (Qt: attempt->noReport).
    const silent = team({ tasks: [task({ id: 268, title: "Finish lore find speed", status: "in_progress", attempt_state: "delivered", no_report: true })] });
    expect(memberStatus(silent, { ...of(silent, "dev"), phase: "working", queue: [268] }))
      .toMatchObject({ kind: "no_report", text: "No report", tone: "warn", problem: true, task: 268 });
    expect(memberStatus(open, { ...dev, error: "hgs: DeepSeek host is still starting" })).toMatchObject({ kind: "failed", text: "Failed to start", problem: true });
    expect(memberStatus(open, { ...dev, stuck: true, minutes_without_progress: 12 })).toMatchObject({ kind: "stuck", text: "No progress for 12 min", problem: true });
    expect(memberStatus(open, { ...dev, state: "paused" })).toMatchObject({ kind: "paused", problem: false });
  });

  it("Done: every member reads done; removed members leave the list, the lead comes first", () => {
    const done = team({ status: "done", members: [member({ id: "dev" }), member({ id: "lead", role: "Lead", lead: true }), member({ id: "old", state: "removed" })] });
    expect(memberStatus(done, of(done, "dev"))).toMatchObject({ kind: "done", tone: "done" });
    expect(shownMembers(done).map((m) => m.id)).toEqual(["lead", "dev"]);
  });
});

describe("team states", () => {
  it("Planning, Paused and Done keep the team's own status", () => {
    for (const status of ["planning", "paused", "done", "draft", "stopped"]) expect(teamState(team({ status }))).toBe(status);
    expect(teamState(team({ tasks: [] }))).toBe("running");
  });

  it("Running while an open task can move: a working member, a report, a ready task with a free member", () => {
    expect(teamState(team({ tasks: [task({ id: 1, status: "in_progress", attempt_state: "delivered" })] }))).toBe("running");
    expect(teamState(team({ tasks: [task({ id: 1, status: "submitted", attempt_state: "reported" })] }))).toBe("running");
    expect(teamState(team({ ready: [1], tasks: [task({ id: 1 })] }))).toBe("running");
    expect(teamState(team({ tasks: [task({ id: 1, status: "in_progress", assignee: "" })] }))).toBe("running");
  });

  it("Stuck when no open task can move without you", () => {
    const offline = team({ tasks: [task({ id: 1, status: "in_progress", attempt_state: "delivered" })] });
    expect(teamState(offline, (m) => LIVE({ offline: m.id === "dev" }))).toBe("stuck");
    expect(teamState(team({ tasks: [task({ id: 1, status: "in_progress", attempt_state: "unconfirmed" })] }))).toBe("stuck");
    // A ready task whose member failed to start, or waits for nothing it can start.
    const failed = team({ ready: [1], tasks: [task({ id: 1 })] });
    failed.members = failed.members.map((m) => (m.id === "dev" ? { ...m, error: "folder" } : m));
    expect(teamState(failed)).toBe("stuck");
    expect(teamState(team({ tasks: [task({ id: 2, deps: [{ task: 1, hard: true }] })] }))).toBe("stuck");
    // Decided and draft tasks never make a team Stuck.
    expect(teamState(team({ tasks: [task({ id: 1, status: "accepted" }), task({ id: 2, status: "draft", phase: 2 })] }))).toBe("running");
  });

  it("labels and tones follow the prototype's badges", () => {
    expect(["planning", "running", "stuck", "done"].map((state) => [stateLabel(state), stateTone(state)])).toEqual([
      ["Planning", "planning"], ["Running", "active"], ["Stuck", "warn"], ["Done", "done"],
    ]);
  });
});

describe("progress and time", () => {
  it("counts accepted, submitted, active and problem tasks, leaving cancelled ones out", () => {
    const t = team({
      tasks: [
        task({ id: 1, status: "accepted" }), task({ id: 2, status: "submitted" }), task({ id: 3, status: "in_progress", attempt_state: "delivered" }),
        task({ id: 4, status: "cancelled" }), task({ id: 5, assignee: "qa" }),
      ],
    });
    expect(progress(t, new Set([3]))).toEqual({ accepted: 1, submitted: 1, active: 1, problems: 1, total: 4 });
    expect(memberProgress(t, of(t, "dev"))).toEqual({ accepted: 1, total: 3 });
  });

  it("reads 12m, 3h 12m, 3d 4h and 2w 3d", () => {
    expect([12 * 60, (3 * 60 + 12) * 60, (3 * 24 + 4) * 3600, (17 * 24) * 3600].map(duration)).toEqual(["12m", "3h 12m", "3d 4h", "2w 3d"]);
  });

  it("gives a role its palette colour, the lead its own, and an unknown role a stable one", () => {
    expect(roleColor("Tester")).toEqual({ dark: "#56c8e0", light: "#137f95" });
    expect(roleColor("Frontend (SolidJS)").dark).toBe("#f08bb4");
    expect(roleColor("Anything", true).dark).toBe("#b79cff");
    expect(roleColor("Designer")).toEqual(roleColor("Designer"));
  });
});
