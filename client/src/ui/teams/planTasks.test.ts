import { describe, expect, it } from "vitest";
import type { TeamTaskView, TeamView } from "../../types";
import { activityText, boardColumn, boardKeys, matches, phaseFinished, problemTasks, taskState, taskTime, waitText } from "./planTasks";
import type { MemberStatus } from "./team-model";

// Task 197 (P2, P7, P8): what the plan says about each task, as Qt words it.

function task(id: number, status: string, extra: Partial<TeamTaskView> = {}): TeamTaskView {
  return { id, title: `Task ${id}`, status, assignee: "dev", phase: 1, deps: [], estimate_minutes: 0, attempts: 0, attempt_state: "",
    decided_by: "", pinned: false, output: [], waits: "", ...extra };
}

function team(tasks: TeamTaskView[]): TeamView {
  return { id: "t", name: "T", status: "running", project: "", folder: "", goal: "", plan_mode: "", lead: "lead", revision: 1, created: 0, updated: 0,
    waiting_for_lead: false, pending_phase: null, settings: {}, members: [{ id: "dev", role: "Backend", agent: "codex" } as never], tasks, phases: [],
    proposals: [], attention: [], validation: { errors: [], warnings: [] }, health: { critical_path: 0, parallelism: 0, warnings: [] },
    ready: [], cycle: [], draft: null, truncated: false };
}

describe("planTasks", () => {
  it("words each status and marks a task's own problem (Qt taskStatus)", () => {
    expect(taskState(task(1, "in_progress", { attempt_state: "unconfirmed" }))).toMatchObject({ text: "Delivery unconfirmed", tone: "danger", problem: true });
    expect(taskState(task(1, "in_progress", { attempt_state: "delivered", no_report: true }))).toMatchObject({ text: "No report", problem: true });
    expect(taskState(task(1, "draft")).tone).toBe("dim");
    expect(taskState(task(1, "assigned")).tone).toBe("assigned");
    // Task 342 (owner, 2026-11-08): what waits for a decision is in review, a
    // blue of its own, not the done green of a task nobody accepted yet.
    expect(taskState(task(1, "submitted")).tone).toBe("review");
    expect(taskState(task(1, "in_review")).tone).toBe("review");
    expect(taskState(task(1, "in_test")).tone).toBe("review");
    expect(taskState(task(1, "accepted")).tone).toBe("done");
    expect(taskState(task(1, "in_progress")).tone).toBe("active");
    expect(taskState(task(1, "cancelled")).tone).toBe("cancelled");
    expect(taskState(task(1, "rejected")).tone).toBe("danger");
  });

  it("names a problem task with its member's reason first (Qt problemTexts)", () => {
    const tasks = [task(1, "in_progress"), task(2, "in_progress", { attempt_state: "unconfirmed" })];
    const statuses = new Map<string, MemberStatus>([["dev", { kind: "offline", text: "Offline", short: "Offline", tone: "danger", task: 1, problem: true, icon: "wifi-off" }]]);
    const problems = problemTasks(team(tasks), statuses);
    expect(problems.get(1)).toEqual({ tone: "danger", text: "Offline" });
    expect(problems.get(2)?.text).toBe("Delivery unconfirmed");
  });

  it("files every status in a column, so assigned and draft tasks never vanish (Qt column)", () => {
    expect(boardColumn(task(1, "assigned"), false)).toBe("active");
    expect(boardColumn(task(1, "draft"), false)).toBe("pending");
    expect(boardColumn(task(1, "something_new"), false)).toBe("pending");
    expect(boardColumn(task(1, "cancelled"), false)).toBe("decided");
    expect(boardColumn(task(1, "accepted"), true)).toBe("problems");
    const keys = boardKeys(team([task(1, "accepted"), task(2, "rejected", { pipeline: "review" })]), new Map());
    expect(keys).toEqual(["pending", "active", "problems", "submitted", "in_review", "in_test", "accepted", "decided"]);
  });

  it("says what a card waits for and how long it worked against its estimate", () => {
    const waiting = task(3, "pending", { wait_for: "deps", wait_on: ["1", "#4"], estimate_minutes: 45, estimate_source: "lead" });
    expect(waitText(waiting)).toBe("after #1, #4");
    expect(`${activityText(waiting, false)} ${taskTime(waiting, 0).text}`).toBe("after #1, #4 ~45 min");
    const idle = task(4, "in_progress", { attempts: 2, attempt_begun: true, running_since: 1000, estimate_minutes: 30, estimate_source: "role" });
    expect(`${activityText(idle, false)} ${taskTime(idle, 1000 + 12 * 60).text}`).toBe("attempt 2, idle 12 of ~30 min");
    expect(activityText(idle, true)).toBe("attempt 2, working");
    const accepted = task(5, "accepted", { worked_s: 40 * 60, estimate_minutes: 45, estimate_source: "team" });
    expect(`${activityText(accepted, false)} ${taskTime(accepted, 0).text}`).toBe("accepted 40 of ~45 min");
    const over = taskTime(task(6, "in_progress", { worked_s: 50 * 60, estimate_minutes: 30, estimate_source: "lead" }), 0);
    expect(over).toEqual({ text: "50 of ~30 min", over: true });
    expect(taskTime(task(7, "in_progress", { worked_s: 90 }), 0).text).toBe("1m");
    expect(activityText(task(8, "in_progress", { attempts: 1 }), false)).toBe("starting");
  });

  it("filters by state and member, and finds a phase with every task decided", () => {
    const problems = new Map([[2, true]]);
    expect(matches(task(1, "pending"), "active", problems, "", false)).toBe(false);
    expect(matches(task(1, "assigned"), "active", problems, "", false)).toBe(true);
    expect(matches(task(2, "pending"), "problems", problems, "", false)).toBe(true);
    expect(matches(task(1, "pending"), "all", problems, "qa", false)).toBe(false);
    expect(matches(task(1, "pending"), "problems", problems, "", true)).toBe(true);
    expect(phaseFinished([task(1, "accepted"), task(2, "cancelled")])).toBe(true);
    expect(phaseFinished([task(1, "accepted"), task(2, "pending")])).toBe(false);
    expect(phaseFinished([])).toBe(false);
  });
});
