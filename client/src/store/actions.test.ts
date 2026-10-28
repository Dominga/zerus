import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { createRoot } from "solid-js";
import { createCatalog } from "./catalog";
import { createActions } from "./actions";
import * as bridge from "../bridge";
import { emptyBatch, type Action, type Batch, type Details, type Outcome, type RowOut, type ViewRow } from "../types";

vi.mock("../bridge", () => ({
  runAction: vi.fn(),
  inspectSession: vi.fn(),
  pendingReadMarks: vi.fn(),
}));

const mocked = vi.mocked(bridge);
const view = (extra: Partial<ViewRow> = {}): ViewRow => ({
  name: "claude/sample-project/a", status: "ready", phase: "idle", unread: false, run_id: "run-1", conversation_id: "conv-1", state: "running", ...extra,
});
const rowOut = (extra: Partial<ViewRow> = {}): RowOut => ({ computer: "", key: "run:a", row: view(extra) });
const batchOf = (part: Partial<Batch>): Batch => ({ ...emptyBatch(), ...part });
const done = (cleanup: Outcome extends infer O ? (O extends { cleanup: infer C } ? C : never) : never = null): Outcome => ({ outcome: "done", receipt: {}, cleanup });
const details = (extra: Partial<Details> = {}): Details => ({
  run_id: "run-1", conversation_id: "conv-1", auth_required: false, expected_id: "", resume_message_can_send: true, allowed_actions: null, action_reasons: {}, ...extra,
});

function setup(entry = rowOut()) {
  const catalog = createCatalog();
  catalog.applyBatch(batchOf({ upserts: [entry] }));
  const actions = createActions(catalog.catalog);
  return { ...catalog, ...actions };
}

beforeEach(() => {
  vi.useFakeTimers();
  vi.setSystemTime(1_790_000_000_000);
  mocked.runAction.mockReset();
  mocked.inspectSession.mockReset();
  mocked.pendingReadMarks.mockReset();
  mocked.pendingReadMarks.mockResolvedValue([]);
  mocked.inspectSession.mockResolvedValue(details());
});
afterEach(() => vi.useRealTimers());

