// @vitest-environment happy-dom
// The New task form's team choice is bound to the form's project and to a new
// card (finding 9 of the second review): another project clears it, a choice
// no longer offered is refused, and a reserved card keeps the destination it
// was reserved with.
import { afterEach, expect, it, vi } from "vitest";
import { createComponent, createSignal } from "solid-js";
import { render } from "solid-js/web";
const f = vi.hoisted(() => ({
  create: vi.fn(async (..._args: unknown[]) => false),
  send: vi.fn(async (..._args: unknown[]) => ({ sent: true })),
  view: (() => undefined) as () => unknown,
  setView: (_value: unknown) => {},
  list: (() => undefined) as () => unknown,
  setList: (_value: unknown) => {},
}));
const TEAM = "33333333-3333-4333-8333-333333333333";
const OTHER = "44444444-4444-4444-8444-444444444444";
vi.mock("../../store/taskForms", async () => {
  const { createSignal: signal } = await import("solid-js");
  const [view, setView] = signal<unknown>();
  f.view = view; f.setView = setView;
  return {
    createCard: f.create,
    newCard: { follow: vi.fn(), locked: () => false, opening: () => false, busy: () => false, view, edit: vi.fn(), flush: vi.fn(async () => {}), error: () => "" },
  };
});
vi.mock("../../store/ui", () => ({ selected: () => undefined }));
vi.mock("../../store/taskBoard", () => ({ sendToTeam: f.send }));
vi.mock("./task-view-read", () => ({ readSelectedTask: async (key: string) => ({ uuid: key.split("\n")[1], stage: "todo", writable: true }) }));
vi.mock("../../store/taskNavigation", () => ({ requestTaskDescription: vi.fn() }));
vi.mock("../../store/teams", async () => {
  const { createSignal: signal } = await import("solid-js");
  const [list, setList] = signal<unknown>();
  f.list = list; f.setList = setList;
  return { teams: { available: () => true, list } };
});
import TaskCreate from "./TaskCreate";

let dispose: (() => void) | undefined;
afterEach(() => { dispose?.(); dispose = undefined; document.body.replaceChildren(); vi.clearAllMocks(); });

const fixes = { id: TEAM, project: "example-project", name: "Fixes", status: "running" };
const card = (project: string, over: Record<string, unknown> = {}) => ({ project, text: "Task title", handle: 1, blocked: false, read_only: false, recovery: false, error: "", ...over });
function setup(project: () => string) {
  const host = document.createElement("div");
  document.body.append(host);
  dispose = render(() => createComponent(TaskCreate, { get project() { return project(); }, scope: "1 task", backlog: false, accessible: true, children: null }), host);
  (host.querySelector('[aria-label="New task"]') as HTMLButtonElement).click();
  return host;
}
const chooser = (host: HTMLElement) => host.querySelector<HTMLButtonElement>("button.tk-team");
const enter = (host: HTMLElement) => (host.querySelector("input.tk-newtitle") as HTMLInputElement).dispatchEvent(new KeyboardEvent("keydown", { key: "Enter", bubbles: true }));
const choose = (host: HTMLElement) => chooser(host)!.dispatchEvent(new KeyboardEvent("keydown", { key: "ArrowDown", bubbles: true }));

it("does not submit a previous project's hidden team choice", async () => {
  const [project, setProject] = createSignal("example-project");
  f.setList({ teams: [fixes], truncated: false, next: null });
  f.setView(card("example-project"));
  const host = setup(project);
  choose(host);
  expect(chooser(host)!.textContent).toContain("Fixes");
  f.setView(card("other-project"));
  setProject("other-project");
  (host.querySelector('[aria-label="New task"]') as HTMLButtonElement).click();
  expect(chooser(host)).toBeNull();
  enter(host);
  await vi.waitFor(() => expect(f.create).toHaveBeenCalled());
  expect(f.create).toHaveBeenCalledWith("other-project", false, undefined, undefined);
});

