import { describe, expect, it, vi } from "vitest";
import { createTasks } from "./tasks";
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
