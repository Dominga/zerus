import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { createSignal } from "solid-js";
import { createStore } from "solid-js/store";

// Resume and open terminal (zerus-5wmh.13.11.3; Qt SessionsWindow.cpp:5466-5476):
// the row's own Resume, then the identity-bound terminal of that same session.

vi.hoisted(() => { vi.stubGlobal("location", new URL("http://localhost/")); });
const fixture = vi.hoisted(() => ({
  change: vi.fn(async (_row: unknown): Promise<{ outcome: string; detail?: string }> => ({ outcome: "done" })),
  notify: vi.fn(),
  open: vi.fn(async (_target: unknown): Promise<string | null> => null),
  offline: false,
  /** The live row stillListed finds; the row asked for when unset. */
  current: undefined as unknown,
}));
vi.mock("./actions", () => ({ change: fixture.change, notify: fixture.notify, stillListed: (row: unknown) => fixture.current ?? row, updating: () => false }));
vi.mock("./batch", () => ({ running: () => false }));
vi.mock("./catalog", () => ({ catalog: { order: [], rows: {} }, computerOffline: () => fixture.offline }));
vi.mock("./notices", () => ({ toast: vi.fn() }));
vi.mock("../bridge", () => ({ sessionTerminalOpen: fixture.open }));

import { STATE_CHANGED, detailsFromInspection } from "../ui/sessions/actions-ui";
import { RESUMED_CHANGED, RESUMED_NOT_RUNNING, bindResumeTerminalConfirm, resumeAndOpenTerminal, resumedRow, sessionTerminalEntry, waitForResumed } from "./sessionTerminal";
import type { Session } from "../ui/sessions/types";
import type { ActionRow } from "../ui/sessions/actions-ui";

const saved = { key: "\nrun:old", computer: "", name: "claude/project/a", machine: "arch", cmd: "claude", state: "paused",
  // As the CLI reports a paused row: no live pane, so no terminal yet (zerus-5o1ns).
  runId: "old", sessionId: "session-1", conversationId: "conversation-1", terminalSupported: false, terminalReported: true,
  tracked: true, resumable: true, activity: "idle", processState: "stopped", allowed: "resume pause terminate kill" } as unknown as ActionRow;
const resumed = (extra: Partial<ActionRow> = {}) => ({ ...saved, key: "\nrun:new", state: "running", runId: "new", terminalSupported: true, ...extra } as ActionRow);

beforeEach(() => { vi.useFakeTimers(); fixture.offline = false; fixture.current = undefined; });
afterEach(() => { vi.useRealTimers(); vi.clearAllMocks(); });

describe("the same session after its resume", () => {
  it("is the saved session's computer and ID, now running with a run", () => {
    expect(resumedRow(saved, [saved])).toBeUndefined();
    expect(resumedRow(saved, [resumed({ runId: undefined })])).toBeUndefined();
    expect(resumedRow(saved, [resumed({ computer: "peer" })]), "another computer").toBeUndefined();
    expect(resumedRow(saved, [resumed({ sessionId: "session-2", name: saved.name })]), "a session reusing the name").toBeUndefined();
    expect(resumedRow(saved, [resumed({ name: "claude/project/renamed" })])).toMatchObject({ runId: "new" });
  });

  it("is found by its conversation when the saved row has no session ID, and refused when its conversation changed", () => {
    const old = { ...saved, sessionId: undefined } as ActionRow;
    expect(resumedRow(old, [resumed({ sessionId: undefined })])).toMatchObject({ runId: "new" });
    expect(resumedRow(saved, [resumed({ conversationId: "conversation-2" })])).toBe("changed");
  });
});

