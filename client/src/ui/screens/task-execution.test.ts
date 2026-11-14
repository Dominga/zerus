import { expect, it } from "vitest";
import { taskItem } from "./tasks-ui";

const projection = () => ({ requirement: 3, attempts: 1, conflict: true, has_unknown: true, has_unexplained: true,
  current: { id: "11111111-1111-4111-8111-111111111111", generation: 2, origin: "send", requirement: 3,
    request_id: "example-request", phase: "submitted", at: 1234, began_at: null, progress_at: null, legacy: false,
    executor: { machine: "example-machine", session: "codex/orchard/worker", session_id: "run_id:example-run", host: "", resolved: true } } });

it("keeps one typed saved execution in the desktop task model without changing workflow or write permissions", () => {
  const execution = projection();
  const task = taskItem({ uuid: "task", stage: "todo", writable: false, execution });
  expect(task).toMatchObject({ stage: "todo", writable: false, execution });
});

it("does not turn old, future or malformed projection data into a current execution", () => {
  for (const execution of [undefined, {}, { ...projection(), current: { ...projection().current, phase: "future" } },
    { ...projection(), current: { ...projection().current, phase: "running" } }]) {
    expect(taskItem({ execution })).not.toHaveProperty("execution");
  }
});

it("retains no raw native payload or arrays and rejects imprecise integer identities", () => {
  const raw = projection();
  const task = taskItem({ execution: { ...raw, unknown: ["private-history"], current: { ...raw.current, native: "private-input" } } });
  expect(JSON.stringify(task)).not.toContain("private");
  for (const generation of [Number.MAX_SAFE_INTEGER + 1, -1, 0, 1.5]) expect(taskItem({ execution: { ...raw, current: { ...raw.current, generation } } })).not.toHaveProperty("execution");
  expect(taskItem({ execution: { ...raw, has_unknown: "false" } })).not.toHaveProperty("execution");
  for (const field of ["phase", "origin"]) expect(taskItem({ execution: { ...raw, current: { ...raw.current, [field]: [raw.current[field as "phase" | "origin"]] } } })).not.toHaveProperty("execution");
});
