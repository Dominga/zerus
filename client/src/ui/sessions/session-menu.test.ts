import { afterEach, describe, expect, it, vi } from "vitest";

vi.hoisted(() => {
  // store/ui.ts reads the page address and storage when it loads.
  const store = new Map<string, string>();
  vi.stubGlobal("location", { search: "", hash: "", href: "http://localhost/" });
  vi.stubGlobal("navigator", { platform: "MacIntel" });
  vi.stubGlobal("localStorage", { getItem: (k: string) => store.get(k) ?? null, setItem: (k: string, v: string) => { store.set(k, v); }, removeItem: (k: string) => { store.delete(k); } });
  // The list module registers Solid's delegated events when it loads; nothing renders here.
  // The composer it loads with Saved drafts listens for the page going.
  vi.stubGlobal("window", { document: { addEventListener: () => {} }, addEventListener: () => {} });
});
vi.mock("../../bridge", () => ({
  // The Teams store subscribes when a session module imports it.
  onTeamDecided: vi.fn(async () => () => {}), teamsFollow: vi.fn(async () => {}),
  // The task store reads further board pages through this bridge call.
  readTaskPage: vi.fn(async () => ({ tasks: [], page: { next: null } })),
  // The native workspace store, loaded by the session entry points.
  nativeUiOpen: vi.fn(), nativeUiRetire: vi.fn(async () => {}), nativeUiVisible: vi.fn(async () => {}),
  nativeUiBounds: vi.fn(async () => {}), nativeUiBrowser: vi.fn(async () => {}), onNativeUi: vi.fn(async () => () => {}),
  archivePage: vi.fn(() => new Promise(() => {})),
  resetArchivePages: vi.fn(async () => {}),
  cancelArchivePages: vi.fn(async () => {}),
  runAction: vi.fn(),
  setKeepWarm: vi.fn(async () => ({})),
  inspectSession: vi.fn(async () => null),
  pendingReadMarks: vi.fn(async () => []),
  attentionMarks: vi.fn(async () => ({ marks: {}, replies: {} })),
  updateAttentionMarks: vi.fn(async () => {}),
  queueReadMarks: vi.fn(async () => []),
  sessionReference: vi.fn(),
  copyCommand: vi.fn(),
  copyText: vi.fn(),
  setNotifications: vi.fn(),
  // The composer and Saved drafts, loaded with the list's Saved drafts button.
  windowVisibility: { hidden: () => false, onChange: () => () => {} },
  composerOpen: vi.fn(async () => null), composerVisibility: vi.fn(async () => {}), composerEdit: vi.fn(async () => {}), composerPick: vi.fn(async () => {}), composerPasteImage: vi.fn(async () => {}),
  composerRemove: vi.fn(async () => {}), composerUndoRemove: vi.fn(async () => {}), composerAllowRetry: vi.fn(async () => {}), composerRefresh: vi.fn(async () => {}),
  composerTab: vi.fn(async () => {}), sendMessage: vi.fn(), sendAnswer: vi.fn(), sendCancel: vi.fn(async () => {}), sendInterrupted: vi.fn(async () => {}), sendInterruptFailed: vi.fn(async () => {}), clearContext: vi.fn(),
  localRetry: vi.fn(), localDelete: vi.fn(async () => {}), localInspect: vi.fn(async () => null), inboxRemove: vi.fn(async () => ({ outcome: "removed" as const })),
  savedDrafts: vi.fn(), draftConflicts: vi.fn(), savedDraft: vi.fn(), draftRecover: vi.fn(), draftRestore: vi.fn(),
  composerClearControl: vi.fn(async () => null),
  sessionFolderStatus: vi.fn(async () => ({ folder: false, shell: false, folder_hint: "Unavailable" })),
  sessionFolderOpen: vi.fn(async () => ""), sessionTerminalOpen: vi.fn(async () => null),
  // Content search (store/contentSearch.ts) and the creation cards start with the list.
  inTauri: () => false,
  onCreations: vi.fn(async () => () => {}),
}));

