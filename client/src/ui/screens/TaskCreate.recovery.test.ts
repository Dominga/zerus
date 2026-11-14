// @vitest-environment happy-dom
// The recovery of a card created but not queued (third confirmation, findings
// 2 and 3): Send waits for the created task, read exactly by its identity or
// held by the board, and never calls an unread task changed; a reply settles
// only the notice it was started for; one Send at a time. The real Send to
// team and the real notification bus run here, with the toasts rendered; the
// native board action, the exact read and the board's cache are fakes.
import { afterEach, expect, it, vi } from "vitest";
import { createComponent } from "solid-js";
import { render } from "solid-js/web";
const f = vi.hoisted(() => ({
  board: vi.fn(async (..._args: unknown[]) => {}), notify: vi.fn(), reads: [] as { key: string; resolve: (row: unknown) => void; reject: (error: unknown) => void }[],
  setCached: (_: unknown) => {}, create: vi.fn(async (..._args: unknown[]) => false),
  view: (() => undefined) as () => unknown, setView: (_value: unknown) => {}, list: (() => undefined) as () => unknown, setList: (_value: unknown) => {},
}));
const TEAM = "33333333-3333-4333-8333-333333333333";
const FIRST = "11111111-1111-4111-8111-111111111111";
const SECOND = "22222222-2222-4222-8222-222222222222";
vi.mock("../../store/taskForms", async () => {
  const { createSignal } = await import("solid-js");
  const [view, setView] = createSignal<unknown>();
  f.view = view; f.setView = setView;
  return { createCard: f.create, newCard: { follow: vi.fn(), locked: () => false, opening: () => false, busy: () => false, view, edit: vi.fn(), flush: vi.fn(async () => {}), error: () => "" } };
});
vi.mock("../../store/ui", () => ({ selected: () => undefined }));
vi.mock("../../bridge", async (original) => ({ ...(await original() as object), taskBoardAction: f.board }));
vi.mock("./task-view-read", () => ({ readSelectedTask: (key: string) => new Promise((resolve, reject) => f.reads.push({ key, resolve, reject })) }));
vi.mock("../../store/tasks", async () => {
  const { createSignal } = await import("solid-js");
  const [cached, setCached] = createSignal<unknown>();
  f.setCached = setCached;
  return { tasks: { project: () => "example-project", find: (uuid: string) => (cached() as { uuid?: string } | undefined)?.uuid === uuid ? cached() : undefined } };
});
vi.mock("../../store/taskNavigation", () => ({ requestTaskDescription: vi.fn() }));
vi.mock("../../store/teams", async () => {
  const { createSignal } = await import("solid-js");
  const [list, setList] = createSignal<unknown>();
  f.list = list; f.setList = setList;
  return { teams: { available: () => true, list } };
});
import TaskCreate from "./TaskCreate";
import { Toasts } from "../components/Overlays";
import { bus, notices } from "../../store/notices";
import { sendToTeam } from "../../store/taskBoard";

let dispose: (() => void) | undefined;
let disposeToasts: (() => void) | undefined;
afterEach(() => {
  disposeToasts?.(); disposeToasts = undefined;
  for (const note of notices()) bus.dismiss(note.id);
  dispose?.(); dispose = undefined; document.body.replaceChildren(); vi.clearAllMocks(); f.reads = []; f.setCached(undefined);
});
/** The rendered toasts' texts, as the person reads them. */
function toasts() {
  if (!disposeToasts) {
    const host = document.createElement("div");
    host.dataset.toasts = "";
    document.body.append(host);
    disposeToasts = render(() => createComponent(Toasts, {}), host);
  }
  return () => [...document.querySelectorAll<HTMLElement>("[data-toasts] .toast")].map((toast) => toast.textContent ?? "");
}

