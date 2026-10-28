import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

vi.hoisted(() => {
  // store/ui.ts reads the page address and storage when it loads.
  const store = new Map<string, string>();
  vi.stubGlobal("location", { search: "", hash: "", href: "http://localhost/" });
  vi.stubGlobal("localStorage", { getItem: (k: string) => store.get(k) ?? null, setItem: (k: string, v: string) => { store.set(k, v); }, removeItem: (k: string) => { store.delete(k); } });
});
vi.mock("../bridge", () => ({
  archivePage: vi.fn(),
  cancelArchivePages: vi.fn(),
  runAction: vi.fn(),
  inspectSession: vi.fn(),
  pendingReadMarks: vi.fn(async () => []),
  attentionMarks: vi.fn(async () => ({ marks: {}, replies: {} })),
  updateAttentionMarks: vi.fn(async () => {}),
  queueReadMarks: vi.fn(async () => []),
  sessionReference: vi.fn(),
  copyCommand: vi.fn(),
  copyText: vi.fn(),
  setNotifications: vi.fn(),
}));

import { createRoot, createSignal } from "solid-js";
import * as bridge from "../bridge";
import { toSession } from "./catalog";
import {
  ARCHIVE_CHANGED, archiveListRows, archivedSession, buttonText, countText, createArchive, emptyPage, pagingComputers, statusOf, tipText,
  type ArchivedSession, type Page, type PagingComputer,
} from "./archive";
import { ALL_UNTIL } from "../ui/sessions/archive-dates";
import type { ArchivePage, ViewRow } from "../types";

const NOW = new Date(2026, 9, 28, 12, 0, 0);
const local = (y: number, m: number, d: number) => new Date(y, m - 1, d).getTime() / 1000;
const uuid = (n: number) => `10000000-0000-4000-8000-${String(n).padStart(12, "0")}`;
const arow = (n: number, at: number, extra: Partial<ViewRow> = {}): ViewRow => ({
  name: `codex/example/a${n}`, agent: "codex", state: "archived", status: "ended", caption: "Archived", archive_id: uuid(n), archived_at: at,
  run_id: `run-${n}`, conversation_id: `conv-${n}`, resumable: true, project: "example", ...extra,
});
const pageOf = (rows: ViewRow[], extra: Partial<ArchivePage> = {}): ArchivePage => ({ rows, next_cursor: "", previous_cursor: null, total: rows.length, position: null, ...extra });
const flush = async () => { for (let i = 0; i < 10; i++) await Promise.resolve(); };
const LOCAL: PagingComputer = { computer: "", machine: "Local" };
const BOX: PagingComputer = { computer: "box", machine: "box" };

interface Call { computer: string; from: number; until: number; cursor: string; query: string; resolve(p: ArchivePage): void; reject(e: unknown): void }

function setup(options: { active?: boolean; computers?: PagingComputer[]; firstRequest?: number } = {}) {
  const calls: Call[] = [];
  const read = vi.fn((computer: string, from: number, until: number, cursor: string, query: string, _request: number) =>
    new Promise<ArchivePage>((resolve, reject) => { calls.push({ computer, from, until, cursor, query, resolve, reject }); }));
  const [active, setActive] = createSignal(options.active ?? true);
  const [query, setQuery] = createSignal("");
  const [computers, setComputers] = createSignal<PagingComputer[]>(options.computers ?? [LOCAL]);
  const cancel = vi.fn((_through: number) => {});
  let archive!: ReturnType<typeof createArchive>;
  const dispose = createRoot((d) => { archive = createArchive({ read, cancel, active, query, computers, now: () => new Date(NOW), firstRequest: options.firstRequest }); return d; });
  /** Answers the call at `i` (default: the latest) and lets its handlers run. */
  const answer = async (page: ArchivePage, i = calls.length - 1) => { calls[i].resolve(page); await flush(); };
  const fail = async (error: unknown, i = calls.length - 1) => { calls[i].reject(error); await flush(); };
  return { archive, read, cancel, calls, answer, fail, setActive, setQuery, setComputers, dispose };
}

beforeEach(() => { vi.useFakeTimers(); vi.setSystemTime(NOW); });
afterEach(() => vi.useRealTimers());

