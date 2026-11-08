// @vitest-environment happy-dom
import { afterEach, beforeEach, expect, it, vi } from "vitest";
import { createComponent } from "solid-js";
import { render } from "solid-js/web";
vi.mock("../../bridge", () => ({ reviewTaskCopy: vi.fn(), followTasks: vi.fn(), readTaskPage: vi.fn() }));
vi.mock("../../store/actions", () => ({ copy: vi.fn(async () => ({ outcome: "done" })), notify: vi.fn() }));
import { reviewTaskCopy } from "../../bridge";
import { tasks } from "../../store/tasks";
import { copyReviewBusy, copyReviewOutcome, createCopyReviewPanel, setCopyReviewBusy, setCopyReviewOutcomes, setReviewingCopy, reviewingCopy, confirmCopyReview } from "../../store/taskCopyReview";
import TaskCopyReview from "./TaskCopyReview";
import { translateWithCatalog } from "../../i18n";
import russian from "../../i18n.ru.json";

const uuid = (n = 1) => `00000000-0000-4000-8000-${String(n).padStart(12, "0")}`;
const fingerprint = "a".repeat(64);
const reply = (state = "review", extra = {}) => ({ abi: "task-copy-review-v1", project: { id: "p" }, uuid: uuid(), state, fingerprint, ...extra });
const flush = async () => { await Promise.resolve(); await Promise.resolve(); await Promise.resolve(); };
let dispose: (() => void) | undefined, host: HTMLElement;
function update(count = 1) {
  tasks.apply([{ computer: "", key: "p", kind: "list", payload: { project: { id: "p" }, tasks: [], damaged: Array.from({ length: count }, (_, n) => ({ uuid: uuid(n + 1), kept: true, error: "copy kept" })) } }]);
}
beforeEach(() => {
  vi.clearAllMocks(); setCopyReviewBusy(false); setCopyReviewOutcomes({}); setReviewingCopy(undefined);
  tasks.follow(undefined); tasks.follow("p"); update();
  HTMLDialogElement.prototype.showModal = function () { this.open = true; };
  HTMLDialogElement.prototype.close = function () { this.open = false; };
  host = document.createElement("div"); document.body.append(host);
  dispose = render(() => createComponent(TaskCopyReview, { project: "p", name: "Project" }), host);
});
afterEach(() => { dispose?.(); document.body.replaceChildren(); setReviewingCopy(undefined); });
const button = (label: string) => Array.from(host.querySelectorAll<HTMLButtonElement>("button")).find((b) => b.textContent === label)!;

it("shows at most five kept cards, exact copyable UUIDs and a remaining count", () => {
  update(25);
  expect(host.querySelectorAll("article")).toHaveLength(5);
  expect(host.textContent).toContain("+20 more damaged tasks");
  expect(host.querySelector('[aria-label="Copy task UUID: ' + uuid() + '"]')?.textContent).toContain(uuid());
  expect(host.textContent).toContain("Project");
});

it("defaults confirmation focus to Cancel and cancellation performs only the read", async () => {
  vi.mocked(reviewTaskCopy).mockResolvedValue(reply() as never);
  button("Review local copy…").click(); await flush();
  expect(document.activeElement?.textContent).toBe("Cancel");
  expect(host.querySelector("dialog")?.textContent).toContain("Attempt history and unsent local edits");
  button("Cancel").click(); await flush();
  expect(reviewingCopy()).toBeUndefined();
  expect(document.activeElement).toBe(button("Review local copy…"));
  expect(reviewTaskCopy).toHaveBeenCalledExactlyOnceWith("p", uuid());
});

it("uses the reviewed fingerprint once, disables pending writes, and retains a moved outcome after the feed removes the damage", async () => {
  let finish!: (value: never) => void;
  vi.mocked(reviewTaskCopy).mockResolvedValueOnce(reply() as never).mockImplementationOnce(() => new Promise((resolve) => { finish = resolve; }));
  button("Review local copy…").click(); await flush();
  button("Move copy aside").click();
  expect(copyReviewBusy()).toBe(true);
  expect(button("Move copy aside").disabled).toBe(true);
  await confirmCopyReview();
  expect(reviewTaskCopy).toHaveBeenCalledTimes(2);
  expect(reviewTaskCopy).toHaveBeenLastCalledWith("p", uuid(), fingerprint);
  finish(reply("moved") as never); await flush(); update(0);
  expect(host.querySelector("article [role=status]")?.textContent).toContain("Local copy moved aside");
  expect(button("Review local copy…").disabled).toBe(true);
});

