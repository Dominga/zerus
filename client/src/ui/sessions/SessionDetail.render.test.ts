// @vitest-environment happy-dom
import { afterEach, beforeEach, expect, it, vi } from "vitest";
import { createComponent } from "solid-js";
import { render } from "solid-js/web";

vi.mock("../../bridge", async (original) => ({
  ...await original<typeof import("../../bridge")>(),
  inTauri: () => false,
  activityPort: { follow: vi.fn(async () => {}), unfollow: vi.fn(async () => {}) },
  terminalStatus: vi.fn(async () => ({ supported: true, available: false, reason: "Private fixture", idle: "Fixture terminal", connecting: "Connecting", disconnected: "Disconnected" })),
  inspectSession: vi.fn(async () => null), composerOpen: vi.fn(async () => null),
  sessionFolderStatus: vi.fn(async () => undefined), followTasks: vi.fn(async () => {}),
  reportDetailInvariant: vi.fn(async () => {}),
  archivePage: vi.fn(async () => ({ rows: [], next_cursor: "", previous_cursor: null, total: 0, position: 0 })),
  archivedActivity: vi.fn(async () => ({ events: [], replace_events: true, conversation_id: "archived-conversation", run_id: "archived-run", cleared_conversations: [], cursor: 0, summary: {} })),
}));
// The actual TerminalTab and its store still mount. Only xterm's canvas/OS
// renderer is inert in happy-dom; no terminal is connected or receives input.
vi.mock("@xterm/xterm", () => ({ Terminal: class {
  cols = 80; rows = 24; options: Record<string, unknown>; textarea?: HTMLTextAreaElement;
  constructor(options: Record<string, unknown>) { this.options = options; }
  loadAddon() {} onData() {} onBinary() {} onResize() {} write(_bytes: unknown, done?: () => void) { done?.(); }
  open(host: HTMLElement) { this.textarea = document.createElement("textarea"); host.append(this.textarea); }
  focus() { this.textarea?.focus(); } dispose() { this.textarea?.remove(); } reset() {} paste() {}
} }));
vi.mock("@xterm/addon-fit", () => ({ FitAddon: class { fit() {} } }));

import * as bridge from "../../bridge";
import { App } from "../../App";
import { receive, activity } from "../../store/session";
import { selected, setSelectedKey, setDetailTab, detailTab, setSubagentOpen, setMenu, menu, setStateFilter } from "../../store/ui";
import { inspectorTab, type InspectorTab } from "../../store/inspector";
import { settingsReady, setSetting, restoreSettings } from "../../store/settings";
import { openCleanup, closeCleanup } from "../../store/terminationCleanup";
import { machineMeta } from "../../store/testing";
import { listRows } from "./model";
import { archive } from "../../store/archive";
import { emptyBatch, type ViewRow } from "../../types";

const flush = async () => { for (let i = 0; i < 12; i++) await Promise.resolve(); };
const machine = "11111111-1111-4111-8111-111111111111";
const uuid = (n: number) => `00000000-0000-4000-8000-${String(n).padStart(12, "0")}`;
let serial = 0, dispose: (() => void) | undefined;
let host: HTMLDivElement;

beforeEach(async () => {
  await settingsReady;
  vi.clearAllMocks();
  vi.mocked(bridge.inspectSession).mockResolvedValue(null);
  vi.mocked(bridge.sessionFolderStatus).mockResolvedValue(undefined as never);
  await setSetting("processes/enabled", true, { persist: false });
  await setSetting("workspace/inspectorOpen", true, { persist: false });
  setStateFilter("all"); setSubagentOpen(undefined);
  host = document.createElement("div"); document.body.append(host);
  dispose = render(() => createComponent(App, { mode: () => "replay" }), host);
});
afterEach(() => {
  closeCleanup(); setMenu(undefined);
  host.querySelector<HTMLDialogElement>("dialog.dlg")?.dispatchEvent(new Event("close"));
  dispose?.(); dispose = undefined; document.body.replaceChildren();
  setSelectedKey(undefined); setSubagentOpen(undefined); setStateFilter("all");
  receive({ ...emptyBatch(), replaces: [{ computer: "", rows: [] }] });
  restoreSettings();
});

