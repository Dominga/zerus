// @vitest-environment happy-dom
// The New task form's team chooser: while Teams are on and the project has a
// team, the form offers one; choosing it creates the card in the team's queue
// (`task add --team`, no session target).
import { afterEach, expect, it, vi } from "vitest";
import { createComponent } from "solid-js";
import { render } from "solid-js/web";
const f = vi.hoisted(() => ({
  create: vi.fn(async () => true),
  follow: vi.fn(), edit: vi.fn(), flush: vi.fn(async () => {}),
  rows: [] as { id: string; project: string; name: string; status: string }[],
  teams: (() => true) as () => boolean,
}));
const TEAM = "33333333-3333-4333-8333-333333333333";
vi.mock("../../store/taskForms", () => ({
  createCard: f.create,
  newCard: {
    follow: f.follow, locked: () => false, opening: () => false, busy: () => false,
    view: () => ({ project: "example-project", text: "Task title", handle: 1, blocked: false, read_only: false, recovery: false, error: "" }),
    edit: f.edit, flush: f.flush, error: () => "",
  },
}));
vi.mock("../../store/ui", () => ({ selected: () => undefined }));
vi.mock("../../store/taskNavigation", () => ({ requestTaskDescription: vi.fn() }));
vi.mock("../../store/teams", () => ({
  teams: {
    available: () => f.teams(),
    list: () => ({ teams: f.rows, truncated: false, next: null }),
  },
}));
import TaskCreate from "./TaskCreate";

let dispose: (() => void) | undefined;
afterEach(() => { dispose?.(); dispose = undefined; document.body.replaceChildren(); vi.clearAllMocks(); f.rows = []; });

function setup(teamsOn = true) {
  f.teams = () => teamsOn;
  if (!f.rows.length) f.rows = [{ id: TEAM, project: "example-project", name: "Fixes", status: "running" }];
  const host = document.createElement("div");
  document.body.append(host);
  dispose = render(() => createComponent(TaskCreate, {
    project: "example-project", scope: "1 task", backlog: false, accessible: true, children: null,
  }), host);
  (host.querySelector('[aria-label="New task"]') as HTMLButtonElement).click();
  return host;
}

const chooser = (host: HTMLElement) => host.querySelector<HTMLButtonElement>("button.tk-team");

it("offers the project's teams in the form", () => {
  const host = setup();
  expect(chooser(host)).not.toBeNull();
  expect(chooser(host)!.textContent).toContain("No team");
  chooser(host)!.dispatchEvent(new KeyboardEvent("keydown", { key: "ArrowDown", bubbles: true }));
  expect(chooser(host)!.textContent).toContain("Fixes");
  expect(chooser(host)!.querySelector(".team-identity svg")).not.toBeNull();
});

it("creates the card in the chosen team's queue", async () => {
  const host = setup();
  chooser(host)!.dispatchEvent(new KeyboardEvent("keydown", { key: "ArrowDown", bubbles: true }));
  const input = host.querySelector("input.tk-newtitle") as HTMLInputElement;
  input.dispatchEvent(new KeyboardEvent("keydown", { key: "Enter", bubbles: true }));
  await vi.waitFor(() => expect(f.create).toHaveBeenCalled());
  expect(f.create).toHaveBeenCalledWith("example-project", false, undefined, TEAM);
});

it("creates an ordinary card without a team", async () => {
  const host = setup();
  const input = host.querySelector("input.tk-newtitle") as HTMLInputElement;
  input.dispatchEvent(new KeyboardEvent("keydown", { key: "Enter", bubbles: true }));
  await vi.waitFor(() => expect(f.create).toHaveBeenCalled());
  expect(f.create).toHaveBeenCalledWith("example-project", false, undefined, undefined);
});

it("shows no chooser while the preview is off", () => {
  const host = setup(false);
  expect(chooser(host)).toBeNull();
});


it("offers only live teams and disambiguates identical names", () => {
  f.rows = [
    { id: TEAM, project: "example-project", name: "Fixes", status: "running" },
    { id: "44444444-4444-4444-8444-444444444444", project: "example-project", name: "Fixes", status: "paused" },
    ...["stopped", "archived", "finished"].map((status) => ({ id: status, project: "example-project", name: status, status })),
  ];
  const host = setup();
  const button = chooser(host)!;
  button.dispatchEvent(new KeyboardEvent("keydown", { key: "ArrowDown", bubbles: true }));
  expect(button.textContent).toContain("Fixes (33333333)");
  button.dispatchEvent(new KeyboardEvent("keydown", { key: "ArrowDown", bubbles: true }));
  expect(button.textContent).toContain("Fixes (44444444)");
  button.dispatchEvent(new KeyboardEvent("keydown", { key: "ArrowDown", bubbles: true }));
  expect(button.textContent).toContain("Fixes (44444444)");
});