import { sessionContextMenu, sessionMenu } from "./SessionList";
import { expandedEntries } from "./session-extra-entries";
import { archivedSession } from "../../store/archive";
import { applyBatch, toSession } from "../../store/catalog";
import { composer } from "../../store/composer";
import { composerClearControl, composerOpen, copyText, inspectSession, runAction, sessionFolderOpen, sessionFolderStatus, sessionReference, sessionTerminalOpen, setKeepWarm } from "../../bridge";
import { detailTab, menu, visibleMenu, multi, peek, selectedKey, setDetailTab, setMenu, setMulti, setPeek, setSelectedKey, setStripMode, setSubagentOpen, subagentOpen, type MenuEntry } from "../../store/ui";
import { clearMulti } from "./organize";
import { setSetting } from "../../store/settings";
import { emptyBatch, type ComposerView, type ViewRow } from "../../types";

const labels = (entries: MenuEntry[]) => entries.map((e) => ("separator" in e ? "---" : "heading" in e ? `# ${e.heading}` : e.label));
const archived = (agent: string): ViewRow => ({
  name: `${agent}/example/a1`, agent, state: "archived", status: "ended", archive_id: "10000000-0000-4000-8000-000000000001",
  archived_at: 100, run_id: "run-a1", conversation_id: "conv-a1", resumable: true,
});

afterEach(() => {
  setMenu(undefined); clearMulti(); setSelectedKey(undefined); setSubagentOpen(undefined);
  setPeek(false); setStripMode(false); setDetailTab("activity"); composer.apply(undefined);
  vi.clearAllMocks();
});

