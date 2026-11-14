// @vitest-environment happy-dom
import { afterEach, beforeEach, expect, it, vi } from "vitest";
import { createComponent } from "solid-js";
import { render } from "solid-js/web";

vi.mock("../../bridge", async (original) => ({
  ...await original<typeof import("../../bridge")>(),
  gitRows: vi.fn(async () => undefined),
  queueReadMarks: vi.fn(async () => undefined),
}));
vi.mock("../../store/marks", async (original) => {
  const marks = await original<typeof import("../../store/marks")>();
  return { ...marks, markRead: vi.fn(marks.markRead), markReadUnavailable: vi.fn(marks.markReadUnavailable) };
});
vi.mock("@tanstack/solid-virtual", () => ({
  createVirtualizer: (options: { count: number; getItemKey(n: number): string }) => ({
    measure() {}, measureElement() {}, _willUpdate() {}, scrollToIndex() {}, getTotalSize: () => options.count * 94,
    getVirtualItems: () => Array.from({ length: options.count }, (_, index) => ({ key: options.getItemKey(index), start: index * 94, index })),
  }),
}));

import * as bridge from "../../bridge";
import { markRead, markReadUnavailable } from "../../store/marks";
import { receive } from "../../store/session";
import { catalog } from "../../store/catalog";
import { emptyBatch, type RowOut, type ViewRow } from "../../types";
import { menu, setMachineFilter, setMenu, setPage, setSelectedKey, setStateFilter, setStripMode } from "../../store/ui";
import { settingsReady } from "../../store/settings";
import { MenuLayer } from "../components/Overlays";
import { SessionList } from "./SessionList";
import { listRows } from "./model";
import { translateWithCatalog } from "../../i18n";
import ru from "../../i18n.ru.json";

let dispose: (() => void) | undefined, serial = 0;
const flush = async () => { for (let i = 0; i < 12; i++) await Promise.resolve(); };
beforeEach(async () => {
  await settingsReady;
  vi.clearAllMocks();
  vi.stubGlobal("ResizeObserver", class { observe() {} disconnect() {} });
  setPage("sessions"); setStripMode(false); setStateFilter("all"); setSelectedKey(undefined); setMachineFilter([]);
});
afterEach(async () => {
  setMenu(undefined); dispose?.(); dispose = undefined; await flush(); document.body.replaceChildren();
  setMachineFilter([]);
  receive({ ...emptyBatch(), replaces: ["", "remote"].map((computer) => ({ computer, rows: [] })) });
  vi.unstubAllGlobals();
});

async function mount(alphaUnread = true) {
  const generation = ++serial;
  const row = (computer: string, id: string, extra: Partial<ViewRow> = {}): RowOut => ({
    computer, key: `run:${generation}-${id}`, row: {
      name: `claude/misleading-project/${generation}-${id}`, project: "Misleading project label", agent: "claude", machine: computer || "workstation",
      state: "running", status: "ready", run_id: `run-${generation}-${id}`, conversation_id: `conversation-${generation}-${id}`,
      reply_id: `reply-${generation}-${id}`, unread: true, unread_candidate: true, shared_read: true, ...extra,
    },
  });
  const alpha = [row("", "local"), row("remote", "remote"), row("", "muted", { muted: true })];
  if (!alphaUnread) for (const item of alpha) item.row.read_reply_id = item.row.reply_id;
  const read = row("", "read"); read.row.read_reply_id = read.row.reply_id;
  const skipped = [read, row("", "archived", { state: "archived" }), row("", "unfinished", { status: "background", unread_candidate: false })];
  const beta = [row("", "other-local"), row("remote", "other-remote")];
  const rows = [...alpha, ...skipped, ...beta];
  receive({ ...emptyBatch(), computers: ["", "remote"].map((computer) => ({ computer, status: "live", attempt: 0 })),
    replaces: ["", "remote"].map((computer) => ({ computer, rows: rows.filter((item) => item.computer === computer) })),
    grouping: { loaded: true, default_project: "beta", headers: ["alpha", "beta"].map((id) => ({ id, name: id, color: null, accessible: true, collapsed: false, is_default: id === "beta", palette: 0 })),
      ready: false, reason: "Project sync is starting.", busy: false, status: "", status_error: false, conflicts: 0 },
    groups: rows.map((item, rank) => ({ computer: item.computer, key: item.key, group: beta.includes(item) ? "beta" : "alpha", rank })),
  });
  const host = document.createElement("div"); document.body.append(host);
  dispose = render(() => [createComponent(SessionList, {}), createComponent(MenuLayer, {})], host);
  await flush();
  const header = () => [...host.querySelectorAll<HTMLButtonElement>(".group-band")].find((button) => button.querySelector(".gb-name")?.textContent === "alpha")!;
  const open = async () => {
    header().dispatchEvent(new MouseEvent("contextmenu", { bubbles: true, cancelable: true })); await flush();
    return [...host.querySelectorAll<HTMLButtonElement>('[role="menuitem"]')].find((button) => button.textContent === "Mark sessions as read")!;
  };
  return { host, alpha, beta, open, header };
}

