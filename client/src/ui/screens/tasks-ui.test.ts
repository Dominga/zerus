import { describe, expect, it } from "vitest";
import { toSession } from "../../store/catalog";
import { STAGES, RECENT_DONE, assignmentLabel, boardRows, boardTask, belongsTo, keepsSessionFilter, taskItem, taskSessions, projectTask, sessionIdentity, sessionLabel, taskPageOffset } from "./tasks-ui";

const session = (extra = {}) => toSession("mac", "run:r1", { name: "codex/sample-project/new-name", run_id: "r1", launch_id: "launch-1", ...extra });
const task = (extra = {}) => taskItem({ uuid: "task-1", stage: "todo", text: "First line\nDescription", target: { host: "mac", session: "codex/sample-project/old-name", session_id: "launch_id:launch-1" }, ...extra });
describe("Qt task board projection", () => {
  it("keeps an older page anchored while newer tasks arrive", () => {
    const previous = Array.from({ length: 100 }, (_, i) => task({ uuid: `task-${i}` }));
    const next = [task({ uuid: "new" }), ...previous];
    expect(taskPageOffset(previous, next, 50)).toBe(51);
    expect(taskPageOffset(previous, next, 0)).toBe(0);
    expect(taskPageOffset(previous, next.filter((t) => t.uuid !== "task-50"), 50)).toBe(51);
  });
  it("keeps Qt's stages, title fallback, nested target and planned worker", () => {
    expect(STAGES).toEqual(["review", "active", "todo", "backlog", "done"]);
    const t = task({ target: { kind: "subagent", parent: { host: "mac", session: session().name, session_id: "launch_id:launch-1" } }, worker: { name: "child" } });
    expect(t.title).toBe("First line"); expect(t.target.name).toBe(session().name); expect(t.worker).toBe("child");
    expect(sessionLabel(t.target)).toBe("new-name @mac");
    expect(assignmentLabel(t)).toBe("Subagent — coordinate through new-name @mac");
    expect(assignmentLabel(task({ stage: "active" }))).toBe("Unconfirmed");
  });
  it("binds the session scope and renamed executors to immutable launch/run identities", () => {
    const s = session();
    expect(sessionIdentity(s)).toBe("launch_id:launch-1"); expect(belongsTo(task(), s)).toBe(true);
    expect(belongsTo(task(), session({ launch_id: "replacement" }))).toBe(false);
    const find = taskSessions([s]);
    expect(projectTask(task(), find).target).toMatchObject({ name: s.name, known: true });
    expect(projectTask(task(), taskSessions([])).target.known).toBe(false);
    expect(taskSessions([s, session({ name: "codex/sample-project/duplicate" })])(task().target)).toBeUndefined();
    expect(sessionIdentity(session({ launch_id: null }))).toBe("run_id:r1");
  });
  it("Only this session includes Any session queued tasks and dispatched own tasks", () => {
    const own = task(), any = task({ uuid: "any", target: { kind: "any" } }), other = task({ uuid: "other", target: { host: "mac", session: "other" } });
    expect(boardRows([own, any, other], "todo", session(), 0).map((t) => t.uuid)).toEqual(["task-1", "any"]);
    const sent = task({ stage: "review", session: { host: "mac", session: "old", session_id: "launch_id:launch-1" }, target: { kind: "any" } });
    expect(belongsTo(sent, session())).toBe(true);
    expect(belongsTo(task({ target: { host: null, session: session().name } }), session())).toBe(false);
  });
  it("Done keeps the last seven days, with older ranks separately preserved", () => {
    const now = 2 * RECENT_DONE;
    const rows = [task({ uuid: "old", stage: "done", stage_at: now - RECENT_DONE - 1 }), task({ uuid: "edge", stage: "done", stage_at: now - RECENT_DONE }), task({ uuid: "new", stage: "done", stage_at: now })];
    expect(boardRows(rows, "done", undefined, now).map((t) => t.uuid)).toEqual(["edge", "new"]);
    expect(boardRows(rows, "done", undefined, now, true).map((t) => t.uuid)).toEqual(["old"]);
  });

  it("finds a board card in the loaded window or in the session's own view (zerus-u68na)", () => {
    const loaded = taskItem({ uuid: "a", stage: "todo" }), viewed = taskItem({ uuid: "b", stage: "todo" });
    const view = Object.fromEntries(STAGES.map((s) => [s, s === "todo" ? [viewed] : []])) as Record<(typeof STAGES)[number], typeof viewed[]>;
    expect(boardTask([loaded], view, "b")).toBe(viewed);
    expect(boardTask([loaded], view, "a")).toBe(loaded);
    expect(boardTask([loaded], undefined, "b")).toBeUndefined();
    expect(boardTask([loaded], view, undefined)).toBeUndefined();
  });

  it("keeps Only this session on for a revealed task the session owns while its stage still loads (zerus-60v37)", () => {
    const empty = Object.fromEntries(STAGES.map((s) => [s, []])) as unknown as Record<(typeof STAGES)[number], ReturnType<typeof task>[]>;
    const anyone = task({ uuid: "any", target: { kind: "any" } });
    const withAnyone = { ...empty, todo: [anyone] };
    // Its own task, before the view's read of To do arrived.
    expect(keepsSessionFilter(task(), session(), empty)).toBe(true);
    // A task for any session the view shows.
    expect(keepsSessionFilter(anyone, session(), withAnyone)).toBe(true);
    // Another session's task leaves the filter.
    expect(keepsSessionFilter(task({ target: { host: "mac", session: "codex/other", session_id: "launch_id:other" } }), session(), withAnyone)).toBe(false);
    // Without the view, membership alone decides, as before.
    expect(keepsSessionFilter(task(), session(), undefined)).toBe(true);
    expect(keepsSessionFilter(anyone, session(), undefined)).toBe(false);
  });
});