describe("perform", () => {
  it("sets the pending state while the action runs and clears it after", async () => {
    await createRoot(async (dispose) => {
      const a = setup();
      let finish: (o: Outcome) => void = () => {};
      mocked.runAction.mockReturnValue(new Promise((r) => { finish = r; }));
      expect(a.updating("", "claude/sample-project/a")).toBe(false);
      const p = a.perform(rowOut(), { kind: "pause" });
      expect(a.updating("", "claude/sample-project/a")).toBe(true);
      finish(done());
      await p;
      expect(a.updating("", "claude/sample-project/a")).toBe(false);
      dispose();
    });
  });

  it("refuses without a call when the run changed", async () => {
    await createRoot(async (dispose) => {
      const a = setup();
      const out = await a.perform(rowOut({ run_id: "run-0" }), { kind: "pause" });
      expect(mocked.runAction).not.toHaveBeenCalled();
      expect(out.outcome).toBe("failed");
      expect(a.notices().map((n) => n.text)).toEqual(["This session changed. Open its actions again."]);
      expect(a.notices()[0].tone).toBe("info");
      dispose();
    });
  });

  it("refuses when the row is gone", async () => {
    await createRoot(async (dispose) => {
      const a = setup();
      await a.perform({ computer: "", key: "run:zz", row: view() }, { kind: "pause" });
      expect(mocked.runAction).not.toHaveBeenCalled();
      expect(a.notices()).toHaveLength(1);
      dispose();
    });
  });

  it("words the success of each action, and says nothing for interrupt and mark read", async () => {
    await createRoot(async (dispose) => {
      const a = setup();
      mocked.runAction.mockResolvedValue(done());
      const cases: [Action, string | undefined][] = [
        [{ kind: "rename", new_name: "claude/sample-project/b", archive_id: null }, "Session renamed. Refreshing…"],
        [{ kind: "pause" }, "Session updated. Refreshing…"],
        [{ kind: "resume" }, "Session updated. Refreshing…"],
        [{ kind: "archive" }, "Session updated. Refreshing…"],
        [{ kind: "terminate" }, "Session updated. Refreshing…"],
        [{ kind: "interrupt", turn_started: 1 }, undefined],
        [{ kind: "mark_read", reply_id: "r", attention_signature: null }, undefined],
      ];
      for (const [action, text] of cases) {
        for (const n of a.notices()) a.dismiss(n.id);
        await a.perform(rowOut(), action);
        expect(a.notices().map((n) => n.text)).toEqual(text ? [text] : []);
        if (text) {
          expect(a.notices()[0].tone).toBe("info");
          expect(a.notices()[0].sticky).toBe(false);
          vi.advanceTimersByTime(6000);
          expect(a.notices()).toEqual([]);
        }
      }
      dispose();
    });
  });

  it("keeps a failure as a sticky error", async () => {
    await createRoot(async (dispose) => {
      const a = setup();
      mocked.runAction.mockResolvedValue({ outcome: "failed", detail: "hgs pause failed" });
      await a.perform(rowOut(), { kind: "pause" });
      vi.advanceTimersByTime(60_000);
      expect(a.notices()).toMatchObject([{ tone: "error", text: "hgs pause failed", sticky: true }]);
      dispose();
    });
  });

  it("shows an unknown outcome (a rejected call) as a failure and a busy session without retry", async () => {
    await createRoot(async (dispose) => {
      const a = setup();
      mocked.runAction.mockRejectedValue("lost the answer");
      const out = await a.perform(rowOut(), { kind: "pause" });
      expect(out).toEqual({ outcome: "failed", detail: "lost the answer" });
      expect(a.notices()[0]).toMatchObject({ tone: "error", sticky: true });
      vi.advanceTimersByTime(20_000);
      expect(mocked.runAction).toHaveBeenCalledTimes(1);
      dispose();
    });
  });

  it("shows a kept worktree as a sticky warning with the reason and a removed one as info", async () => {
    await createRoot(async (dispose) => {
      const a = setup();
      mocked.runAction.mockResolvedValue(done({ status: "kept", path: "/home/example/wt", common_dir: "/home/example/.git", reason: "uncommitted changes" }));
      await a.perform(rowOut(), { kind: "archive" });
      expect(a.notices().find((n) => n.sticky)).toMatchObject({ tone: "warn", text: "Worktree kept for review: uncommitted changes" });
      mocked.runAction.mockResolvedValue(done({ status: "unconfirmed", path: "/home/example/wt", common_dir: "/home/example/.git", reason: "process use unknown" }));
      await a.perform(rowOut(), { kind: "terminate" });
      expect(a.notices().some((n) => n.text === "Worktree kept for review: process use unknown" && n.sticky)).toBe(true);
      for (const n of a.notices()) a.dismiss(n.id);
      mocked.runAction.mockResolvedValue(done({ status: "removed", path: "/home/example/wt", common_dir: "/home/example/.git", reason: null }));
      await a.perform(rowOut(), { kind: "archive" });
      expect(a.notices().map((n) => n.text)).toContain("Worktree removed automatically. The branch and archived conversation are kept.");
      dispose();
    });
  });

  it("calls runAction exactly once per perform, even 10 s later", async () => {
    await createRoot(async (dispose) => {
      const a = setup();
      mocked.runAction.mockResolvedValue({ outcome: "failed", detail: "x" });
      await a.perform(rowOut(), { kind: "resume" });
      vi.advanceTimersByTime(10_000);
      await Promise.resolve();
      expect(mocked.runAction).toHaveBeenCalledTimes(1);
      expect(mocked.runAction).toHaveBeenCalledWith({ computer: "", name: "claude/sample-project/a", run_id: "run-1", conversation_id: "conv-1" }, { kind: "resume" });
      dispose();
    });
  });

  it("caps the notices at 5", async () => {
    await createRoot(async (dispose) => {
      const a = setup();
      mocked.runAction.mockResolvedValue({ outcome: "failed", detail: "x" });
      for (let i = 0; i < 8; i++) await a.perform(rowOut(), { kind: "pause" });
      expect(a.notices()).toHaveLength(5);
      dispose();
    });
  });
});

