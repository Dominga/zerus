import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

vi.hoisted(() => {
  // store/ui.ts reads the page address and storage when it loads; the skin
  // adds its style sheet and the views register Solid's delegated events.
  const store = new Map<string, string>();
  vi.stubGlobal("location", { search: "", hash: "", href: "http://localhost/" });
  vi.stubGlobal("localStorage", { getItem: (k: string) => store.get(k) ?? null, setItem: (k: string, v: string) => { store.set(k, v); }, removeItem: (k: string) => { store.delete(k); } });
  const document = { addEventListener: () => {}, removeEventListener: () => {}, createElement: () => ({}), getElementById: () => null, head: { prepend: () => {}, append: () => {} }, visibilityState: "visible" };
  vi.stubGlobal("document", document);
  vi.stubGlobal("window", { document, addEventListener: () => {}, removeEventListener: () => {} });
  vi.stubGlobal("matchMedia", () => ({ matches: false, addEventListener: () => {}, removeEventListener: () => {} }));
});
vi.mock("../../bridge", () => ({
  ARCHIVED_ACTIVITY_BUSY: "busy",
  archivedActivity: vi.fn(() => new Promise(() => {})),
  activityPort: { follow: vi.fn(async () => {}), unfollow: vi.fn(async () => {}) },
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
  onScreensChanged: vi.fn(() => () => {}),
  screenVisible: vi.fn(async () => {}),
  // The detail's Terminal (store/terminal), loaded with SessionDetail.
  terminalStatus: vi.fn(() => new Promise(() => {})),
  terminalConnect: vi.fn(async () => {}),
  terminalWrite: vi.fn(async () => {}),
  terminalResize: vi.fn(async () => {}),
  terminalAck: vi.fn(async () => {}),
  terminalDisconnect: vi.fn(async () => {}),
  terminalRetarget: vi.fn(async () => {}),
  windowVisibility: { hidden: () => false, onChange: () => () => {} },
  // The selected session's composer (store/composer), opened by store/session.
  composerOpen: vi.fn(async () => null), composerEdit: vi.fn(async () => {}), composerPick: vi.fn(async () => {}), composerPasteImage: vi.fn(async () => {}),
  composerRemove: vi.fn(async () => {}), composerUndoRemove: vi.fn(async () => {}), composerAllowRetry: vi.fn(async () => {}), composerRefresh: vi.fn(async () => {}),
  composerTab: vi.fn(async () => {}), sendMessage: vi.fn(), sendAnswer: vi.fn(), sendCancel: vi.fn(async () => {}), sendInterrupted: vi.fn(async () => {}), clearContext: vi.fn(),
}));

import * as bridge from "../../bridge";
import { focusTranscript, paneStates, readsOlder, transcriptOf } from "./Activity";
import { activitySourceFor, detailTabs } from "./SessionDetail";
import { activity } from "../../store/session";
import { archiveActivity } from "../../store/archiveActivity";
import type { ActivitySource } from "../../store/activitySource";
import { followedFor } from "../../store/actions";
import { archive, archivedSession } from "../../store/archive";
import { applyBatch, toSession } from "../../store/catalog";
import { restoreSession } from "../../store/restores";
import { selected, setPage, setSelectedKey, setStateFilter } from "../../store/ui";
import { snapshotOf } from "./actions-ui";
import { emptyBatch, type ActivityEvent, type ArchivePage, type ArchivedActivity, type ViewRow } from "../../types";

const mocked = vi.mocked(bridge);
const flush = async () => { for (let i = 0; i < 10; i++) await Promise.resolve(); };
const uuid = (n: number) => `10000000-0000-4000-8000-${String(n).padStart(12, "0")}`;
const ev = (seq: number, type: string, detail: string): ActivityEvent => ({ seq, type, source: "codex", detail, at: 1_793_000_000 + seq });
const archivedRow = (n: number): ViewRow => ({ name: `codex/example/a${n}`, agent: "codex", state: "archived", status: "ended", archive_id: uuid(n), archived_at: 100 + n, run_id: `run-a${n}`, conversation_id: `conv-a${n}` });
const liveRow = (status: string, extra: Partial<ViewRow> = {}) => toSession("", "run:run-live", { name: "codex/example/live", agent: "codex", state: "running", status, run_id: "run-live", conversation_id: "conv-live", summary: "cargo test", ...extra });