describe("Resume and open terminal", () => {
  it("opens the identity-bound terminal of the resumed run once it reports running", async () => {
    const [rows, setRows] = createSignal<ActionRow[]>([saved]);
    const pending = resumeAndOpenTerminal(saved, (row) => waitForResumed(row, 30_000, rows));
    await vi.advanceTimersByTimeAsync(10);
    expect(fixture.change).toHaveBeenCalledExactlyOnceWith(saved);
    expect(fixture.open).not.toHaveBeenCalled();
    setRows([resumed()]);
    await vi.advanceTimersByTimeAsync(10);
    await pending;
    expect(fixture.open).toHaveBeenCalledExactlyOnceWith({ computer: "", name: saved.name, run_id: "new", conversation_id: "conversation-1" });
    expect(fixture.notify).not.toHaveBeenCalled();
  });

  it("opens nothing after the bound, and says the session was resumed", async () => {
    const [rows] = createSignal<ActionRow[]>([saved]);
    const pending = resumeAndOpenTerminal(saved, (row) => waitForResumed(row, 30_000, rows));
    await vi.advanceTimersByTimeAsync(30_001);
    await pending;
    expect(fixture.open).not.toHaveBeenCalled();
    expect(fixture.notify).toHaveBeenCalledExactlyOnceWith("info", RESUMED_NOT_RUNNING.replace("%1", saved.name), true);
  });

  it("opens nothing when the session runs another conversation, or the Resume did not succeed", async () => {
    const [rows] = createSignal<ActionRow[]>([resumed({ conversationId: "conversation-2" })]);
    await resumeAndOpenTerminal(saved, (row) => waitForResumed(row, 30_000, rows));
    expect(fixture.notify).toHaveBeenCalledExactlyOnceWith("error", RESUMED_CHANGED, true);
    fixture.change.mockResolvedValueOnce({ outcome: "failed", detail: "refused" });
    const wait = vi.fn();
    await resumeAndOpenTerminal(saved, wait);
    expect(wait, "the Resume reports its own failure").not.toHaveBeenCalled();
    expect(fixture.open).not.toHaveBeenCalled();
  });
});

describe("the row menu's terminal entry", () => {
  const entry = (row: Partial<ActionRow>) => sessionTerminalEntry({ ...saved, ...row } as unknown as Session) as { label: string; disabled?: boolean; hint?: string; action: () => void };

  it("is Resume and open terminal for a paused or stopped session without a live pane, and Open terminal for a running one", () => {
    // The CLI reports terminal_supported false for every saved row (zerus-5o1ns).
    expect(entry({})).toMatchObject({ label: "Resume and open terminal", disabled: false });
    expect(entry({ state: "stopped", processState: "exited" })).toMatchObject({ label: "Resume and open terminal", disabled: false });
    expect(entry({ state: "running", runId: "live", terminalSupported: true })).toMatchObject({ label: "Open terminal", disabled: false });
  });

  it("says why it is off: offline, an agent without Terminal, OpenCode without attach, or no identity to resume by", () => {
    fixture.offline = true;
    expect(entry({})).toMatchObject({ disabled: true, hint: "Machine unavailable. Reconnect when it is online." });
    fixture.offline = false;
    expect(entry({ cmd: "dsh" })).toMatchObject({ disabled: true, hint: "This native adapter does not provide Terminal." });
    expect(entry({ cmd: "opencode", terminalAttachReported: true, terminalAttachSupported: false })).toMatchObject({ disabled: true, hint: "Terminal is unavailable for this session." });
    expect(entry({ cmd: "opencode", terminalAttachReported: true, terminalAttachSupported: true })).toMatchObject({ disabled: false });
    expect(entry({ sessionId: undefined, conversationId: undefined })).toMatchObject({ disabled: true, hint: "Refresh this session's identity before opening its terminal." });
  });

  it("asks once, then resumes only after the confirmation", async () => {
    const ask = vi.fn(async () => false);
    bindResumeTerminalConfirm(ask);
    entry({}).action();
    await vi.advanceTimersByTimeAsync(0);
    expect(ask).toHaveBeenCalledTimes(1);
    expect(fixture.change, "a cancelled question resumes nothing").not.toHaveBeenCalled();
    ask.mockResolvedValueOnce(true);
    entry({}).action();
    await vi.advanceTimersByTimeAsync(0);
    expect(fixture.change).toHaveBeenCalledTimes(1);
    expect(fixture.change.mock.calls[0][0]).toMatchObject({ key: saved.key, state: "paused", sessionId: "session-1" });
  });
});

