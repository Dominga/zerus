import { expect, it, vi } from "vitest";
vi.mock("../bridge", () => ({ readTaskSettings: vi.fn(), saveTaskSettings: vi.fn() }));
vi.mock("./actions", () => ({ notify: vi.fn() }));
import { createTaskSettings, settingsChanges, SETTINGS_LIMIT_ERROR, type TaskSettings } from "./taskSettings";
const base: TaskSettings = { task_template: "Task {ref}: {prompt}", cancel_template: "Cancel {ref}", agents_other: true, agent_limit: 0 };
it("reads on open and keeps the captured project through a partial save", async () => {
  const read = vi.fn(async () => ({ ...base })), save = vi.fn(async () => ({ ...base, agent_limit: 3 }));
  const store = createTaskSettings({ read, save }, vi.fn());
  expect(read).not.toHaveBeenCalled(); await store.open("example", "Example");
  expect(read).toHaveBeenCalledExactlyOnceWith("example");
  const current = store.dialog()!;
  expect(await store.save(current, { ...base, agent_limit: 3 })).toBe(true);
  expect(save).toHaveBeenCalledExactlyOnceWith("example", { agent_limit: 3 }); expect(store.dialog()).toBeUndefined();
});
it("Cancel and unchanged Save make no write; invalid limits keep the form", async () => {
  const save = vi.fn(), store = createTaskSettings({ read: async () => ({ ...base }), save }, vi.fn());
  await store.open("example", "Example"); store.close(store.dialog()!); expect(save).not.toHaveBeenCalled();
  await store.open("example", "Example"); const current = store.dialog()!;
  for (const agent_limit of [-1, 1001, 1.5, NaN]) expect(await store.save(current, { ...base, agent_limit })).toBe(false);
  expect(store.error()).toBe(SETTINGS_LIMIT_ERROR); expect(save).not.toHaveBeenCalled();
  expect(await store.save(current, { ...base })).toBe(true); expect(save).not.toHaveBeenCalled();
  expect(settingsChanges(base, { ...base, agents_other: false })).toEqual({ agents_other: false });
});
it("reports a read failure, retains a failed save and never repeats an uncertain write", async () => {
  const report = vi.fn(), read = vi.fn().mockRejectedValueOnce(new Error("unavailable")).mockResolvedValue(base), save = vi.fn().mockRejectedValueOnce(new Error("response lost"));
  const store = createTaskSettings({ read, save }, report);
  await store.open("example", "Example"); expect(store.dialog()).toBeUndefined(); expect(report).toHaveBeenCalledOnce();
  await store.open("example", "Example"); const current = store.dialog()!;
  expect(await store.save(current, { ...base, agent_limit: 2 })).toBe(false);
  expect(store.dialog()).toBe(current); expect(store.error()).toContain("response lost"); expect(save).toHaveBeenCalledOnce();
  await Promise.resolve(); expect(save).toHaveBeenCalledOnce();
});