describe("the owner's compact session row menu", () => {
  it("an archived row has no terminal entry and no Mute or Mark group, and no doubled separators", () => {
    const expected = [
      "Compact session row", "---", "Copy restore command", "Copy session ID", "---",
      "Rename…", "Move to project…", "---", "Forget archived session…",
    ];
    expect(labels(sessionMenu(archivedSession("", "Local", archived("codex"))))).toEqual(expected);
    expect(labels(sessionMenu(archivedSession("", "Local", archived("dsh"))))).toEqual(expected);
  });

  it("a live row starts with Mark then Mute and keeps its terminal entry", () => {
    const live = toSession("", "run:run-live", { name: "codex/example/live", agent: "codex", state: "running", status: "ready", run_id: "run-live", conversation_id: "conv-live" });
    expect(labels(sessionMenu(live))).toEqual([
      "Mark needs attention", "Mute", "Keep cache warm", "---", "Compact session row", "---",
      "Open terminal", "Copy connect command", "Copy session ID", "---",
      "Clear session…", "Rename…", "Move to project…", "---", "Terminate session…",
    ]);
  });

  it("Keep cache warm follows the host's state and toggles off or one hour with its revision", () => {
    const host = (over: Record<string, unknown> = {}) => ({
      enabled: false, until: null, revision: 3, state: "off", next_at: null, reason: "", turn_active: false,
      supported: true, reason_code: "", ttl_seconds: 3600, ttl_source: "reported", interval_seconds: 3000, cache_expires_at: null, ...over,
    }) as ViewRow["keep_warm"];
    const row = (keep?: ViewRow["keep_warm"], extra: Partial<ViewRow> = {}): ViewRow => ({
      name: "codex/example/warm", agent: "codex", state: "running", status: "ready", run_id: "run-warm", conversation_id: "conv-warm", ...(keep ? { keep_warm: keep } : {}), ...extra,
    });
    const entry = (raw: ViewRow) => sessionMenu(toSession("", "run:run-warm", raw)).find((e) => "label" in e && e.label === "Keep cache warm") as Extract<MenuEntry, { label: string }>;
    const target = { computer: "", name: "codex/example/warm", run_id: "run-warm", conversation_id: "conv-warm" };
    // A host that reports no keep-warm state has nothing to toggle.
    expect(entry(row())).toMatchObject({ checked: false, disabled: true });
    expect(entry(row(host()))).toMatchObject({ checked: false, disabled: false, hint: "Send the idle agent a one-word service message before its prompt cache expires" });
    // Turned on from the menu: one hour, with the revision the listing reports.
    entry(row(host())).action?.();
    expect(setKeepWarm).toHaveBeenLastCalledWith(target, { kind: "hours", hours: 1 }, 3);
    // While it is on, the menu turns it off; the hours set in Details stay until then.
    const on = entry(row(host({ enabled: true, until: 4_000_000_000, state: "waiting", revision: 4 })));
    expect(on.checked).toBe(true);
    on.action?.();
    expect(setKeepWarm).toHaveBeenLastCalledWith(target, { kind: "off" }, 4);
    // An agent that cannot keep its cache warm cannot be turned on, and a session without a conversation cannot keep warm.
    expect(entry(row(host({ supported: false, reason: "This agent reports no prompt cache that pings can keep warm." })))).toMatchObject({ disabled: true });
    expect(entry(row(host(), { conversation_id: undefined })).disabled).toBe(true);
  });

  it("Clear session checks an unselected row and opens the same dialog for it", async () => {
    const row: ViewRow = { name: "codex/example/live", agent: "codex", state: "running", status: "ready", run_id: "run-live", conversation_id: "conv-live" };
    applyBatch({ ...emptyBatch(), computers: [{ computer: "", status: "live", attempt: 0 }], upserts: [{ computer: "", key: "run:run-live", row }] });
    const live = toSession("", "run:run-live", row);
    const clear = { enabled: true, dismiss: false, title: "Clear session?", text: "", confirm: "Clear session", run: "run-live", conversation: "conv-live",
      cache_enabled: true, running: false, reason: "", cache_reason: "" };
    vi.mocked(composerClearControl).mockResolvedValue(clear);
    setSelectedKey(live.key);
    const entry = (s = live) => sessionMenu(s).find((e) => "label" in e && e.label === "Clear session…") as Extract<MenuEntry, { label: string }>;
    expect(entry()).toMatchObject({ disabled: true, hint: "Checking whether this session can be cleared…" });
    await vi.waitFor(() => expect(entry().disabled).toBe(false));
    expect(composerClearControl).toHaveBeenCalledWith({ computer: "", key: "run:run-live", name: "codex/example/live", archive_id: null, session_id: null });
    entry().action?.();
    await vi.waitFor(() => expect(composer.dialog()).toMatchObject({ kind: "clear", control: { run: "run-live" } }));
    expect(selectedKey()).toBe(live.key);
    composer.closeDialog();

    composer.apply({ handle: 1, key: "\ncodex/example/live", loaded: true, text_epoch: 1, text: "", attachments: [], local: [], clear } as unknown as ComposerView);
    entry().action?.();
    expect(composer.dialog()).toMatchObject({ kind: "clear", control: { run: "run-live" } });
    composer.closeDialog();
    // The row paused since the menu opened: nothing opens.
    const stale = entry();
    applyBatch({ ...emptyBatch(), upserts: [{ computer: "", key: "run:run-live", row: { ...row, state: "paused", status: "paused" } }] });
    stale.action?.();
    expect(composer.dialog()).toBeUndefined();
    composer.apply({ handle: 1, key: "\ncodex/example/live", loaded: true, text_epoch: 1, text: "", attachments: [], local: [], clear: { ...clear, enabled: false, reason: "Wait for the session update to finish." } } as unknown as ComposerView);
    setSelectedKey(live.key);
    expect(entry()).toMatchObject({ disabled: true, hint: "Wait for the session update to finish." });
    composer.apply(undefined);
    setSelectedKey(undefined);
  });
});

function contextRows() {
  const row = (tag: string): ViewRow => ({ name: `codex/context/${tag}`, agent: "codex", state: "running", status: "ready", run_id: `context-${tag}`, conversation_id: `conversation-${tag}`, session_id: `session-${tag}`, cwd: `/tmp/context-${tag}` });
  const rows = ["open", "target", "other"].map((tag) => ({ computer: "", key: `run:context-${tag}`, row: row(tag) }));
  applyBatch({ ...emptyBatch(), computers: [{ computer: "", status: "live", attempt: 0 }], upserts: rows });
  return rows.map((r) => toSession(r.computer, r.key, r.row));
}
function contextEvent(overrides: Partial<MouseEvent> = {}): MouseEvent {
  return { button: 2, ctrlKey: false, clientX: 20, clientY: 40, preventDefault: vi.fn(), stopPropagation: vi.fn(), ...overrides } as unknown as MouseEvent;
}
const menuItem = (label: string, items = menu()?.items ?? []) => {
  const item = items.find((e) => "label" in e && e.label === label);
  if (!item || !("label" in item)) throw Error(`Missing menu action: ${label}`);
  return item;
};

