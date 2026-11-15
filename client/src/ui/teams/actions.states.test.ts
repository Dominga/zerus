import { describe, expect, it } from "vitest";
import { barActions, memberActions, taskButtons, type TaskButton } from "./actions";
import { teamView } from "./fixtures";
import actionKinds from "../../../core/src/teams/fixtures/action-kinds.json";
import type { MemberView, TaskView, TeamView } from "./types";

// Each action is offered only in the states where the store accepts it
// (second guide review, number 14). The rules are the store's operations on
// integrate/teams: team.rs pause (running), resume (paused), stop (common.rs
// active: running or paused), archive (stopped or finished); queue.rs admit
// (not admitted), hold (not held), release (held); decisions.rs accept
// (reported), return (reported, with a holder), cancel (any); members.rs pause
// (active), resume (paused, with a session), stop (active or paused), archive
// (not removed, not the lead). Task and member actions need an active team and
// a live task (not accepted or cancelled).

const TEAM_STATES = ["draft", "planning", "running", "paused", "finished", "stopped", "archived"];
const TASK_STATES = ["draft", "waiting", "pending", "in_progress", "reported", "accepted", "cancelled"];
const MEMBER_STATES = ["not_created", "active", "paused", "removed"];
const ACTIVE = new Set(["running", "paused"]);
const card = (over: Partial<TaskView>): TaskView => ({ ...teamView.groups.flatMap((group) => group.cards)[0], ...over });
const member = (over: Partial<MemberView>): MemberView => ({ ...teamView.members[1], ...over });
const view = (status: string): TeamView => ({ ...teamView, bar: { ...teamView.bar, status } });

/** What the store accepts for a task, by the rules above. */
function expectedTask(team: string, state: string, admitted: boolean, held: boolean, holder: boolean): string[] {
  if (!ACTIVE.has(team) || state === "accepted" || state === "cancelled") return [];
  return [
    ...(admitted ? [] : ["admit"]),
    ...(state === "reported" ? ["accept"] : []),
    ...(state === "reported" && holder ? ["return"] : []),
    held ? "release" : "hold",
    "cancel",
  ];
}

describe("the actions the Team tab offers", () => {
  it("the team bar: by the team's status", () => {
    // This status-only projection has no planning details. Stop stays available;
    // the writer-derived planning tests cover known pause and unknown state.
    const expected: Record<string, string[]> = {
      draft: [], planning: ["stop"], running: ["pause", "stop"], paused: ["resume", "stop"],
      finished: ["archive"], stopped: ["archive"], archived: [],
    };
    for (const status of TEAM_STATES) expect(barActions(view(status)).map((action) => action.id), status).toEqual(expected[status]);
  });

  it("a task: by the team's status, the task's state, admission, hold and holder", () => {
    let cases = 0;
    for (const team of TEAM_STATES) for (const state of TASK_STATES) for (const admitted of [false, true]) for (const held of [false, true]) for (const holder of [false, true]) {
      const task = card({ state, admitted, held, implementer: holder ? "m1" : "" });
      expect(taskButtons(task, team).map((button) => button.id), JSON.stringify({ team, state, admitted, held, holder })).toEqual(expectedTask(team, state, admitted, held, holder));
      cases++;
    }
    expect(cases).toBe(TEAM_STATES.length * TASK_STATES.length * 8);
  });

  it("never offers Return on a task in progress, the store's refusal of the guide review", () => {
    for (const held of [false, true]) expect(taskButtons(card({ state: "in_progress", implementer: "m1", held, admitted: true }), "running").map((button) => button.id)).not.toContain("return");
  });

  it("asks a reason for Return and Cancel only", () => {
    const task = card({ state: "reported", implementer: "m1", admitted: false });
    for (const button of taskButtons(task, "running")) expect(!!button.reason, button.id).toBe(button.id === "return" || button.id === "cancel");
  });

  it("a member: by the team's status, the member's state, its session and the lead", () => {
    for (const team of TEAM_STATES) for (const state of MEMBER_STATES) for (const session of [false, true]) for (const lead of [false, true]) {
      const value = member({ state, lead, session: session ? teamView.members[1].session : null });
      const expected = !ACTIVE.has(team) ? [] : [
        ...(state === "active" ? ["pause_member"] : []),
        ...(state === "paused" && session ? ["resume_member"] : []),
        ...(state === "active" || state === "paused" ? ["stop_member"] : []),
        ...(state !== "removed" && !lead ? ["archive_member"] : []),
      ];
      expect(memberActions(value, team).map((action) => action.id), JSON.stringify({ team, state, session, lead })).toEqual(expected);
    }
  });
});

it("every action the Team tab offers is a kind the core reads (fixtures/action-kinds.json)", () => {
  const offered = new Set<string>();
  for (const status of TEAM_STATES) {
    for (const action of barActions({ ...teamView, bar: { ...teamView.bar, status } } as TeamView)) offered.add(action.id);
    for (const state of TASK_STATES) for (const held of [false, true]) for (const admitted of [false, true]) {
      for (const button of taskButtons(card({ state, held, admitted, implementer: "m1" }), status)) offered.add(button.id);
    }
    for (const state of MEMBER_STATES) for (const lead of [false, true]) {
      for (const action of memberActions({ ...teamView.members[1], state, lead } as MemberView, status)) offered.add(action.id);
    }
  }
  expect(offered.size).toBeGreaterThan(10);
  for (const kind of offered) expect(actionKinds, kind).toContain(kind);
});

