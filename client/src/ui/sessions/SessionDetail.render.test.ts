// @vitest-environment happy-dom
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
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
  loadAddon() {} onData() {} onBinary() {} onResize() {} attachCustomKeyEventHandler() {} write(_bytes: unknown, done?: () => void) { done?.(); }
  // The team-notice folder (terminal-notices.ts, fa2fd9a01) reads these.
  buffer = { active: { baseY: 0, cursorY: 0, length: 0, getLine: () => undefined } };
  onWriteParsed() { return { dispose() {} }; } onRender() { return { dispose() {} }; } onScroll() { return { dispose() {} }; }
  registerMarker() { return undefined; } registerDecoration() { return undefined; }
  // Terminal's own keys (terminal-keys.ts) follow the selection.
  onSelectionChange() { return { dispose() {} }; } hasSelection() { return false; }
  open(host: HTMLElement) { this.textarea = document.createElement("textarea"); host.append(this.textarea); }
  focus() { this.textarea?.focus(); } dispose() { this.textarea?.remove(); } reset() {} paste() {}
} }));
vi.mock("@xterm/addon-fit", () => ({ FitAddon: class { fit() {} } }));

import * as bridge from "../../bridge";
import { App } from "../../App";
import { receive, activity } from "../../store/session";
import { selected, setSelectedKey, setDetailTab, detailTab, setSubagentOpen, setMenu, menu, setStateFilter, setSearch } from "../../store/ui";
import { inspectorTab, type InspectorTab } from "../../store/inspector";
import { settingsReady, setSetting, restoreSettings } from "../../store/settings";
import { openCleanup, closeCleanup } from "../../store/terminationCleanup";
import { machineMeta } from "../../store/testing";
import { listRows } from "./model";
import { archive } from "../../store/archive";
import { clearSearchPreview, setSearchPreview } from "../../store/searchSelection";
import type { SearchHit } from "../../store/content-search-model";
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

describe("an archive search keeps the archive status and paging (zerus-5wmh.13.17 (4))", () => {
  afterEach(() => setSearch(""));

  it("shows the window, Newer and Older for the query's page, and a capped total as N+", async () => {
    vi.mocked(bridge.archivePage).mockResolvedValue({ rows: [], next_cursor: "c", previous_cursor: "p", total: 10_000, total_capped: true, position: 50 } as never);
    put(fixtureRow());
    setStateFilter("archived"); await flush();
    setSearch("needle"); await flush();
    await vi.waitFor(() => expect(vi.mocked(bridge.archivePage).mock.calls.some((call) => call[4] === "needle")).toBe(true), { timeout: 3000 });
    await vi.waitFor(() => expect(host.querySelector(".archive-status")?.textContent).toBe("0 shown of 10000+"), { timeout: 3000 });
    expect([...host.querySelectorAll(".archive-nav")].map((b) => b.getAttribute("aria-label"))).toEqual(["Newer sessions", "Older sessions"]);
    // The results list still shows the search (messages and the page's archive rows).
    expect(host.querySelector(".search-results, [role=listbox]")).not.toBeNull();
  });

  it("never says no matches while the archive scan is still partial (zerus-080n8)", async () => {
    vi.mocked(bridge.archivePage).mockResolvedValue({ rows: [], next_cursor: "c", previous_cursor: null, total: 0, total_capped: true, position: 0, partial: true, searched: 25_000, bounded: true } as never);
    put(fixtureRow());
    setStateFilter("archived"); await flush();
    setSearch("needle"); await flush();
    await vi.waitFor(() => expect(host.querySelector(".archive-status")?.textContent).toBe("0 shown, 25000 searched so far"), { timeout: 3000 });
    await vi.waitFor(() => expect(host.querySelector(".content-search .empty h3")?.textContent).toBe("0 shown, 25000 searched so far"), { timeout: 3000 });
    expect(host.querySelector(".content-search .empty p")?.textContent).toBe("Older continues the search.");
    expect(host.textContent).not.toContain("No matching sessions");
    expect(host.textContent).not.toContain("0+");
  });
});