/** A read-only source as the archived store gives it: recorded events, no older pages, no end. */
function readOnlySource(extra: Partial<ActivitySource> = {}): ActivitySource {
  return {
    target: () => ({ id: `\narchive:${uuid(1)}` }),
    events: () => [ev(1, "UserPromptSubmit", "Summarize the notes"), ev(2, "PreToolUse", "rg notes"), ev(3, "PostToolUse", "rg notes"), ev(4, "Stop", "Done.")],
    summary: () => ({ current_tool: "Bash", activity_summary: "Running rg" }),
    loaded: () => true,
    loadingOlder: () => false,
    olderDone: () => true,
    ended: () => false,
    loadOlder: vi.fn(),
    error: () => undefined,
    readOnly: true,
    ...extra,
  };
}

describe("the Activity view with a read-only source", () => {
  const rows = [liveRow("working", { phase: "tool" }), liveRow("attention"), liveRow("error"), liveRow("ready"), archivedSession("", "Local", archivedRow(1))];

  it("builds its transcript from the source and shows none of the live states", () => {
    const source = readOnlySource();
    expect(transcriptOf(source).map((i) => i.kind)).toEqual(["user", "tools", "agent"]);
    for (const row of rows) {
      expect(paneStates(source, row)).toEqual({ archived: false, working: false, attention: false, error: false, offline: false, ended: false, failure: "" });
    }
    // Its older pages are done: scrolling to the top reads nothing.
    expect(readsOlder(source)).toBe(false);
  });

  it("shows the source's failure as a notice", () => {
    const source = readOnlySource({ error: () => "Could not refresh activity: timed out" });
    expect(paneStates(source, rows[4]).failure).toBe("Could not refresh activity: timed out");
  });

  it("keeps the live states for a live source", () => {
    const live = readOnlySource({ readOnly: undefined, olderDone: () => false, ended: () => true, error: () => "x" });
    expect(paneStates(live, rows[0])).toMatchObject({ working: true, ended: true });
    // The live pane never showed its follow error; the notice is for read-only sources.
    expect(paneStates(live, rows[0]).failure).toBe("");
    expect(paneStates(live, rows[1]).attention).toBe(true);
    expect(paneStates(live, rows[2]).error).toBe(true);
    expect(paneStates(live, rows[4]).archived).toBe(true);
    expect(readsOlder(live)).toBe(true);
  });
});

describe("the session detail", () => {
  it("takes the archived Activity for archive keys only, and shows no Terminal tab for them", () => {
    expect(activitySourceFor(`\narchive:${uuid(1)}`)).toBe(archiveActivity);
    expect(activitySourceFor(`box\narchive:${uuid(1)}`)).toBe(archiveActivity);
    expect(activitySourceFor("\nrun:run-live")).toBe(activity);
    expect(detailTabs(`\narchive:${uuid(1)}`).map((t) => t.label)).toEqual(["Activity"]);
    expect(detailTabs("\nrun:run-live").map((t) => t.label)).toContain("Activity");
    // Qt hides only the Terminal for an archived row; Processes stays while enabled.
    const all = [{ id: "activity", label: "Activity" }, { id: "terminal", label: "Terminal" }, { id: "processes", label: "Processes" }];
    expect(detailTabs(`box\narchive:${uuid(1)}`, all).map((t) => t.label)).toEqual(["Activity", "Processes"]);
    expect(detailTabs("\nrun:run-live", all).map((t) => t.label)).toEqual(["Activity", "Terminal", "Processes"]);
  });
});