it("shows a refusal reason and never retries an unknown transport outcome", async () => {
  vi.mocked(reviewTaskCopy).mockResolvedValueOnce(reply() as never).mockResolvedValueOnce(reply("refused", { reason: "copy changed" }) as never);
  button("Review local copy…").click(); await flush(); button("Move copy aside").click(); await flush();
  expect(host.textContent).toContain("Local copy review refused: copy changed");
  vi.mocked(reviewTaskCopy).mockResolvedValueOnce(reply() as never).mockRejectedValueOnce(new Error("lost reply"));
  button("Review local copy…").click(); await flush(); button("Move copy aside").click(); await flush();
  expect(host.textContent).toContain("outcome is unknown");
  expect(button("Review local copy…").disabled).toBe(true);
  await createCopyReviewPanel().open("p", uuid(), button("Review local copy…"));
  expect(reviewTaskCopy).toHaveBeenCalledTimes(4);
});

it("does not open a stale confirmation after navigation changes the project", async () => {
  let finish!: (value: never) => void;
  vi.mocked(reviewTaskCopy).mockImplementationOnce(() => new Promise((resolve) => { finish = resolve; }));
  button("Review local copy…").click(); tasks.follow("other"); finish(reply() as never); await flush();
  expect(reviewingCopy()).toBeUndefined();
  expect(reviewTaskCopy).toHaveBeenCalledTimes(1);
});

it("does not report success from another task's confirmation reply", async () => {
  vi.mocked(reviewTaskCopy).mockResolvedValueOnce(reply() as never).mockResolvedValueOnce(reply("moved", { uuid: uuid(2) }) as never);
  button("Review local copy…").click(); await flush(); button("Move copy aside").click(); await flush();
  expect(host.textContent).toContain("outcome is unknown");
  expect(host.textContent).not.toContain("Local copy moved aside");
  expect(button("Review local copy…").disabled).toBe(true);
});

it("names both losses in the Russian confirmation too", () => {
  const source = "Move aside the damaged local copy of task %1 and accept its next received copy? Attempt history and unsent local edits may be missing from that copy.";
  const text = translateWithCatalog(russian, "ru", source, uuid());
  expect(text).toContain(uuid()); expect(text).toContain("история попыток"); expect(text).toContain("неотправленные локальные правки");
});

it("retires a pending review read when its actual panel closes within the same project", async () => {
  let finish!: (value: never) => void;
  vi.mocked(reviewTaskCopy).mockImplementationOnce(() => new Promise((resolve) => { finish = resolve; }));
  button("Review local copy…").click();
  expect(copyReviewBusy()).toBe(true);
  dispose?.(); dispose = undefined;
  finish(reply() as never); await flush();
  expect(copyReviewBusy()).toBe(false);
  expect(reviewingCopy()).toBeUndefined();
});

it("a retired read cannot clear the busy state or replace a newer panel's confirmed write", async () => {
  let oldRead!: (value: never) => void, write!: (value: never) => void;
  vi.mocked(reviewTaskCopy).mockImplementationOnce(() => new Promise((resolve) => { oldRead = resolve; }))
    .mockResolvedValueOnce(reply() as never).mockImplementationOnce(() => new Promise((resolve) => { write = resolve; }));
  try {
    button("Review local copy…").click(); dispose?.(); dispose = undefined;
    expect(copyReviewBusy()).toBe(false);
    dispose = render(() => createComponent(TaskCopyReview, { project: "p", name: "Project" }), host);
    button("Review local copy…").click(); await flush(); button("Move copy aside").click();
    const current = reviewingCopy();
    oldRead(reply() as never); await flush();
    expect(copyReviewBusy()).toBe(true);
    expect(reviewingCopy()).toBe(current);
    expect(reviewTaskCopy).toHaveBeenCalledTimes(3);
    write(reply("moved") as never); await flush();
    expect(copyReviewOutcome("p", uuid())?.state).toBe("moved");
    expect(copyReviewBusy()).toBe(false);
  } finally { oldRead?.(reply() as never); write?.(reply("moved") as never); await flush(); }
});

it("ignores a retired read's error without publishing a refusal or disturbing a new read", async () => {
  let fail!: (error: Error) => void, finish!: (value: never) => void;
  vi.mocked(reviewTaskCopy).mockImplementationOnce(() => new Promise((_, reject) => { fail = reject; }))
    .mockImplementationOnce(() => new Promise((resolve) => { finish = resolve; }));
  try {
    button("Review local copy…").click(); dispose?.(); dispose = undefined;
    dispose = render(() => createComponent(TaskCopyReview, { project: "p", name: "Project" }), host);
    button("Review local copy…").click(); fail(new Error("old read failed")); await flush();
    expect(copyReviewBusy()).toBe(true);
    expect(copyReviewOutcome("p", uuid())).toBeUndefined();
    finish(reply() as never); await flush();
    expect(reviewingCopy()).toBeDefined();
    expect(copyReviewBusy()).toBe(false);
  } finally { fail?.(new Error("old read failed")); finish?.(reply() as never); await flush(); }
});

