// @vitest-environment happy-dom
import { afterEach, expect, it, vi } from "vitest";
import { createComponent } from "solid-js";
import { render } from "solid-js/web";
import type { TaskDetail } from "../../store/taskDetails";
const state = vi.hoisted(() => ({ detail: undefined as TaskDetail | undefined, error: "", offline: "", project: "orchard" }));
vi.mock("../../bridge", () => ({ onTaskExecutors: async () => () => {}, attachmentPin: vi.fn(), attachmentUnpin: vi.fn(), inTauri: () => false, openTaskAttachment: vi.fn() }));
vi.mock("../../store/ui", () => ({ selected: () => undefined, setMenu: vi.fn() }));
vi.mock("../../store/taskBoard", () => ({ writingTask: () => undefined }));
vi.mock("../../store/taskAssignment", () => ({ openTaskAssignment: vi.fn() }));
vi.mock("../../store/taskExecutors", () => ({ pendingExecutor: () => undefined, taskExecutors: { follow: vi.fn(), refresh: vi.fn() }, setTaskExecutorReview: vi.fn() }));
vi.mock("../../store/taskStart", () => ({ taskStarts: { follow: vi.fn() }, supportsTaskStart: () => false, taskStartBlocker: () => undefined, pendingStart: () => undefined, reviewTaskStart: vi.fn() }));
vi.mock("../../store/taskCancel", () => ({ PAUSE_REASON: "Individual-task pause is unavailable.", requestAttemptCancellation: vi.fn() }));
vi.mock("../../store/taskDetails", () => ({ taskDetails: { data: () => state.detail, error: () => state.error, loading: () => false, select: vi.fn(), refresh: vi.fn(), before: () => undefined } }));
vi.mock("../../store/tasks", () => ({ tasks: { error: () => state.offline, project: () => state.project } }));
vi.mock("../../store/taskFiles", () => ({ taskFileCard: vi.fn(), taskFiles: {} }));
vi.mock("../../store/taskNavigation", () => ({ taskRenameRequest: () => undefined, setTaskRenameRequest: vi.fn(), taskDescriptionRequest: () => undefined, setTaskDescriptionRequest: vi.fn() }));
vi.mock("./TaskWorkflow", () => ({ TaskWorkflow: () => null }));
vi.mock("./TaskTitle", () => ({ TaskTitle: () => null }));
vi.mock("./TaskInput", () => ({ TaskInput: () => null }));
vi.mock("../lib/icons", () => ({ Icon: () => null }));
vi.mock("../sessions/Activity", () => ({ Body: () => null }));
import { TaskInspector } from "./TaskInspector";
import { taskItem } from "./tasks-ui";

let dispose: (() => void) | undefined;
afterEach(() => { dispose?.(); document.body.replaceChildren(); state.error = ""; state.offline = ""; state.project = "orchard"; });
function mount() {
  const task = taskItem({ uuid: "task", id: "T-234567", stage: "active", writable: false });
  state.detail = { ...task, execution: { requirement: 3, attempts: 1, current: null, conflict: false, has_unknown: true, has_unexplained: false },
    files: [], log: [], total: 0, started: false, dispatch: "", unknownAttempts: [] };
  const host = document.createElement("div"); document.body.append(host);
  dispose = render(() => createComponent(TaskInspector, { project: "orchard", task, executorAvailable: false, openExecutor: vi.fn() }), host);
  return host;
}
it("the actual inspector renders the selected detail's execution even when the board row has none", () => {
  const host = mount(), view = host.querySelector('[aria-label="Saved execution"]');
  expect(view).not.toBeNull();expect(view?.textContent).toContain("No current attempt recorded.");
  expect(view?.textContent).toContain("Some attempt delivery is unknown.");expect(view?.querySelectorAll("button,a")).toHaveLength(0);
});
it("a disconnected task subscription marks the retained saved evidence as stale", () => {
  state.offline = "connection ended";
  const host = mount();expect(host.querySelector(".tv-execution")?.textContent).toContain("could not be refreshed");
});

it("another project's subscription failure does not mark the selected detail as stale", () => {
  state.project = "another-project";state.offline = "connection ended";
  const host = mount();expect(host.querySelector(".tv-execution")?.textContent).not.toContain("could not be refreshed");
});