describe("session context targeting", () => {
  it("opens within the bound while capability inspection is stalled, without checking folders", async () => {
    vi.useFakeTimers();
    try {
      const [, target] = contextRows();
      let inspected!: (value: Awaited<ReturnType<typeof inspectSession>>) => void;
      vi.mocked(inspectSession).mockImplementationOnce(() => new Promise((resolve) => { inspected = resolve; }));
      sessionContextMenu(contextEvent(), target);
      expect(visibleMenu()).toBeUndefined();
      await vi.advanceTimersByTimeAsync(200);
      expect(visibleMenu()?.sessionKey).toBe(target.key);
      expect(menuItem("Open terminal").disabled).toBe(false);
      expect(sessionFolderStatus).not.toHaveBeenCalled();
      inspected(null);
    } finally { setMenu(undefined); vi.useRealTimers(); }
  });

  it.each([false, true])("publishes the prepared clear snapshot when reads settle (failure=%s)", async (failure) => {
    const tag = `prepared-${failure}`;
    const row: ViewRow = { name: `codex/context/${tag}`, agent: "codex", state: "running", status: "ready", run_id: tag, conversation_id: `conversation-${tag}` };
    applyBatch({ ...emptyBatch(), computers: [{ computer: "", status: "live", attempt: 0 }], upserts: [{ computer: "", key: `run:${tag}`, row }] });
    const target = toSession("", `run:${tag}`, row);
    let inspected!: (value: Awaited<ReturnType<typeof inspectSession>>) => void;
    let cleared!: (value: Awaited<ReturnType<typeof composerClearControl>>) => void;
    vi.mocked(inspectSession).mockImplementationOnce(() => new Promise((resolve) => { inspected = resolve; }));
    vi.mocked(composerClearControl).mockImplementationOnce(() => new Promise((resolve) => { cleared = resolve; }));
    sessionContextMenu(contextEvent(), target);
    expect(visibleMenu()).toBeUndefined();
    inspected(null);
    cleared(failure ? null : { enabled: true, dismiss: false, title: "Clear session?", text: "", confirm: "Clear session", run: target.runId!, conversation: target.conversationId!, cache_enabled: true, running: false, reason: "", cache_reason: "" });
    await vi.waitFor(() => expect(visibleMenu()?.sessionKey).toBe(target.key));
    expect(menuItem("Clear session…").disabled).toBe(failure);
    expect(sessionFolderStatus).not.toHaveBeenCalled();
    expect(composerClearControl).toHaveBeenCalledTimes(1);
  });

  it.each([false, true])("opening and closing a context menu preserves the conversation, draft and selection (strip=%s)", async (strip) => {
    const [open, target] = contextRows();
    setStripMode(strip); setSelectedKey(open.key); setSubagentOpen("open-child");
    setMulti(open.key, true); setPeek(true); setDetailTab("terminal");
    composer.apply({ handle: 17, key: open.key, loaded: true, text_epoch: 1, text: "unsent draft", attachments: [], local: [] } as unknown as ComposerView);
    const draft = composer.view();
    sessionContextMenu(contextEvent(), target);
    expect(menu()?.sessionKey).toBe(target.key);
    expect(selectedKey()).toBe(open.key); expect(subagentOpen()).toBe("open-child");
    expect(multi[open.key]).toBe(true); expect(multi[target.key]).not.toBe(true);
    expect(peek()).toBe(true); expect(detailTab()).toBe("terminal");
    expect(composer.view()).toBe(draft); expect(composerOpen).not.toHaveBeenCalled();
    setMenu(undefined);
    await Promise.resolve(); await Promise.resolve();
    expect(menu()).toBeUndefined(); // Late capability reads cannot restore the highlight.
    expect(selectedKey()).toBe(open.key); expect(composer.view()).toBe(draft);
  });

  it("copies the context session's ID and opens its external connection while another session stays open", async () => {
    const [open, target] = contextRows(); setSelectedKey(open.key);
    sessionContextMenu(contextEvent(), target);
    await vi.waitFor(() => expect(visibleMenu()?.sessionKey).toBe(target.key));
    const items = menu()!.items; setMenu(undefined);
    menuItem("Copy session ID", items).action?.();
    expect(copyText).toHaveBeenCalledWith(target.conversationId);
    expect(copyText).not.toHaveBeenCalledWith(target.name);
    expect(copyText).not.toHaveBeenCalledWith(target.runId);
    menuItem("Open terminal", items).action?.();
    expect(sessionTerminalOpen).toHaveBeenCalledWith({ computer: target.computer, name: target.name, run_id: target.runId, conversation_id: target.conversationId });
    expect(selectedKey()).toBe(open.key); expect(composerOpen).not.toHaveBeenCalled();
    expect(sessionFolderStatus).not.toHaveBeenCalled();
  });

  it("does not let a late inspection retarget a replacement or closed menu", async () => {
    const [open, target, other] = contextRows(); setSelectedKey(open.key);
    let finish!: (value: Awaited<ReturnType<typeof inspectSession>>) => void;
    vi.mocked(inspectSession).mockImplementationOnce(() => new Promise((resolve) => { finish = resolve; }));
    sessionContextMenu(contextEvent(), target);
    sessionContextMenu(contextEvent(), other);
    finish(null);
    await Promise.resolve(); await Promise.resolve();
    expect(menu()?.sessionKey).toBe(other.key);
    setMenu(undefined); await Promise.resolve();
    expect(menu()).toBeUndefined(); expect(selectedKey()).toBe(open.key);
  });

  it("opens running commands for the context session instead of the current subagent", async () => {
    const [open, target] = contextRows();
    setSelectedKey(open.key); setSubagentOpen("open-child");
    await setSetting("processes/enabled", true);
    try {
      const item = menuItem("Show running commands", expandedEntries({ ...target, phase: "idle", activity: "idle", backgroundJobs: 1 }));
      expect(item.disabled).toBe(false); expect(selectedKey()).toBe(open.key);
      setMenu(undefined); item.action?.();
      expect(selectedKey()).toBe(target.key); expect(subagentOpen()).toBeUndefined();
      expect(detailTab()).toBe("processes");
    } finally { await setSetting("processes/enabled", false); }
  });

  it("keeps macOS Control-click as additive selection without a context menu", () => {
    const [open, target] = contextRows(); setSelectedKey(open.key);
    const event = contextEvent({ button: 0, ctrlKey: true });
    sessionContextMenu(event, target);
    expect(event.stopPropagation).toHaveBeenCalled(); // Do not open the list background menu.
    expect(menu()).toBeUndefined(); expect(selectedKey()).toBe(open.key);
    expect(multi[open.key]).toBe(true); expect(multi[target.key]).toBe(true);
    expect(sessionFolderStatus).not.toHaveBeenCalled(); expect(inspectSession).not.toHaveBeenCalled();
  });

  it.each([false, true])("preserves the existing multi-selection menu (strip=%s)", (strip) => {
    const [open, target] = contextRows(); setStripMode(strip); setSelectedKey(open.key);
    setMulti(open.key, true); setMulti(target.key, true);
    sessionContextMenu(contextEvent(), target);
    expect(menu()?.items[0]).toEqual({ heading: "2 selected sessions" });
    expect(menu()?.sessionKey).toBeUndefined();
    expect(multi[open.key]).toBe(true); expect(multi[target.key]).toBe(true);
    expect(selectedKey()).toBe(open.key); expect(sessionFolderStatus).not.toHaveBeenCalled();
  });
});


