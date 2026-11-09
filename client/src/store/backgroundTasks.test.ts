import { describe, expect, it, vi } from "vitest";
vi.mock("../bridge", () => ({ inTauri: () => false }));
import { backgroundTasks, showsCleanupChip } from "./backgroundTasks";
import { emptyCleanup, type CleanupRow, type CleanupView } from "./terminationCleanup";

const row = (over: Partial<CleanupRow>): CleanupRow => ({ id: "job", label: "work", path: "/srv/example/work", machine: "this computer", computer: "",
  branch: "work", size: "1 KiB", status: "ready", reason: "", details: "", can_check: false, ready: false, ...over });
/** The counts follow the rows the way the native view counts them, plus any overflow. */
const view = (rows: CleanupRow[], overflow: { kept?: number; unknown?: number } = {}): CleanupView => ({ ...emptyCleanup(), rows,
  kept: rows.filter((r) => r.status === "kept").length + (overflow.kept ?? 0),
  unknown: rows.filter((r) => r.status === "unknown").length + (overflow.unknown ?? 0) });

const STATUSES: CleanupRow["status"][] = ["removed", "kept", "review", "unknown", "ready", "checking", "removing", "stopping"];
const CHOICES = STATUSES.flatMap((status) => [true, false].map((busy) => ({ status, busy })));
const OVERFLOWS = [{}, { kept: 1 }, { unknown: 1 }, { kept: 2, unknown: 3 }];
/** Every held state: no rows, one row or two rows of any status, running or not, with any overflow. */
const everyView = () => OVERFLOWS.flatMap((overflow) => [
  view([], overflow),
  ...CHOICES.map((a) => view([row(a)], overflow)),
  ...CHOICES.flatMap((a) => CHOICES.map((b) => view([row({ ...a, id: "a" }), row({ ...b, id: "b" })], overflow))),
]);

describe("the running list", () => {
  it("lists exactly the rows native marks busy, with each folder's name and state", () => {
    const tasks = backgroundTasks(view([
      row({ id: "1", label: "one", busy: true, status: "checking", activity: "checking" }),
      row({ id: "2", label: "two", busy: false, status: "kept" }),
      row({ id: "3", label: "three", busy: true, status: "removing", activity: "removing" }),
      row({ id: "4", label: "four", busy: true, status: "stopping", activity: "stopping" }),
      row({ id: "5", label: "five", busy: true, status: "kept", activity: "restoring" }),
      row({ id: "6", label: "six", status: "unknown" }),
    ]));
    expect(tasks.map(({ id, name, state }) => [id, name, state])).toEqual([
      ["1", "one", "Checking…"],
      ["3", "three", "Checking again and removing…"],
      ["4", "four", "Stopping programs…"],
      ["5", "five", "Restoring session…"],
    ]);
  });
  it("takes the state from the activity, so an outcome never reads as finished work", () => {
    const [task] = backgroundTasks(view([row({ busy: true, status: "kept", activity: "checking" })]));
    expect(task.state).toBe("Checking…");
  });
  it("still names a running row whose activity this build doesn't know", () => {
    expect(backgroundTasks(view([row({ busy: true, activity: "phase-from-a-later-build" })]))[0].state).toBe("Working…");
    expect(backgroundTasks(view([row({ busy: true, activity: null })]))[0].state).toBe("Working…");
    expect(backgroundTasks(view([row({ busy: true, activity: "constructor" })]))[0].state).toBe("Working…");
  });
  it("is empty once nothing runs, whatever outcomes remain", () => {
    expect(backgroundTasks(view([row({ status: "kept" }), row({ status: "unknown" })], { kept: 1 }))).toEqual([]);
  });
});

describe("hiding the running chip", () => {
  it("can never hide a kept or unknown result", () => {
    let outcomes = 0;
    for (const current of everyView()) {
      if (current.kept === 0 && current.unknown === 0) continue;
      outcomes++;
      expect(showsCleanupChip(current), JSON.stringify(current)).toBe(true);
    }
    expect(outcomes, "the generator must keep covering every combination").toBe(935);
  });
  it("leaves nothing unreachable: a held row or count is on one chip or the other", () => {
    for (const current of everyView()) {
      const held = current.rows.length > 0 || current.kept > 0 || current.unknown > 0;
      if (held) expect(backgroundTasks(current).length > 0 || showsCleanupChip(current), JSON.stringify(current)).toBe(true);
    }
  });
  it("shows only the running chip while every row runs and no result is held", () => {
    const running = view([row({ busy: true, status: "checking" }), row({ busy: true, status: "removing" })]);
    expect(showsCleanupChip(running)).toBe(false);
    expect(backgroundTasks(running)).toHaveLength(2);
  });
  it("keeps a folder waiting for review on the cleanup chip", () => {
    expect(showsCleanupChip(view([row({ status: "ready" })]))).toBe(true);
    expect(showsCleanupChip(view([]))).toBe(false);
  });
});
