import { describe, expect, it } from "vitest";
import { accountLabel, focusedTasks, limitText, memberMenu, memberTree, moveAffected, reportsOf, reviewerOf, scaleRoom, waitingForTask, type MembersTeam } from "./members";
import { sideLists } from "./sideLists";
import { forcedWaiting, runStatusLabel, suiteRuns, testStatus, testSuites } from "./planModes";
import type { TeamTestsView } from "../../types";

// Task 199: the side column's members, menu and lists as Qt's TeamWorkspace.

const session = (name: string, host = "") => ({ host, name, run_id: "r", conversation_id: "c" });
const member = (id: string, extra: Record<string, unknown> = {}) => ({
  id, role: id[0].toUpperCase() + id.slice(1), agent: "claude", state: "active", error: "", lead: false, session: session(`claude/x/${id}`), ...extra,
}) as MembersTeam["members"][number];
const team = (extra: Partial<MembersTeam> = {}): MembersTeam => ({
  status: "running", lead: "lead", settings: { max_members: 4 },
  members: [
    member("lead", { lead: true }),
    member("arch", { sub: true }),
    member("api", { parent: "arch" }),
    member("ui", { parent: "arch", agent: "codex" }),
    member("qa", { session: null }),
  ],
  tasks: [{ id: 1, assignee: "api", status: "running" }, { id: 2, assignee: "ui", status: "accepted" }, { id: 3, assignee: "qa", status: "pending" }],
  ...extra,
});
const ids = (items: { id: string }[]) => items.map((item) => item.id);

describe("member tree (spec 5.6)", () => {
  it("puts each member under its sub-lead, the rest under the lead", () => {
    const t = team();
    expect(reviewerOf(t, "api")).toBe("arch");
    expect(reviewerOf(t, "arch")).toBe("lead");
    expect(reviewerOf(t, "lead")).toBe("");
    expect(ids(reportsOf(t, "arch"))).toEqual(["api", "ui"]);
    expect(memberTree(t).map((node) => [node.member.id, node.depth])).toEqual([["lead", 0], ["arch", 1], ["api", 2], ["ui", 2], ["qa", 1]]);
    expect(memberTree(t, new Set(["arch"])).filter((node) => !node.hidden).map((node) => node.member.id)).toEqual(["lead", "arch", "qa"]);
  });

  it("does not follow a parent that is no sub-lead, and ends a cycle", () => {
    const t = team({ members: [member("lead", { lead: true }), member("a", { parent: "b" }), member("b", { parent: "a" })] });
    expect(reviewerOf(t, "a")).toBe("lead");
    expect(memberTree(t).map((node) => node.member.id)).toEqual(["lead", "a", "b"]);
  });
});

describe("member menu (Qt memberMenu)", () => {
  it("offers Qt's items for a member, with Remove off while it has open tasks", () => {
    const t = team();
    const menu = memberMenu(t, t.members[2]);
    expect(ids(menu)).toEqual(["open", "message", "iron_rules", "pause", "relaunch", "scale", "drain", "remove", "reports_to", "sub_lead", "swap", "move_account"]);
    const remove = menu.find((item) => item.id === "remove")!;
    expect(remove.enabled).toBe(false);
    expect(remove.tip).toBe("Has open tasks #1 — reassign or cancel them first");
    expect(menu.find((item) => item.id === "relaunch")!.enabled).toBe(false);
    // Reports to: the lead and sub-leads, never itself or one below it.
    expect(menu.find((item) => item.id === "reports_to")!.choices).toEqual([
      { id: "lead", text: "Lead (coordinator)", checked: false },
      { id: "arch", text: "Arch (sub-lead)", checked: true },
    ]);
  });

  it("keeps a sub-lead with reports from being removed or demoted, and the lead to its own items", () => {
    const t = team();
    const arch = memberMenu(t, t.members[1]);
    expect(arch.find((item) => item.id === "remove")!.tip).toBe("Move its reports to another lead first");
    expect(arch.find((item) => item.id === "sub_lead")).toMatchObject({ text: "Stop being a sub-lead", enabled: false });
    expect(arch.find((item) => item.id === "reports_to")!.choices!.map((choice) => choice.id)).toEqual(["lead"]);
    const lead = memberMenu(t, t.members[0]);
    expect(ids(lead)).toEqual(["open", "message", "iron_rules", "reports_to", "sub_lead", "swap", "move_account"]);
    expect(lead.find((item) => item.id === "reports_to")!.enabled).toBe(false);
  });

  it("relaunches only a failed launch, drains or cancels, and scales within the limit", () => {
    const t = team();
    const failed = memberMenu(t, member("qa", { session: null, error: "launch failed" }));
    expect(failed.find((item) => item.id === "relaunch")!.enabled).toBe(true);
    expect(failed.find((item) => item.id === "move_account")!.tip).toBe("It has no session yet; its account applies when it starts");
    expect(memberMenu(t, member("api", { draining: true, parent: "arch" })).find((item) => item.id === "undrain")).toBeTruthy();
    expect(memberMenu(t, member("api", { state: "paused" })).find((item) => item.id === "resume")).toBeTruthy();
    expect(scaleRoom(t)).toBe(0);
    expect(memberMenu(t, t.members[2]).find((item) => item.id === "scale")).toMatchObject({ enabled: false, tip: "The team has its 4 members; raise its limit first" });
    expect(scaleRoom({ ...t, settings: {} })).toBe(10);
    const kimi = memberMenu(t, member("k", { agent: "kimi" })).find((item) => item.id === "move_account")!;
    expect(kimi).toMatchObject({ enabled: false, tip: "Only Claude and Codex members change account in place" });
    expect(ids(memberMenu({ ...t, status: "done" }, t.members[2]))).toEqual(["open", "message", "iron_rules", "pause", "relaunch", "remove"]);
  });
});