describe("a shown tool search hit (zerus-tso9j, zerus-2r73y)", () => {
  const toolHit = (row: ViewRow, detail: string): SearchHit => ({ name: row.name!, run_id: row.run_id!, conversation_id: row.conversation_id!, archive_id: "", message_id: "m",
    at: 1_793_000_000, role: "tool", snippet: "", event: { seq: 1, type: "PostToolUse", tool: "Bash", at: 1_793_000_000, detail }, content_truncated: false, session: null, group: "" });
  const show = async (detail: string) => {
    const row = fixtureRow(); const key = await open(row);
    setSearchPreview({ key, run: row.run_id!, conversation: row.conversation_id!, query: "needle", hit: toolHit(row, detail) }); await flush();
    return () => host.querySelector(".search-preview .act-tool-detail");
  };
  afterEach(() => clearSearchPreview());

  it("Show more reveals a match past the first 2,000 characters", async () => {
    const detail = await show(`${"x".repeat(2_200)}needle${"y".repeat(1_000)}`);
    expect(detail()!.textContent).toHaveLength(2_000);
    [...host.querySelectorAll<HTMLButtonElement>(".search-preview button")].find((b) => b.textContent === "Show more")!.click(); await flush();
    expect(detail()!.textContent).toHaveLength(3_206);
    expect([...detail()!.querySelectorAll("mark")].map((m) => m.textContent)).toEqual(["needle"]);
  });

  it("marks the new query when another hit of the same session shows (zerus-2r73y.1)", async () => {
    const row = fixtureRow(); const key = await open(row);
    const marks = () => [...host.querySelectorAll(".search-preview mark")].map((m) => m.textContent);
    const showHit = async (query: string, hit: SearchHit) => { setSearchPreview({ key, run: row.run_id!, conversation: row.conversation_id!, query, hit }); await flush(); };
    await showHit("needle", toolHit(row, "needle and prefix"));
    expect(marks()).toEqual(["needle"]);
    await showHit("prefix", { ...toolHit(row, "prefix only here"), message_id: "m2", event: { seq: 2, type: "PostToolUse", tool: "Bash", at: 1_793_000_100, detail: "prefix only here" } });
    expect(marks()).toEqual(["prefix"]);
    // A reply hit (Markdown) follows the query the same way.
    const reply = (n: number, detail: string): SearchHit => ({ ...toolHit(row, detail), message_id: `r${n}`, role: "assistant", event: { seq: 10 + n, type: "AgentMessage", at: 1_793_000_200 + n, detail } });
    await showHit("alpha", reply(1, "alpha beta"));
    await vi.waitFor(() => expect(marks()).toEqual(["alpha"]));
    await showHit("beta", reply(2, "beta gamma"));
    await vi.waitFor(() => expect(marks()).toEqual(["beta"]));
  });

  it("marks the query in a tool hit's output, and only inside the hit", async () => {
    const detail = await show("a needle tool output with NEEDLE twice");
    expect([...detail()!.querySelectorAll("mark")].map((m) => m.textContent)).toEqual(["needle", "NEEDLE"]);
    clearSearchPreview(); await flush();
    expect(host.querySelector(".search-preview")).toBeNull();
    expect(host.querySelector(".act-scroll mark")).toBeNull();
  });
});

describe("the header's keep-warm mark (zerus-yacns)", () => {
  const warmRow = (keep_warm: Record<string, unknown> | undefined) => ({ ...fixtureRow("Warm"), keep_warm }) as ViewRow;
  const now = () => Date.now() / 1000;
  const on = (until: number | null, revision = 2) => ({ enabled: true, until, revision, state: "waiting", next_at: null, reason: "", turn_active: false });
  const mark = () => host.querySelector<HTMLButtonElement>(".detail-head .dh-row2 .dh-warm");

  it("shows the bolt and the time left, or an infinity sign for Always, at the row's end", async () => {
    const row = warmRow(on(now() + 2 * 3600 - 30)); await open(row);
    const shown = mark()!;
    expect(shown.textContent).toBe("1 h 59 m");
    expect(shown.getAttribute("aria-label")).toBe("Keep cache warm: 1 h 59 m left");
    expect(shown.title.split("\n")[0]).toBe("Keep cache warm: 1 h 59 m left");
    // Owner, 2026-11-09 (07e0ddacb): the header no longer repeats the model.
    expect(shown.nextElementSibling).toBeNull();
    expect(host.querySelector(".detail-head .dh-model")).toBeNull();
    put({ ...row, keep_warm: on(null, 3) }); await flush();
    expect(mark()).toBe(shown);
    expect([mark()!.textContent, mark()!.getAttribute("aria-label")]).toEqual(["∞", "Keep cache warm: Always"]);
  });

  it("shows nothing while keep-warm is off or not reported", async () => {
    const row = warmRow({ ...on(null), enabled: false, state: "off" }); await open(row);
    expect(mark()).toBeNull();
    put({ ...row, keep_warm: undefined }); await flush();
    expect(mark()).toBeNull();
  });

  it("keeps the same mark node through refreshes and clock ticks", async () => {
    vi.useFakeTimers({ toFake: ["setInterval", "clearInterval", "Date"] });
    try {
      const row = warmRow(on(now() + 3 * 3600)); await open(row);
      const shown = mark()!, removed: Node[] = [];
      expect(shown).not.toBeNull();
      const observer = new MutationObserver((records) => records.forEach((record) => removed.push(...record.removedNodes)));
      observer.observe(host, { subtree: true, childList: true });
      for (let tick = 0; tick < 3; tick++) {
        vi.advanceTimersByTime(30_000);
        put({ ...row, summary: `Refresh ${tick}`, keep_warm: on(now() + 3 * 3600 - tick, 2 + tick) }); await flush();
      }
      observer.disconnect();
      expect(mark()).toBe(shown);
      expect(removed.filter((node) => node === shown || node.contains(shown))).toEqual([]);
    } finally { vi.useRealTimers(); }
  });

  it("opens Details at the Keep cache warm control when clicked", async () => {
    await setSetting("workspace/inspectorOpen", false, { persist: false });
    await setSetting("workspace/inspectorClosed", JSON.stringify(["details-session"]), { persist: false });
    vi.mocked(bridge.inspectSession).mockResolvedValue(null);
    const key = put(warmRow(on(null))); setSelectedKey(key); setDetailTab("activity"); await flush();
    mark()!.click(); await vi.dynamicImportSettled(); await flush();
    await vi.waitFor(() => expect(host.querySelector(".kw-row button[role=switch]")).not.toBeNull());
    expect(inspectorTab()).toBe("details");
    await vi.waitFor(() => expect(document.activeElement).toBe(host.querySelector(".kw-row button[role=switch]")));
  });
});
