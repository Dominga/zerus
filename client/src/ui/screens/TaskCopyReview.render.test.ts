// @vitest-environment happy-dom
import { afterEach, beforeEach, expect, it, vi } from "vitest";
import { createComponent } from "solid-js";
import { render } from "solid-js/web";
vi.mock("../../bridge", () => ({ reviewTaskCopy: vi.fn(), followTasks: vi.fn(), readTaskPage: vi.fn() }));
vi.mock("../../store/actions", () => ({ copy: vi.fn(async () => ({ outcome: "done" })), notify: vi.fn() }));
import { reviewTaskCopy } from "../../bridge";
import { tasks } from "../../store/tasks";
import { copyReviewBusy, setCopyReviewBusy, setCopyReviewOutcomes, setReviewingCopy, openCopyReview, reviewingCopy, confirmCopyReview } from "../../store/taskCopyReview";
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
  await openCopyReview("p", uuid(), button("Review local copy…"));
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