describe("the app's archived Activity", () => {
  beforeEach(() => vi.useFakeTimers());
  afterEach(() => vi.useRealTimers());

  it("reads the selected archived row, never follows it live, and drops it once the selection leaves", async () => {
    const pages: ((p: ArchivePage) => void)[] = [];
    mocked.archivePage.mockImplementation(() => new Promise((resolve) => { pages.push(resolve); }));
    const reads: { args: unknown[]; resolve(r: ArchivedActivity): void }[] = [];
    mocked.archivedActivity.mockImplementation((...args: unknown[]) => new Promise((resolve) => { reads.push({ args, resolve }); }));
    applyBatch({ ...emptyBatch(), computers: [{ computer: "", status: "live", attempt: 0 }], upserts: [{ computer: "", key: "run:run-live", row: { name: "codex/example/live", agent: "codex", state: "running", status: "ready", run_id: "run-live", conversation_id: "conv-live" } }] });
    setStateFilter("archived");
    pages.shift()!({ rows: [archivedRow(1)], next_cursor: "", previous_cursor: null, total: 1, position: 0 });
    await flush();

    const key = `\narchive:${uuid(1)}`;
    setSelectedKey(key);
    expect(reads.map((r) => r.args)).toEqual([["", "codex/example/a1", uuid(1), 0]]);
    expect(mocked.activityPort.follow).not.toHaveBeenCalled();
    expect(followedFor(key)).toEqual({ value: undefined, settled: true });
    // Recorded Activity has no composer: none is opened for the archived row.
    await flush();
    expect(mocked.composerOpen.mock.calls.every(([session]) => session === undefined)).toBe(true);
    reads[0].resolve({ events: [ev(1, "UserPromptSubmit", "Summarize the notes"), ev(2, "Stop", "Done.")], replace_events: false, conversation_id: "conv-a1", run_id: "run-a1", cleared_conversations: [], cursor: 2, summary: {} });
    await flush();
    expect(archiveActivity.events().map((e) => e.seq)).toEqual([1, 2]);

    // Another page stops the reads and keeps nothing; the Sessions page reads again from 0.
    setPage("settings");
    expect(archiveActivity.events()).toEqual([]);
    vi.advanceTimersByTime(10_000);
    expect(reads.length).toBe(1);
    setPage("sessions");
    expect(reads.map((r) => r.args[3])).toEqual([0, 0]);

    // A live row is followed live and the archived Activity drops everything.
    setSelectedKey("\nrun:run-live");
    expect(archiveActivity.target()).toBeUndefined();
    expect(archiveActivity.events()).toEqual([]);
    expect(mocked.activityPort.follow).toHaveBeenCalledWith(expect.objectContaining({ key: "run:run-live" }));
    await flush();
    expect(mocked.composerOpen).toHaveBeenLastCalledWith({ computer: "", key: "run:run-live", name: "codex/example/live", archive_id: null });
    vi.advanceTimersByTime(10_000);
    expect(reads.length).toBe(2);
  });

  it("keeps reading a restore's archived row while the archive reloads, as Qt's selected() keeps its source (3136-3139)", async () => {
    const pages: ((p: ArchivePage) => void)[] = [];
    mocked.archivePage.mockImplementation(() => new Promise((resolve) => { pages.push(resolve); }));
    const reads: { args: unknown[]; resolve(r: ArchivedActivity): void }[] = [];
    mocked.archivedActivity.mockImplementation((...args: unknown[]) => new Promise((resolve) => { reads.push({ args, resolve }); }));
    setStateFilter("archived");
    archive.refresh();
    await flush();
    pages.splice(0).forEach((answer) => answer({ rows: [{ ...archivedRow(3), resumable: true }], next_cursor: "", previous_cursor: null, total: 1, position: 0 }));
    await flush();
    const key = `\narchive:${uuid(3)}`;
    setSelectedKey(key);
    expect(reads.at(-1)?.args).toEqual(["", "codex/example/a3", uuid(3), 0]);
    reads.at(-1)!.resolve({ events: [ev(1, "Stop", "Done.")], replace_events: false, conversation_id: "conv-a3", run_id: "run-a3", cleared_conversations: [], cursor: 1, summary: {} });
    await flush();

    mocked.runAction.mockResolvedValueOnce({ outcome: "done", receipt: {}, cleanup: null });
    await restoreSession(snapshotOf(selected()!));
    await flush();
    // The page is reloading: the row is the pending restore's source, still selected and still read.
    expect(selected()?.archiveId).toBe(uuid(3));
    expect(archiveActivity.target()?.id).toBe(key);
    expect(archiveActivity.events().map((e) => e.seq)).toEqual([1]);
    const before = reads.length;
    vi.advanceTimersByTime(3000);
    expect(reads.length).toBe(before + 1);
    expect(reads.at(-1)?.args).toEqual(["", "codex/example/a3", uuid(3), 1]);
  });
});

// Task 6 fix round 2: without a mounted transcript, a hiding chip's focus goes to the field (composer-ui.ts moveFocusOff).
describe("focusTranscript", () => {
  it("answers false when no transcript is shown to take the focus", () => {
    expect(focusTranscript()).toBe(false);
  });
});
