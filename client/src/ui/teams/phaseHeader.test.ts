import { describe, expect, it } from "vitest";
import type { TeamPhaseView } from "../../types";
import { acceptedUnstarted, buttonText, menuText, moveTarget, movablePhases, pauseText, phaseHeader, type HeaderTask, type HeaderTeam } from "./phaseHeader";
import { phasePlans } from "./plan";

// Task 196: the phase header by state, as Qt's TeamWorkspace phase header.

const phase = (n: number, state: string, extra: Partial<TeamPhaseView> = {}): TeamPhaseView => ({
  n, title: `Phase ${n}`, state, status: "", done: 0, total: 0, barrier: false, auto: false, acceptance: "", paused: false, health: "", ...extra,
});
const task = (id: number, n: number, status = "pending", attempts = 0, extra: Partial<HeaderTask> = {}): HeaderTask => ({
  id, title: `Task ${id}`, status, phase: n, attempts, ...extra,
});
const team = (phases: TeamPhaseView[], tasks: HeaderTask[], status = "running"): HeaderTeam => ({ status, phases, tasks });
const ids = (items: { id: string }[]) => items.map((item) => item.id);

describe("phase header", () => {
  it("offers Reject and Accept on a draft with its menu, and who proposed it", () => {
    const view = team([phase(1, "running"), phase(2, "draft", { added_by: "Lead", added_at: 1700000000 }), phase(3, "draft")], [task(1, 1, "running", 1), task(2, 2), task(3, 3)]);
    const head = phaseHeader(view, view.phases[1]);
    expect(head.buttons).toEqual(["reject", "accept"]);
    expect(ids(head.menu)).toEqual(["move_earlier", "move_later", "barrier", "owner_acceptance", "discard"]);
    expect(head.menu.find((item) => item.id === "move_earlier")!.enabled).toBe(false);
    expect(head.menu.find((item) => item.id === "move_later")!.enabled).toBe(true);
    expect(head.menu.find((item) => item.id === "owner_acceptance")!.checked).toBe(false);
    expect(head.notes.find((note) => note.id === "proposed")!.text).toMatch(/^proposed by Lead \d\d:\d\d$/);
  });

  it("offers nothing on a draft while the team plans", () => {
    const view = team([phase(2, "draft")], [task(2, 2)], "planning");
    expect(phaseHeader(view, view.phases[0]).buttons).toEqual([]);
    expect(phaseHeader(view, view.phases[0]).menu).toEqual([]);
  });

  it("shows a send-back with Undo until the lead reworks it", () => {
    const view = team([phase(2, "rejected", { rejection: "Split it" })], [task(2, 2)]);
    const head = phaseHeader(view, view.phases[0]);
    expect(head.buttons).toEqual(["undo"]);
    expect(head.notes.find((note) => note.id === "sent_back")!.text).toBe("Sent back: \u201cSplit it\u201d");
    const reworked = { ...view.phases[0], reworked: true };
    expect(phaseHeader(view, reworked).buttons).toEqual([]);
  });

  it("asks the owner to Send back or Accept a phase that awaits acceptance", () => {
    const view = team([phase(2, "awaiting_acceptance")], [task(2, 2, "accepted", 1)]);
    expect(phaseHeader(view, view.phases[0]).buttons).toEqual(["send_back", "accept_phase"]);
  });

  it("offers Add task on a running phase, Healthy N/M and the started phase's menu", () => {
    const view = team([phase(1, "running"), phase(2, "running", { accepted_by_auto: true })],
      [task(1, 1, "accepted", 1), task(2, 2, "running", 1), task(3, 2, "running", 1, { held: true }), task(4, 2, "cancelled")]);
    const head = phaseHeader(view, view.phases[1]);
    expect(head.buttons).toEqual(["add_task"]);
    expect(head.notes.find((note) => note.id === "health")!.text).toBe("Healthy 1/2");
    expect(head.notes.find((note) => note.id === "health")!.tone).toBe("warn");
    expect(head.notes.find((note) => note.id === "by_auto")!.text).toBe("accepted by Auto");
    expect(ids(head.menu)).toEqual(["edit", "move_earlier", "move_later", "pause", "barrier", "owner_acceptance", "return_to_draft", "discard"]);
    // Its work started: Edit, Pause, Return to draft and Discard are off; the
    // barrier is scheduling, so it stays the owner's to change.
    expect(head.menu.filter((item) => item.enabled).map((item) => item.id)).toEqual(["barrier", "owner_acceptance"]);
    // P10: a cancelled task is not owed.
    expect([head.done, head.total]).toEqual([0, 2]);
  });

  it("says when a queued phase starts, and its pause with Remove pause", () => {
    const view = team([phase(1, "running"), phase(2, "queued", { waiting: "Starts after phase 1 is accepted" }),
      phase(3, "queued", { paused: true, pause_owner: true, pause_reason: "after the release" })],
    [task(1, 1, "running", 1), task(2, 2), task(3, 3)]);
    const second = phaseHeader(view, view.phases[1]);
    expect(second.notes.find((note) => note.id === "queued")!.text).toBe("Starts after phase 1 is accepted");
    expect(second.menu.filter((item) => item.enabled).map((item) => item.id)).toEqual(["edit", "move_later", "pause", "barrier", "owner_acceptance", "return_to_draft", "discard"]);
    const third = phaseHeader(view, view.phases[2]);
    expect(third.notes.find((note) => note.id === "queued")!.text).toBe("Starts after phase 2");
    expect(third.notes.find((note) => note.id === "paused")!.text).toBe("Paused by you: after the release");
    expect(ids(third.menu)).toContain("unpause");
    expect(ids(third.menu)).not.toContain("pause");
  });

  it("folds a collapsed phase into one line of its tasks and counts the lead's changes", () => {
    const view = team([phase(2, "draft", { changes: 3 })], [task(4, 2), task(5, 2)]);
    const head = phaseHeader(view, view.phases[0]);
    expect(head.summary).toBe("#4 Task 4, #5 Task 5");
    expect(head.notes.find((note) => note.id === "changes")!.text).toBe("3 changes from the lead");
    expect(phaseHeader(view, { ...view.phases[0], changes: 1 }).notes.find((note) => note.id === "changes")!.text).toBe("1 change from the lead");
  });

  it("moves only drafts and accepted phases whose work has not started", () => {
    const view = team([phase(1, "running"), phase(2, "queued"), phase(3, "running"), phase(4, "draft"), phase(5, "done")],
      [task(1, 1, "running", 1), task(2, 2), task(3, 3, "running", 1), task(4, 4)]);
    expect(acceptedUnstarted(view, 2)).toBe(true);
    expect(acceptedUnstarted(view, 3)).toBe(false);
    expect(movablePhases(view)).toEqual([2, 4]);
    expect(moveTarget(view, 4, false)).toEqual({ to: 2, position: "before" });
    expect(moveTarget(view, 2, true)).toEqual({ to: 4, position: "after" });
    expect(moveTarget(view, 4, true)).toBeUndefined();
    expect(movablePhases({ ...view, status: "done" })).toEqual([]);
  });

  it("names who paused a phase", () => {
    expect(pauseText(phase(2, "queued", { pause_by: "Lead" }))).toBe("Paused by the lead");
    expect(pauseText(phase(2, "queued", { pause_by: "Ops" }))).toBe("Paused by Ops");
  });

  it("words every button and menu item", () => {
    expect(buttonText("reject").text).toBe("Reject\u2026");
    expect(buttonText("add_task").text).toBe("Add task\u2026");
    expect(menuText("return_to_draft")).toBe("Return to draft\u2026");
    expect(menuText("owner_acceptance")).toBe("Requires owner acceptance");
    expect(menuText("barrier")).toBe("Waits for earlier phases (barrier)");
  });

  // Owner, 2026-11-11: the lead set a barrier the owner could neither see in
  // the menu nor remove; an approved phase stayed queued behind it.
  it("shows the barrier as a checked menu item the owner can switch, in a draft and in an accepted phase", () => {
    const view = team([phase(1, "running"), phase(2, "queued", { barrier: true }), phase(3, "draft", { barrier: true }), phase(4, "draft")],
      [task(1, 1, "running", 1), task(2, 2), task(3, 3), task(4, 4)]);
    for (const at of [1, 2]) {
      const item = phaseHeader(view, view.phases[at]).menu.find((found) => found.id === "barrier")!;
      expect([item.enabled, item.checked]).toEqual([true, true]);
    }
    expect(phaseHeader(view, view.phases[3]).menu.find((found) => found.id === "barrier")!.checked).toBe(false);
  });
});

describe("plan counts (P10)", () => {
  it("leaves cancelled tasks out of a phase's total", () => {
    const tasks = [1, 2, 3].map((id) => ({
      id, title: `T${id}`, status: id === 3 ? "cancelled" : "accepted", assignee: "dev", phase: 1, deps: [], estimate_minutes: 0, attempts: 1,
      attempt_state: "", decided_by: "", pinned: false, output: [], waits: "",
    }));
    const plan = phasePlans({ phases: [phase(1, "done")], tasks } as never, { team: "t", nodes: tasks.map((item) => ({ id: item.id, title: item.title, status: item.status, phase: 1, layer: 0, deps: [] })), edges: [], layers: 1 } as never);
    expect([plan[0].phase.done, plan[0].phase.total]).toEqual([2, 2]);
  });
});