// review-win-d's control on #1817: a live row is updated in place, so the
// answer must act on the snapshot the question showed.
it("a confirmed Resume never becomes Pause when the session resumed elsewhere meanwhile", async () => {
  const [live, setLive] = createStore<ActionRow>({ ...saved });
  fixture.current = live;
  let answer!: (value: boolean) => void;
  const ask = vi.fn((_shown: ActionRow) => new Promise<boolean>((resolve) => { answer = resolve; }));
  bindResumeTerminalConfirm(ask);
  const item = sessionTerminalEntry(live as unknown as Session) as { action: () => void };
  item.action();
  expect(ask.mock.calls[0][0].state).toBe("paused");
  // The original run resumes elsewhere while its confirmation is still open.
  setLive("state", "running");
  answer(true);
  await vi.advanceTimersByTimeAsync(0);
  expect(fixture.change, "the confirmed paused snapshot must be kept or refused, never retargeted to running/Pause").not.toHaveBeenCalled();
  expect(fixture.notify).toHaveBeenCalledExactlyOnceWith("error", STATE_CHANGED, true);
});

it("a confirmed Resume acts on the snapshot the question showed", async () => {
  const [live] = createStore<ActionRow>({ ...saved });
  fixture.current = live;
  let answer!: (value: boolean) => void;
  bindResumeTerminalConfirm(() => new Promise<boolean>((resolve) => { answer = resolve; }));
  (sessionTerminalEntry(live as unknown as Session) as { action: () => void }).action();
  answer(true);
  await vi.advanceTimersByTimeAsync(0);
  expect(fixture.change).toHaveBeenCalledTimes(1);
  const passed = fixture.change.mock.calls[0][0] as ActionRow;
  expect(passed).not.toBe(live);
  expect(passed).toMatchObject({ state: "paused", runId: "old", sessionId: "session-1" });
});

// review-win-c's controls on #1835.
it("a saved OpenCode row follows fresh attach details over the row's", () => {
  const row = { ...saved, cmd: "opencode", terminalAttachReported: true, terminalAttachSupported: true } as unknown as Session;
  expect(sessionTerminalEntry(row, detailsFromInspection({ terminal_attach_supported: false }))).toMatchObject({ disabled: true });
  expect(sessionTerminalEntry({ ...row, terminalAttachSupported: false }, detailsFromInspection({ terminal_attach_supported: true }))).toMatchObject({ disabled: false });
  expect(sessionTerminalEntry({ ...row, terminalAttachReported: false, terminalAttachSupported: true })).toMatchObject({ disabled: true });
});

it("a resumed row still needs its newly reported live terminal", async () => {
  const noPane = resumed({ terminalSupported: false, terminalReported: true });
  await resumeAndOpenTerminal(saved, () => Promise.resolve(noPane));
  expect(fixture.change).toHaveBeenCalledExactlyOnceWith(saved);
  expect(fixture.open).not.toHaveBeenCalled();
  expect(fixture.notify).toHaveBeenCalledWith("info", expect.stringContaining("Terminal is unavailable"), true);
});

it("a row changed between the match and the handoff never retargets the terminal (zerus-1gima)", async () => {
  const [live, setLive] = createStore<ActionRow>(resumed());
  const [rows, setRows] = createSignal<ActionRow[]>([saved]);
  const pending = resumeAndOpenTerminal(saved, (row) => waitForResumed(row, 30_000, rows));
  await vi.advanceTimersByTimeAsync(0);
  setRows([live]);
  queueMicrotask(() => setLive("conversationId", "replacement-conversation"));
  await vi.advanceTimersByTimeAsync(0);
  await pending;
  expect(fixture.open).not.toHaveBeenCalledWith(expect.objectContaining({ conversation_id: "replacement-conversation" }));
  // The matched identity goes to Rust, which compares it with the live row before attaching.
  expect(fixture.open).toHaveBeenCalledExactlyOnceWith({ computer: "", name: saved.name, run_id: "new", conversation_id: "conversation-1" });
});
