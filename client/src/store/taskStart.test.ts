import { beforeEach, expect, it, vi } from "vitest";
import { taskItem } from "../ui/screens/tasks-ui";
import type { Session } from "../ui/sessions/types";
const fixture = vi.hoisted(() => ({ project: "example", rows: [] as ReturnType<typeof taskItem>[], selected: undefined as Session | undefined, offline: false }));
vi.mock("../bridge", () => ({ taskStartOpen: vi.fn(), taskStart: vi.fn(), taskStartRelease: vi.fn() }));
vi.mock("./tasks", () => ({ tasks: { project: () => fixture.project, tasks: () => fixture.rows } }));
vi.mock("./ui", () => ({ selected: () => fixture.selected }));
vi.mock("./taskContext", () => ({ findTaskSession: () => (r: { id: string }) => fixture.selected && r.id === `run_id:${fixture.selected.runId}` ? fixture.selected : undefined }));
vi.mock("./catalog", () => ({ computerOffline: () => fixture.offline }));
vi.mock("./taskNavigation", () => ({ openTask: vi.fn() }));
vi.mock("./actions", () => ({ notify: vi.fn() }));
vi.mock("./notices", () => ({ toast: vi.fn() }));
vi.mock("./taskBoard", async () => { const { createSignal } = await import("solid-js"); const [writingTask, setWritingTask] = createSignal<string>(); return { writingTask, setWritingTask }; });
import { taskStart, taskStartOpen, taskStartRelease } from "../bridge";
import { notify } from "./actions";
import { toast } from "./notices";
import { setWritingTask } from "./taskBoard";
import { createTaskStarts, taskStarts, taskStartBlocker, startTask, releaseTaskStart, setDescriptionEditing, type StartView } from "./taskStart";
const task = taskItem({ uuid: "11111111-1111-4111-8111-111111111111", id: "T-example", stage: "todo", stage_at: 123, target: { kind: "any" } });
const view = (overrides: Partial<StartView> = {}): StartView => ({ project: "example", task: task.uuid, id: "22222222-2222-4222-8222-222222222222", phase: "review", pending: true, session: "codex/example/worker", snapshot: "original", error: "Uncertain", ...overrides });
const tick = async () => { await Promise.resolve(); await Promise.resolve(); await Promise.resolve(); };
beforeEach(async () => {
  taskStarts.follow(); setDescriptionEditing(undefined); setWritingTask(undefined); await tick(); vi.clearAllMocks();
  fixture.project = "example"; fixture.rows = [{ ...task }]; fixture.offline = false;
  fixture.selected = { key: "\nrun:worker", catalogKey: "run:worker", computer: "", name: "codex/example/worker", runId: "worker", conversationId: "conversation", tracked: true, state: "running", processState: "running", actionNeeded: false } as Session;
  vi.mocked(taskStartOpen).mockResolvedValue(view({ pending: false, phase: "", id: "", error: "" }));
  vi.mocked(taskStart).mockResolvedValue(view({ pending: false, phase: "started", outcome: "submitted" }));
});
it("reads only the latest bounded selection and ignores an obsolete open", async () => {
  const reads: string[] = []; let first!: (v: StartView) => void;
  const store = createTaskStarts({ open: async (_, task) => { reads.push(task); return reads.length === 1 ? new Promise<StartView>((yes) => { first = yes; }) : view({ task }); }, start: vi.fn(), release: vi.fn() });
  store.follow("example", "old"); store.follow("example", "middle"); store.follow("example", "latest");
  expect(reads).toEqual(["old"]); first(view({ task: "old" })); await tick();
  expect(reads).toEqual(["old", "latest"]); expect(store.view()?.task).toBe("latest"); store.follow();
});
it("keeps pending state and never submits from a read or refresh", async () => {
  taskStarts.follow("example", task.uuid); await tick();
  vi.mocked(taskStartOpen).mockResolvedValueOnce(view()); taskStarts.refresh(); await tick();
  expect(taskStarts.view()?.pending).toBe(true); expect(taskStart).not.toHaveBeenCalled(); expect(taskStartRelease).not.toHaveBeenCalled();
  expect(taskStartBlocker("example", task)).toBe("the previous start of this task may already have reached its executor.");
});
it("uses Qt blockers for description edits, offline, missing and attention", () => {
  setDescriptionEditing({ project: "example", task: task.uuid }); expect(taskStartBlocker("example", task)).toBe("Save the description before starting this task");
  setDescriptionEditing(undefined); fixture.offline = true; expect(taskStartBlocker("example", task)).toBe("The agent is not running. Resume it before sending.");
  fixture.offline = false; fixture.selected!.actionNeeded = true; expect(taskStartBlocker("example", task)).toContain("agent is waiting for input");
  fixture.selected = undefined; expect(taskStartBlocker("example", task)).toBe("Open the session to send this task to");
});
it("sends one explicit Start with exact task, assignment and executor identities", async () => {
  await startTask("example", task);
  expect(taskStart).toHaveBeenCalledExactlyOnceWith({ project: "example", task: task.uuid, stage_at: 123, assignment: { kind: "any", machine: "", session_id: "", name: "" }, target: { computer: "", name: "codex/example/worker", run_id: "worker", conversation_id: "conversation" }, catalog_key: "run:worker", session_id: "run_id:worker" });
  expect(notify).toHaveBeenCalledWith("info", "Task started.");
});
it("refuses stale stage or assignment and never retries a lost Start response", async () => {
  fixture.rows[0] = { ...task, targetKind: "session" }; await startTask("example", task); expect(taskStart).not.toHaveBeenCalled();
  fixture.rows[0] = { ...task }; vi.mocked(taskStart).mockRejectedValueOnce(new Error("response lost"));
  await startTask("example", task); await tick(); expect(taskStart).toHaveBeenCalledTimes(1);
});
it("settles a receipt or releases the exact reviewed snapshot, with the specified toasts", async () => {
  const review = { task, view: view() };
  vi.mocked(taskStartRelease).mockResolvedValueOnce(view({ pending: false, phase: "started", outcome: "submitted" }));
  expect(await releaseTaskStart(review)).toBe(true); expect(taskStartRelease).toHaveBeenCalledExactlyOnceWith(review.view);
  expect(toast).toHaveBeenCalledWith("Delivery receipt found", "The task reached its executor. Nothing was sent again.");
  vi.mocked(taskStartRelease).mockResolvedValueOnce(view({ pending: false, phase: "released", outcome: "released" }));
  expect(await releaseTaskStart(review)).toBe(true); expect(toast).toHaveBeenCalledWith("Start record released", "Another start is allowed. Nothing was stopped or sent again.");
  vi.mocked(taskStartRelease).mockRejectedValueOnce(new Error("changed")); expect(await releaseTaskStart(review)).toBe(false);
  expect(notify).toHaveBeenCalledWith("error", "Could not release the start record.", true);
});
