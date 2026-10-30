import { describe, expect, it, vi } from "vitest";

vi.hoisted(() => {
  // store/ui.ts reads the page address and storage when it loads.
  const store = new Map<string, string>();
  vi.stubGlobal("location", { search: "", hash: "", href: "http://localhost/" });
  vi.stubGlobal("localStorage", { getItem: (k: string) => store.get(k) ?? null, setItem: (k: string, v: string) => { store.set(k, v); }, removeItem: (k: string) => { store.delete(k); } });
  // The list module registers Solid's delegated events when it loads; nothing renders here.
  // The composer it loads with Saved drafts listens for the page going.
  vi.stubGlobal("window", { document: { addEventListener: () => {} }, addEventListener: () => {} });
});
vi.mock("../../bridge", () => ({
  archivePage: vi.fn(() => new Promise(() => {})),
  resetArchivePages: vi.fn(async () => {}),
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
  // The composer and Saved drafts, loaded with the list's Saved drafts button.
  windowVisibility: { hidden: () => false, onChange: () => () => {} },
  composerOpen: vi.fn(async () => null), composerEdit: vi.fn(async () => {}), composerPick: vi.fn(async () => {}), composerPasteImage: vi.fn(async () => {}),
  composerRemove: vi.fn(async () => {}), composerUndoRemove: vi.fn(async () => {}), composerAllowRetry: vi.fn(async () => {}), composerRefresh: vi.fn(async () => {}),
  composerTab: vi.fn(async () => {}), sendMessage: vi.fn(), sendAnswer: vi.fn(), sendCancel: vi.fn(async () => {}), sendInterrupted: vi.fn(async () => {}), clearContext: vi.fn(),
  localRetry: vi.fn(), localDelete: vi.fn(async () => {}), localInspect: vi.fn(async () => null),
  savedDrafts: vi.fn(), draftConflicts: vi.fn(), savedDraft: vi.fn(), draftRecover: vi.fn(), draftRestore: vi.fn(),
  composerClearControl: vi.fn(async () => null),
}));

import { sessionMenu } from "./SessionList";
import { archivedSession } from "../../store/archive";
import { applyBatch, toSession } from "../../store/catalog";
import { composer } from "../../store/composer";
import { composerClearControl } from "../../bridge";
import { selectedKey, setSelectedKey, type MenuEntry } from "../../store/ui";
import { emptyBatch, type ComposerView, type ViewRow } from "../../types";

const labels = (entries: MenuEntry[]) => entries.map((e) => ("separator" in e ? "---" : "heading" in e ? `# ${e.heading}` : e.label));
const archived = (agent: string): ViewRow => ({
  name: `${agent}/example/a1`, agent, state: "archived", status: "ended", archive_id: "10000000-0000-4000-8000-000000000001",
  archived_at: 100, run_id: "run-a1", conversation_id: "conv-a1", resumable: true,
});

describe("the session menu (Qt showSessionMenu 5266-5382)", () => {
  it("an archived row has no terminal entry and no Mute or Mark group, and no doubled separators", () => {
    const expected = [
      "Compact session row", "Expand all collapsed", "---",
      "Open folder", "Open shell in session folder", "Copy restore command", "---",
      "Restore session", "Rename…", "Fork session…", "Move to project…", "---",
      "Copy session name", "Copy session reference", "Copy folder path", "---",
      "Forget archived session…",
    ];
    expect(labels(sessionMenu(archivedSession("", "Local", archived("codex"))))).toEqual(expected);
    expect(labels(sessionMenu(archivedSession("", "Local", archived("dsh"))))).toEqual(expected);
  });

  it("a live row keeps its terminal entry and the Mute and Mark group", () => {
    const live = toSession("", "run:run-live", { name: "codex/example/live", agent: "codex", state: "running", status: "ready", run_id: "run-live", conversation_id: "conv-live" });
    expect(labels(sessionMenu(live))).toEqual([
      "Compact session row", "Expand all collapsed", "---",
      "Open terminal", "Open folder", "Open shell in session folder", "Copy connect command", "---",
      "Mute", "Mark needs attention", "---",
      "Pause session", "Clear session…", "Rename…", "Fork session…", "Move to project…", "---",
      "Copy session name", "Copy session reference", "Copy folder path", "---",
      "Terminate session…",
    ]);
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
    expect(composerClearControl).toHaveBeenCalledWith({ computer: "", key: "run:run-live", name: "codex/example/live", archive_id: null });
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