const fixes = { id: TEAM, project: "example-project", name: "Fixes", status: "running" };
const card = (over: Record<string, unknown> = {}) => ({ project: "example-project", text: "Task title", handle: 1, blocked: false, read_only: false, recovery: false, error: "", ...over });
const row = (uuid: string, over: Record<string, unknown> = {}) => ({ uuid, stage: "todo", writable: true, title: uuid === FIRST ? "First task" : "Second task", ...over });
const settle = async () => { for (let n = 0; n < 10; n++) await Promise.resolve(); };
function setup(project: () => string = () => "example-project") {
  f.setList({ teams: [fixes], truncated: false, next: null });
  f.setView(card());
  const host = document.createElement("div");
  document.body.append(host);
  dispose = render(() => createComponent(TaskCreate, { get project() { return project(); }, scope: "1 task", backlog: false, accessible: true, children: null }), host);
  (host.querySelector('[aria-label="New task"]') as HTMLButtonElement).click();
  return host;
}
const choose = (host: HTMLElement) => host.querySelector<HTMLButtonElement>("button.tk-team")!.dispatchEvent(new KeyboardEvent("keydown", { key: "ArrowDown", bubbles: true }));
const enter = (host: HTMLElement) => (host.querySelector("input.tk-newtitle") as HTMLInputElement).dispatchEvent(new KeyboardEvent("keydown", { key: "Enter", bubbles: true }));
const button = (host: HTMLElement, label: string) => [...host.querySelectorAll<HTMLButtonElement>("[data-not-queued] button")].find((candidate) => candidate.textContent === label);
/** A create whose queue refused the task (a refusal's code, so the notice gives its reason). */
async function createNotQueued(host: HTMLElement, id: string, reason: string) {
  f.create.mockImplementationOnce(async () => { f.setView(card({ text: "", created: id, not_queued: { task: id, team: TEAM, reason, code: "invalid_state" } })); return true; });
  f.setView(card({ text: `Card ${id.slice(0, 1)}` }));
  (host.querySelector('[aria-label="New task"]') as HTMLButtonElement).click();
  choose(host); enter(host);
  await vi.waitFor(() => expect(host.querySelector("[data-not-queued]")?.textContent).toContain(reason));
}

it("waits for the created task, never calls it changed, then sends with the row it read (finding 2)", async () => {
  const host = setup();
  await createNotQueued(host, FIRST, "The team is stopped.");
  expect(f.reads.map((read) => read.key)).toEqual([`example-project\n${FIRST}`]);
  const send = button(host, "Send to team")!;
  expect(send.disabled).toBe(true);
  expect(send.getAttribute("aria-busy")).toBe("true");
  expect(host.querySelector("[data-recovery-reading]")?.textContent).toBe("Reading the task…");
  const shown = toasts();
  send.click(); await settle();
  expect(f.board).not.toHaveBeenCalled();
  expect(shown().join(" ")).not.toContain("changed");
  f.reads[0].resolve(row(FIRST)); await settle();
  expect(send.disabled).toBe(false);
  send.click(); await settle();
  expect(f.board).toHaveBeenCalledWith("example-project", FIRST, "todo", { kind: "send_to_team", team: TEAM });
  expect(host.querySelector("[data-not-queued]")).toBeNull();
});

it("the board's own row, when it arrives first, is enough (the reviewer's cache half)", async () => {
  const host = setup();
  await createNotQueued(host, FIRST, "The team is stopped.");
  const send = button(host, "Send to team")!;
  expect(send.disabled).toBe(true);
  f.setCached(row(FIRST)); await settle();
  expect(send.disabled).toBe(false);
  send.click(); await settle();
  expect(f.board).toHaveBeenCalledTimes(1);
});

it("a failed read says so and can be read again; the existing checks still refuse a task that left To do", async () => {
  const host = setup();
  await createNotQueued(host, FIRST, "The team is stopped.");
  f.reads[0].reject(new Error("offline")); await settle();
  expect(host.querySelector("[data-recovery-unread]")?.textContent).toContain("offline");
  expect(button(host, "Send to team")!.disabled).toBe(true);
  button(host, "Read again")!.click();
  expect(f.reads).toHaveLength(2);
  f.reads[1].resolve(row(FIRST, { stage: "active" })); await settle();
  button(host, "Send to team")!.click(); await settle();
  expect(f.board).not.toHaveBeenCalled();
  expect(host.querySelector("[data-recovery-refusal]")?.textContent).toContain("The task changed");
  expect(host.querySelector("[data-not-queued]"), "the notice stays with the reason").not.toBeNull();
});

