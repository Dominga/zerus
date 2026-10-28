import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { createRoot } from "solid-js";
import { createCatalog, toSession } from "./catalog";
import { createActions, type Followed } from "./actions";
import * as bridge from "../bridge";
import { emptyBatch, type Action, type Batch, type Details, type Outcome, type RowOut, type ViewRow } from "../types";

vi.mock("../bridge", () => ({
  runAction: vi.fn(),
  inspectSession: vi.fn(),
  pendingReadMarks: vi.fn(),
  sessionReference: vi.fn(),
  copyCommand: vi.fn(),
  copyText: vi.fn(),
}));

const mocked = vi.mocked(bridge);
const view = (extra: Partial<ViewRow> = {}): ViewRow => ({
  name: "claude/sample-project/a", status: "ready", phase: "idle", unread: false, run_id: "run-1", conversation_id: "conv-1", state: "running",
  tracked: true, agent: "claude", ...extra,
});
const rowOut = (extra: Partial<ViewRow> = {}, key = "run:a"): RowOut => ({ computer: "", key, row: view(extra) });
/** The catalog's own row for a view row, as the menus and dialogs snapshot it. */
const session = (extra: Partial<ViewRow> = {}, key = "run:a") => toSession("", key, view(extra));
const batchOf = (part: Partial<Batch>): Batch => ({ ...emptyBatch(), ...part });
const done = (cleanup: Outcome extends infer O ? (O extends { cleanup: infer C } ? C : never) : never = null): Outcome => ({ outcome: "done", receipt: {}, cleanup });
const details = (extra: Partial<Details> = {}): Details => ({
  run_id: "run-1", conversation_id: "conv-1", auth_required: false, expected_id: "", resume_message_can_send: true, allowed_actions: null, action_reasons: {},
  interrupt_supported: false, turn_started: 0, phase: "", ...extra,
});
const flush = async () => { for (let i = 0; i < 5; i++) await Promise.resolve(); };

function setup(entry = rowOut(), online: (computer: string) => boolean = () => true) {
  const catalog = createCatalog();
  catalog.applyBatch(batchOf({ upserts: [entry] }));
  const actions = createActions(catalog.catalog, online);
  return { ...catalog, ...actions };
}