describe("detailsFor", () => {
  const flush = async () => { for (let i = 0; i < 5; i++) await Promise.resolve(); };

  it("fetches once per selection, refetches on a run change and drops a mismatch", async () => {
    await createRoot(async (dispose) => {
      const a = setup();
      expect(a.detailsFor(rowOut())).toBeUndefined();
      a.detailsFor(rowOut());
      await flush();
      expect(mocked.inspectSession).toHaveBeenCalledTimes(1);
      expect(mocked.inspectSession).toHaveBeenCalledWith({ computer: "", name: "claude/sample-project/a", run_id: "run-1", conversation_id: "conv-1" }, null);
      expect(a.detailsFor(rowOut())?.run_id).toBe("run-1");
      expect(mocked.inspectSession).toHaveBeenCalledTimes(1);

      a.applyBatch(batchOf({ upserts: [rowOut({ run_id: "run-2" })] }));
      mocked.inspectSession.mockResolvedValue(details({ run_id: "run-1" }));
      expect(a.detailsFor(rowOut({ run_id: "run-2" }))).toBeUndefined();
      await flush();
      expect(mocked.inspectSession).toHaveBeenCalledTimes(2);
      expect(a.detailsFor(rowOut({ run_id: "run-2" }))).toBeUndefined();
      expect(mocked.inspectSession).toHaveBeenCalledTimes(2);
      dispose();
    });
  });

  it("refetches when the state changes and runs one inspect per session at a time", async () => {
    await createRoot(async (dispose) => {
      const a = setup();
      let finish: (d: Details) => void = () => {};
      mocked.inspectSession.mockReturnValueOnce(new Promise((r) => { finish = r; }));
      a.detailsFor(rowOut());
      a.detailsFor(rowOut({ state: "paused" }));
      expect(mocked.inspectSession).toHaveBeenCalledTimes(1);
      finish(details());
      await flush();
      expect(mocked.inspectSession).toHaveBeenCalledTimes(2);
      dispose();
    });
  });
});

describe("read-mark retry", () => {
  const flush = async () => { for (let i = 0; i < 5; i++) await Promise.resolve(); };
  const mark = { computer: "", name: "claude/sample-project/a", run_id: "run-1", conversation_id: "conv-1", reply_id: "r1", attention_signature: null };

  it("retries a pending mark on a batch that still shows unread, at most once per 5 s", async () => {
    await createRoot(async (dispose) => {
      mocked.pendingReadMarks.mockResolvedValue([mark]);
      mocked.runAction.mockResolvedValue({ outcome: "failed", detail: "offline" });
      const a = setup();
      await a.loadReadMarks();
      const unread = rowOut({ unread: true, reply_id: "r1" });
      const b = batchOf({ upserts: [unread] });
      a.applyBatch(b);
      a.observeBatch(b);
      await flush();
      expect(mocked.runAction).toHaveBeenCalledTimes(1);
      expect(mocked.runAction.mock.calls[0][1]).toEqual({ kind: "mark_read", reply_id: "r1", attention_signature: null });
      a.observeBatch(b);
      vi.advanceTimersByTime(4000);
      a.observeBatch(b);
      await flush();
      expect(mocked.runAction).toHaveBeenCalledTimes(1);
      vi.advanceTimersByTime(1500);
      a.observeBatch(b);
      await flush();
      expect(mocked.runAction).toHaveBeenCalledTimes(2);
      dispose();
    });
  });

  it("does not retry a row that no longer shows unread", async () => {
    await createRoot(async (dispose) => {
      mocked.pendingReadMarks.mockResolvedValue([mark]);
      const a = setup();
      await a.loadReadMarks();
      const b = batchOf({ upserts: [rowOut({ unread: false })] });
      a.applyBatch(b);
      a.observeBatch(b);
      await flush();
      expect(mocked.runAction).not.toHaveBeenCalled();
      dispose();
    });
  });
});