function fixtureRow(title = "Fixture"): ViewRow {
  const n = ++serial;
  return { name: `codex/example/fixture-${n}`, title, project: "Example", agent: "codex", status: "ready", caption: "Ready", state: "running", tracked: true,
    run_id: `original-${n}`, conversation_id: `conversation-${n}`, session_id: uuid(n), cwd: "/srv/example/fixture" };
}
function put(row: ViewRow) {
  const key = `run:${row.run_id}`;
  receive({ ...emptyBatch(), computers: [{ computer: "", status: "live", attempt: 0 }],
    upserts: [{ computer: "", key, row }] });
  return `\n${key}`;
}
function remove(key: string) {
  receive({ ...emptyBatch(), removes: [{ computer: "", key: key.slice(1) }],
    activity: [{ computer: "", key: key.slice(1), kind: "ended", payload: {} }] });
}
async function open(row: ViewRow, tab: "activity" | "processes" | "terminal" = "activity", side: InspectorTab = "plan") {
  const key = put(row); setSelectedKey(key); await flush();
  await setSetting("workspace/inspectorTab", side, { persist: false });
  setDetailTab(tab); await vi.dynamicImportSettled(); await flush();
  expect(detailTab()).toBe(tab); expect(inspectorTab()).toBe(side);
  expect(host.querySelector(`.${tab === "activity" ? "act-scroll" : tab === "terminal" ? "term-tab" : "proc-tab"}`)).not.toBeNull();
  expect(host.querySelector(".detail-head")).not.toBeNull();
  expect(host.querySelector(".inspector")).not.toBeNull();
  activity.applyActivity([{ computer: "", key: key.slice(1), kind: "state", payload: { rev: 1, value: { run_id: row.run_id, conversation_id: row.conversation_id, activity: "idle", phase: "idle", activity_summary: "Ready" } } }]);
  return key;
}
function expectEmpty() {
  expect(selected()).toBeUndefined();
  expect(host.querySelector(".detail-empty")).not.toBeNull();
  for (const selector of [".detail-head", ".detail-tabs", ".inspector"]) expect(host.querySelector(selector), selector).toBeNull();
  expect(bridge.reportDetailInvariant).not.toHaveBeenCalled();
}

const routes = (["activity", "processes", "terminal"] as const).flatMap((tab) =>
  (["plan", "details", "worktrees", "tasks"] as const).map((side) => ({ tab, side })));
it.each(routes)("retires the complete $tab / $side frame together when the catalog row disappears", async ({ tab, side }) => {
  const key = await open(fixtureRow(), tab, side);
  const parts = [".detail-head", ".detail-tabs", ".inspector"].map((selector) => host.querySelector(selector)!);
  const removed: Node[] = [];
  const observer = new MutationObserver((records) => records.forEach((record) => removed.push(...record.removedNodes)));
  observer.observe(host, { subtree: true, childList: true });
  remove(key); await flush(); observer.disconnect(); expectEmpty();
  // This is the new defensive ownership invariant, not a reproduction of the
  // historical screenshot: one removed DOM ancestor contains every old part.
  expect(removed.some((node) => parts.every((part) => node.contains(part)))).toBe(true);
});

it("keeps header, transcript, focus and inspector continuity on ordinary metadata updates", async () => {
  const row = fixtureRow(); await open(row);
  const head = host.querySelector(".detail-head"), scroll = host.querySelector<HTMLElement>(".act-scroll")!, inspector = host.querySelector(".inspector");
  scroll.focus(); put({ ...row, title: "Updated title", summary: "Updated metadata" }); await flush();
  expect(host.querySelector(".detail-head")).toBe(head);
  expect(host.querySelector(".act-scroll")).toBe(scroll);
  expect(host.querySelector(".inspector")).toBe(inspector);
  expect(host.querySelector(".detail-head h1")!.textContent).toContain("Updated title");
  expect(document.activeElement).toBe(scroll);
  expect(bridge.reportDetailInvariant).not.toHaveBeenCalled();
});

it.each(["terminate dialog", "cleanup panel", "pending replies"])("clears the detail with %s still outstanding", async (stage) => {
  const replies: (() => void)[] = [];
  vi.mocked(bridge.sessionFolderStatus).mockImplementation(() => new Promise((resolve) => replies.push(() => resolve(undefined as never))));
  vi.mocked(bridge.inspectSession).mockImplementation(() => new Promise((resolve) => replies.push(() => resolve(null))));
  const key = await open(fixtureRow());
  if (stage === "terminate dialog") {
    host.querySelector<HTMLButtonElement>('.detail-head button[data-tip="Session actions"]')!.click(); await flush();
    [...host.querySelectorAll<HTMLButtonElement>(".menu-item")].find((button) => button.textContent === "Terminate session…")!.click(); await flush();
    expect(host.querySelector("#action-dlg-title")!.textContent).toContain("Terminate");
  } else if (stage === "cleanup panel") {
    openCleanup(); await vi.dynamicImportSettled(); await flush();
    expect(host.querySelector(".worktree-cleanup[open]")).not.toBeNull();
  }
  expect(replies.length).toBeGreaterThan(0);
  remove(key); await flush(); replies.forEach((reply) => reply()); await flush(); expectEmpty();
});

