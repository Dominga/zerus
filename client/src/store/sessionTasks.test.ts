import { describe, expect, it, vi } from "vitest";
import { createTasks, WINDOW } from "./tasks";
import { UPDATE_FOR_SESSION_PAGES } from "./sessionTasks";
import type { TasksOut } from "../types";

const row = (uuid: string, stage = "todo") => ({ uuid, id: `T-${uuid}`, stage, text: `Task ${uuid}` });
const tick = async () => { for (let i = 0; i < 8; i++) await Promise.resolve(); };
const stages = (todo: { total: number; next: string | null }) => ({
  review: { total: 0, next: null }, active: { total: 0, next: null }, todo, backlog: { total: 0, next: null }, done: { total: 0, next: null },
});
const windowed = (tasks: unknown[], revision = "r1"): TasksOut => ({
  computer: "", key: "p1", kind: "list",
  payload: { project: { id: "p1", name: "Project" }, tasks, damaged: [], window: { counts: {}, damaged_total: 0, revision, window: { schema: 1, limit: WINDOW, stages: stages({ total: 120, next: "w1" }) } } },
});
const counts = (todo: number) => ({ review: 0, active: 1, todo, backlog: 0, done: 0 });
const page = (tasks: unknown[], next: string | null, todo = 3) => ({ tasks, page: { next }, counts: counts(todo) });
const mine = { host: "", sessionId: "launch_id:L1" };
/** A whole first page of the view: hgs returns a short page only at the end. */
const full = Array.from({ length: WINDOW }, (_, i) => row(`m${i + 1}`));
const order = (store: ReturnType<typeof createTasks>) => store.session.rows("todo").map((t) => t.uuid);