const id = (row: RowOut) => `${row.computer}\n${row.key}`;
const headerUnread = () => listRows().find((row) => row.type === "header" && row.project === "alpha");

it("marks exactly this project's unread sessions through the per-session read lane, including hidden remote and muted members", async () => {
  const t = await mount();
  // The header counters and action include project members hidden by a machine filter.
  setMachineFilter(["workstation"]); await flush();
  expect(listRows().some((row) => row.type === "card" && row.session.key === id(t.alpha[1]))).toBe(false);
  expect(headerUnread()).toMatchObject({ unread: 2 });
  const item = await t.open();
  expect(item).toBeTruthy(); expect(item.getAttribute("aria-disabled")).toBe("false");
  item.click(); await flush();
  expect(vi.mocked(markRead).mock.calls.map(([row]) => row.key)).toEqual(t.alpha.map(id));
  expect(vi.mocked(bridge.queueReadMarks).mock.calls.flatMap(([marks]) => marks.map((mark) => [mark.computer, mark.name]))).toEqual(t.alpha.map((row) => [row.computer, row.row.name]));
  for (const row of t.alpha) expect(catalog.rows[id(row)].unread).toBe(false);
  for (const row of t.beta) expect(catalog.rows[id(row)].unread).toBe(true);
  expect(headerUnread()).toMatchObject({ unread: 0 });
  expect(t.header().querySelector(".gb-count.hot")).toBeNull();
  expect((await t.open()).getAttribute("aria-disabled")).toBe("true");
});

it("keeps the item visible but disabled when only another project's sessions are unread", async () => {
  const t = await mount(false), item = await t.open();
  expect(item.getAttribute("aria-disabled")).toBe("true");
  item.click(); await flush();
  expect(markRead).not.toHaveBeenCalled(); expect(bridge.queueReadMarks).not.toHaveBeenCalled();
  expect(t.beta.every((row) => catalog.rows[id(row)].unread)).toBe(true);
});

it("skips a session moved to another project while the menu is open", async () => {
  const t = await mount(), item = await t.open();
  receive({ ...emptyBatch(), groups: [{ computer: t.alpha[1].computer, key: t.alpha[1].key, group: "beta", rank: 0 }] });
  item.click(); await flush();
  expect(vi.mocked(markRead).mock.calls.map(([row]) => row.key)).toEqual([id(t.alpha[0]), id(t.alpha[2])]);
  expect(catalog.rows[id(t.alpha[1])].unread).toBe(true);
});

it("retains the per-session reply identity guard when a newer reply arrives under the open menu", async () => {
  const t = await mount(), item = await t.open();
  receive({ ...emptyBatch(), upserts: [{ ...t.alpha[0], row: { ...t.alpha[0].row, reply_id: "newer-reply" } }] });
  item.click(); await flush();
  expect(vi.mocked(markRead).mock.results.map((result) => result.value)).toEqual([false, true, true]);
  expect(catalog.rows[id(t.alpha[0])].unread).toBe(true);
  expect(vi.mocked(bridge.queueReadMarks).mock.calls.flatMap(([marks]) => marks.map((mark) => mark.name))).toEqual(t.alpha.slice(1).map((row) => row.row.name));
});

it("uses the per-session read availability check and leaves refused replies unread", async () => {
  const t = await mount(), item = await t.open();
  vi.mocked(markReadUnavailable).mockReturnValueOnce("Read marks are unavailable until the background service is ready.");
  item.click(); await flush();
  expect(markReadUnavailable).toHaveBeenCalledTimes(3);
  expect(vi.mocked(markRead).mock.calls.map(([row]) => row.key)).toEqual(t.alpha.slice(1).map(id));
  expect(catalog.rows[id(t.alpha[0])].unread).toBe(true);
  expect(menu()).toBeUndefined();
});

it("uses the owner's Russian wording", () => {
  expect(translateWithCatalog(ru, "ru", "Mark sessions as read")).toBe("Отметить прочитанными");
});