describe("pages", () => {
  it("load the first page of every paging computer only while the Archive filter shows the list", async () => {
    const t = setup({ active: false, computers: [LOCAL, BOX] });
    expect(t.read).not.toHaveBeenCalled();
    t.setActive(true);
    expect(t.calls.map((c) => [c.computer, c.from, c.until, c.cursor, c.query])).toEqual([["", 0, ALL_UNTIL, "", ""], ["box", 0, ALL_UNTIL, "", ""]]);
    expect(t.archive.status().summary).toBe("Loading archive…");
    await t.answer(pageOf([arow(1, 100)]), 0);
    await t.answer(pageOf([arow(2, 200), arow(3, 300)], { total: 4, next_cursor: "c" }), 1);
    expect(t.archive.status().summary).toBe("3 loaded of 5");
    t.dispose();
  });

  it("Older loads the cursor and Newer the previous cursor, each replacing the one bounded page", async () => {
    const t = setup();
    await t.answer(pageOf([arow(1, 300), arow(2, 200)], { total: 3, next_cursor: "c1", position: 0 }));
    expect(t.archive.window()).toBe(1);
    let s = t.archive.status();
    expect(s.summary).toBe("1–2 of 3");
    expect(s.older).toEqual({ visible: true, enabled: true });
    expect(s.newer.visible).toBe(false);
    t.archive.older();
    expect(t.calls[1].cursor).toBe("c1");
    expect(t.archive.status().older).toEqual({ visible: true, enabled: false });
    await t.answer(pageOf([arow(3, 100)], { total: 3, previous_cursor: "p0", position: 2 }));
    expect(t.archive.rows().map((r) => r.name)).toEqual(["codex/example/a3"]);
    expect(t.archive.window()).toBe(2);
    s = t.archive.status();
    expect(s.summary).toBe("3–3 of 3");
    expect(s.older.visible).toBe(false);
    expect(s.newer).toEqual({ visible: true, enabled: true });
    t.archive.older();
    expect(t.calls).toHaveLength(2);
    t.archive.newer();
    expect(t.calls[2].cursor).toBe("p0");
    await t.answer(pageOf([arow(1, 300), arow(2, 200)], { total: 3, next_cursor: "c1", position: 0 }));
    expect(t.archive.rows().map((r) => r.name)).toEqual(["codex/example/a1", "codex/example/a2"]);
    expect(t.archive.status().newer.visible).toBe(false);
    t.dispose();
  });

  it("an empty page while rows are shown is the Archive changed error and keeps the rows", async () => {
    const t = setup();
    await t.answer(pageOf([arow(1, 300)], { total: 2, next_cursor: "c1" }));
    t.archive.older();
    await t.answer(pageOf([], { total: 0 }));
    expect(t.archive.rows().map((r) => r.name)).toEqual(["codex/example/a1"]);
    expect(t.archive.pages()[""].cursor).toBe("c1");
    expect(t.archive.window()).toBe(1);
    expect(t.archive.status().error).toBe(`Local: ${ARCHIVE_CHANGED}`);
    expect(ARCHIVE_CHANGED).toBe("Archive changed. Refresh to start a new view.");
    t.dispose();
  });

  it("a failed read shows its error by machine, at most 240 characters, the full text in the hint", async () => {
    const t = setup({ computers: [LOCAL, BOX] });
    await t.fail("Invalid archive page. Update the computer CLI and refresh.", 0);
    const long = "x".repeat(300);
    await t.fail(long, 1);
    const s = t.archive.status();
    expect(s.errorHint).toBe(`Local: Invalid archive page. Update the computer CLI and refresh.\nbox: ${long}`);
    expect(s.error).toBe(s.errorHint.slice(0, 240));
    expect(s.error).toHaveLength(240);
    expect(t.archive.rows()).toEqual([]);
    t.dispose();
  });

  it("ignores a superseded read", async () => {
    const t = setup();
    await t.fail("superseded");
    const s = t.archive.status();
    expect(s.error).toBe("");
    expect(s.summary).not.toBe("Loading archive…");
    t.dispose();
  });
});