it.each([false, true])("removes the selected parent or preserves its selected child (child selected %s)", async (childSelected) => {
  const parent = fixtureRow("Parent"), child = fixtureRow("Child");
  child.parent = { machine, session_id: parent.session_id!, name: parent.name!, conversation_id: parent.conversation_id!, run_id: parent.run_id!, state: "live" };
  const parentKey = put(parent), childKey = put(child);
  receive({ ...emptyBatch(), machines: [{ computer: "", machine: machineMeta("fixture", {}, { machine_id: machine }) }],
    grouping: { loaded: true, default_project: "ungrouped", headers: [{ id: "example", name: "Example", color: null, accessible: true, collapsed: false, is_default: false, palette: 0 }], ready: true, reason: null, busy: false, status: "", status_error: false, conflicts: 0 },
    groups: [{ computer: "", key: parentKey.slice(1), group: "example", rank: 1 }, { computer: "", key: childKey.slice(1), group: "example", rank: 2 }] });
  setSelectedKey(childSelected ? childKey : parentKey); setDetailTab("activity"); await flush();
  expect(listRows().find((r) => r.type === "card" && r.session.key === childKey)).toMatchObject({ depth: 1 });
  remove(parentKey); await flush();
  if (childSelected) {
    expect(selected()?.key).toBe(childKey);
    expect(host.querySelector(".detail-head h1")!.textContent).toContain("Child");
    expect(host.querySelector(".detail-empty")).toBeNull();
    expect(listRows().find((r) => r.type === "card" && r.session.key === childKey)).not.toMatchObject({ depth: 1 });
  } else expectEmpty();
  remove(childKey); await flush(); expectEmpty();
});

it("keeps normal recorded archive browsing outside the live catalog", async () => {
  const liveKey = await open(fixtureRow());
  const originalFrame = host.querySelector(".detail-head")!.parentElement;
  const id = uuid(++serial), key = `\narchive:${id}`;
  vi.mocked(bridge.archivePage).mockResolvedValue({ rows: [{ group: "example", rank: 1, name: "codex/example/archived", agent: "codex", state: "archived", status: "ended", archive_id: id, archived_at: 100, run_id: "archived-run", conversation_id: "archived-conversation" }], next_cursor: "", previous_cursor: null, total: 1, position: 0 } as never);
  setStateFilter("archived"); archive.refresh(); await flush();
  setSelectedKey(key); setDetailTab("activity"); await vi.dynamicImportSettled(); await flush();
  expect(selected()?.archiveId).toBe(id);
  expect(host.querySelector(".detail-head h1")!.textContent).toContain("archived");
  expect(host.querySelector(".inspector")).not.toBeNull();
  expect(host.querySelector(".detail-empty")).toBeNull();
  expect(host.querySelector(".detail-head")!.parentElement).toBe(originalFrame);
  remove(liveKey); await flush();
  await new Promise<void>((resolve) => requestAnimationFrame(() => resolve()));
  expect(bridge.reportDetailInvariant).not.toHaveBeenCalled();
  const unrelated = fixtureRow("Unrelated"), unrelatedKey = put(unrelated); remove(unrelatedKey); await flush();
  expect(selected()?.key).toBe(key);
  expect(host.querySelector(".detail-head h1")!.textContent).toContain("archived");
  expect(bridge.reportDetailInvariant).not.toHaveBeenCalled();
});


it("keeps the moved row actions in the real session header menu", async () => {
  await open(fixtureRow());
  const button = host.querySelector<HTMLButtonElement>('button[data-tip="Session actions"]')!;
  button.click(); await flush();
  const labels = menu()!.items.flatMap((item) => "label" in item ? [item.label] : []);
  for (const label of ["Expand all collapsed", "Open folder", "Open shell in session folder", "Pause session", "Fork session…", "Set parent…", "Copy session name", "Copy session reference", "Copy folder path"]) {
    expect(labels, label).toContain(label);
  }
});
