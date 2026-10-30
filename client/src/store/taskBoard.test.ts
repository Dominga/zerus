import { beforeEach, describe, expect, it, vi } from "vitest";
import { taskItem } from "../ui/screens/tasks-ui";
const fixture = vi.hoisted(() => ({ project: "example", rows: [] as ReturnType<typeof taskItem>[] }));
vi.mock("../bridge", () => ({ taskBoardAction: vi.fn().mockResolvedValue(undefined) }));
vi.mock("./tasks", () => ({ tasks: { project: () => fixture.project, tasks: () => fixture.rows } }));
vi.mock("./actions", () => ({ notify: vi.fn() }));
import { taskBoardAction } from "../bridge";
import { notify } from "./actions";
import { actOnTask, boardActions, primaryAction, reorderGap, stageDropAction, setWritingTask } from "./taskBoard";
const task = taskItem({ uuid: "11111111-1111-4111-8111-111111111111", title: "Example", stage: "todo", writable: true });
const anchor = { ...task, uuid: "22222222-2222-4222-8222-222222222222" };
beforeEach(() => { vi.clearAllMocks(); fixture.project = "example"; fixture.rows = [{ ...task }, { ...anchor }]; setWritingTask(undefined); });
describe("ordinary task actions", () => {
  it("keeps Qt's stage-specific menu order and primary actions", () => {
    expect(boardActions("todo")).toEqual(["to_backlog", "top"]);
    expect(boardActions("review")).toEqual(["back_to_todo", "accept"]);
    expect(primaryAction("review")).toBe("accept"); expect(primaryAction("done")).toBe("back_to_todo");
    expect(boardActions("active")).toEqual([]);
    expect(stageDropAction(task, "backlog")).toEqual({ kind: "to_backlog" });
    expect(stageDropAction({ ...task, stage: "review" }, "todo")).toBeUndefined();
    expect(stageDropAction(task, "done")).toBeUndefined();
  });
  it("snapshots task, project and stage in a single explicit call", async () => {
    await actOnTask("example", task, { kind: "to_backlog" });
    expect(taskBoardAction).toHaveBeenCalledExactlyOnceWith("example", task.uuid, "todo", { kind: "to_backlog" });
  });
  it("refuses stale selection, foreign project, readonly and cross-stage anchors", async () => {
    fixture.rows[0].stage = "review"; await actOnTask("example", task, { kind: "top" });
    fixture.rows[0] = { ...task, writable: false }; await actOnTask("example", task, { kind: "top" });
    fixture.rows[0] = { ...task }; fixture.rows[1].stage = "done"; await actOnTask("example", task, { kind: "reorder", anchor: anchor.uuid, before: true });
    fixture.project = "other"; await actOnTask("example", task, { kind: "top" });
    expect(taskBoardAction).not.toHaveBeenCalled(); expect(notify).toHaveBeenCalledTimes(4);
  });
  it("does not queue or replay an uncertain mutation", async () => {
    let reject!: (error: Error) => void;
    vi.mocked(taskBoardAction).mockImplementationOnce(() => new Promise((_, no) => { reject = no; }));
    const first = actOnTask("example", task, { kind: "top" }); await actOnTask("example", task, { kind: "top" });
    expect(taskBoardAction).toHaveBeenCalledTimes(1); reject(new Error("uncertain")); await first; await Promise.resolve();
    expect(taskBoardAction).toHaveBeenCalledTimes(1); expect(notify).toHaveBeenCalledWith("error", "Error: uncertain");
  });
  it("maps visible same-stage gaps and suppresses adjacent no-op drops", () => {
    const rows = [{ uuid: "a", top: 10, height: 40 }, { uuid: "b", top: 55, height: 40 }, { uuid: "c", top: 100, height: 40 }];
    expect(reorderGap(rows, "b", 51)).toBeUndefined(); expect(reorderGap(rows, "b", 90)).toBeUndefined();
    expect(reorderGap(rows, "b", 11)).toEqual({ gap: 0, anchor: "a", before: true });
    expect(reorderGap(rows, "b", 130)).toEqual({ gap: 3, anchor: "c", before: false });
    expect(reorderGap([], "a", 10)).toBeUndefined();
  });
});