describe("cancellation by the store's request token", () => {
  it("Refresh drops the pages, discards the answer in flight and loads the first page again", async () => {
    const t = setup();
    await t.answer(pageOf([arow(1, 300)], { total: 2, next_cursor: "c1" }));
    t.archive.older();
    t.archive.refresh();
    expect(t.archive.rows()).toEqual([]);
    expect(t.calls.map((c) => c.cursor)).toEqual(["", "c1", ""]);
    await t.answer(pageOf([arow(2, 200)], { total: 2 }), 1);
    expect(t.archive.rows()).toEqual([]);
    expect(t.archive.status().summary).toBe("Loading archive…");
    await t.answer(pageOf([arow(1, 300)], { total: 2, next_cursor: "c1" }), 2);
    expect(t.archive.rows().map((r) => r.name)).toEqual(["codex/example/a1"]);
    t.dispose();
  });

  it("a query change drops the pages and reloads after 300 ms with the trimmed query", async () => {
    const t = setup();
    t.setQuery("  review ");
    expect(t.archive.rows()).toEqual([]);
    await t.answer(pageOf([arow(1, 300)]), 0);
    expect(t.archive.rows()).toEqual([]);
    vi.advanceTimersByTime(200);
    t.setQuery("review 2");
    vi.advanceTimersByTime(299);
    expect(t.calls).toHaveLength(1);
    vi.advanceTimersByTime(1);
    expect(t.calls.map((c) => c.query)).toEqual(["", "review 2"]);
    t.setQuery(" review 2 ");
    vi.advanceTimersByTime(300);
    expect(t.calls).toHaveLength(2);
    t.dispose();
  });

  it("Refresh during the query delay loads at once and ends the delay", () => {
    const t = setup();
    t.setQuery("x");
    t.archive.refresh();
    expect(t.calls.map((c) => c.query)).toEqual(["", "x"]);
    vi.advanceTimersByTime(300);
    expect(t.calls).toHaveLength(2);
    t.dispose();
  });

  it("leaving the Archive filter discards answers in flight and keeps the loaded pages, as Qt's configure", async () => {
    const t = setup({ computers: [LOCAL, BOX] });
    await t.answer(pageOf([arow(1, 300)]), 0);
    t.setActive(false);
    await t.answer(pageOf([arow(2, 200)]), 1);
    expect(t.archive.rows().map((r) => r.name)).toEqual(["codex/example/a1"]);
    expect(t.archive.status().summary).toBe("1 loaded");
    t.archive.older();
    expect(t.calls).toHaveLength(2);
    t.setActive(true);
    expect(t.calls.map((c) => c.computer)).toEqual(["", "box", "box"]);
    expect(t.archive.rows().map((r) => r.name)).toEqual(["codex/example/a1"]);
    t.dispose();
  });

  it("a change of the paging computers drops every page and loads again; the same computers do not", async () => {
    const t = setup();
    await t.answer(pageOf([arow(1, 300)]));
    t.setComputers([LOCAL]);
    expect(t.calls).toHaveLength(1);
    t.setComputers([LOCAL, BOX]);
    expect(t.archive.rows()).toEqual([]);
    expect(t.calls.map((c) => c.computer)).toEqual(["", "", "box"]);
    t.dispose();
  });
});

