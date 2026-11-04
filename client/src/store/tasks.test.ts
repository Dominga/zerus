import { describe, expect, it, vi } from "vitest";
import { createComputed, createRoot } from "solid-js";
import { createTasks, WINDOW } from "./tasks";
import { TASK_LIMIT } from "../ui/screens/tasks-ui";
import { emptyBatch, isEmptyBatch, type TasksOut } from "../types";

const row = (uuid = "a", stage = "todo") => ({ uuid, id: `T-${uuid}`, stage, text: `Task ${uuid}` });
const list = (key = "p1", tasks: unknown[] = [row()], extra = {}): TasksOut => ({
  computer: "", key, kind: "list", payload: { project: { id: key, name: "Project" }, tasks, damaged: [], ...extra },
});
const tick = async () => { for (let i = 0; i < 8; i++) await Promise.resolve(); };

describe("project task stream", () => {
  it("only follows the open project and serializes subscriptions through close", async () => {
    let finish!: () => void;
    const read = vi.fn().mockImplementationOnce(() => new Promise<void>((r) => { finish = r; })).mockResolvedValue(undefined);
    const store = createTasks(read);
    store.follow("p1"); store.follow("p1"); store.follow("p2"); store.follow(undefined); await tick();
    expect(read).toHaveBeenCalledExactlyOnceWith("p1");
    finish(); await tick();
    expect(read.mock.calls).toEqual([["p1"], ["p2"], [null]]);
    expect(store.tasks()).toEqual([]);
  });
  it("publishes a whole list in native rank order and computes To do positions", () => {
    const store = createTasks(vi.fn().mockResolvedValue(undefined)); store.follow("p1");
    store.apply([list("p1", [row("b"), row("active", "active"), row("a"), row("b")])]);
    expect(store.tasks().map((t) => [t.uuid, t.queue])).toEqual([["b", 1], ["active", 0], ["a", 2]]);
    expect(store.loaded()).toBe(true);
    store.apply([list("p1", [row("a")])]); expect(store.tasks().map((t) => t.uuid)).toEqual(["a"]);
  });
  it("rejects stale project/computer replies and keeps the known list on failure", () => {
    const store = createTasks(vi.fn().mockResolvedValue(undefined)); store.follow("p1");
    store.apply([list(), list("old"), { ...list(), computer: "peer" }, list("p1", [row("wrong")], { project: { id: "other" } })]);
    expect(store.tasks().map((t) => t.uuid)).toEqual(["a"]);
    store.apply([{ computer: "", key: "p1", kind: "failed", payload: { message: "offline" } }]);
    expect(store.error()).toBe("offline"); expect(store.tasks()).toHaveLength(1);
    store.apply([list("p1", [], { damaged: ["bad"], truncated: true })]);
    expect(store.error()).toBe(""); expect(store.damaged()).toBe(1); expect(store.truncated()).toBe(true);
  });
  it("bounds retained tasks, resets on selection and exposes an ended subscription", () => {
    const store = createTasks(vi.fn().mockResolvedValue(undefined)); store.follow("p1");
    store.apply([list("p1", Array.from({ length: TASK_LIMIT + 1 }, (_, i) => row(String(i))))]);
    expect(store.tasks()).toHaveLength(TASK_LIMIT); expect(store.truncated()).toBe(true);
    store.follow("p2"); expect(store.tasks()).toEqual([]); expect(store.loaded()).toBe(false);
    store.apply([{ computer: "", key: "p2", kind: "ended", payload: null }]);
    expect(store.error()).toContain("subscription ended");
  });
  it("a tasks-only batch is content and cannot be dropped by the frontend pump", () => {
    expect(isEmptyBatch({ ...emptyBatch(), tasks: [list()] })).toBe(false);
  });
  it("retains journal-only revision changes for the selected inspector", () => {
    const store = createTasks(vi.fn().mockResolvedValue(undefined)); store.follow("p1");
    store.apply([list("p1", [{ ...row(), history_revision: "first" }])]);
    expect(store.tasks()[0].historyRevision).toBe("first");
    store.apply([list("p1", [{ ...row(), history_revision: "after-note" }])]);
    expect(store.tasks()[0].historyRevision).toBe("after-note");
  });

});

