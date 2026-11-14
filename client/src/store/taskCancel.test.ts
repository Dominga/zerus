import { beforeEach, expect, it, vi } from "vitest";
import { taskItem } from "../ui/screens/tasks-ui";
const fixture = vi.hoisted(() => ({ project: "example", rows: [] as ReturnType<typeof taskItem>[] }));
vi.mock("../bridge", () => ({ cancelTaskExecution: vi.fn().mockResolvedValue(null), cancelTaskAttempt: vi.fn().mockResolvedValue(undefined), taskBoardAction: vi.fn() }));
vi.mock("./tasks", () => ({ tasks: { project: () => fixture.project, tasks: () => fixture.rows, find: (uuid: string) => fixture.rows.find((t) => t.uuid === uuid) } }));
vi.mock("./actions", () => ({ notify: vi.fn() }));
import { cancelTaskAttempt, cancelTaskExecution } from "../bridge";
import { notify } from "./actions";
import { setWritingTask } from "./taskBoard";
import { canCancel, requestTaskCancellation, requestAttemptCancellation, taskCancellation, setTaskCancellation, confirmTaskCancellation, PAUSE_REASON } from "./taskCancel";
const task = taskItem({ uuid: "11111111-1111-4111-8111-111111111111", id: "T-example", stage: "active", stage_at: 123, dispatch_id: "dispatch-1", session: { machine: "node-example", host: "", session: "codex/example/worker", session_id: "run_id:worker" } });
const attempt = "22222222-2222-4222-8222-222222222222";
beforeEach(() => { vi.clearAllMocks(); fixture.project = "example"; fixture.rows = [{ ...task, executor: { ...task.executor } }]; setTaskCancellation(undefined); setWritingTask(undefined); });
it("opens a pinned confirmation and Keep running makes no call", () => {
  requestTaskCancellation("example", task);
  expect(taskCancellation()?.execution).toEqual({ dispatch_id: "dispatch-1", stage_at: 123, machine: "node-example", session_id: "run_id:worker" });
  expect(cancelTaskExecution).not.toHaveBeenCalled(); setTaskCancellation(undefined);
  expect(cancelTaskExecution).not.toHaveBeenCalled(); expect(PAUSE_REASON).toBe("Pausing an individual task is not available yet");
});
it("rechecks after confirmation and sends the exact identity and reason once", async () => {
  requestTaskCancellation("example", task); await confirmTaskCancellation(" --agent ");
  expect(cancelTaskExecution).toHaveBeenCalledExactlyOnceWith("example", task.uuid, { dispatch_id: "dispatch-1", stage_at: 123, machine: "node-example", session_id: "run_id:worker" }, "--agent");
  expect(taskCancellation()).toBeUndefined(); await confirmTaskCancellation("again"); expect(cancelTaskExecution).toHaveBeenCalledTimes(1);
});
it("refuses changed dispatch, executor, stage, readonly and project", async () => {
  for (const change of [(t: typeof task) => { t.stageAt = 124; }, (t: typeof task) => { t.dispatchId = "later"; }, (t: typeof task) => { t.executor.id = "run_id:later"; }, (t: typeof task) => { t.executor.machine = "other"; }, (t: typeof task) => { t.stage = "review"; }, (t: typeof task) => { t.writable = false; }, () => { fixture.project = "other"; }]) {
    fixture.project = "example"; fixture.rows = [{ ...task, executor: { ...task.executor } }]; requestTaskCancellation("example", task); change(fixture.rows[0]); await confirmTaskCancellation("");
  }
  expect(cancelTaskExecution).not.toHaveBeenCalled(); expect(notify).toHaveBeenCalledTimes(7);
  expect(canCancel({ ...task, dispatchId: "" })).toBe(false);
});
it("unknown or undelivered outcomes are reported, never queued or replayed", async () => {
  vi.mocked(cancelTaskExecution).mockRejectedValueOnce(new Error("uncertain"));
  requestTaskCancellation("example", task); await confirmTaskCancellation(""); await Promise.resolve();
  expect(cancelTaskExecution).toHaveBeenCalledTimes(1); expect(notify).toHaveBeenCalledWith("error", "Error: uncertain");
  vi.mocked(cancelTaskExecution).mockResolvedValueOnce("offline"); requestTaskCancellation("example", task); await confirmTaskCancellation("");
  expect(notify).toHaveBeenCalledWith("error", "Task cancelled. Cancel message not sent: offline");
});
it("confirms one exact unknown attempt in any stage without ordinary cancellation", async () => {
  const held = { ...task, stage: "todo", dispatchId: "", unknownAttempts: [attempt] };
  requestAttemptCancellation("example", held, attempt);
  expect(taskCancellation()?.attempt).toBe(attempt);
  expect(cancelTaskAttempt).not.toHaveBeenCalled();
  setTaskCancellation(undefined); // Keep attempt writes nothing.
  requestAttemptCancellation("example", held, attempt);
  await confirmTaskCancellation(" --agent ");
  await confirmTaskCancellation("again");
  expect(cancelTaskAttempt).toHaveBeenCalledExactlyOnceWith("example", task.uuid, attempt, "--agent");
  expect(cancelTaskExecution).not.toHaveBeenCalled();
  expect(notify).toHaveBeenCalledWith("info", "Attempt abandoned. Delivery remains unknown.");
});
it("refuses unknown-attempt selection changes and never replays an unknown outcome", async () => {
  const held = { ...task, unknownAttempts: [attempt] };
  requestAttemptCancellation("example", held, task.uuid);
  requestAttemptCancellation("example", { ...held, writable: false }, attempt);
  expect(taskCancellation()).toBeUndefined();
  requestAttemptCancellation("example", held, attempt);
  fixture.project = "other";
  await confirmTaskCancellation("");
  expect(cancelTaskAttempt).not.toHaveBeenCalled();
  fixture.project = "example";
  vi.mocked(cancelTaskAttempt).mockRejectedValueOnce(new Error("unknown outcome"));
  requestAttemptCancellation("example", held, attempt);
  await confirmTaskCancellation("");
  await confirmTaskCancellation("again");
  expect(cancelTaskAttempt).toHaveBeenCalledTimes(1);
  expect(cancelTaskExecution).not.toHaveBeenCalled();
  expect(notify).toHaveBeenCalledWith("error", "Error: unknown outcome");
});
