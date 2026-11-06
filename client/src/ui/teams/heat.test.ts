import { describe, expect, it } from "vitest";
import type { TeamProfileLateness, TeamProfileView } from "../../types";
import { durationText, LATE_SCALE_S, taskHeat } from "./heat";

const late = (start: number, inherited: number, from: number | null, extra: Partial<TeamProfileLateness> = {}): TeamProfileLateness => ({
  known: true, unknown: "", planned_start: null, planned_end: null, actual_start: null, actual_end: null,
  start_late_s: start, inherited_s: inherited, own_s: start - inherited, from, ...extra,
});

// Task 82: the owner's case. #1 ran 2h 20m late by its own; #2 waits on it
// and only inherits 1h 16m; #3 waits on #4, which runs on schedule; #5 is a
// later layer still before its planned start; #6 has no plan baseline.
const profile: TeamProfileView = {
  team: "t", elapsed_s: null, worked_s: null, tasks: 6, accepted: 1, tokens: null, tokens_partial: false,
  losses: [
    { kind: "lead", task: 1, seconds: 7200, tokens: null },
    { kind: "blocked_chain", task: 1, seconds: 4560, tokens: null },
    { kind: "rework", task: 1, seconds: 2400, tokens: null },
    { kind: "member_wait", task: 1, seconds: 60, tokens: null },
  ],
  roles: [],
  task_rows: [
    { task: 1, lateness: late(8400 + 4560, 4560, 9), overrun: { started: true, known: true, estimate_s: 3600, work_s: 6120, overrun_s: 2520, ratio: 1.7, running: false } },
    { task: 2, lateness: late(4560, 4560, 1), overrun: { started: true, known: true, estimate_s: 3600, work_s: 2880, overrun_s: -720, ratio: 0.8, running: true } },
    { task: 3, lateness: late(0, 0, null), overrun: { started: false, known: false, estimate_s: 0, work_s: 0, overrun_s: 0, ratio: 0, running: false } },
    { task: 4, lateness: late(0, 0, null), overrun: { started: true, known: false, estimate_s: 0, work_s: 600, overrun_s: 0, ratio: 0, running: true } },
    { task: 5, lateness: late(0, 0, null, { planned_start: 2_000_000_000 }), overrun: { started: false, known: false, estimate_s: 0, work_s: 0, overrun_s: 0, ratio: 0, running: false } },
    { task: 6, lateness: null, overrun: null },
    { task: 7, lateness: { ...late(0, 0, null), known: false, unknown: "estimate missing" }, overrun: null },
  ],
  task_rows_more: false,
};

describe("lateness heat (task 82)", () => {
  it("tints own lateness against the 2 h scale and names the inherited part", () => {
    const one = taskHeat(profile, 1)!;
    expect(one.label).toBe("late 2h 20m (1h 16m from #9)");
    expect(one.muted).toBe(false);
    expect(one.tint).toBe(1);
    expect(taskHeat({ ...profile, task_rows: [{ task: 1, lateness: late(3600, 0, null), overrun: null }] }, 1)!.tint).toBe(3600 / LATE_SCALE_S);
  });

  it("shows inherited lateness muted with its source, untinted", () => {
    const two = taskHeat(profile, 2)!;
    expect(two.label).toBe("late 1h 16m from #1");
    expect(two.muted).toBe(true);
    expect(two.tint).toBe(0);
  });

  it("leaves a task waiting on on-schedule dependencies, and a later layer before its start, cold", () => {
    for (const id of [3, 5]) {
      const heat = taskHeat(profile, id)!;
      expect(heat.label).toBe("on time");
      expect(heat.tint).toBe(0);
      expect(heat.muted).toBe(true);
      expect(heat.badge).toBe("");
      expect(heat.tip).toContain("On schedule");
      expect(heat.tip).toContain("Overrun: not started");
    }
  });

  it("puts the overrun in its own badge with its own scale", () => {
    const one = taskHeat(profile, 1)!;
    expect(one.badge).toBe("+42m over 1h 00m");
    expect(one.badgeCompact).toBe("x1.7");
    expect(one.over).toBeCloseTo(0.7);
    const two = taskHeat(profile, 2)!;
    expect(two.badge).toBe("within 1h 00m");
    expect(two.over).toBe(0);
    expect(taskHeat(profile, 4)!.badge).toBe("no estimate");
  });

  it("lists the planned and actual moments, the parts of the lateness and the causes", () => {
    const tip = taskHeat(profile, 1)!.tip.split("\n");
    expect(tip[0]).toBe("Planned: start unknown, end unknown");
    expect(tip[1]).toBe("Actual: start not yet, end not yet");
    expect(tip[2]).toBe("Late 3h 36m: own 2h 20m, 1h 16m from #9 (it ran late)");
    expect(tip[3]).toBe("Overrun: own work 1h 42m against a 1h 00m estimate (x1.7)");
    expect(tip[4]).toMatch(/^Causes: .+ 2h 00m; .+ 1h 16m; .+ 40m$/);
  });

  it("says when a task has no baseline or an unknown schedule", () => {
    const six = taskHeat(profile, 6)!;
    expect(six.label).toBe("");
    expect(six.tip).toContain("No plan baseline");
    const seven = taskHeat(profile, 7)!;
    expect(seven.label).toBe("lateness unknown");
    expect(seven.tint).toBe(0);
    expect(taskHeat(profile, 99)).toBeNull();
    expect(taskHeat(null, 1)).toBeNull();
  });

  it("words durations as the Qt GUI does", () => {
    expect(durationText(30)).toBe("<1m");
    expect(durationText(2400)).toBe("40m");
    expect(durationText(3900)).toBe("1h 05m");
  });
});
