// @vitest-environment happy-dom
import { afterEach, expect, it, vi } from "vitest";
import { createComponent } from "solid-js";
import { render } from "solid-js/web";
import { STAGES } from "./tasks-ui";

const f = vi.hoisted(() => ({ read: vi.fn(), project: { id: "example-project", name: "Example", accessible: true } }));
vi.mock("../../store/tasks", async (original) => {
  const module = await original<typeof import("../../store/tasks")>();
  return { ...module, tasks: module.createTasks(async () => {}, f.read) };
});
vi.mock("../../store/taskContext", async () => {
  const { tasks } = await import("../../store/tasks");
  const { STAGES: stages } = await import("./tasks-ui");
  return {
    taskProject: () => f.project,
    projectSessions: () => [], findTaskSession: () => () => undefined, projectTasks: () => [],
    sessionTaskRows: () => tasks.session.active() ? Object.fromEntries(stages.map((s) => [s, tasks.session.rows(s)])) : undefined,
  };
});
vi.mock("./TaskCreate", () => ({ default: (props: { children: unknown }) => props.children }));
import { tasks } from "../../store/tasks";
import { TasksPanel } from "./Tasks";

let dispose: (() => void) | undefined;
afterEach(() => { dispose?.(); dispose = undefined; tasks.follow(undefined); document.body.replaceChildren(); vi.useRealTimers(); vi.clearAllMocks(); });
const tick = async () => { for (let i = 0; i < 15; i++) await Promise.resolve(); };

it("binds the search field to bounded reads, keeps stage tabs and clears an empty result", async () => {
  vi.useFakeTimers();
  f.read.mockResolvedValue({ tasks: [], page: { next: null }, counts: Object.fromEntries(STAGES.map((s) => [s, 0])) });
  tasks.follow(f.project.id);
  const host = document.createElement("div"); document.body.append(host);
  dispose = render(() => createComponent(TasksPanel, {}), host);
  await tick();
  const input = host.querySelector<HTMLInputElement>('input[aria-label="Search tasks"]')!;
  expect(input).not.toBeNull();
  input.value = "T-aaaa01"; input.dispatchEvent(new Event("input", { bubbles: true }));
  expect(f.read).not.toHaveBeenCalled();
  await vi.advanceTimersByTimeAsync(200); await tick();
  expect(f.read).toHaveBeenCalledWith(f.project.id, "review", 50, null, undefined, "T-aaaa01");
  expect(f.read).toHaveBeenCalledWith(f.project.id, "active", 50, null, undefined, "T-aaaa01");
  expect(host.querySelector(".tk-empty")?.textContent).toContain("No tasks match your search");
  expect(host.querySelectorAll('[role="tab"]')).toHaveLength(6);
  host.querySelector<HTMLButtonElement>('[data-filter="todo"]')!.click(); await tick();
  expect(f.read).toHaveBeenLastCalledWith(f.project.id, "todo", 50, null, undefined, "T-aaaa01");
  host.querySelector<HTMLButtonElement>('[aria-label="Clear task search"]')!.click(); await tick();
  expect(host.querySelector<HTMLInputElement>('input[aria-label="Search tasks"]')!.value).toBe("");
  expect(tasks.session.active()).toBe(false);
  expect(host.querySelector('[data-filter="todo"]')?.getAttribute("aria-selected")).toBe("true");
});