describe("external terminal availability", () => {
  it.each([undefined, ""])("opens the original startup run before conversation binding (%j)", (conversation) => {
    const raw: ViewRow = { name: "codex/context/startup", agent: "codex", state: "running", status: "starting", run_id: "startup-run", conversation_id: conversation, terminal_supported: true };
    applyBatch({ ...emptyBatch(), computers: [{ computer: "peer", status: "live", attempt: 0 }], upserts: [{ computer: "peer", key: "run:startup-run", row: raw }] });
    const target = toSession("peer", "run:startup-run", raw);
    const item = menuItem("Open terminal", sessionMenu(target));
    expect(item.disabled).toBe(false);
    expect(sessionTerminalOpen).not.toHaveBeenCalled();
    item.action?.();
    expect(sessionTerminalOpen).toHaveBeenCalledExactlyOnceWith({ computer: "peer", name: raw.name, run_id: "startup-run", conversation_id: "" });
  });
  it.each(["run", "name", "created", "conversation", "removed"])("retains the startup row's click-time source guard (%s)", (change) => {
    const raw: ViewRow = { name: "codex/context/startup", agent: "codex", state: "running", status: "starting", run_id: "startup-run", created: 100 };
    applyBatch({ ...emptyBatch(), computers: [{ computer: "peer", status: "live", attempt: 0 }], upserts: [{ computer: "peer", key: "run:startup-run", row: raw }] });
    const item = menuItem("Open terminal", sessionMenu(toSession("peer", "run:startup-run", raw)));
    expect(item.disabled).toBe(false);
    if (change === "removed") {
      applyBatch({ ...emptyBatch(), removes: [{ computer: "peer", key: "run:startup-run" }] });
    } else {
      const row = { ...raw, ...({ run: { run_id: "replacement" }, name: { name: "codex/context/renamed" }, created: { created: 101 }, conversation: { conversation_id: "new-binding" } }[change]) };
      applyBatch({ ...emptyBatch(), upserts: [{ computer: "peer", key: "run:startup-run", row }] });
    }
    item.action?.(); expect(sessionTerminalOpen).not.toHaveBeenCalled();
  });
  it.each([
    [{ agent: "deepseek", terminal_supported: false }, "This native adapter does not provide Terminal."],
    [{ run_id: "" }, "Refresh this session's identity before opening its terminal."],
  ])("explains an unavailable row %j", (extra, reason) => {
    const row = toSession("", "run:unavailable", { name: "codex/context/unavailable", agent: "codex", state: "running", status: "ready", run_id: "run-unavailable", conversation_id: "conversation-unavailable", ...extra } as ViewRow);
    const item = menuItem("Open terminal", sessionMenu(row));
    expect(item).toMatchObject({ disabled: true, hint: reason });
    item.action?.(); expect(sessionTerminalOpen).not.toHaveBeenCalled();
  });
  it("offers Resume and open terminal for a saved row instead (zerus-5wmh.13.11.3)", () => {
    const row = toSession("", "run:saved", { name: "codex/context/saved", agent: "codex", state: "paused", status: "ready", run_id: "run-saved", conversation_id: "conversation-saved" } as ViewRow);
    const items = sessionMenu(row);
    expect(() => menuItem("Open terminal", items)).toThrow();
    expect(menuItem("Resume and open terminal", items)).toBeDefined();
  });
  it.each(["paused", "stopped"])("enables Resume and open terminal for a %s row, which the CLI reports without a live pane (zerus-5o1ns)", (state) => {
    const row = toSession("", `run:saved-${state}`, { name: `codex/context/${state}`, agent: "codex", state, status: "ready", run_id: `run-${state}`,
      conversation_id: `conversation-${state}`, session_id: `session-${state}`, tracked: true, resumable: true, terminal_supported: false } as ViewRow);
    expect(row.terminalSupported).toBe(false);
    expect(menuItem("Resume and open terminal", sessionMenu(row))).toMatchObject({ disabled: false });
  });
  it("does not open a replacement run after the menu opened", () => {
    const [, target] = contextRows();
    const item = menuItem("Open terminal", sessionMenu(target));
    applyBatch({ ...emptyBatch(), upserts: [{ computer: "", key: target.catalogKey, row: { name: target.name, agent: "codex", state: "running", status: "ready", run_id: "replacement", conversation_id: target.conversationId } }] });
    item.action?.(); expect(sessionTerminalOpen).not.toHaveBeenCalled();
  });
});


it("does not open a row that went offline after the menu opened", () => {
  const [, target] = contextRows();
  const item = menuItem("Open terminal", sessionMenu(target));
  applyBatch({ ...emptyBatch(), computers: [{ computer: "", status: "ended", attempt: 0 }] });
  item.action?.(); expect(sessionTerminalOpen).not.toHaveBeenCalled();
  expect(menuItem("Open terminal", sessionMenu(target))).toMatchObject({ disabled: true, hint: "Machine unavailable. Reconnect when it is online." });
});