describe("names and cancelling (ArchiveBrowser::configure, cancelArchivePages)", () => {
  it("a machine name change drops the pages and loads again, so no row keeps the old name", async () => {
    const t = setup();
    await t.answer(pageOf([arow(1, 300)]));
    expect(t.archive.rows().map((r) => r.machine)).toEqual(["Local"]);
    t.setComputers([{ computer: "", machine: "workstation" }]);
    expect(t.archive.rows()).toEqual([]);
    expect(t.calls).toHaveLength(2);
    await t.answer(pageOf([arow(1, 300)]));
    expect(t.archive.rows().map((r) => r.machine)).toEqual(["workstation"]);
    expect(t.archive.status().hint).toBe("workstation: 1 shown of 1");
    t.dispose();
  });

  it("an answer for the old name is discarded", async () => {
    const t = setup();
    t.setComputers([{ computer: "", machine: "workstation" }]);
    await t.answer(pageOf([arow(1, 300)]), 0);
    expect(t.archive.rows()).toEqual([]);
    await t.answer(pageOf([arow(2, 200)]), 1);
    expect(t.archive.rows().map((r) => [r.name, r.machine])).toEqual([["codex/example/a2", "workstation"]]);
    t.dispose();
  });

  it("passes its request number to every read and cancels through the highest one issued", () => {
    const t = setup({ computers: [LOCAL, BOX] });
    expect(t.read.mock.calls.map((c) => c[5])).toEqual([1, 2]);
    expect(t.cancel.mock.calls).toEqual([[0]]);
    t.archive.refresh();
    expect(t.cancel.mock.calls.at(-1)).toEqual([2]);
    expect(t.read.mock.calls.slice(2).map((c) => c[5])).toEqual([3, 4]);
    t.setActive(false);
    expect(t.cancel.mock.calls.at(-1)).toEqual([4]);
    t.dispose();
  });

  it("numbers its requests from the first number it is given (the app's start time)", () => {
    const t = setup({ firstRequest: 1_790_000_000_000 });
    expect(t.read.mock.calls.map((c) => c[5])).toEqual([1_790_000_000_001]);
    t.dispose();
  });

  it("cancels the reads in flight on leaving, a computers change, a query change and Refresh, and not on a plain re-configure", () => {
    const t = setup({ active: false });
    // The paging computers changed from none.
    expect(t.cancel).toHaveBeenCalledTimes(1);
    t.setActive(true);
    t.setComputers([LOCAL]);
    expect(t.cancel).toHaveBeenCalledTimes(1);
    t.setActive(false);
    expect(t.cancel).toHaveBeenCalledTimes(2);
    t.setActive(true);
    expect(t.cancel).toHaveBeenCalledTimes(2);
    t.setComputers([LOCAL, BOX]);
    expect(t.cancel).toHaveBeenCalledTimes(3);
    t.setQuery("x");
    expect(t.cancel).toHaveBeenCalledTimes(4);
    t.setQuery(" x ");
    expect(t.cancel).toHaveBeenCalledTimes(4);
    const before = t.read.mock.calls.length;
    t.archive.refresh();
    expect(t.cancel).toHaveBeenCalledTimes(5);
    expect(t.cancel.mock.invocationCallOrder[4]).toBeLessThan(t.read.mock.invocationCallOrder[before]);
    // A date-range change refreshes (Qt's rangeChanged); an unchanged range does nothing.
    t.archive.setRange({ ...t.archive.range() });
    expect(t.cancel).toHaveBeenCalledTimes(5);
    t.archive.setRange({ ...t.archive.range(), period: 2 });
    expect(t.cancel).toHaveBeenCalledTimes(6);
    t.dispose();
  });
});

describe("the date range", () => {
  it("a changed range reloads with its bounds, an unchanged one does not, and from >= until loads nothing", async () => {
    const t = setup();
    await t.answer(pageOf([arow(1, 300)]));
    const range = t.archive.range();
    expect(range.period).toBe(0);
    t.archive.setRange({ ...range });
    expect(t.calls).toHaveLength(1);
    t.archive.setRange({ ...range, period: 2 });
    expect(t.archive.rows()).toEqual([]);
    expect([t.calls[1].from, t.calls[1].until]).toEqual([local(2026, 10, 22), local(2026, 10, 29)]);
    expect(t.archive.includes(local(2026, 10, 22))).toBe(true);
    expect(t.archive.includes(local(2026, 10, 29))).toBe(false);
    t.archive.setRange({ period: 4, first: "2026-10-15", last: "2026-10-14" });
    expect(t.calls).toHaveLength(2);
    expect(t.archive.status().older.visible).toBe(false);
    t.dispose();
  });
});

