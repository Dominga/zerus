import { beforeEach, describe, expect, it, vi } from "vitest";
import { taskItem } from "../ui/screens/tasks-ui";
import type { Session } from "../ui/sessions/types";
const fixture = vi.hoisted(() => ({ rows: {} as Record<string, Session>, tasks: [] as ReturnType<typeof taskItem>[], project: "example", online: true }));
vi.mock("../bridge", () => ({ assignTask: vi.fn().mockResolvedValue(undefined) }));
vi.mock("./catalog", () => ({ catalog: { rows: fixture.rows }, computerFresh: () => fixture.online }));
vi.mock("./organization", () => ({ groupOf: () => fixture.project }));
vi.mock("./tasks", () => ({ tasks: { project: () => "example", tasks: () => fixture.tasks } }));
vi.mock("./actions", () => ({ notify: vi.fn() }));
import { assignTask } from "../bridge";
import { assign, canAssign, taskDrop, taskDragStart, readTaskDrag, setDraggedTask, TASK_MIME } from "./taskAssignment";
const row = { key: "peer\nrun:one", catalogKey: "run:one", computer: "peer", name: "codex/example/one", runId: "run", conversationId: "conversation", launchId: "launch", tracked: true, state: "running" } as Session;
const task = taskItem({ uuid: "11111111-1111-4111-8111-111111111111", stage: "todo", writable: true, title: "Example" });
beforeEach(() => { vi.clearAllMocks(); fixture.rows[row.key] = { ...row }; fixture.tasks = [task]; fixture.project = "example"; fixture.online = true; });
describe("guarded task assignment", () => {
  it("snapshots exact identities and sends once after a deliberate assignment", async () => {
    await assign("example", task, { ...row });
    expect(assignTask).toHaveBeenCalledExactlyOnceWith("example", task.uuid, { computer: "peer", name: row.name, run_id: "run", conversation_id: "conversation" }, "run:one", "launch_id:launch");
  });
  it("refuses another project, offline, active and changed sessions", async () => {
    expect(canAssign({ ...task, stage: "active" }, "example", row)).toBe(false);
    fixture.project = "other"; await assign("example", task, row); expect(assignTask).not.toHaveBeenCalled();
    fixture.project = "example"; fixture.online = false; await assign("example", task, row); expect(assignTask).not.toHaveBeenCalled();
    fixture.online = true; fixture.rows[row.key].conversationId = "new"; await assign("example", task, row); expect(assignTask).not.toHaveBeenCalled();
  });
  it("does not replay an uncertain outcome or queue a second in-flight write", async () => {
    let reject!: (error: Error) => void;
    vi.mocked(assignTask).mockImplementationOnce(() => new Promise((_, no) => { reject = no; }));
    const first = assign("example", task, row); await assign("example", task, row);
    expect(assignTask).toHaveBeenCalledTimes(1); reject(new Error("uncertain")); await first; await Promise.resolve();
    expect(assignTask).toHaveBeenCalledTimes(1);
  });
  it("rejects a foreign or mismatched drag without a write", () => {
    setDraggedTask({ project: "example", uuid: task.uuid });
    const e = { dataTransfer: { types: [TASK_MIME], getData: () => JSON.stringify({ project: "other", uuid: task.uuid }) }, preventDefault: vi.fn(), stopPropagation: vi.fn() } as unknown as DragEvent;
    taskDrop(e, "example", row); expect(assignTask).not.toHaveBeenCalled();
  });
  it("allows same-stage reordering drags for review tasks without allowing assignment", async () => {
   const review = { ...task, stage: "review" }; fixture.tasks = [review];
   const payload = new Map<string, string>();
   const e = { dataTransfer: { types: [TASK_MIME], setData: (key: string, value: string) => payload.set(key, value), getData: (key: string) => payload.get(key) }, preventDefault: vi.fn(), stopPropagation: vi.fn() } as unknown as DragEvent;
   taskDragStart(e, review); expect(readTaskDrag(e, "example")).toBe(review);
   taskDrop(e, "example", row); await Promise.resolve(); expect(assignTask).not.toHaveBeenCalled();
 });
});