it("a late read for a dismissed notice changes nothing", async () => {
  const host = setup();
  await createNotQueued(host, FIRST, "First not queued");
  button(host, "Dismiss")!.click();
  await createNotQueued(host, SECOND, "Second not queued");
  f.reads[0].resolve(row(FIRST)); await settle();
  expect(button(host, "Send to team")!.disabled, "the first task's read does not ready the second notice").toBe(true);
  f.reads[1].resolve(row(SECOND)); await settle();
  expect(button(host, "Send to team")!.disabled).toBe(false);
});

it("an earlier Send's reply cannot settle a later task's notice (finding 3)", async () => {
  const host = setup();
  await createNotQueued(host, FIRST, "First not queued");
  f.reads[0].resolve(row(FIRST)); await settle();
  let finish!: () => void;
  f.board.mockImplementationOnce(() => new Promise<void>((resolve) => { finish = resolve; }));
  button(host, "Send to team")!.click(); await settle();
  expect(button(host, "Send to team")!.disabled, "one Send at a time").toBe(true);
  button(host, "Send to team")!.click(); await settle();
  expect(f.board).toHaveBeenCalledTimes(1);
  button(host, "Dismiss")!.click();
  f.setView(card({ text: "Second task" }));
  await createNotQueued(host, SECOND, "Second not queued");
  finish(); await settle();
  expect(host.querySelector("[data-not-queued]")?.textContent).toContain("Second not queued");
  expect(host.querySelector("[data-recovery-refusal]")).toBeNull();
});

it("an earlier Send's refusal is not shown on a later notice", async () => {
  const host = setup();
  await createNotQueued(host, FIRST, "First not queued");
  f.reads[0].resolve(row(FIRST)); await settle();
  let refuse!: (error: unknown) => void;
  f.board.mockImplementationOnce(() => new Promise<void>((_resolve, reject) => { refuse = reject; }));
  button(host, "Send to team")!.click(); await settle();
  await createNotQueued(host, SECOND, "Second not queued");
  refuse("invalid_state: The first team is stopped."); await settle();
  expect(host.querySelector("[data-recovery-refusal]")).toBeNull();
  expect(host.querySelector("[data-not-queued]")?.textContent).toContain("Second not queued");
});

// Fourth and fifth confirmations: every reply of a recovery's Send shows one
// notification naming the task by its bounded title and the team, wherever
// the person is and whatever happened to the notice, and never claims that a
// rejected send did not happen. The notice is handled only while it is the one
// the reply was for and still shown.
async function staleSend(host: HTMLElement, settleWith: (finish: () => void, refuse: (error: unknown) => void) => void) {
  await createNotQueued(host, FIRST, "First not queued");
  f.reads[0].resolve(row(FIRST)); await settle();
  let finish!: () => void, refuse!: (error: unknown) => void;
  f.board.mockImplementationOnce(() => new Promise<void>((resolve, reject) => { finish = resolve; refuse = reject; }));
  button(host, "Send to team")!.click(); await settle();
  button(host, "Dismiss")!.click();
  await createNotQueued(host, SECOND, "Second not queued");
  const before = host.querySelector("[data-not-queued]")!.textContent;
  settleWith(finish, refuse); await settle();
  return before;
}

it("a stale success shows one notification naming task A, and B's notice is unchanged", async () => {
  const host = setup();
  const shown = toasts();
  const before = await staleSend(host, (finish) => finish());
  expect(shown()).toEqual(["“First task” was sent to Fixes."]);
  expect(shown().join(" ")).not.toContain("Second");
  expect(host.querySelector("[data-not-queued]")!.textContent).toBe(before);
  expect(host.querySelector("[data-recovery-refusal]")).toBeNull();
});