describe("member card facts", () => {
  it("shows the account its session runs on, waits for a task, filters by focus", () => {
    expect(accountLabel(member("a", { account: "work", session_account: "native-claude" }))).toBe("Default account");
    expect(accountLabel(member("a", { account: "work", session: null, session_account: "native-claude" }))).toBe("work");
    expect(accountLabel(member("a", { account: "work" }), { work: "Work account" })).toBe("Work account");
    const t = team();
    expect(moveAffected(t, "claude", "native-claude")).toEqual(["lead", "arch", "api"]);
    expect(waitingForTask({ ...t, tasks: [] }, member("qa", { session: null }))).toBe(true);
    expect(waitingForTask(t, member("qa", { session: null }))).toBe(false);
    expect(focusedTasks(t.tasks, "api").map((task) => task.id)).toEqual([1]);
    expect(focusedTasks(t.tasks, null)).toHaveLength(3);
    const at = (seconds: number) => `t${seconds}`;
    expect(limitText({ limited: { at: 1, resets_at: 0, detail: "" } }, at)).toBe("Usage limit, reset unknown");
    expect(limitText({ limited: { at: 1, resets_at: 50, detail: "" }, limit_resume: true }, at)).toBe("Usage limit: continues after the reset (t50)");
  });
});

describe("side lists (Qt 3617-3700)", () => {
  const base = {
    status: "running", lead: "lead", attention: [] as unknown[], validation: { errors: [], warnings: [] },
    members: [{ id: "lead", role: "Lead", session: session("l"), question: false }, { id: "api", role: "Backend", session: session("a"), question: true }],
    tasks: [{ id: 4, title: "Saved cards", assignee: "api", questions: [{ id: "q1", text: "Which API?", at: 1 }] }],
    phases: [{ n: 1, state: "running", title: "Build" }, { n: 2, state: "draft", title: "Docs" }],
  } as unknown as Parameters<typeof sideLists>[0];

  it("lists no Needs your attention or Needs your answer above Members (task 345)", () => {
    // Every request is in its member's card, the Questions list or the plan.
    const lists = sideLists({ ...base, attention: [{ kind: "lead_stalled", summary: "The lead is stalled", task: 4 }] } as never);
    expect(lists).toEqual([]);
  });

  it("shows plan problems only while the team plans, errors first", () => {
    const planning = { ...base, status: "planning", tasks: [], members: [],
      validation: { errors: [{ kind: "cycle", text: "Cycle", task: 3, member: "" }], warnings: [{ kind: "w", text: "Wide", task: null, member: "" }] } } as never;
    const plan = sideLists(planning).find((list) => list.id === "plan")!;
    expect(plan.note).toBe("1 error, 1 warning");
    expect(plan.rows.map((row) => [row.tone, row.target])).toEqual([["danger", { task: 3 }], ["warn", null]]);
    expect(sideLists(base).some((list) => list.id === "plan")).toBe(false);
  });
});

describe("Tests tab (Qt TeamTests)", () => {
  const run = (id: string, suite: string, status: string) => ({ id, suite, status, asked_by: "", task: 0, at: 0, duration_ms: 1000, recorded_ms: 0, runner: "", excerpt: "", flags: [], reused_from: "", due: false, over_budget: false });
  const tests = { team: "t", next: null, queue: [{ id: "q", suite: "web", asked_by: "You", state: "waiting", task: 0, forced: true, started: 0, seconds: 0, reason: "" }],
    runs: [run("1", "core", "flaky"), run("2", "core", "failed"), run("3", "app", "passed")] } as TeamTestsView;

  it("names a run that passed on retry as Qt does, not as a failure", () => {
    expect(testStatus("flaky")).toBe("flaky");
    expect(runStatusLabel("flaky")).toBe("Passed on retry");
    expect(runStatusLabel("failed")).toBe("Failed");
  });

  it("lists the suites of the queue and the history, with a forced one waiting", () => {
    expect(testSuites(tests)).toEqual(["app", "core", "web"]);
    expect(suiteRuns(tests, "core").map((found) => found.id)).toEqual(["1", "2"]);
    expect(forcedWaiting(tests, "web")).toBe(true);
    expect(forcedWaiting(tests, "core")).toBe(false);
    expect(testSuites(null)).toEqual([]);
  });
});
