// @vitest-environment happy-dom
import { afterEach, expect, it, vi } from "vitest";
import { createComponent } from "solid-js";
import { render } from "solid-js/web";
import { STAGES } from "./tasks-ui";
import type { Session } from "../sessions/types";
import { toSession } from "../../store/catalog";

const f = vi.hoisted(() => ({
  read: vi.fn(), project: { id: "example-project", name: "Example", accessible: true },
  setSelected: (_session: Session | undefined) => {}, setOnly: (_only: boolean) => {},
}));
vi.mock("../../store/ui", async (original) => {
  const module = await original<typeof import("../../store/ui")>();
  const { createSignal } = await import("solid-js");
  const [selected, setSelected] = createSignal<Session>();
  f.setSelected = setSelected;
  return { ...module, selected };
});
vi.mock("../../store/settings", async (original) => {
  const module = await original<typeof import("../../store/settings")>();
  const { createSignal } = await import("solid-js");
  const [only, setOnly] = createSignal(false);
  f.setOnly = setOnly;
  return { ...module,
    flag: (key: string) => key === "tasks/onlySession" ? only() : module.flag(key),
    setSetting: async (key: string, value: Parameters<typeof module.setSetting>[1]) => key === "tasks/onlySession" ? void setOnly(!!value) : module.setSetting(key, value),
  };
});
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
afterEach(() => { dispose?.(); dispose = undefined; tasks.follow(undefined); f.setSelected(undefined); f.setOnly(false); document.body.replaceChildren(); vi.useRealTimers(); vi.clearAllMocks(); });
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

it("never opens a global search when Only this session has no stable identity", async () => {
  vi.useFakeTimers();
  f.read.mockResolvedValue({ tasks: [], page: { next: null }, counts: Object.fromEntries(STAGES.map((s) => [s, 0])) });
  const legacy = toSession("", "name:codex/example/legacy", { name: "codex/example/legacy", state: "stopped" });
  expect([legacy.launchId, legacy.runId]).toEqual([undefined, undefined]);
  f.setSelected(legacy); f.setOnly(true);
  tasks.follow(f.project.id);
  const host = document.createElement("div"); document.body.append(host);
  dispose = render(() => createComponent(TasksPanel, {}), host);
  await tick();
  const input = host.querySelector<HTMLInputElement>('input[aria-label="Search tasks"]')!;
  input.value = "Needle"; input.dispatchEvent(new Event("input", { bubbles: true }));
  await vi.advanceTimersByTimeAsync(200); await tick();
  expect(f.read, "an unknown session identity must never become a project-wide query").not.toHaveBeenCalled();
  expect(host.querySelectorAll(".tk-card")).toHaveLength(0);
  expect(host.querySelectorAll(".tk-more")).toHaveLength(0);
  expect(host.querySelector('[aria-label="Only this session"]')?.getAttribute("aria-pressed")).toBe("true");
  expect(host.querySelector(".tk-empty")?.textContent).toContain("This session has no stable identity");
  host.querySelector<HTMLButtonElement>('[data-filter="todo"]')!.click(); await tick();
  expect(f.read).not.toHaveBeenCalled();
  expect([...host.querySelectorAll(".tk-tile-count")].map((el) => el.textContent)).toEqual(["0", "0", "0", "0", "0", "0"]);
  // The explicit Only this session toggle can still open a project-wide search.
  host.querySelector<HTMLButtonElement>('[aria-label="Only this session"]')!.click(); await tick();
  expect(f.read).toHaveBeenLastCalledWith(f.project.id, "todo", 50, null, undefined, "Needle");
});

it("drops in-flight global results when an identity-less session filter is selected, then restores identified scope", async () => {
  vi.useFakeTimers();
  const pending: ((page: unknown) => void)[] = [];
  f.read.mockImplementation(() => new Promise((resolve) => pending.push(resolve)));
  tasks.follow(f.project.id);
  const host = document.createElement("div"); document.body.append(host);
  dispose = render(() => createComponent(TasksPanel, {}), host);
  await tick();
  const input = host.querySelector<HTMLInputElement>('input[aria-label="Search tasks"]')!;
  input.value = "Needle"; input.dispatchEvent(new Event("input", { bubbles: true }));
  await vi.advanceTimersByTimeAsync(200); await tick();
  expect(f.read).toHaveBeenCalledTimes(2);
  f.setSelected(toSession("", "name:codex/example/legacy", { name: "codex/example/legacy", state: "stopped" }));
  f.setOnly(true); await tick();
  for (const resolve of pending) resolve({ tasks: [], page: { next: null }, counts: Object.fromEntries(STAGES.map((s) => [s, 123])) });
  await tick();
  expect(f.read).toHaveBeenCalledTimes(2);
  expect(tasks.session.active()).toBe(false);
  expect([...host.querySelectorAll(".tk-tile-count")].map((el) => el.textContent)).toEqual(["0", "0", "0", "0", "0", "0"]);
  expect(host.querySelector(".tk-empty")?.textContent).toContain("This session has no stable identity");
  f.setSelected(toSession("", "launch:identified", { name: "codex/example/identified", launch_id: "identified" }));
  await tick();
  expect(f.read).toHaveBeenCalledWith(f.project.id, "review", 50, null, { host: "", sessionId: "launch_id:identified" }, "Needle");
  expect(f.read).toHaveBeenLastCalledWith(f.project.id, "active", 50, null, { host: "", sessionId: "launch_id:identified" }, "Needle");
});
