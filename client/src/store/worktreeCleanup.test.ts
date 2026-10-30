import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import type { WorktreeReview } from "./worktreeCleanup";
const calls = vi.hoisted(() => ({ open: vi.fn(), view: vi.fn(), refresh: vi.fn(), prepare: vi.fn(), confirm: vi.fn(), close: vi.fn(async () => {}), toast: vi.fn() }));
vi.mock("../bridge", () => ({ inTauri: () => false, worktreeCleanupOpen: calls.open, worktreeCleanupView: calls.view, worktreeCleanupRefresh: calls.refresh, worktreeCleanupPrepare: calls.prepare, worktreeCleanupConfirm: calls.confirm, worktreeCleanupClose: calls.close }));
vi.mock("./notices", () => ({ toast: calls.toast }));
import * as store from "./worktreeCleanup";
const value = (id = "review-1", revision = 1): WorktreeReview => ({ id, revision, computer: "example-peer", machine: "example-peer", repository: "/srv/example", focus: "", rows: [], busy: false, summary: "No linked worktrees.", unknown: false });
function deferred<T>() { let resolve!: (value: T) => void; const promise = new Promise<T>((done) => { resolve = done; }); return { promise, resolve }; }
beforeEach(() => { vi.clearAllMocks(); calls.close.mockImplementation(async () => {}); calls.open.mockImplementation(async () => value()); calls.view.mockImplementation(async (id) => value(id)); calls.refresh.mockImplementation(async () => value("review-1", 2)); });
afterEach(() => { store.setWorktreeReview(undefined); store.setWorktreeReviewIssuing(false); store.closeWorktreeReview(); });
describe("explicit native Worktrees review", () => {
  it("Cancel waits for revocation before another confirmation can be prepared", async () => {
    await store.openWorktreeReview("example-peer", "/srv/example");
    const pending = deferred<void>(); calls.close.mockReturnValue(pending.promise); calls.view.mockResolvedValue(value("review-1", 2));
    const cancel = store.cancelWorktreeConfirmation(); expect(store.worktreeReviewLoading()).toBe(true);
    await store.prepareWorktreeRemoval([0], false); expect(calls.prepare).not.toHaveBeenCalled();
    pending.resolve(); await cancel; expect(store.worktreeReviewLoading()).toBe(false); expect(store.worktreeReview()?.revision).toBe(2);
  });
  it("unchanged passive status cannot replace the current review or reset its choices", async () => {
    await store.openWorktreeReview("example-peer", "/srv/example"); const original = store.worktreeReview();
    store.applyWorktreeReview({ ...value(), summary: "late duplicate" }); expect(store.worktreeReview()).toBe(original);
  });
  it("opens read-only and sends only immutable IDs/revision and explicit choices to prepare", async () => {
    await store.openWorktreeReview("example-peer", "/srv/example");
    expect(calls.open).toHaveBeenCalledWith("example-peer", "/srv/example", "");
    expect(calls.confirm).not.toHaveBeenCalled();
    calls.prepare.mockResolvedValue({ id: "one-use" });
    await store.prepareWorktreeRemoval([2], false);
    expect(calls.prepare).toHaveBeenCalledWith("review-1", 1, [2], false);
    expect(calls.confirm).not.toHaveBeenCalled();
    store.closeWorktreeReview(); expect(calls.close).toHaveBeenCalledWith("review-1");
  });
  it("holds the dialog during dispatch acceptance and never resends a confirmation", async () => {
    await store.openWorktreeReview("example-peer", "/srv/example");
    const pending = deferred<WorktreeReview>(); calls.confirm.mockReturnValue(pending.promise);
    const run = store.confirmWorktreeRemoval("one-use", true);
    expect(store.worktreeReviewIssuing()).toBe(true);
    store.closeWorktreeReview(); expect(store.worktreeReviewDialog()).toBeDefined();
    await store.confirmWorktreeRemoval("one-use", true); expect(calls.confirm).toHaveBeenCalledTimes(1);
    pending.resolve({ ...value(), revision: 2, busy: true }); await run;
    store.closeWorktreeReview(); expect(store.worktreeReviewDialog()).toBeDefined();
  });
  it("a late status for a closed dialog never replaces a new target", async () => {
    await store.openWorktreeReview("example-peer", "/srv/example");
    const pending = deferred<WorktreeReview>(); calls.view.mockReturnValue(pending.promise);
    const read = store.refreshWorktreeView(); store.closeWorktreeReview();
    calls.open.mockResolvedValue(value("review-2")); await store.openWorktreeReview("other-peer", "/srv/other");
    pending.resolve({ ...value("review-1", 99), unknown: true }); await read;
    expect(store.worktreeReview()?.id).toBe("review-2"); expect(store.worktreeReview()?.unknown).toBe(false);
  });
  it("closing during an initial read revokes the eventual handle and publishes no dialog", async () => {
    const pending = deferred<WorktreeReview>(); calls.open.mockReturnValue(pending.promise);
    const read = store.openWorktreeReview("example-peer", "/srv/example"); store.closeWorktreeReview();
    pending.resolve(value()); await read;
    expect(store.worktreeReviewDialog()).toBeUndefined(); expect(store.worktreeReview()).toBeUndefined(); expect(calls.close).toHaveBeenCalledWith("review-1"); expect(calls.confirm).not.toHaveBeenCalled();
  });
  it("a lost confirmation response only reads status, with no automatic retry", async () => {
    await store.openWorktreeReview("example-peer", "/srv/example");
    calls.confirm.mockRejectedValue("lost response"); calls.view.mockResolvedValue({ ...value("review-1", 3), unknown: true });
    await store.confirmWorktreeRemoval("one-use", true);
    expect(calls.confirm).toHaveBeenCalledTimes(1); expect(calls.view).toHaveBeenCalledWith("review-1"); expect(store.worktreeReview()?.unknown).toBe(true);
  });
});
