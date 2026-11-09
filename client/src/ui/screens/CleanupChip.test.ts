// @vitest-environment happy-dom
import { afterEach, beforeEach, expect, it, vi } from "vitest";
import { createComponent } from "solid-js";
import { render } from "solid-js/web";
vi.mock("../../bridge", () => ({ inTauri: () => false }));
import { CleanupChip, CleanupReviewButton } from "./CleanupChip";
import { applyCleanup, cleanup, cleanupDialog, closeCleanup, emptyCleanup, type CleanupRow } from "../../store/terminationCleanup";
const row = (id: string, patch: Partial<CleanupRow> = {}): CleanupRow => ({ id, folder_key: id, label: id, path: `/srv/example/${id}`, machine: "this computer", computer: "", branch: id,
  status: "kept", reason: "", details: "", size: "1 KiB", can_check: false, ready: false, ...patch });
let revision = 0, dispose: (() => void)[] = [];
beforeEach(() => { closeCleanup(); applyCleanup({ ...emptyCleanup(), revision: ++revision }); });
afterEach(() => { for (const close of dispose) close(); dispose = []; closeCleanup(); document.body.replaceChildren(); });
function mount() {
  const list = document.createElement("footer"), bar = document.createElement("footer");
  list.className = "sp-foot"; bar.className = "statusbar"; document.body.append(list, bar);
  dispose.push(render(() => createComponent(CleanupReviewButton, {}), list), render(() => createComponent(CleanupChip, {}), bar));
  return { list, bar, review: list.querySelector<HTMLButtonElement>(".cleanup-review")! };
}

it("keeps an idle review entry in the session footer with no status-bar result", () => {
  const { list, bar, review } = mount();
  expect(list.querySelector("svg")).not.toBeNull(); expect(review.textContent).toBe("");
  expect(review.classList.contains("attention")).toBe(false); expect(review.classList.contains("warn")).toBe(false);
  expect(review.getAttribute("aria-label")).toBe("Review working folders"); expect(bar.textContent).toBe("");
  review.focus(); review.click(); expect(cleanupDialog()?.origin).toBe(review);
  closeCleanup(); review.click(); expect(cleanupDialog()).toBeDefined();
});

it("counts a held folder once, retaining every raw original, and keeps results separate", () => {
  const original = row("original", { folder_key: "shared", status: "unknown" });
  const sibling = row("sibling", { folder_key: "shared" });
  applyCleanup({ ...emptyCleanup(), revision: ++revision, rows: [original, sibling], unknown: 1, text: "Working folders: 1 unknown", title: "Original result" });
  const { list, bar, review } = mount();
  expect(review.classList.contains("attention")).toBe(true); expect(review.classList.contains("warn")).toBe(true);
  expect(review.textContent).toBe("1"); expect(review.getAttribute("aria-label")).toContain("Working folders needing review: 1");
  expect(list.textContent).not.toContain("Working folders:"); expect(bar.textContent).toBe("Working folders: 1 unknown");
  expect(bar.querySelector(".cleanup-review")).toBeNull();
  bar.querySelector<HTMLButtonElement>(".cleanup-result")!.click(); expect(cleanupDialog()).toBeDefined();
  expect(cleanup().rows[0]).toBe(original); expect(cleanup().rows[1]).toBe(sibling);
});

it("counts native overflow plus distinct ready folders and reacts back to idle", () => {
  const { review, bar } = mount();
  applyCleanup({ ...emptyCleanup(), revision: ++revision, kept: 2, unknown: 1,
    rows: [row("ready", { status: "ready" }), row("ready-duplicate", { status: "ready", folder_key: "ready" })], text: "Working folders: 2 kept  1 unknown" });
  expect(review.textContent).toBe("4"); expect(bar.querySelector(".cleanup-result")).not.toBeNull();
  applyCleanup({ ...emptyCleanup(), revision: ++revision });
  expect(review.textContent).toBe(""); expect(review.classList.contains("attention")).toBe(false);
  expect(bar.querySelector(".cleanup-result")).toBeNull();
});

it("leaves only successful results or running work out of the attention count", () => {
  applyCleanup({ ...emptyCleanup(), revision: ++revision, rows: [row("done", { status: "removed" })], removed: 1, text: "Working folders: 1 removed" });
  const { review, bar } = mount(); expect(review.textContent).toBe("");
  expect(bar.textContent).toBe("Working folders: 1 removed");
  applyCleanup({ ...emptyCleanup(), revision: ++revision, rows: [row("running", { status: "checking", busy: true })], busy: true, cleaning: true, text: "Cleaning working folders…" });
  expect(review.classList.contains("attention")).toBe(false); expect(review.textContent).toBe("");
  expect(bar.querySelector(".cleanup-result")).toBeNull();
});
