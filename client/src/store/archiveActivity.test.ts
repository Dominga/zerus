import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

vi.hoisted(() => {
  // store/ui.ts reads the page address and storage when it loads.
  const store = new Map<string, string>();
  vi.stubGlobal("location", { search: "", hash: "", href: "http://localhost/" });
  vi.stubGlobal("localStorage", { getItem: (k: string) => store.get(k) ?? null, setItem: (k: string, v: string) => { store.set(k, v); }, removeItem: (k: string) => { store.delete(k); } });
});
vi.mock("../bridge", () => ({
  ARCHIVED_ACTIVITY_BUSY: "busy",
  archivedActivity: vi.fn(() => new Promise(() => {})),
  archivePage: vi.fn(() => new Promise(() => {})),
  cancelArchivePages: vi.fn(async () => {}),
  runAction: vi.fn(),
  inspectSession: vi.fn(async () => null),
  pendingReadMarks: vi.fn(async () => []),
  attentionMarks: vi.fn(async () => ({ marks: {}, replies: {} })),
  updateAttentionMarks: vi.fn(async () => {}),
  queueReadMarks: vi.fn(async () => []),
  sessionReference: vi.fn(),
  copyCommand: vi.fn(),
  copyText: vi.fn(),
  setNotifications: vi.fn(),
}));

import { createMemo, createRoot, createSignal } from "solid-js";
import { ARCHIVED_EVENTS_MAX, ARCHIVED_POLL_MS, ARCHIVED_RETRY_MS, createArchiveActivity, type ArchiveTarget } from "./archiveActivity";
import { activityReadOnly } from "./activitySource";
import type { ActivityEvent, ArchivedActivity } from "../types";

const uuid = (n: number) => `10000000-0000-4000-8000-${String(n).padStart(12, "0")}`;
const target = (n: number, computer = ""): ArchiveTarget => ({ id: `${computer}\narchive:${uuid(n)}`, computer, name: `codex/example/a${n}`, archiveId: uuid(n) });
const ev = (seq: number, extra: Partial<ActivityEvent> = {}): ActivityEvent => ({ seq, type: "PostToolUse", source: "codex", tool: "Bash", detail: `step ${seq}`, at: 1_793_000_000 + seq, ...extra });
const reply = (events: ActivityEvent[], extra: Partial<ArchivedActivity> = {}): ArchivedActivity => ({
  events, replace_events: false, conversation_id: "conv-1", run_id: "run-1", cleared_conversations: [],
  cursor: events.reduce((m, e) => Math.max(m, e.seq ?? 0), 0), summary: { state: "archived" }, ...extra,
});
const flush = async () => { for (let i = 0; i < 10; i++) await Promise.resolve(); };
const seqs = (list: readonly ActivityEvent[]) => list.map((e) => e.seq);

interface Call { computer: string; name: string; archiveId: string; after: number; resolve(r: ArchivedActivity): void; reject(e: unknown): void }

function setup(options: { showing?: boolean; defaultClock?: boolean } = {}) {
  const calls: Call[] = [];
  const read = vi.fn((computer: string, name: string, archiveId: string, after: number) =>
    new Promise<ArchivedActivity>((resolve, reject) => { calls.push({ computer, name, archiveId, after, resolve, reject }); }));
  const [showing, setShowing] = createSignal(options.showing ?? true);
  const [offline, setOffline] = createSignal<string[]>([]);
  let store!: ReturnType<typeof createArchiveActivity>;
  // The fake timers move Date; the app's default clock is the monotonic one.
  const now = options.defaultClock ? undefined : () => Date.now();
  const online = (computer: string) => !offline().includes(computer);
  const dispose = createRoot((d) => { store = createArchiveActivity({ read, showing, now, online }); return d; });
  /** Answers the call at `i` (default: the latest) and lets its handlers run. */
  const answer = async (r: ArchivedActivity, i = calls.length - 1) => { calls[i].resolve(r); await flush(); };
  const fail = async (error: unknown, i = calls.length - 1) => { calls[i].reject(error); await flush(); };
  return { store, read, calls, answer, fail, setShowing, setOffline, dispose };
}