it("keeps the choice while the title is edited", async () => {
  f.setList({ teams: [fixes], truncated: false, next: null });
  f.setView(card("example-project"));
  const host = setup(() => "example-project");
  choose(host);
  expect(chooser(host)!.textContent).toContain("Fixes");
  // Each edit is a new view of the same card.
  f.setView(card("example-project", { text: "Task title, edited", rev: 2 }));
  f.setView(card("example-project", { text: "Task title, edited again", rev: 3 }));
  expect(chooser(host)!.textContent).toContain("Fixes");
  enter(host);
  await vi.waitFor(() => expect(f.create).toHaveBeenCalled());
  expect(f.create).toHaveBeenCalledWith("example-project", false, undefined, TEAM);
});

it("refuses a choice the form no longer offers and sends nothing", async () => {
  f.setList({ teams: [fixes, { id: OTHER, project: "example-project", name: "Other", status: "running" }], truncated: false, next: null });
  f.setView(card("example-project"));
  const host = setup(() => "example-project");
  choose(host);
  expect(chooser(host)!.textContent).toContain("Fixes");
  // The team leaves the list while the form is open.
  f.setList({ teams: [{ id: OTHER, project: "example-project", name: "Other", status: "running" }], truncated: false, next: null });
  enter(host);
  await Promise.resolve();
  expect(f.create).not.toHaveBeenCalled();
  expect(host.querySelector(".tk-create-error")?.textContent).toContain("no longer offered");
});

it("a reserved card keeps its destination and the chooser cannot change it", async () => {
  f.setList({ teams: [fixes, { id: OTHER, project: "example-project", name: "Other", status: "running" }], truncated: false, next: null });
  f.setView(card("example-project", { recovery: true, team: TEAM }));
  const host = setup(() => "example-project");
  expect(chooser(host)!.disabled).toBe(true);
  expect(chooser(host)!.textContent).toContain("Fixes");
  enter(host);
  await vi.waitFor(() => expect(f.create).toHaveBeenCalled());
  expect(f.create).toHaveBeenCalledWith("example-project", false, undefined, undefined);
});

it("an older reserved card without a team shows no chooser and sends no team", async () => {
  f.setList({ teams: [fixes], truncated: false, next: null });
  f.setView(card("example-project", { recovery: true }));
  const host = setup(() => "example-project");
  expect(chooser(host)).toBeNull();
  enter(host);
  await vi.waitFor(() => expect(f.create).toHaveBeenCalled());
  expect(f.create).toHaveBeenCalledWith("example-project", false, undefined, undefined);
});

it("a card created but not queued says so with the team's reason and offers Send to team for that task (second confirmation, finding 6)", async () => {
  f.setList({ teams: [fixes], truncated: false, next: null });
  f.setView(card("example-project"));
  const created = "55555555-5555-4555-8555-555555555555";
  f.create.mockImplementationOnce(async () => {
    f.setView(card("example-project", { text: "", created, not_queued: { task: created, team: TEAM, code: "invalid_state", reason: "The team is stopped." } }));
    return true;
  });
  const host = setup(() => "example-project");
  choose(host);
  enter(host);
  await vi.waitFor(() => expect(host.querySelector("[data-not-queued]")).not.toBeNull());
  const notice = host.querySelector("[data-not-queued]")!;
  expect(notice.textContent).toContain("The task was created, but Fixes did not queue it: The team is stopped.");
  expect(f.create).toHaveBeenCalledTimes(1);
  ([...notice.querySelectorAll("button")].find((button) => button.textContent === "Send to team") as HTMLButtonElement).click();
  await vi.waitFor(() => expect(f.send).toHaveBeenCalledWith("example-project", { uuid: created }, TEAM, "Fixes", { read: expect.objectContaining({ uuid: created }), quiet: true }));
  await vi.waitFor(() => expect(host.querySelector("[data-not-queued]")).toBeNull());
  expect(f.create, "no second card").toHaveBeenCalledTimes(1);
});

// Third confirmation, the same two classes in the rest of the recovery: an
// answer is taken only for the card it confirms, and settles only what it was
// started for.
it("an answer for another project's card shows no not-queued notice", async () => {
  f.setList({ teams: [fixes], truncated: false, next: null });
  f.setView(card("example-project"));
  const created = "55555555-5555-4555-8555-555555555555";
  f.create.mockImplementationOnce(async () => {
    f.setView(card("other-project", { text: "", created, not_queued: { task: created, team: TEAM, reason: "Not ours." } }));
    return true;
  });
  const host = setup(() => "example-project");
  choose(host);
  enter(host);
  await vi.waitFor(() => expect(f.create).toHaveBeenCalled());
  await Promise.resolve(); await Promise.resolve();
  expect(host.querySelector("[data-not-queued]")).toBeNull();
});

