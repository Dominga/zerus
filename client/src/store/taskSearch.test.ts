import { describe, expect, it, vi } from "vitest";
import { createSessionView } from "./sessionTasks";
import { createTasks } from "./tasks";

const tick = async () => { for (let i = 0; i < 8; i++) await Promise.resolve(); };
const counts = { review: 0, active: 0, todo: 3, backlog: 1, done: 0 };
// These tests exercise read scopes and cursors without fabricating task rows.
// Matching and actual row paging use TaskDoc writers in tasks/index/tests/search.rs.
const page = (next: string | null = null) => ({ tasks: [], page: { next }, counts });
const mine = { host: "", sessionId: "launch_id:example" };

describe("paged task search", () => {
  it("reads a global search as a bounded page and keeps stage counts", async () => {
    const read = vi.fn().mockResolvedValue(page());
    const view = createSessionView(read, 2, 200, 64);
    view.show("project-1", undefined, ["todo"], "Needle"); await tick();
    expect(read).toHaveBeenCalledExactlyOnceWith("project-1", "todo", 2, null, undefined, "Needle");
    expect(view.active()).toBe(true);
    expect(view.total("backlog")).toBe(1);
    expect(view.remaining("todo")).toBe(0);
  });

  it("resets the search cursor and ignores an old query's late reply", async () => {
    let finish!: (v: unknown) => void;
    const read = vi.fn().mockImplementationOnce(() => new Promise((r) => { finish = r; }))
      .mockResolvedValue({ tasks: [], page: { next: null }, counts: { ...counts, todo: 0 } });
    const view = createSessionView(read, 2, 200, 64);
    view.show("project-1", undefined, ["todo"], "old");
    view.show("project-1", undefined, ["todo"], "missing"); await tick();
    finish(page()); await tick();
    expect(read).toHaveBeenLastCalledWith("project-1", "todo", 2, null, undefined, "missing");
    expect(view.rows("todo")).toEqual([]);
    expect(view.read("todo")).toBe(true);
    expect(view.total("todo")).toBe(0);
    expect(view.remaining("todo")).toBe(0);
  });

  it("keeps the query through stage changes, session filtering and refresh", async () => {
    const read = vi.fn().mockResolvedValue(page());
    const view = createSessionView(read, 2, 200, 64);
    view.show("project-1", mine, ["todo"], "Needle"); await tick();
    view.show("project-1", mine, ["backlog"], "Needle"); await tick();
    expect(read).toHaveBeenLastCalledWith("project-1", "backlog", 2, null, mine, "Needle");
    view.changed(); await tick();
    expect(read).toHaveBeenCalledTimes(3);
    view.show("project-1", undefined, ["todo"], "Needle"); await tick();
    expect(read).toHaveBeenLastCalledWith("project-1", "todo", 2, null, undefined, "Needle");
    view.show("project-1", undefined, ["todo"], "");
    expect(view.active()).toBe(false);
  });

  it("forwards the query through the task store to the bridge", async () => {
    const read = vi.fn().mockResolvedValue(page());
    const store = createTasks(vi.fn().mockResolvedValue(undefined), read);
    store.follow("project-1");
    store.session.show("project-1", mine, ["todo"], "Needle"); await tick();
    expect(read).toHaveBeenCalledExactlyOnceWith("project-1", "todo", 50, null, mine, "Needle");
  });

  it("keeps a failed search active rather than displaying unfiltered rows from an older peer", async () => {
    const read = vi.fn().mockRejectedValue(new Error("Update hgs to read one session's tasks."));
    const view = createSessionView(read, 2, 200, 64);
    view.show("project-1", mine, ["todo"], "Needle"); await tick();
    expect(view.active()).toBe(true);
    expect(view.unsupported()).toBe(false);
    expect(view.error()).toContain("Update hgs");
    expect(view.rows("todo")).toEqual([]);
  });
});