describe("totals and the count chip", () => {
  it("show … until every total is known, then the count with + while more pages remain", async () => {
    const t = setup({ computers: [LOCAL, BOX] });
    expect(t.archive.countText(0)).toBe("…");
    expect(t.archive.totalsKnown()).toBe(false);
    await t.answer(pageOf([arow(1, 300), arow(2, 200)]), 0);
    expect(t.archive.total()).toBe(2);
    expect(t.archive.countText(2)).toBe("2+");
    await t.answer(pageOf([arow(3, 100)], { total: 5, next_cursor: "c" }), 1);
    expect(t.archive.totalsKnown()).toBe(true);
    expect(t.archive.hasMore()).toBe(true);
    expect(t.archive.total()).toBe(7);
    expect(t.archive.countText(3)).toBe("3+");
    expect(t.archive.status().summary).toBe("3 loaded of 7");
    t.dispose();
  });

  it("count without + once every page is complete, and plainly without paging computers", () => {
    const page = (rows: number, extra: Partial<Page> = {}): Page => ({ ...emptyPage(), loaded: true, total: rows, rows: Array.from({ length: rows }, () => ({}) as ArchivedSession), ...extra });
    const hosts = [LOCAL, BOX];
    expect(countText(3, { "": page(2), box: page(1) }, hosts)).toBe("3");
    expect(countText(2, { "": page(2, { total: 3 }), box: page(0) }, hosts)).toBe("2+");
    expect(countText(1, { "": page(1, { previous: "p" }), box: page(0) }, hosts)).toBe("1+");
    expect(countText(1, { "": page(1, { error: "boom" }), box: page(0) }, hosts)).toBe("1+");
    expect(countText(0, { "": page(0), box: page(0) }, hosts)).toBe("0");
    expect(countText(0, {}, [])).toBe("0");
  });
});

describe("the count chip's tooltip (SessionsWindowRows.cpp 564)", () => {
  it("says how many are loaded and whether more can be loaded, or the plain count without paging", () => {
    const page = (rows: number, extra: Partial<Page> = {}): Page => ({ ...emptyPage(), loaded: true, total: rows, rows: Array.from({ length: rows }, () => ({}) as ArchivedSession), ...extra });
    expect(tipText("Archive", 2, { "": page(2, { cursor: "c" }) }, [LOCAL])).toBe("Archive / 2 loaded; more can be loaded in Archive");
    expect(tipText("Archive", 0, {}, [LOCAL])).toBe("Archive / 0 loaded; more can be loaded in Archive");
    expect(tipText("Archive", 3, { "": page(2), box: page(1) }, [LOCAL, BOX])).toBe("Archive / 3 loaded");
    expect(tipText("Archive", 0, {}, [])).toBe("Archive / 0");
  });
});

describe("the status text (ArchiveBrowser::updateStatus)", () => {
  const loaded = (rows: number, total: number, extra: Partial<Page> = {}): Page =>
    ({ ...emptyPage(), loaded: true, total, rows: Array.from({ length: rows }, () => ({}) as ArchivedSession), ...extra });

  it("words loading, no computers, one window and the loaded totals", () => {
    expect(statusOf({ "": { ...emptyPage(), request: 4 } }, [LOCAL], true).summary).toBe("Loading archive…");
    expect(statusOf({}, [], true).summary).toBe("Loaded sessions");
    expect(statusOf({ "": loaded(50, 257, { position: 50 }) }, [LOCAL], true).summary).toBe("51–100 of 257");
    expect(statusOf({ "": loaded(1, 2) }, [LOCAL], true).summary).toBe("1 shown of 2");
    expect(statusOf({ "": loaded(0, 5, { position: 0 }) }, [LOCAL], true).summary).toBe("0 shown of 5");
    expect(statusOf({ "": loaded(2, 2), box: loaded(1, 5) }, [LOCAL, BOX], true).summary).toBe("3 loaded of 7");
    expect(statusOf({ "": loaded(2, 2) }, [LOCAL, BOX], true).summary).toBe("2 loaded");
    expect(statusOf({}, [LOCAL], true).summary).toBe("0 loaded");
  });

  it("puts each computer's window in the hint and sets the buttons as Qt", () => {
    const s = statusOf({ "": loaded(50, 257, { position: 0, cursor: "c" }), box: loaded(1, 2, { previous: "p" }) }, [LOCAL, BOX], true);
    expect(s.hint).toBe("Local: 1–50 of 257\nbox: 1 shown of 2");
    expect(s.older).toEqual({ visible: true, enabled: true });
    expect(s.newer).toEqual({ visible: true, enabled: true });
    expect(s.refreshEnabled).toBe(true);
    const invalid = statusOf({ "": loaded(1, 2, { cursor: "c", previous: "p" }) }, [LOCAL], false);
    expect(invalid.older).toEqual({ visible: true, enabled: false });
    expect(invalid.newer).toEqual({ visible: true, enabled: false });
    const busy = statusOf({ "": { ...loaded(1, 2), request: 9 } }, [LOCAL], true);
    expect(busy.refreshEnabled).toBe(false);
    expect(busy.older).toEqual({ visible: true, enabled: false });
    expect(statusOf({ "": loaded(1, 1) }, [LOCAL], true).older.visible).toBe(false);
    expect(statusOf({}, [], true).older.visible).toBe(false);
  });

  it("drops the Older and Newer text below 310 px", () => {
    expect(buttonText(309, "Older")).toBe("");
    expect(buttonText(310, "Older")).toBe("Older");
    expect(buttonText(400, "Newer")).toBe("Newer");
  });
});