beforeEach(() => {
  vi.useFakeTimers();
  vi.setSystemTime(1_790_000_000_000);
  mocked.runAction.mockReset();
  mocked.inspectSession.mockReset();
  mocked.pendingReadMarks.mockReset();
  mocked.sessionReference.mockReset();
  mocked.copyCommand.mockReset();
  mocked.copyText.mockReset();
  mocked.copyText.mockResolvedValue(undefined);
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
      const p = a.perform(session(), { kind: "pause" });
      expect(a.updating("", "claude/sample-project/a")).toBe(true);
      finish(done());
      await p;
      expect(a.updating("", "claude/sample-project/a")).toBe(false);
      dispose();
    });
  });

  it("refuses without a call when the listed row has another run than the menu's", async () => {
    await createRoot(async (dispose) => {
      const a = setup();
      const menu = session();
      a.applyBatch(batchOf({ upserts: [rowOut({ run_id: "run-2" })] }));
      const out = await a.perform(menu, { kind: "pause" });
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
      await a.perform(session({}, "run:zz"), { kind: "pause" });
      expect(mocked.runAction).not.toHaveBeenCalled();
      expect(a.notices()).toHaveLength(1);
      dispose();
    });
  });

  it("words the success of each action, and says nothing for mark read", async () => {
    await createRoot(async (dispose) => {
      const a = setup();
      mocked.runAction.mockResolvedValue(done());
      const cases: [Action, string | undefined][] = [
        [{ kind: "rename", new_name: "claude/sample-project/b", archive_id: null }, "Session renamed. Refreshing…"],
        [{ kind: "pause" }, "Session updated. Refreshing…"],
        [{ kind: "resume" }, "Session updated. Refreshing…"],
        [{ kind: "archive" }, "Session updated. Refreshing…"],
        [{ kind: "terminate" }, "Session updated. Refreshing…"],
        [{ kind: "kill" }, "Session updated. Refreshing…"],
        [{ kind: "mark_read", reply_id: null, attention_signature: null }, undefined],
      ];
      for (const [action, text] of cases) {
        for (const n of a.notices()) a.dismiss(n.id);
        await a.perform(session(), action);
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

  it("words Stop as Qt does: sent, already ended, or failed", async () => {
    await createRoot(async (dispose) => {
      const a = setup();
      const stop: Action = { kind: "interrupt", turn_started: 1793000000.5 };
      mocked.runAction.mockResolvedValueOnce(done());
      await a.perform(session(), stop);
      expect(a.notices()).toMatchObject([{ tone: "info", sticky: false,
        text: "Interrupt sent to claude/sample-project/a (this computer). Waiting for the agent to stop…" }]);
      vi.advanceTimersByTime(6000);
      mocked.runAction.mockResolvedValueOnce({ outcome: "failed", detail: "hgs: This turn is no longer working" });
      await a.perform(session(), stop);
      expect(a.notices()).toMatchObject([{ tone: "info", sticky: false,
        text: "Stop claude/sample-project/a (this computer): the turn already ended or is waiting for input. No action needed." }]);
      vi.advanceTimersByTime(6000);
      mocked.runAction.mockResolvedValueOnce({ outcome: "failed", detail: "Request timed out. Check Terminal before retrying." });
      await a.perform(session(), stop);
      expect(a.notices()).toMatchObject([{ tone: "error", sticky: true,
        text: "Stop claude/sample-project/a (this computer) failed: Request timed out. Check Terminal before retrying.\nOpen Terminal to check the agent before trying again." }]);
      expect(mocked.runAction.mock.calls[0]).toEqual([{ computer: "", name: "claude/sample-project/a", run_id: "run-1", conversation_id: "conv-1" }, stop]);
      dispose();
    });
  });

  it("keeps a failure as a sticky error", async () => {
    await createRoot(async (dispose) => {
      const a = setup();
      mocked.runAction.mockResolvedValue({ outcome: "failed", detail: "hgs pause failed" });
      await a.perform(session(), { kind: "pause" });
      vi.advanceTimersByTime(60_000);
      expect(a.notices()).toMatchObject([{ tone: "error", text: "hgs pause failed", sticky: true }]);
      dispose();
    });
  });

  it("shows an unknown outcome (a rejected call) as a failure and never retries it", async () => {
    await createRoot(async (dispose) => {
      const a = setup();
      mocked.runAction.mockRejectedValue("lost the answer");
      const out = await a.perform(session(), { kind: "pause" });
      expect(out).toEqual({ outcome: "failed", detail: "lost the answer" });
      expect(a.notices()[0]).toMatchObject({ tone: "error", sticky: true });
      vi.advanceTimersByTime(20_000);
      expect(mocked.runAction).toHaveBeenCalledTimes(1);
      dispose();
    });
  });

  it("shows a kept worktree as a sticky warning with the reason as given and a removed one as info", async () => {
    await createRoot(async (dispose) => {
      const a = setup();
      mocked.runAction.mockResolvedValue(done({ status: "kept", path: "/home/example/wt", common_dir: "/home/example/.git", reason: "uncommitted changes" }));
      await a.perform(session(), { kind: "archive" });
      expect(a.notices().find((n) => n.sticky)).toMatchObject({ tone: "warn", text: "Worktree kept for review: uncommitted changes" });
      for (const n of a.notices()) a.dismiss(n.id);
      mocked.runAction.mockResolvedValue(done({ status: "unconfirmed", path: "/home/example/wt", common_dir: "/home/example/.git", reason: null }));
      await a.perform(session(), { kind: "terminate" });
      expect(a.notices().find((n) => n.sticky)?.text).toBe("Worktree kept for review: ");
      for (const n of a.notices()) a.dismiss(n.id);
      mocked.runAction.mockResolvedValue(done({ status: "removed", path: "/home/example/wt", common_dir: "/home/example/.git", reason: null }));
      await a.perform(session(), { kind: "archive" });
      expect(a.notices().map((n) => n.text)).toContain("Worktree removed automatically. The branch and archived conversation are kept.");
      dispose();
    });
  });

  it("calls runAction exactly once per perform, even 10 s later", async () => {
    await createRoot(async (dispose) => {
      const a = setup();
      mocked.runAction.mockResolvedValue({ outcome: "failed", detail: "x" });
      await a.perform(session(), { kind: "resume" });
      vi.advanceTimersByTime(10_000);
      await Promise.resolve();
      expect(mocked.runAction).toHaveBeenCalledTimes(1);
      expect(mocked.runAction).toHaveBeenCalledWith({ computer: "", name: "claude/sample-project/a", run_id: "run-1", conversation_id: "conv-1" }, { kind: "resume" });
      dispose();
    });
  });

  it("caps the notices at 5, dropping non-sticky ones before a sticky warning", async () => {
    await createRoot(async (dispose) => {
      const a = setup();
      mocked.runAction.mockResolvedValueOnce({ outcome: "failed", detail: "kept" });
      await a.perform(session(), { kind: "pause" });
      mocked.runAction.mockResolvedValue(done());
      for (let i = 0; i < 6; i++) await a.perform(session(), { kind: "pause" });
      expect(a.notices()).toHaveLength(5);
      expect(a.notices()[0]).toMatchObject({ text: "kept", sticky: true });
      mocked.runAction.mockResolvedValue({ outcome: "failed", detail: "x" });
      for (let i = 0; i < 8; i++) await a.perform(session(), { kind: "pause" });
      expect(a.notices()).toHaveLength(5);
      expect(a.notices().every((n) => n.sticky)).toBe(true);
      dispose();
    });
  });
});

describe("targets", () => {
  it("names a session without a run for kill, rename, pause, resume and archive", async () => {
    await createRoot(async (dispose) => {
      const bare = rowOut({ run_id: null, conversation_id: null, tracked: false }, "name:claude/sample-project/a");
      const a = setup(bare);
      mocked.runAction.mockResolvedValue(done());
      const row = toSession("", bare.key, bare.row);
      const actions: Action[] = [{ kind: "kill" }, { kind: "rename", new_name: "claude/sample-project/b", archive_id: null }, { kind: "pause" }, { kind: "resume" }, { kind: "archive" }];
      for (const action of actions) await a.perform(row, action);
      expect(mocked.runAction.mock.calls.map((c) => c[0])).toEqual(actions.map(() => ({ computer: "", name: "claude/sample-project/a", run_id: "", conversation_id: "" })));
      dispose();
    });
  });

  it("keeps interrupt, mark read and terminate to an identified run", async () => {
    await createRoot(async (dispose) => {
      const bare = rowOut({ run_id: null, conversation_id: null }, "name:claude/sample-project/a");
      const a = setup(bare);
      const row = toSession("", bare.key, bare.row);
      for (const action of [{ kind: "interrupt", turn_started: 1 }, { kind: "mark_read", reply_id: null, attention_signature: null }, { kind: "terminate" }] as Action[]) {
        expect((await a.perform(row, action)).outcome).toBe("failed");
      }
      expect(mocked.runAction).not.toHaveBeenCalled();
      expect(a.notices().map((n) => n.text)).toEqual(Array(3).fill("This session changed. Open its actions again."));
      dispose();
    });
  });
});

describe("rechecks after a confirmation", () => {
  it("ends the session with the dialog's call while it still runs", async () => {
    await createRoot(async (dispose) => {
      const a = setup();
      mocked.runAction.mockResolvedValue(done());
      await a.end(session(), "kill");
      expect(mocked.runAction.mock.calls[0][1]).toEqual({ kind: "kill" });
      dispose();
    });
  });

  it("makes no call and no notice when the machine went offline before the click", async () => {
    await createRoot(async (dispose) => {
      const a = setup(rowOut(), () => false);
      expect((await a.end(session(), "terminate")).outcome).toBe("failed");
      expect(mocked.runAction).not.toHaveBeenCalled();
      expect(a.notices()).toEqual([]);
      dispose();
    });
  });

  it("refuses with Qt's text when the session stopped running meanwhile", async () => {
    await createRoot(async (dispose) => {
      const a = setup();
      const dialog = session();
      a.applyBatch(batchOf({ upserts: [rowOut({ state: "paused" })] }));
      expect((await a.end(dialog, "terminate")).outcome).toBe("failed");
      const menu = session();
      a.applyBatch(batchOf({ upserts: [rowOut({ state: "running" })] }));
      a.applyBatch(batchOf({ upserts: [rowOut({ state: "stopped" })] }));
      await a.change(menu);
      expect(mocked.runAction).not.toHaveBeenCalled();
      expect(a.notices()).toMatchObject([
        { tone: "error", sticky: true, text: "The session state changed. Open its actions again." },
        { tone: "error", sticky: true, text: "The session state changed. Open its actions again." },
      ]);
      dispose();
    });
  });

  it("pauses a running row from its menu and resumes a stopped one", async () => {
    await createRoot(async (dispose) => {
      const a = setup();
      mocked.runAction.mockResolvedValue(done());
      await a.change(session());
      a.applyBatch(batchOf({ upserts: [rowOut({ state: "stopped" })] }));
      await a.change(session({ state: "stopped" }));
      expect(mocked.runAction.mock.calls.map((c) => c[1])).toEqual([{ kind: "pause" }, { kind: "resume" }]);
      dispose();
    });
  });

  it("marks read only the reply and attention the menu showed", async () => {
    await createRoot(async (dispose) => {
      const a = setup(rowOut({ unread: true, reply_id: "r1", attention_signature: "s1", shared_read: true }));
      const menu = session({ unread: true, reply_id: "r1", attention_signature: "s1", shared_read: true });
      a.applyBatch(batchOf({ upserts: [rowOut({ unread: true, reply_id: "r2", attention_signature: "s1", shared_read: true })] }));
      const out = await a.perform(menu, { kind: "mark_read", reply_id: menu.replyId ?? null, attention_signature: menu.attentionSignature ?? null });
      expect(out.outcome).toBe("failed");
      expect(mocked.runAction).not.toHaveBeenCalled();
      expect(a.notices()).toMatchObject([{ tone: "error", sticky: true, text: "A new reply or question arrived. Open the menu again to mark it as read." }]);
      mocked.runAction.mockResolvedValue(done());
      const fresh = session({ unread: true, reply_id: "r2", attention_signature: "s1", shared_read: true });
      await a.perform(fresh, { kind: "mark_read", reply_id: "r2", attention_signature: "s1" });
      expect(mocked.runAction.mock.calls[0][1]).toEqual({ kind: "mark_read", reply_id: "r2", attention_signature: "s1" });
      dispose();
    });
  });
});

describe("detailsFor", () => {
  const state = (extra: Record<string, unknown> = {}) => ({ run_id: "run-1", conversation_id: "conv-1", phase: "tool", turn_started: 5, interrupt_supported: true, ...extra });

  it("takes the followed session's state value and runs no inspect", async () => {
    await createRoot(async (dispose) => {
      const a = setup();
      const d = a.detailsFor(session(), true, { value: state(), settled: true });
      await flush();
      expect(d).toMatchObject({ run_id: "run-1", interrupt_supported: true, turn_started: 5, phase: "tool" });
      expect(mocked.inspectSession).not.toHaveBeenCalled();
      dispose();
    });
  });

  it("inspects once the subscription answered without a value of this run, and not before", async () => {
    await createRoot(async (dispose) => {
      const a = setup();
      const waiting: Followed = { value: undefined, settled: false };
      expect(a.detailsFor(session(), true, waiting)).toBeUndefined();
      await flush();
      expect(mocked.inspectSession).not.toHaveBeenCalled();
      const other: Followed = { value: state({ run_id: "run-0" }), settled: true };
      expect(a.detailsFor(session(), true, other)).toBeUndefined();
      await flush();
      expect(mocked.inspectSession).toHaveBeenCalledTimes(1);
      expect(mocked.inspectSession).toHaveBeenCalledWith({ computer: "", name: "claude/sample-project/a", run_id: "run-1", conversation_id: "conv-1" }, null);
      expect(a.detailsFor(session(), true, other)?.run_id).toBe("run-1");
      expect(mocked.inspectSession).toHaveBeenCalledTimes(1);
      dispose();
    });
  });

  it("never inspects while the machine is offline", async () => {
    await createRoot(async (dispose) => {
      const a = setup();
      expect(a.detailsFor(session(), false)).toBeUndefined();
      await flush();
      expect(mocked.inspectSession).not.toHaveBeenCalled();
      dispose();
    });
  });

  it("refetches on a run change and drops a reply for another run", async () => {
    await createRoot(async (dispose) => {
      const a = setup();
      a.detailsFor(session(), true);
      await flush();
      expect(a.detailsFor(session(), true)?.run_id).toBe("run-1");
      a.applyBatch(batchOf({ upserts: [rowOut({ run_id: "run-2" })] }));
      expect(a.detailsFor(session({ run_id: "run-2" }), true)).toBeUndefined();
      await flush();
      expect(mocked.inspectSession).toHaveBeenCalledTimes(2);
      expect(a.detailsFor(session({ run_id: "run-2" }), true)).toBeUndefined();
      expect(mocked.inspectSession).toHaveBeenCalledTimes(2);
      dispose();
    });
  });

  it("refetches when the state changes and runs one inspect per session at a time", async () => {
    await createRoot(async (dispose) => {
      const a = setup();
      let finish: (d: Details) => void = () => {};
      mocked.inspectSession.mockReturnValueOnce(new Promise((r) => { finish = r; }));
      a.detailsFor(session(), true);
      a.detailsFor(session({ state: "paused" }), true);
      expect(mocked.inspectSession).toHaveBeenCalledTimes(1);
      finish(details());
      await flush();
      expect(mocked.inspectSession).toHaveBeenCalledTimes(2);
      dispose();
    });
  });
});

describe("read-mark retry", () => {
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

  it("also retries for rows inside a computer's snapshot", async () => {
    await createRoot(async (dispose) => {
      mocked.pendingReadMarks.mockResolvedValue([mark]);
      mocked.runAction.mockResolvedValue(done());
      const a = setup();
      await a.loadReadMarks();
      const b = batchOf({ replaces: [{ computer: "", rows: [rowOut({ unread: true, reply_id: "r1" })] }] });
      a.applyBatch(b);
      a.observeBatch(b);
      await flush();
      expect(mocked.runAction).toHaveBeenCalledTimes(1);
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

  it("retries quietly: no notice and no Updating, and a write still runs meanwhile", async () => {
    await createRoot(async (dispose) => {
      mocked.pendingReadMarks.mockResolvedValue([mark]);
      let finish: (o: Outcome) => void = () => {};
      mocked.runAction.mockReturnValueOnce(new Promise((r) => { finish = r; }));
      const a = setup();
      await a.loadReadMarks();
      const b = batchOf({ upserts: [rowOut({ unread: true, reply_id: "r1" })] });
      a.applyBatch(b);
      a.observeBatch(b);
      await flush();
      expect(a.reading("", "claude/sample-project/a")).toBe(true);
      expect(a.updating("", "claude/sample-project/a")).toBe(false);
      mocked.runAction.mockResolvedValue(done());
      await a.perform(session({ unread: true, reply_id: "r1" }), { kind: "terminate" });
      expect(mocked.runAction.mock.calls.map((c) => c[1].kind)).toEqual(["mark_read", "terminate"]);
      finish({ outcome: "failed", detail: "offline" });
      await flush();
      expect(a.reading("", "claude/sample-project/a")).toBe(false);
      expect(a.notices().map((n) => n.text)).toEqual(["Session updated. Refreshing…"]);
      dispose();
    });
  });

  it("a user's mark read fails loudly and holds off the quiet retry for 5 s", async () => {
    await createRoot(async (dispose) => {
      const a = setup(rowOut({ unread: true, reply_id: "r1", shared_read: true }));
      await a.loadReadMarks();
      mocked.runAction.mockResolvedValue({ outcome: "failed", detail: "offline" });
      mocked.pendingReadMarks.mockResolvedValue([mark]);
      await a.perform(session({ unread: true, reply_id: "r1", shared_read: true }), { kind: "mark_read", reply_id: "r1", attention_signature: null });
      expect(a.notices()).toMatchObject([{ tone: "error", text: "offline" }]);
      expect(mocked.runAction).toHaveBeenCalledTimes(1);
      const b = batchOf({ upserts: [rowOut({ unread: true, reply_id: "r1", shared_read: true })] });
      a.applyBatch(b);
      a.observeBatch(b);
      await flush();
      expect(mocked.runAction).toHaveBeenCalledTimes(1);
      vi.advanceTimersByTime(5000);
      a.observeBatch(b);
      await flush();
      expect(mocked.runAction).toHaveBeenCalledTimes(2);
      dispose();
    });
  });

  it("does not retry an older mark against a newer reply", async () => {
    await createRoot(async (dispose) => {
      mocked.pendingReadMarks.mockResolvedValue([mark]);
      const a = setup();
      await a.loadReadMarks();
      const b = batchOf({ upserts: [rowOut({ unread: true, reply_id: "r2" })] });
      a.applyBatch(b);
      a.observeBatch(b);
      await flush();
      expect(mocked.runAction).not.toHaveBeenCalled();
      dispose();
    });
  });
});

describe("pending guard", () => {
  it("makes no call and no notice for a second write while one is pending", async () => {
    await createRoot(async (dispose) => {
      const a = setup();
      let finish: (o: Outcome) => void = () => {};
      mocked.runAction.mockReturnValue(new Promise((r) => { finish = r; }));
      const first = a.perform(session(), { kind: "pause" });
      expect(a.interrupting("", "claude/sample-project/a")).toBe(false);
      await a.perform(session(), { kind: "pause" });
      expect(mocked.runAction).toHaveBeenCalledTimes(1);
      expect(a.notices()).toEqual([]);
      finish(done());
      await first;
      dispose();
    });
  });

  it("sends Stop beside a pending write, and a running Stop shows no Updating", async () => {
    await createRoot(async (dispose) => {
      const a = setup();
      const finishes: ((o: Outcome) => void)[] = [];
      mocked.runAction.mockImplementation(() => new Promise((r) => { finishes.push(r); }));
      const write = a.perform(session(), { kind: "pause" });
      const stop = a.perform(session(), { kind: "interrupt", turn_started: 1 });
      expect(mocked.runAction).toHaveBeenCalledTimes(2);
      expect(a.updating("", "claude/sample-project/a")).toBe(true);
      expect(a.interrupting("", "claude/sample-project/a")).toBe(true);
      finishes[0](done());
      await write;
      expect(a.updating("", "claude/sample-project/a")).toBe(false);
      expect(a.interrupting("", "claude/sample-project/a")).toBe(true);
      await a.perform(session(), { kind: "interrupt", turn_started: 1 });
      expect(mocked.runAction).toHaveBeenCalledTimes(2);
      finishes[1](done());
      await stop;
      expect(a.interrupting("", "claude/sample-project/a")).toBe(false);
      dispose();
    });
  });

  it("treats a busy rejection as a silent no-op", async () => {
    await createRoot(async (dispose) => {
      const a = setup();
      mocked.runAction.mockRejectedValue("busy");
      await a.perform(session(), { kind: "pause" });
      expect(a.notices()).toEqual([]);
      dispose();
    });
  });
});

describe("copy", () => {
  it("copies silently and reports a failure as a sticky error", async () => {
    await createRoot(async (dispose) => {
      const a = setup();
      await a.copy("claude/sample-project/a");
      expect(mocked.copyText).toHaveBeenCalledWith("claude/sample-project/a");
      expect(a.notices()).toEqual([]);
      mocked.copyText.mockRejectedValue("Nothing to copy.");
      await a.copy("x");
      expect(a.notices()).toEqual([expect.objectContaining({ tone: "error", sticky: true, text: "Zerus: could not copy\nNothing to copy." })]);
      dispose();
    });
  });

  it("copies a command with the archive id only for an archived row", async () => {
    await createRoot(async (dispose) => {
      const a = setup();
      mocked.copyCommand.mockResolvedValue("hgs attach x");
      await a.copyCommand(session());
      expect(mocked.copyCommand).toHaveBeenCalledWith("", "claude/sample-project/a", null);
      expect(mocked.copyText).toHaveBeenCalledWith("hgs attach x");
      a.applyBatch(batchOf({ upserts: [rowOut({ state: "archived", archive_id: "ar1" })] }));
      await a.copyCommand(session({ state: "archived", archive_id: "ar1" }));
      expect(mocked.copyCommand).toHaveBeenLastCalledWith("", "claude/sample-project/a", "ar1");
      dispose();
    });
  });

  it("copies the reference of the details' run, then says so", async () => {
    await createRoot(async (dispose) => {
      const a = setup();
      mocked.sessionReference.mockResolvedValue("zerus://session?x=1");
      await a.copyReference(session(), details({ run_id: "run-9", conversation_id: "conv-9" }));
      expect(mocked.sessionReference).toHaveBeenCalledWith({ computer: "", name: "claude/sample-project/a", run_id: "run-9", conversation_id: "conv-9" }, null);
      expect(mocked.copyText).toHaveBeenCalledWith("zerus://session?x=1");
      expect(a.notices()).toEqual([expect.objectContaining({ tone: "info", text: "Session reference copied", sticky: false })]);
      a.applyBatch(batchOf({ upserts: [rowOut({ state: "archived", archive_id: "ar1" })] }));
      await a.copyReference(session({ state: "archived", archive_id: "ar1" }));
      expect(mocked.sessionReference).toHaveBeenLastCalledWith({ computer: "", name: "claude/sample-project/a", run_id: "run-1", conversation_id: "conv-1" }, "ar1");
      dispose();
    });
  });

  it("shows a reference failure as a sticky error and copies nothing", async () => {
    await createRoot(async (dispose) => {
      const a = setup();
      mocked.sessionReference.mockRejectedValue("hgs: no such run");
      await a.copyReference(session());
      expect(mocked.copyText).not.toHaveBeenCalled();
      expect(a.notices()).toEqual([expect.objectContaining({ tone: "error", sticky: true, text: "hgs: no such run" })]);
      dispose();
    });
  });
});

describe("copy click-time recheck", () => {
  const gone = "This session is no longer available. Open its actions again.";
  it("copies nothing and says so when the row is gone", async () => {
    await createRoot(async (dispose) => {
      const a = setup();
      const old = session();
      a.applyBatch(batchOf({ removes: [{ computer: "", key: "run:a" }] }));
      for (const out of [await a.copyName(old), await a.copyCommand(old), await a.copyFolder(old), await a.copyReference(old)]) expect(out.outcome).toBe("failed");
      expect(mocked.copyText).not.toHaveBeenCalled();
      expect(a.notices()).toHaveLength(4);
      expect(a.notices()[0]).toMatchObject({ text: gone, sticky: true });
      dispose();
    });
  });

  it("copies nothing when the conversation changed", async () => {
    await createRoot(async (dispose) => {
      const a = setup();
      const old = session();
      a.applyBatch(batchOf({ upserts: [rowOut({ conversation_id: "conv-2" })] }));
      await a.copyName(old);
      expect(mocked.copyText).not.toHaveBeenCalled();
      expect(a.notices()).toEqual([expect.objectContaining({ text: gone, sticky: true })]);
      dispose();
    });
  });

  it("copies the current name and folder of a renamed row", async () => {
    await createRoot(async (dispose) => {
      const a = setup();
      const old = session();
      a.applyBatch(batchOf({ upserts: [rowOut({ name: "claude/sample-project/b", cwd: "/home/example/b" })] }));
      await a.copyName(old);
      expect(mocked.copyText).toHaveBeenLastCalledWith("claude/sample-project/b");
      await a.copyFolder(old);
      expect(mocked.copyText).toHaveBeenLastCalledWith("/home/example/b");
      expect(a.notices()).toEqual([]);
      dispose();
    });
  });

  it("copies nothing for an archived row without an archive id", async () => {
    await createRoot(async (dispose) => {
      const a = setup(rowOut({ state: "archived" }));
      const out = await a.copyCommand(session({ state: "archived" }));
      expect(out.outcome).toBe("failed");
      expect(mocked.copyCommand).not.toHaveBeenCalled();
      expect(mocked.copyText).not.toHaveBeenCalled();
      dispose();
    });
  });
});