const windowed = (tasks: unknown[], stages: Record<string, { total: number; next: string | null }>, revision = "r1"): TasksOut =>
  list("p1", tasks, { window: { counts: {}, damaged_total: 0, revision, window: { schema: 1, limit: WINDOW, stages } } });
const todo = (store: ReturnType<typeof createTasks>) => store.tasks().filter((t) => t.stage === "todo").map((t) => t.uuid);
const stagesOf = (todo: { total: number; next: string | null }) => ({
  review: { total: 0, next: null }, active: { total: 0, next: null }, todo, backlog: { total: 1, next: null }, done: { total: 0, next: null },
});

describe("windowed project tasks", () => {
  it("publishes each stage's window in board order with totals and what remains", () => {
    const store = createTasks(vi.fn().mockResolvedValue(undefined), vi.fn()); store.follow("p1");
    store.apply([windowed([row("b1", "backlog"), row("t1"), row("t2")], stagesOf({ total: 120, next: "c1" }))]);
    expect(store.windowed()).toBe(true);
    expect(store.tasks().map((t) => [t.uuid, t.queue])).toEqual([["t1", 1], ["t2", 2], ["b1", 0]]);
    expect([store.total("todo"), store.remaining("todo"), store.remaining("backlog")]).toEqual([120, 118, 0]);
  });
  it("Show more appends the next page once, after the window's cursor", async () => {
    const read = vi.fn().mockResolvedValue({ tasks: [row("t2"), row("t3")], page: { next: "c2" } });
    const store = createTasks(vi.fn().mockResolvedValue(undefined), read); store.follow("p1");
    store.apply([windowed([row("t1"), row("t2")], stagesOf({ total: 5, next: "c1" }))]);
    const first = store.more("todo"), second = store.more("todo");
    expect(store.loading().todo).toBe(true);
    await first; await second;
    expect(read).toHaveBeenCalledExactlyOnceWith("p1", "todo", WINDOW, "c1");
    expect(store.tasks().map((t) => t.uuid)).toEqual(["t1", "t2", "t3"]);
    expect(store.remaining("todo")).toBe(2); expect(store.loading().todo).toBe(false);
  });
  it("a new window reads an extended stage again to its extent, keeping its rows meanwhile", async () => {
    let finish!: (page: unknown) => void;
    const read = vi.fn()
      .mockResolvedValueOnce({ tasks: [row("t3"), row("t4")], page: { next: "c2" } })
      .mockImplementationOnce(() => new Promise((r) => { finish = r; }));
    const store = createTasks(vi.fn().mockResolvedValue(undefined), read); store.follow("p1");
    store.apply([windowed([row("t1"), row("t2")], stagesOf({ total: 6, next: "c1" }))]);
    await store.more("todo");
    store.apply([windowed([row("t0"), row("t1")], stagesOf({ total: 7, next: "c9" }), "r2")]);
    // No collapse while the stage is read again: the window first, then the rows loaded before.
    expect(store.tasks().filter((t) => t.stage === "todo").map((t) => t.uuid)).toEqual(["t0", "t1", "t2", "t3", "t4"]);
    expect(read).toHaveBeenLastCalledWith("p1", "todo", 4, null);
    finish({ tasks: [row("t0"), row("t1"), row("t2"), row("t3")], page: { next: "c3" } }); await tick();
    expect(store.tasks().filter((t) => t.stage === "todo").map((t) => t.uuid)).toEqual(["t0", "t1", "t2", "t3"]);
    expect(store.remaining("todo")).toBe(3);
  });
  it("ends a re-read whose cursor does not advance instead of looping on it", async () => {
    const read = vi.fn()
      .mockResolvedValueOnce({ tasks: [row("t3"), row("t4")], page: { next: "c2" } })
      .mockResolvedValue({ tasks: [row("t0")], page: { next: "stuck" } });
    const store = createTasks(vi.fn().mockResolvedValue(undefined), read); store.follow("p1");
    store.apply([windowed([row("t1"), row("t2")], stagesOf({ total: 6, next: "c1" }))]);
    await store.more("todo");
    store.apply([windowed([row("t0"), row("t1")], stagesOf({ total: 7, next: "c9" }), "r2")]);
    await tick(); await tick();
    expect(read).toHaveBeenCalledTimes(3);
    expect(read).toHaveBeenLastCalledWith("p1", "todo", 3, "stuck");
    expect(store.loading().todo).toBe(false);
  });
  it("a new window ends a Show more in flight, so no page lands after a head it was not read for", async () => {
    let finish!: (page: unknown) => void;
    const read = vi.fn().mockImplementationOnce(() => new Promise((r) => { finish = r; }));
    const store = createTasks(vi.fn().mockResolvedValue(undefined), read); store.follow("p1");
    store.apply([windowed([row("t1"), row("t2")], stagesOf({ total: 5, next: "c2" }))]);
    const pending = store.more("todo");
    // t0 arrives first in To do: the window's cursor now follows t1, not t2.
    store.apply([windowed([row("t0"), row("t1")], stagesOf({ total: 6, next: "c1" }), "r2")]);
    expect(store.loading().todo).toBe(false);
    finish({ tasks: [row("t3"), row("t4")], page: { next: "c4" } }); await pending;
    expect(todo(store)).toEqual(["t0", "t1"]);
    read.mockResolvedValueOnce({ tasks: [row("t2"), row("t3")], page: { next: "c3" } });
    await store.more("todo");
    expect(read).toHaveBeenLastCalledWith("p1", "todo", WINDOW, "c1");
    expect(todo(store)).toEqual(["t0", "t1", "t2", "t3"]);
  });
  it("a task that moved stages is shown once, in the window's stage, while and after its old stage is read again", async () => {
    let fail!: (error: unknown) => void;
    const read = vi.fn()
      .mockResolvedValueOnce({ tasks: [row("t3"), row("t4")], page: { next: "c4" } })
      .mockImplementationOnce(() => new Promise((_, reject) => { fail = reject; }));
    const store = createTasks(vi.fn().mockResolvedValue(undefined), read); store.follow("p1");
    store.apply([windowed([row("t1"), row("t2")], stagesOf({ total: 6, next: "c2" }))]);
    await store.more("todo");
    // t4 is completed: the next window has it first in Done.
    store.apply([windowed([row("t4", "done"), row("t1"), row("t2")], { ...stagesOf({ total: 5, next: "c2" }), done: { total: 1, next: null } }, "r2")]);
    const shown = () => store.tasks().map((t) => [t.uuid, t.stage, t.queue]);
    expect(shown()).toEqual([["t1", "todo", 1], ["t2", "todo", 2], ["t3", "todo", 3], ["t4", "done", 0]]);
    fail(new Error("hgs stopped")); await tick();
    expect(store.pageError()).toContain("hgs stopped");
    expect(shown()).toEqual([["t1", "todo", 1], ["t2", "todo", 2], ["t3", "todo", 3], ["t4", "done", 0]]);
  });
  it("a page read after the window moves a task it shows out of every other stage", async () => {
    const read = vi.fn().mockResolvedValue({ tasks: [row("d1", "todo")], page: { next: null } });
    const store = createTasks(vi.fn().mockResolvedValue(undefined), read); store.follow("p1");
    store.apply([windowed([row("d1", "review"), row("t1")], { ...stagesOf({ total: 2, next: "c1" }), review: { total: 1, next: null } })]);
    await store.more("todo");
    expect(store.tasks().map((t) => [t.uuid, t.stage])).toEqual([["t1", "todo"], ["d1", "todo"]]);
  });
  it("a superseded re-read stops reading", async () => {
    const waiting: ((page: unknown) => void)[] = [];
    const read = vi.fn()
      .mockResolvedValueOnce({ tasks: [row("t3"), row("t4")], page: { next: "c4" } })
      .mockImplementation(() => new Promise((r) => { waiting.push(r); }));
    const store = createTasks(vi.fn().mockResolvedValue(undefined), read); store.follow("p1");
    store.apply([windowed([row("t1"), row("t2")], stagesOf({ total: 9, next: "c2" }))]);
    await store.more("todo");
    store.apply([windowed([row("t1"), row("t2")], stagesOf({ total: 9, next: "c2" }), "r2")]);
    store.apply([windowed([row("t1"), row("t2")], stagesOf({ total: 9, next: "c2" }), "r3")]);
    expect(read).toHaveBeenCalledTimes(3);
    // The first re-read's page comes back short: it does not ask for another.
    waiting[0]({ tasks: [row("t1")], page: { next: "x1" } }); await tick();
    expect(read).toHaveBeenCalledTimes(3);
    waiting[1]({ tasks: [row("t1"), row("t2"), row("t3"), row("t4")], page: { next: "c4" } }); await tick();
    expect(todo(store)).toEqual(["t1", "t2", "t3", "t4"]); expect(store.loading().todo).toBe(false);
  });
  it("drops a page that arrives after another project was chosen, and reports a failed read", async () => {
    let finish!: (page: unknown) => void;
    const read = vi.fn().mockImplementationOnce(() => new Promise((r) => { finish = r; })).mockRejectedValueOnce(new Error("hgs too old"));
    const store = createTasks(vi.fn().mockResolvedValue(undefined), read); store.follow("p1");
    store.apply([windowed([row("t1")], stagesOf({ total: 3, next: "c1" }))]);
    const pending = store.more("todo"); store.follow("p2");
    finish({ tasks: [row("late")], page: { next: null } }); await pending;
    expect(store.tasks()).toEqual([]);
    store.apply([{ ...windowed([row("t1")], stagesOf({ total: 3, next: "c1" })), key: "p2", payload: { ...windowed([row("t1")], stagesOf({ total: 3, next: "c1" })).payload as object, project: { id: "p2" } } }]);
    await store.more("todo");
    expect(store.pageError()).toContain("hgs too old"); expect(store.tasks().map((t) => t.uuid)).toEqual(["t1"]);
  });
  it("a stage's total follows each new whole list from an older hgs", () => {
    const store = createTasks(vi.fn().mockResolvedValue(undefined), vi.fn()); store.follow("p1");
    const seen: number[] = [];
    const dispose = createRoot((dispose) => { createComputed(() => seen.push(store.total("todo"))); return dispose; });
    store.apply([list("p1", [row("a")])]);
    store.apply([list("p1", [row("a"), row("b"), row("c")])]);
    dispose();
    expect(seen.at(-1)).toBe(3);
  });
  it("what remains of a stage follows the rows Show more loads", async () => {
    const read = vi.fn().mockResolvedValue({ tasks: [row("t3"), row("t4")], page: { next: null } });
    const store = createTasks(vi.fn().mockResolvedValue(undefined), read); store.follow("p1");
    store.apply([windowed([row("t1"), row("t2")], stagesOf({ total: 4, next: "c2" }))]);
    const seen: number[] = [];
    const dispose = createRoot((dispose) => { createComputed(() => seen.push(store.remaining("todo"))); return dispose; });
    await store.more("todo");
    dispose();
    expect(seen).toEqual([2, 0]);
  });
  it("a whole list from an older hgs has no window and nothing more to load", () => {
    const store = createTasks(vi.fn().mockResolvedValue(undefined), vi.fn()); store.follow("p1");
    store.apply([list("p1", [row("a"), row("b", "done")])]);
    expect(store.windowed()).toBe(false); expect(store.remaining("todo")).toBe(0); expect(store.total("done")).toBe(1);
  });

  it("finds loaded rows first and keeps a bounded set of tasks read on their own", async () => {
    const { ADOPTED } = await import("./tasks");
    const store = createTasks(vi.fn().mockResolvedValue(undefined), vi.fn()); store.follow("p1");
    store.apply([windowed([row("t1")], stagesOf({ total: 9, next: "c1" }))]);
    const far = { ...store.tasks()[0], uuid: "far", stage: "done" };
    store.adopt(far);
    expect(store.find("t1")?.uuid).toBe("t1"); expect(store.find("far")).toEqual(far);
    for (let i = 0; i < ADOPTED; i++) store.adopt({ ...far, uuid: `x${i}` });
    expect(store.find("far")).toBeUndefined();
    store.follow("p2"); expect(store.find("x1")).toBeUndefined();
  });
});

