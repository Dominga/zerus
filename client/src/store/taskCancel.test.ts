import { beforeEach, expect, it, vi } from "vitest";
import { taskItem } from "../ui/screens/tasks-ui";
const fixture = vi.hoisted(() => ({ project: "example", rows: [] as ReturnType<typeof taskItem>[] }));
vi.mock("../bridge", () => ({ cancelTaskExecution: vi.fn().mockResolvedValue(null), taskBoardAction: vi.fn() }));
vi.mock("./tasks", () => ({ tasks: { project: () => fixture.project, tasks: () => fixture.rows } }));
vi.mock("./actions", () => ({ notify: vi.fn() }));
import { cancelTaskExecution } from "../bridge";
import { notify } from "./actions";
import { setWritingTask } from "./taskBoard";
import { canCancel, requestTaskCancellation, taskCancellation, setTaskCancellation, confirmTaskCancellation, PAUSE_REASON } from "./taskCancel";
const task = taskItem({ uuid: "11111111-1111-4111-8111-111111111111", id: "T-example", stage: "active", stage_at: 123, dispatch_id: "dispatch-1", session: { machine: "node-example", host: "", session: "codex/example/worker", session_id: "run_id:worker" } });
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