it("a stale refusal shows one notification naming task A with its reason, and B's notice is unchanged", async () => {
  const host = setup();
  const shown = toasts();
  const before = await staleSend(host, (_finish, refuse) => refuse("invalid_state: The team is stopped."));
  expect(shown()).toEqual(["Result of sending “First task” to Fixes: The team is stopped."]);
  expect(shown().join(" ")).not.toContain("Second");
  expect(host.querySelector("[data-not-queued]")!.textContent).toBe(before);
  expect(host.querySelector("[data-recovery-refusal]")).toBeNull();
});

it("a stale reply names a long title cut on a character", async () => {
  const host = setup();
  const shown = toasts();
  await createNotQueued(host, FIRST, "First not queued");
  f.reads[0].resolve(row(FIRST, { title: `${"Ünïcode ".repeat(20)}🧑‍💻 tail` })); await settle();
  let finish!: () => void;
  f.board.mockImplementationOnce(() => new Promise<void>((resolve) => { finish = resolve; }));
  button(host, "Send to team")!.click(); await settle();
  button(host, "Dismiss")!.click();
  finish(); await settle();
  const [text] = shown();
  const title = text.slice(1, text.indexOf("”"));
  expect([...new Intl.Segmenter(undefined, { granularity: "grapheme" }).segment(title)].length).toBeLessThanOrEqual(60);
  expect(title.startsWith("Ünïcode Ünïcode")).toBe(true);
  expect(title.endsWith("…")).toBe(true);
  expect(text).toMatch(/” was sent to Fixes\.$/u);
});

it("a visible current success clears the notice and shows one notification naming the task", async () => {
  const host = setup();
  const shown = toasts();
  await createNotQueued(host, FIRST, "First not queued");
  f.reads[0].resolve(row(FIRST)); await settle();
  button(host, "Send to team")!.click(); await settle();
  expect(host.querySelector("[data-not-queued]")).toBeNull();
  expect(shown()).toEqual(["“First task” was sent to Fixes."]);
});

it("a visible current rejection keeps its reason on the notice, Send available, and one notification naming the task", async () => {
  const host = setup();
  const shown = toasts();
  await createNotQueued(host, FIRST, "First not queued");
  f.reads[0].resolve(row(FIRST)); await settle();
  f.board.mockRejectedValueOnce("invalid_state: The team is stopped.");
  button(host, "Send to team")!.click(); await settle();
  expect(host.querySelector("[data-recovery-refusal]")?.textContent, "the team's sentence, without its code").toBe("The team is stopped.");
  expect(button(host, "Send to team")!.disabled).toBe(false);
  expect(shown()).toEqual(["Result of sending “First task” to Fixes: The team is stopped."]);
});

it("a rejected Send reads once: the queue's reason, the team's sentence only when it says something else, never a code", async () => {
  const host = setup();
  const shown = toasts();
  await createNotQueued(host, FIRST, "The team is stopped.");
  f.reads[0].resolve(row(FIRST)); await settle();
  f.board.mockRejectedValueOnce("invalid_state: The team is stopped.");
  button(host, "Send to team")!.click(); await settle();
  const notice = host.querySelector("[data-not-queued]")!.textContent!;
  expect(notice.split("The team is stopped.").length - 1, "the same sentence is not read twice").toBe(1);
  expect(notice).not.toContain("invalid_state");
  expect(host.querySelector("[data-recovery-refusal]")).toBeNull();
  expect(shown()).toEqual(["Result of sending “First task” to Fixes: The team is stopped."]);
});

it("after an unknown queue result, a rejected Send still shows the team's sentence on the notice", async () => {
  const host = setup();
  const reason = "The result is not known; repeat the same request with the same request ID.";
  f.create.mockImplementationOnce(async () => { f.setView(card({ text: "", created: FIRST, not_queued: { task: FIRST, team: TEAM, reason, code: "storage_failed" } })); return true; });
  f.setView(card({ text: "Card 1" }));
  (host.querySelector('[aria-label="New task"]') as HTMLButtonElement).click();
  choose(host); enter(host);
  await vi.waitFor(() => expect(host.querySelector("[data-not-queued]")).not.toBeNull());
  f.reads[0].resolve(row(FIRST)); await settle();
  f.board.mockRejectedValueOnce(`storage_failed: ${reason}`);
  button(host, "Send to team")!.click(); await settle();
  // The notice did not show the store's sentence, so the Send's reply is not a repeat of it.
  expect(host.querySelector("[data-recovery-refusal]")?.textContent).toBe(reason);
});