describe("rows", () => {
  it("sort newest first, keyed by computer and archive id, named as the paging computer names them, without marks", async () => {
    const t = setup({ computers: [LOCAL, BOX] });
    await t.answer(pageOf([arow(1, 100), arow(3, 300)]), 0);
    await t.answer(pageOf([arow(2, 200)]), 1);
    const rows = t.archive.rows();
    expect(rows.map((r) => [r.key, r.machine, r.archivedAt])).toEqual([
      [`\narchive:${uuid(3)}`, "Local", 300], [`box\narchive:${uuid(2)}`, "box", 200], [`\narchive:${uuid(1)}`, "Local", 100],
    ]);
    expect(rows[0]).toMatchObject({ state: "archived", archiveId: uuid(3), catalogKey: `archive:${uuid(3)}`, caption: "Archived", status: "ended", computer: "" });
    expect(t.archive.row(`box\narchive:${uuid(2)}`)?.name).toBe("codex/example/a2");
    expect(t.archive.row(`box\narchive:${uuid(9)}`)).toBeUndefined();
    t.dispose();
  });

  it("an archived session keeps the row's own machine and takes the host values as they are", () => {
    const s = archivedSession("box", "box", arow(4, 50, { machine: "studio", unread: true, muted: true }));
    expect(s.machine).toBe("studio");
    expect(s.unread).toBe(true);
    expect(s.reviewLater).toBe(false);
    expect(s.archivedAt).toBe(50);
  });

  it("list as flat cards inside the date range and the machine and project filters", () => {
    const one = archivedSession("", "Local", arow(1, 300));
    const two = archivedSession("box", "box", arow(2, 200, { project: "other" }));
    const old = archivedSession("", "Local", arow(3, 10));
    const scope = { machines: [] as string[], projects: [] as string[], includes: (at: number) => at >= 100, compact: (key: string) => key === two.key };
    expect(archiveListRows([one, two, old], scope)).toEqual([
      { type: "card", id: one.key, session: one, expandable: false, expanded: false, compactRow: false },
      { type: "card", id: two.key, session: two, expandable: false, expanded: false, compactRow: true },
    ]);
    expect(archiveListRows([one, two, old], { ...scope, machines: ["box"] }).map((r) => r.id)).toEqual([two.key]);
    expect(archiveListRows([one, two, old], { ...scope, projects: ["example"] }).map((r) => r.id)).toEqual([one.key]);
  });
});

describe("the paging computers", () => {
  it("are the live computers inside the machine filter, named as their rows name them", () => {
    const row = (computer: string, machine: string) => toSession(computer, `run:${computer}`, { name: "codex/p/t", machine });
    const computers = {
      "": { id: "", status: "live", attempt: 0 },
      box: { id: "box", status: "live", attempt: 0 },
      gone: { id: "gone", status: "reconnecting", attempt: 2 },
      idle: { id: "idle", status: "live", attempt: 0 },
    };
    const rows = [row("box", "build-box"), row("", "workstation"), row("gone", "gone")];
    expect(pagingComputers(computers, rows, [])).toEqual([
      { computer: "", machine: "workstation" }, { computer: "box", machine: "build-box" }, { computer: "idle", machine: "idle" },
    ]);
    expect(pagingComputers(computers, rows, ["build-box"])).toEqual([{ computer: "box", machine: "build-box" }]);
    expect(pagingComputers({ "": { id: "", status: "live", attempt: 0 } }, [], [])).toEqual([{ computer: "", machine: "this computer" }]);
  });
});

describe("the bridge call", () => {
  it("the app's store reads nothing until the Archive filter shows the list", () => {
    expect(vi.mocked(bridge).archivePage).not.toHaveBeenCalled();
  });
});