beforeEach(() => { vi.useFakeTimers(); vi.setSystemTime(new Date(2026, 9, 28, 12, 0, 0)); });
afterEach(() => vi.useRealTimers());

describe("archived Activity (Qt inspect 3467-3492, acceptInspection 3587-3660)", () => {
  it("reads a new selection from cursor 0 after resetting the events, summary and error", async () => {
    const t = setup();
    t.store.follow(target(1));
    expect(t.calls.map((c) => [c.computer, c.name, c.archiveId, c.after])).toEqual([["", "codex/example/a1", uuid(1), 0]]);
    expect(t.store.loaded()).toBe(false);
    await t.answer(reply([ev(1), ev(2)]));
    expect(seqs(t.store.events())).toEqual([1, 2]);
    expect(t.store.summary()).toEqual({ state: "archived" });
    expect(t.store.loaded()).toBe(true);
    expect(t.store.target()?.id).toBe(`\narchive:${uuid(1)}`);

    vi.advanceTimersByTime(ARCHIVED_POLL_MS);
    await t.fail("timed out");
    expect(t.store.error()).toBe("Could not refresh activity: timed out");

    t.store.follow(target(2, "box"));
    expect(t.store.events()).toEqual([]);
    expect(t.store.summary()).toBeUndefined();
    expect(t.store.error()).toBeUndefined();
    expect(t.store.loaded()).toBe(false);
    expect(t.calls.at(-1)).toMatchObject({ computer: "box", name: "codex/example/a2", archiveId: uuid(2), after: 0 });
    // Following the same row again changes nothing.
    t.store.follow(target(2, "box"));
    expect(t.calls.length).toBe(3);
    t.dispose();
  });

  it("reads its attachments in the archived row's conversation, as the last read named it", async () => {
    const t = setup();
    // Read as Activity reads it: through a memo, which reruns only on a reactive change.
    let scope!: () => ReturnType<typeof t.store.attachmentScope>;
    const disposeScope = createRoot((d) => { scope = createMemo(() => t.store.attachmentScope()); return d; });
    expect(scope()).toBeUndefined();
    t.store.follow(target(1, "box"));
    expect(scope()).toEqual({ computer: "box", name: "codex/example/a1", conversation_id: "", archive_id: uuid(1) });
    await t.answer(reply([ev(1)]));
    expect(scope()?.conversation_id).toBe("conv-1");
    // Another conversation's window names its own.
    vi.advanceTimersByTime(ARCHIVED_POLL_MS);
    await t.answer(reply([ev(1)], { conversation_id: "conv-2" }));
    expect(scope()?.conversation_id).toBe("conv-2");
    disposeScope();
    t.dispose();
  });

  it("polls every 2500 ms from the start of the previous read, never two at a time", async () => {
    const t = setup();
    t.store.follow(target(1));
    // The read still runs after the interval: no second one starts.
    vi.advanceTimersByTime(ARCHIVED_POLL_MS * 2);
    expect(t.calls.length).toBe(1);
    // It ends after 5 s: the next starts at once, with the reply's cursor.
    await t.answer(reply([ev(1), ev(2)], { cursor: 7 }));
    vi.advanceTimersByTime(0);
    expect(t.calls.map((c) => c.after)).toEqual([0, 7]);
    // An answer after 1 s: the next read starts 2500 ms after this one started.
    vi.advanceTimersByTime(1000);
    await t.answer(reply([], { cursor: 7 }));
    vi.advanceTimersByTime(ARCHIVED_POLL_MS - 1000 - 1);
    expect(t.calls.length).toBe(2);
    vi.advanceTimersByTime(1);
    expect(t.calls.length).toBe(3);
    // A failure keeps the cadence.
    await t.fail("exit code 2");
    vi.advanceTimersByTime(ARCHIVED_POLL_MS - 1);
    expect(t.calls.length).toBe(3);
    vi.advanceTimersByTime(1);
    expect(t.calls.length).toBe(4);
    t.dispose();
  });

  it("measures the cadence on a monotonic clock, as Qt's QElapsedTimer: a wall clock set back does not stall it", async () => {
    const t = setup({ defaultClock: true });
    t.store.follow(target(1));
    vi.setSystemTime(new Date(2026, 9, 28, 11, 0, 0));
    await t.answer(reply([ev(1)]));
    vi.advanceTimersByTime(ARCHIVED_POLL_MS);
    expect(t.calls.map((c) => c.after)).toEqual([0, 1]);
    t.dispose();
  });

  it("ignores a busy answer, as Qt's `return false`, and tries again at the next 500 ms tick", async () => {
    const t = setup();
    t.store.follow(target(1));
    await t.fail("busy");
    expect(t.store.error()).toBeUndefined();
    expect(t.store.loaded()).toBe(false);
    vi.advanceTimersByTime(ARCHIVED_RETRY_MS - 1);
    expect(t.calls.length).toBe(1);
    vi.advanceTimersByTime(1);
    expect(t.calls.map((c) => c.after)).toEqual([0, 0]);
    await t.answer(reply([ev(3)]));
    expect(seqs(t.store.events())).toEqual([3]);
    t.dispose();
  });

  it("appends events above the cursor and not yet known, in the reply's order, and takes the reply's cursor", async () => {
    const t = setup();
    t.store.follow(target(1));
    await t.answer(reply([ev(1), ev(2), ev(3)]));
    const first = t.store.events();
    vi.advanceTimersByTime(ARCHIVED_POLL_MS);
    expect(t.calls.at(-1)?.after).toBe(3);
    // 2 and 3 are at or below the cursor; an event without a seq never appends (its seq reads as 0).
    await t.answer(reply([ev(2), ev(3), ev(4), { type: "Stop", detail: "no seq" }, ev(5)], { cursor: 5 }));
    expect(seqs(t.store.events())).toEqual([1, 2, 3, 4, 5]);
    expect(t.store.events().slice(0, 3)).toEqual(first);
    vi.advanceTimersByTime(ARCHIVED_POLL_MS);
    expect(t.calls.at(-1)?.after).toBe(5);
    // Nothing new keeps the list as it is.
    const before = t.store.events();
    await t.answer(reply([ev(5)], { cursor: 5 }));
    expect(t.store.events()).toBe(before);
    t.dispose();
  });

  it("replaces the events when the reply says so", async () => {
    const t = setup();
    t.store.follow(target(1));
    await t.answer(reply([ev(1), ev(2)]));
    vi.advanceTimersByTime(ARCHIVED_POLL_MS);
    await t.answer(reply([{ type: "UserPromptSubmit", detail: "window" }, ev(9)], { replace_events: true, cursor: 0, summary: { history_truncated: true } }));
    expect(t.store.events().map((e) => e.detail)).toEqual(["window", "step 9"]);
    expect(t.store.summary()).toEqual({ history_truncated: true });
    vi.advanceTimersByTime(ARCHIVED_POLL_MS);
    expect(t.calls.at(-1)?.after).toBe(0);
    t.dispose();
  });

  it("starts over on another conversation, reading its window again at once without doubling it", async () => {
    const t = setup();
    t.store.follow(target(1));
    await t.answer(reply([ev(1), ev(2)], { conversation_id: "conv-1" }));
    vi.advanceTimersByTime(ARCHIVED_POLL_MS);
    vi.advanceTimersByTime(1000);
    await t.fail("timed out");
    vi.advanceTimersByTime(ARCHIVED_POLL_MS);
    await t.answer(reply([ev(7)], { conversation_id: "conv-2", cursor: 7 }));
    expect(seqs(t.store.events())).toEqual([7]);
    // The cursor of the previous conversation never skips the new history: the next read asks from 0, at once.
    expect(t.calls.at(-1)?.after).toBe(0);
    expect(t.calls.length).toBe(4);
    await t.answer(reply([ev(7), ev(8)], { conversation_id: "conv-2", cursor: 8 }));
    expect(seqs(t.store.events())).toEqual([7, 8]);
    expect(t.store.error()).toBeUndefined();
    // A conversation cleared into this one continues the same timeline.
    vi.advanceTimersByTime(ARCHIVED_POLL_MS);
    await t.answer(reply([ev(9)], { conversation_id: "conv-3", cleared_conversations: ["conv-2"], cursor: 9 }));
    expect(seqs(t.store.events())).toEqual([7, 8, 9]);
    vi.advanceTimersByTime(0);
    expect(t.calls.at(-1)?.after).toBe(8);
    t.dispose();
  });

  it("drops a late answer or failure of a row that is no longer selected", async () => {
    const t = setup();
    t.store.follow(target(1));
    t.store.follow(target(2));
    await t.answer(reply([ev(1)]), 0);
    expect(t.store.events()).toEqual([]);
    expect(t.store.loaded()).toBe(false);
    await t.answer(reply([ev(5)], { conversation_id: "conv-a2" }), 1);
    expect(seqs(t.store.events())).toEqual([5]);
    vi.advanceTimersByTime(ARCHIVED_POLL_MS);
    expect(t.calls[2]).toMatchObject({ archiveId: uuid(2), after: 5 });
    // Back to the first row: the second row's read still running neither fails nor fills it.
    t.store.follow(target(1));
    expect(t.calls[3]).toMatchObject({ archiveId: uuid(1), after: 0 });
    await t.fail("timed out", 2);
    expect(t.store.error()).toBeUndefined();
    expect(t.store.loaded()).toBe(false);
    expect(t.store.target()?.id).toBe(`\narchive:${uuid(1)}`);
    await t.answer(reply([ev(1)]), 3);
    expect(seqs(t.store.events())).toEqual([1]);
    t.dispose();
  });

  it("shows a failure as Qt words it and clears it with the next answer", async () => {
    const t = setup();
    t.store.follow(target(1));
    await t.fail("cannot start 'hgs': not found");
    expect(t.store.error()).toBe("Could not refresh activity: cannot start 'hgs': not found");
    expect(t.store.loaded()).toBe(true);
    vi.advanceTimersByTime(ARCHIVED_POLL_MS);
    await t.fail(new Error("Invalid activity response"));
    expect(t.store.error()).toBe("Could not refresh activity: Invalid activity response");
    vi.advanceTimersByTime(ARCHIVED_POLL_MS);
    await t.answer(reply([ev(1)]));
    expect(t.store.error()).toBeUndefined();
    expect(seqs(t.store.events())).toEqual([1]);
    t.dispose();
  });

  it(`keeps at most ${ARCHIVED_EVENTS_MAX} events, the oldest dropped first (acceptInspection 3651)`, async () => {
    expect(ARCHIVED_EVENTS_MAX).toBe(500);
    const t = setup();
    t.store.follow(target(1));
    const many = Array.from({ length: ARCHIVED_EVENTS_MAX + 100 }, (_, i) => ev(i + 1));
    await t.answer(reply(many));
    expect(t.store.events().length).toBe(ARCHIVED_EVENTS_MAX);
    expect(t.store.events()[0].seq).toBe(101);
    vi.advanceTimersByTime(ARCHIVED_POLL_MS);
    await t.answer(reply([ev(ARCHIVED_EVENTS_MAX + 101), ev(ARCHIVED_EVENTS_MAX + 102)]));
    expect(t.store.events().length).toBe(ARCHIVED_EVENTS_MAX);
    expect(t.store.events()[0].seq).toBe(103);
    expect(t.store.events().at(-1)?.seq).toBe(ARCHIVED_EVENTS_MAX + 102);
    t.dispose();
  });

  it("keeps nothing and reads nothing once the selection leaves the row", async () => {
    const t = setup();
    t.store.follow(target(1));
    await t.answer(reply([ev(1), ev(2)]));
    vi.advanceTimersByTime(ARCHIVED_POLL_MS);
    expect(t.calls.length).toBe(2);
    t.store.follow(undefined);
    expect(t.store.target()).toBeUndefined();
    expect(t.store.events()).toEqual([]);
    expect(t.store.summary()).toBeUndefined();
    expect(t.store.loaded()).toBe(false);
    await t.answer(reply([ev(3)]));
    expect(t.store.events()).toEqual([]);
    vi.advanceTimersByTime(ARCHIVED_POLL_MS * 10);
    expect(t.calls.length).toBe(2);
    t.dispose();
  });

  it("reads only while the Sessions page shows; hiding it stops the timer and drops everything", async () => {
    const t = setup({ showing: false });
    t.store.follow(target(1));
    vi.advanceTimersByTime(ARCHIVED_POLL_MS * 4);
    expect(t.calls.length).toBe(0);
    t.setShowing(true);
    expect(t.calls.map((c) => c.after)).toEqual([0]);
    await t.answer(reply([ev(1), ev(2)]));
    t.setShowing(false);
    expect(t.store.events()).toEqual([]);
    expect(t.store.loaded()).toBe(false);
    expect(t.store.target()?.id).toBe(`\narchive:${uuid(1)}`);
    vi.advanceTimersByTime(ARCHIVED_POLL_MS * 4);
    expect(t.calls.length).toBe(1);
    t.setShowing(true);
    expect(t.calls.map((c) => c.after)).toEqual([0, 0]);
    // A read that was running when the page hid does not apply afterwards.
    t.setShowing(false);
    t.setShowing(true);
    await t.answer(reply([ev(4)]), 1);
    expect(t.store.events()).toEqual([]);
    t.dispose();
    vi.advanceTimersByTime(ARCHIVED_POLL_MS * 4);
    expect(t.calls.length).toBe(3);
  });

  it("reads nothing while the row's computer is offline, keeps its events and reads again once it is back (Qt inspect 3472)", async () => {
    const t = setup();
    t.store.follow(target(1, "box"));
    await t.answer(reply([ev(1), ev(2)]));
    t.setOffline(["box"]);
    vi.advanceTimersByTime(ARCHIVED_POLL_MS * 4);
    expect(t.calls.length).toBe(1);
    expect(seqs(t.store.events())).toEqual([1, 2]);
    t.setOffline([]);
    expect(t.calls.map((c) => c.after)).toEqual([0, 2]);
    // Another computer going offline changes nothing for this row.
    t.setOffline(["other"]);
    await t.answer(reply([ev(3)]));
    vi.advanceTimersByTime(ARCHIVED_POLL_MS);
    expect(t.calls.length).toBe(3);
    // A row selected while its computer is offline is read once it is back.
    t.setOffline(["box"]);
    t.store.follow(target(4, "box"));
    expect(t.calls.length).toBe(3);
    expect(t.store.events()).toEqual([]);
    t.setOffline([]);
    expect(t.calls.at(-1)).toMatchObject({ computer: "box", archiveId: uuid(4), after: 0 });
    t.dispose();
  });

  it("is a read-only source: no older pages, no end, no live state", () => {
    const t = setup();
    t.store.follow(target(1));
    expect(t.store.readOnly).toBe(true);
    expect(t.store.loadingOlder()).toBe(false);
    expect(t.store.olderDone()).toBe(true);
    expect(t.store.ended()).toBe(false);
    t.store.loadOlder();
    expect(t.calls.length).toBe(1);
    t.dispose();
  });
});

describe("activityReadOnly", () => {
  it("is true for archive keys, as catalog keys or row ids", () => {
    expect(activityReadOnly(`archive:${uuid(1)}`)).toBe(true);
    expect(activityReadOnly(`\narchive:${uuid(1)}`)).toBe(true);
    expect(activityReadOnly(`box\narchive:${uuid(1)}`)).toBe(true);
    expect(activityReadOnly("\nrun:run-1")).toBe(false);
    expect(activityReadOnly("run:archive:x")).toBe(false);
    expect(activityReadOnly("")).toBe(false);
  });
});