/** Starts Send on a not-queued notice whose task was read; the native write waits. */
async function startSend(host: HTMLElement) {
  await createNotQueued(host, FIRST, "First not queued");
  f.reads[0].resolve(row(FIRST)); await settle();
  const pending = { resolve: () => {}, reject: (_error: unknown) => {} };
  f.board.mockImplementationOnce(() => new Promise<void>((resolve, reject) => { pending.resolve = resolve; pending.reject = reject; }));
  button(host, "Send to team")!.click(); await settle();
  expect(f.board).toHaveBeenCalledTimes(1);
  return pending;
}

for (const kind of ["success", "rejection"] as const) {
  const settleSend = (pending: Awaited<ReturnType<typeof startSend>>) => kind === "success" ? pending.resolve() : pending.reject("The queue is full.");
  const told = kind === "success" ? "“First task” was sent to Fixes." : "Result of sending “First task” to Fixes: The queue is full.";

  it(`after the form left its project, a ${kind} names the task in one notification (R5-1)`, async () => {
    const { createSignal } = await import("solid-js");
    const [project, setProject] = createSignal("example-project");
    const host = setup(project);
    const shown = toasts();
    const pending = await startSend(host);
    setProject("other-project"); await settle();
    expect(host.querySelector("[data-not-queued]")).toBeNull();
    settleSend(pending); await settle();
    expect(shown()).toEqual([told]);
    setProject("example-project"); await settle();
    expect(host.querySelector("[data-not-queued]"), "the retired notice does not come back").toBeNull();
  });

  it(`after the form was disposed, a ${kind} names the task in one notification (R5-1)`, async () => {
    const host = setup();
    const shown = toasts();
    const pending = await startSend(host);
    dispose?.(); dispose = undefined;
    expect(host.querySelector("[data-not-queued]")).toBeNull();
    settleSend(pending); await settle();
    expect(shown()).toEqual([told]);
  });
}

it("an unconfirmed write is told as its result, never as a send that did not happen (R5-2)", async () => {
  const host = setup();
  const shown = toasts();
  const unconfirmed = "Task change unconfirmed. Refresh the task before changing it again.";
  const pending = await startSend(host);
  pending.reject(unconfirmed); await settle();
  expect(shown()).toEqual([`Result of sending “First task” to Fixes: ${unconfirmed}`]);
  expect(host.querySelector("[data-recovery-refusal]")?.textContent).toBe(unconfirmed);
  expect(host.textContent).not.toMatch(/not sent|was not/u);
});

it("an unconfirmed write after Dismiss is told the same way (R5-2)", async () => {
  const host = setup();
  const shown = toasts();
  const unconfirmed = "Task change unconfirmed. Refresh the task before changing it again.";
  const pending = await startSend(host);
  button(host, "Dismiss")!.click();
  pending.reject(unconfirmed); await settle();
  expect(shown()).toEqual([`Result of sending “First task” to Fixes: ${unconfirmed}`]);
});

it("a board card's Send keeps its own notification", async () => {
  const shown = toasts();
  f.setCached(row(FIRST));
  expect(await sendToTeam("example-project", { uuid: FIRST }, TEAM, "Fixes")).toMatchObject({ sent: true });
  await settle();
  expect(shown()).toEqual(["Sent to Fixes"]);
  f.board.mockRejectedValueOnce("The queue is full.");
  await sendToTeam("example-project", { uuid: FIRST }, TEAM, "Fixes");
  await settle();
  expect(shown()).toEqual(["Sent to Fixes", "The queue is full."]);
});