// zerus-63rru.29: the rows now carry what the store's three refusals depend on,
// so the view disables and explains those actions before they are sent.
describe("the refusals the rows let the view foresee", () => {
  const reported = (over: Partial<TaskView>) => card({ state: "reported", admitted: true, held: false, implementer: "m1", ...over });
  const byId = (buttons: TaskButton[]) => Object.fromEntries(buttons.map((button) => [button.id, button.blocked ?? ""]));

  it("a pending decision blocks accept, return, release and cancel with its reason; admit and hold stay open", () => {
    const pending = "The task's previous decision has not settled yet.";
    expect(byId(taskButtons(reported({ holder: "m1", pending_decision: "accept" }), "running"))).toEqual({ accept: pending, return: pending, hold: "", cancel: pending });
    expect(byId(taskButtons(card({ state: "pending", admitted: false, held: true, pending_decision: "cancel" }), "running"))).toEqual({ admit: "", release: pending, cancel: pending });
    expect(byId(taskButtons(reported({ holder: "m1", pending_decision: null }), "running"))).toEqual({ accept: "", return: "", hold: "", cancel: "" });
  });

  it("Return follows the row's holder; a row from an older hgs without one keeps the implementer", () => {
    const nobody = "Nobody holds this task's attempt, so there is nobody to return it to.";
    expect(byId(taskButtons(reported({ holder: "m3" }), "running")).return).toBe("");
    expect(byId(taskButtons(reported({ holder: null }), "running")).return, "the store refuses a Return without a holder").toBe(nobody);
    expect(byId(taskButtons(reported({ holder: undefined, implementer: "m1" }), "running")).return).toBe("");
    expect(taskButtons(reported({ holder: undefined, implementer: "" }), "running").map((button) => button.id)).not.toContain("return");
  });

  it("a member the person stopped cannot be stopped again; the person may still resume it", () => {
    const stopped = memberActions({ ...teamView.members[1], state: "paused", person_stopped: true } as MemberView, "running");
    expect(stopped.find((action) => action.id === "resume_member")?.disabled).toBeFalsy();
    const stop = stopped.find((action) => action.id === "stop_member")!;
    expect(stop.disabled).toBe(true);
    expect(stop.reason).toBe("You stopped this member. Resume it to let it work again.");
    const running = memberActions({ ...teamView.members[1], state: "active", person_stopped: false } as MemberView, "running");
    expect(running.find((action) => action.id === "stop_member")?.disabled).toBeFalsy();
  });
});

// zerus-63rru.43: a pending Return or Reassign that has not started can be
// replaced by a new Return, Reassign or Cancel; an acceptance never is, and a
// started decision refuses as before. The row's pending_replaceable says which.
describe("a pending decision that can be replaced", () => {
  const waiting = "The task's previous decision has not settled yet.";
  const replaced = (kind: string) => ({ return: "A new decision here replaces the pending return.", reassign: "A new decision here replaces the pending reassignment." } as Record<string, string>)[kind]
    ?? `A new decision here replaces the pending decision (${kind}).`;
  const looks = (buttons: TaskButton[]) => Object.fromEntries(buttons.map((button) => [button.id, button.blocked ? `blocked: ${button.blocked}` : button.note ? `note: ${button.note}` : "open"]));
  const task = (over: Partial<TaskView>) => card({ state: "reported", admitted: true, held: false, implementer: "m1", holder: "m1", ...over });

  it("keeps Return and Cancel with a line naming what they replace; Accept and Release stay blocked", () => {
    expect(looks(taskButtons(task({ pending_decision: "return", pending_replaceable: true }), "running"))).toEqual({
      accept: `blocked: ${waiting}`, return: `note: ${replaced("return")}`, hold: "open", cancel: `note: ${replaced("return")}`,
    });
    expect(looks(taskButtons(task({ state: "pending", held: true, pending_decision: "reassign", pending_replaceable: true }), "running"))).toEqual({
      release: `blocked: ${waiting}`, cancel: `note: ${replaced("reassign")}`,
    });
    expect(looks(taskButtons(task({ pending_decision: "route", pending_replaceable: true }), "running")).return).toBe(`note: ${replaced("route")}`);
  });

  it("blocks all four when the pending decision cannot be replaced, or the row does not say", () => {
    for (const replaceable of [false, undefined]) {
      expect(looks(taskButtons(task({ pending_decision: "return", pending_replaceable: replaceable }), "running")), String(replaceable)).toEqual({
        accept: `blocked: ${waiting}`, return: `blocked: ${waiting}`, hold: "open", cancel: `blocked: ${waiting}`,
      });
    }
  });

  it("a Return with nobody to go to stays blocked for that reason even when the pending one can be replaced", () => {
    expect(looks(taskButtons(task({ holder: null, pending_decision: "return", pending_replaceable: true }), "running")).return)
      .toBe("blocked: Nobody holds this task's attempt, so there is nobody to return it to.");
  });
});