describe("one session's view of the task board", () => {
  it("reads only the stages shown, by the session's identity, never the loaded window", async () => {
    const read = vi.fn().mockResolvedValue(page(full, "c2", 70));
    const store = createTasks(vi.fn().mockResolvedValue(undefined), read); store.follow("p1");
    store.apply([windowed([row("t1"), row("m1")])]);
    store.session.show("p1", mine, ["todo"]); await tick();
    expect(read).toHaveBeenCalledExactlyOnceWith("p1", "todo", WINDOW, null, mine);
    expect(store.session.active()).toBe(true);
    expect(order(store)).toEqual(full.map((t) => t.uuid));
    expect([store.session.total("todo"), store.session.total("active"), store.session.remaining("todo")]).toEqual([70, 1, 20]);
    expect([store.session.read("todo"), store.session.read("backlog")]).toEqual([true, false]);
    // The board's own window is unchanged, and a row of the view is found for the write guards.
    expect(store.tasks().map((t) => t.uuid)).toEqual(["t1", "m1"]);
    expect(store.find("m2")?.uuid).toBe("m2");
  });

  it("Show more continues the view after its own cursor", async () => {
    const read = vi.fn().mockResolvedValueOnce(page(full, "c2", 51)).mockResolvedValueOnce(page([row("last")], null, 51));
    const store = createTasks(vi.fn().mockResolvedValue(undefined), read); store.follow("p1");
    store.apply([windowed([row("t1")])]);
    store.session.show("p1", mine, ["todo"]); await tick();
    await store.session.more("todo");
    expect(read).toHaveBeenCalledTimes(2);
    expect(read).toHaveBeenLastCalledWith("p1", "todo", WINDOW, "c2", mine);
    expect(order(store)).toEqual([...full.map((t) => t.uuid), "last"]);
    expect(store.session.remaining("todo")).toBe(0);
    // The list changes: the view is read again to the rows the user loaded.
    read.mockResolvedValueOnce(page([...full, row("last")], null, 51));
    store.apply([windowed([row("t1")], "r2")]); await tick();
    expect(read).toHaveBeenLastCalledWith("p1", "todo", WINDOW + 1, null, mine);
  });

  it("a changed list reads the shown stages again to their extent, once more after a read in flight", async () => {
    const waiting: ((value: unknown) => void)[] = [];
    const read = vi.fn().mockImplementation(() => new Promise((r) => { waiting.push(r); }));
    const store = createTasks(vi.fn().mockResolvedValue(undefined), read); store.follow("p1");
    store.apply([windowed([row("t1")])]);
    store.session.show("p1", mine, ["todo"]);
    // Two changes while the first read is in flight: one more read after it, not two.
    store.apply([windowed([row("t1")], "r2")]); store.apply([windowed([row("t1")], "r3")]);
    expect(read).toHaveBeenCalledTimes(1);
    waiting[0](page([row("m1")], null)); await tick();
    expect(read).toHaveBeenCalledTimes(2);
    waiting[1](page([row("m0"), row("m1")], null, 2)); await tick();
    expect(order(store)).toEqual(["m0", "m1"]); expect(store.session.loading().todo).toBe(false);
    // A stage not shown is read when it is shown, not on every change.
    store.apply([windowed([row("t1")], "r4")]);
    expect(read).toHaveBeenCalledTimes(3);
    store.session.show("p1", mine, ["backlog"]);
    expect(read).toHaveBeenLastCalledWith("p1", "backlog", WINDOW, null, mine);
  });

  it("another session or project starts a new view and drops replies of the old one", async () => {
    let finish!: (value: unknown) => void;
    const read = vi.fn().mockImplementationOnce(() => new Promise((r) => { finish = r; })).mockResolvedValue(page([row("o1")], null, 1));
    const store = createTasks(vi.fn().mockResolvedValue(undefined), read); store.follow("p1");
    store.apply([windowed([row("t1")])]);
    store.session.show("p1", mine, ["todo"]);
    const twin = { host: "", sessionId: "launch_id:L2" };
    store.session.show("p1", twin, ["todo"]); await tick();
    finish(page([row("late")], null)); await tick();
    expect(order(store)).toEqual(["o1"]);
    expect(read).toHaveBeenLastCalledWith("p1", "todo", WINDOW, null, twin);
    store.follow("p2");
    expect([store.session.active(), order(store)]).toEqual([false, []]);
  });

  it("an hgs without session pages hands the board back to its loaded rows", async () => {
    const read = vi.fn().mockRejectedValue(new Error(UPDATE_FOR_SESSION_PAGES));
    const store = createTasks(vi.fn().mockResolvedValue(undefined), read); store.follow("p1");
    store.apply([windowed([row("t1")])]);
    store.session.show("p1", mine, ["todo"]); await tick();
    expect([store.session.active(), store.session.unsupported(), store.session.error()]).toEqual([false, true, ""]);
    store.apply([windowed([row("t1")], "r2")]);
    expect(read).toHaveBeenCalledTimes(1);
  });

  it("reports a failed read and keeps the rows it had", async () => {
    const read = vi.fn().mockResolvedValueOnce(page([row("m1")], null, 1)).mockRejectedValueOnce(new Error("hgs stopped"));
    const store = createTasks(vi.fn().mockResolvedValue(undefined), read); store.follow("p1");
    store.apply([windowed([row("t1")])]);
    store.session.show("p1", mine, ["todo"]); await tick();
    store.apply([windowed([row("t1")], "r2")]); await tick();
    expect(store.session.error()).toContain("hgs stopped");
    expect([store.session.active(), order(store)]).toEqual([true, ["m1"]]);
  });

  it("a reply read before the list changed never overwrites newer totals (zerus-0h8lg)", async () => {
    const waiting: ((value: unknown) => void)[] = [];
    const read = vi.fn().mockImplementation(() => new Promise((r) => { waiting.push(r); }));
    const store = createTasks(vi.fn().mockResolvedValue(undefined), read); store.follow("p1");
    store.apply([windowed([row("t1")])]);
    const counted = (backlog: number, tasks: unknown[] = []) =>
      ({ tasks, page: { next: null }, counts: { review: 0, active: 0, todo: 0, backlog, done: 0 } });
    store.session.show("p1", mine, ["todo"]);
    // The list changes while To do is read, then the board shows Backlog.
    store.apply([windowed([row("t1")], "r2")]);
    store.session.show("p1", mine, ["backlog"]);
    waiting[1](counted(1, [row("b1", "backlog")])); await tick();
    expect(store.session.total("backlog")).toBe(1);
    // The To do read began before the change: its counts are older.
    waiting[0](counted(0)); await tick();
    expect(store.session.total("backlog")).toBe(1);
    expect(store.session.rows("backlog").map((t) => t.uuid)).toEqual(["b1"]);
  });

  it("a board that stops showing the view stops its reads and keeps its rows (zerus-fgyiy)", async () => {
    let finish!: (value: unknown) => void;
    const read = vi.fn().mockResolvedValue(page([row("r1", "review")], null, 1));
    const store = createTasks(vi.fn().mockResolvedValue(undefined), read); store.follow("p1");
    store.apply([windowed([row("t1")])]);
    store.session.show("p1", mine, ["review", "active"]); await tick();
    expect(read).toHaveBeenCalledTimes(2);
    // A read in flight when the board goes away is dropped.
    read.mockImplementationOnce(() => new Promise((r) => { finish = r; }));
    store.apply([windowed([row("t1")], "r2")]);
    store.session.suspend();
    expect(store.session.loading().review).toBe(false);
    finish(page([row("late", "review")], null, 1)); await tick();
    expect(store.session.rows("review").map((t) => t.uuid)).toEqual(["r1"]);
    // Nothing is read while nothing shows the view; its rows stay for the write guards.
    store.apply([windowed([row("t1")], "r3")]); await tick();
    expect(read).toHaveBeenCalledTimes(4);
    expect(store.find("r1")?.uuid).toBe("r1");
    // Shown again: the stages are read afresh.
    store.session.show("p1", mine, ["review", "active"]); await tick();
    expect(read).toHaveBeenCalledTimes(6);
  });
});