it.each(["moved", "refused", "unknown"])("a confirmed %s outcome survives panel disposal without reopening its dialog", async (state) => {
  let finish!: (value: never) => void;
  vi.mocked(reviewTaskCopy).mockResolvedValueOnce(reply() as never)
    .mockImplementationOnce(() => new Promise((resolve) => { finish = resolve; }));
  try {
    button("Review local copy…").click(); await flush(); button("Move copy aside").click();
    dispose?.(); dispose = undefined;
    expect(copyReviewBusy()).toBe(true);
    dispose = render(() => createComponent(TaskCopyReview, { project: "p", name: "Project" }), host);
    expect(host.querySelector("dialog")).toBeNull();
    expect(button("Review local copy…").disabled).toBe(true);
    finish(reply(state, { reason: "original result" }) as never); await flush();
    expect(copyReviewBusy()).toBe(false);
    expect(reviewingCopy()).toBeUndefined();
    expect(copyReviewOutcome("p", uuid())).toEqual({ state, reason: "original result" });
    expect(host.querySelector("[role=status]")?.textContent).toBeTruthy();
    expect(reviewTaskCopy).toHaveBeenCalledTimes(2);
  } finally { finish?.(reply(state) as never); await flush(); }
});

it("retires an unconfirmed dialog whose original damage card disappears", async () => {
  vi.mocked(reviewTaskCopy).mockResolvedValueOnce(reply() as never);
  button("Review local copy…").click(); await flush();
  expect(reviewingCopy()).toBeDefined();
  const move = button("Move copy aside");
  tasks.apply([{computer:"",key:"p",kind:"list",payload:{project:{id:"p"},tasks:[],damaged:[{uuid:uuid(2),kept:true,error:"another kept copy"}]}}]);
  expect(host.querySelector('[aria-label="Damaged task copy ' + uuid() + '"]')).toBeNull();
  expect(host.querySelectorAll("article")).toHaveLength(1);
  expect(host.querySelector("dialog")).toBeNull();
  move.click(); await flush();
  expect(reviewTaskCopy).toHaveBeenCalledTimes(1);
  expect(copyReviewBusy()).toBe(false);
  expect(reviewingCopy()).toBeUndefined();
});

const keepSecondCard = () => tasks.apply([{ computer: "", key: "p", kind: "list", payload: { project: { id: "p" }, tasks: [], damaged: [{ uuid: uuid(2), kept: true, error: "another kept copy" }] } }]);

it("retires a removed card's pending read before its late reply can open a dialog", async () => {
  let finish!: (value: never) => void;
  vi.mocked(reviewTaskCopy).mockImplementationOnce(() => new Promise((resolve) => { finish = resolve; }));
  button("Review local copy…").click(); keepSecondCard();
  expect(copyReviewBusy()).toBe(false);
  finish(reply() as never); await flush();
  expect(reviewingCopy()).toBeUndefined();
  expect(host.querySelector("dialog")).toBeNull();
  expect(reviewTaskCopy).toHaveBeenCalledTimes(1);
});

it("removing another card does not retire the original unconfirmed review", async () => {
  update(2); vi.mocked(reviewTaskCopy).mockResolvedValueOnce(reply() as never);
  button("Review local copy…").click(); await flush();
  const current = reviewingCopy();
  update(1); await flush();
  expect(reviewingCopy()).toBe(current);
  expect(host.querySelector("dialog")).not.toBeNull();
  expect(reviewTaskCopy).toHaveBeenCalledTimes(1);
});

it.each(["moved", "refused", "unknown"])("card cleanup retains an already-confirmed %s write and its original outcome", async (state) => {
  let finish!: (value: never) => void;
  vi.mocked(reviewTaskCopy).mockResolvedValueOnce(reply() as never)
    .mockImplementationOnce(() => new Promise((resolve) => { finish = resolve; }));
  try {
    button("Review local copy…").click(); await flush(); button("Move copy aside").click();
    const current = reviewingCopy(); keepSecondCard();
    expect(copyReviewBusy()).toBe(true);
    expect(reviewingCopy()).toBe(current);
    finish(reply(state, { reason: "original result" }) as never); await flush();
    expect(copyReviewBusy()).toBe(false);
    expect(reviewingCopy()).toBeUndefined();
    expect(copyReviewOutcome("p", uuid())).toEqual({ state, reason: "original result" });
    expect(reviewTaskCopy).toHaveBeenCalledTimes(2);
  } finally { finish?.(reply(state) as never); await flush(); }
});

it("also retires an idle confirmation if its origin disconnects outside card cleanup", async () => {
  vi.mocked(reviewTaskCopy).mockResolvedValueOnce(reply() as never);
  const origin = button("Review local copy…"); origin.click(); await flush(); origin.remove();
  await confirmCopyReview();
  expect(reviewingCopy()).toBeUndefined();
  expect(host.querySelector("dialog")).toBeNull();
  expect(reviewTaskCopy).toHaveBeenCalledTimes(1);
});