it("a notice naming another team than the one sent is not taken", async () => {
  f.setList({ teams: [fixes, { id: OTHER, project: "example-project", name: "Other", status: "running" }], truncated: false, next: null });
  f.setView(card("example-project"));
  const created = "55555555-5555-4555-8555-555555555555";
  f.create.mockImplementationOnce(async () => {
    f.setView(card("example-project", { text: "", created, not_queued: { task: created, team: OTHER, reason: "Another team." } }));
    return true;
  });
  const host = setup(() => "example-project");
  choose(host);
  expect(chooser(host)!.textContent).toContain("Fixes");
  enter(host);
  await vi.waitFor(() => expect(f.create).toHaveBeenCalled());
  await Promise.resolve(); await Promise.resolve();
  expect(host.querySelector("[data-not-queued]")).toBeNull();
});

it("a team chosen while the submit was in flight is kept", async () => {
  f.setList({ teams: [fixes, { id: OTHER, project: "example-project", name: "Other", status: "running" }], truncated: false, next: null });
  f.setView(card("example-project"));
  let finish!: (value: boolean) => void;
  f.create.mockImplementationOnce(() => new Promise<boolean>((resolve) => { finish = resolve; }));
  const host = setup(() => "example-project");
  choose(host);
  enter(host);
  await vi.waitFor(() => expect(f.create).toHaveBeenCalled());
  // The form is open again with the next card; the person picks another team.
  (host.querySelector('[aria-label="New task"]') as HTMLButtonElement).click();
  choose(host);
  expect(chooser(host)!.textContent).toContain("Other");
  finish(true);
  await Promise.resolve(); await Promise.resolve();
  (host.querySelector('[aria-label="New task"]') as HTMLButtonElement).click();
  expect(chooser(host)!.textContent).toContain("Other");
});

// zerus-63rru.19 and the polish package's item 5: the queue's answer is a
// refusal only with a refusal's code. A storage failure of the team store, or
// no answer, may have queued the task: the notice says so in the client's own
// words, chosen by the code, and never the store's sentence for command-line
// callers. Send to team is the way on; the store queues a task at most once,
// by its identity (src/teams/ops/queue.rs existing_node).
const UNKNOWN = "The task was created; whether Fixes queued it is not known. Send to team queues it now, and never twice.";
it.each([
  ["a storage failure", "storage_failed", "The result is not known; repeat the same request with the same request ID.", UNKNOWN],
  ["no answer from the queue", "", "The team did not confirm that it queued the task.", UNKNOWN],
  ["a refusal", "invalid_state", "The team is stopped.", "The task was created, but Fixes did not queue it: The team is stopped."],
  ["a busy store (nothing was written)", "store_busy", "The team store is busy. Nothing was written; try again.", "The task was created, but Fixes did not queue it: The team store is busy. Nothing was written; try again."],
])("a card whose queue gave %s is told by the code", async (_label, code, reason, told) => {
  f.setList({ teams: [fixes], truncated: false, next: null });
  f.setView(card("example-project"));
  const created = "55555555-5555-4555-8555-555555555555";
  f.create.mockImplementationOnce(async () => {
    f.setView(card("example-project", { text: "", created, not_queued: { task: created, team: TEAM, code, reason } }));
    return true;
  });
  const host = setup(() => "example-project");
  choose(host);
  enter(host);
  await vi.waitFor(() => expect(host.querySelector("[data-not-queued]")).not.toBeNull());
  expect(host.querySelector("[data-not-queued] > span")!.textContent).toBe(told);
  const text = host.querySelector("[data-not-queued]")!.textContent!;
  if (told === UNKNOWN) {
    expect(text, "the store's sentence for command-line callers is not shown").not.toContain(reason);
    expect(text).not.toContain("did not queue it");
  }
  expect([...host.querySelectorAll("[data-not-queued] button")].map((button) => button.textContent)).toContain("Send to team");
});
