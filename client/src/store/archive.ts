import { batch, createEffect, createMemo, createRoot, createSignal, on, untrack } from "solid-js";
import * as bridge from "../bridge";
import { bindArchivedRows as bindActionRows } from "./actions";
import { catalog, toSession, type ComputerState } from "./catalog";
import type { RowMarks } from "./marks";
import { bindArchivedRows as bindSelectedRows, compactRows, machineFilter, page, projectFilter, search, stateFilter } from "./ui";
import { initialRange, rangeBounds, type DateRange } from "../ui/sessions/archive-dates";
import type { ArchivePage, ComputerId, ViewRow } from "../types";
import type { Row } from "../ui/sessions/model";
import type { Session } from "../ui/sessions/types";

// The Archive view's pages, after tray/src/ArchiveBrowser.cpp: one bounded
// page and two cursor anchors per computer, never a growing history. Older
// loads a page's cursor and Newer its previous cursor, each replacing the
// page. Pages load only while the Archive filter shows the list. Refresh, a
// query change (after 300 ms), a date-range change and a change of the paging
// computers drop every page; each request carries the store's own token
// (the page's request id), so an answer for a dropped or cancelled page is
// discarded, as Qt's cancelArchivePages. Leaving the Archive filter discards
// the answers in flight and keeps the loaded pages (Qt's configure).

export const ARCHIVE_CHANGED = "Archive changed. Refresh to start a new view.";
export const QUERY_DELAY_MS = 300;
/** Errors show at most this many characters; the hint has them all. */
export const ERROR_CHARS = 240;
/** Older and Newer lose their text below this width. */
export const NARROW_PX = 310;
/** A newer request of the same computer replaced this one (src-tauri `archive_page`). */
const SUPERSEDED = "superseded";

/** A computer whose archive is paged: its id and its machine name as its rows name it. */
export interface PagingComputer { computer: ComputerId; machine: string }

/** An archived row: a Session keyed by computer and archive id, and when it was archived. */
export type ArchivedSession = Session & { archivedAt: number };

export interface Page {
  rows: ArchivedSession[];
  cursor: string;
  previous: string;
  error: string;
  /** The request in flight (the store's token), 0 when none. */
  request: number;
  total: number;
  /** The window's first row in the whole archive, -1 when not reported. */
  position: number;
  loaded: boolean;
  newer: boolean;
}

export const emptyPage = (): Page => ({ rows: [], cursor: "", previous: "", error: "", request: 0, total: 0, position: -1, loaded: false, newer: false });

export interface ArchiveStatus {
  summary: string;
  /** Each computer's window, one per line. */
  hint: string;
  /** "{machine}: {error}" per computer, at most ERROR_CHARS characters. */
  error: string;
  errorHint: string;
  older: { visible: boolean; enabled: boolean };
  newer: { visible: boolean; enabled: boolean };
  refreshEnabled: boolean;
}

type Pages = Readonly<Record<string, Page>>;

/** The pages in Qt's map order (by computer id). */
const pageIds = (pages: Pages) => Object.keys(pages).sort();
const machineOf = (hosts: readonly PagingComputer[], computer: string) => hosts.find((h) => h.computer === computer)?.machine ?? computer;

export const totalOf = (pages: Pages) => Object.values(pages).reduce((n, p) => n + p.total, 0);

/** Every paging computer's page has loaded once. */
export const totalsKnownOf = (pages: Pages, hosts: readonly PagingComputer[]) => hosts.length > 0 && hosts.every((h) => pages[h.computer]?.loaded);

/** Some computer has rows beyond its page, a page still to load, or an error. */
export const hasMoreOf = (pages: Pages, hosts: readonly PagingComputer[]) => hosts.some((h) => {
  const p = pages[h.computer] ?? emptyPage();
  return !p.loaded || !!p.cursor || !!p.previous || p.rows.length < p.total || !!p.error;
});

/** The Archive filter's count (SessionsWindowRows.cpp 563-565): "…" until the totals are known with none counted, "+" while more remains. */
export function countText(count: number, pages: Pages, hosts: readonly PagingComputer[]): string {
  const paged = hosts.length > 0;
  const known = totalsKnownOf(pages, hosts);
  if (paged && !known && !count) return "…";
  return `${count}${paged && (!known || hasMoreOf(pages, hosts)) ? "+" : ""}`;
}

/** The Archive filter's tooltip (SessionsWindowRows.cpp 564): "{caption} / {n} loaded", and whether more can be loaded; the plain count without paging. */
export function tipText(caption: string, count: number, pages: Pages, hosts: readonly PagingComputer[]): string {
  if (!hosts.length) return `${caption} / ${count}`;
  const partial = !totalsKnownOf(pages, hosts) || hasMoreOf(pages, hosts);
  return `${caption} / ${count} loaded${partial ? "; more can be loaded in Archive" : ""}`;
}

/** ArchiveBrowser::updateStatus. `valid`: the range's from is below its until. */
export function statusOf(pages: Pages, hosts: readonly PagingComputer[], valid: boolean): ArchiveStatus {
  let busy = false, more = false, newer = false, single = "", shown = 0;
  const errors: string[] = [], windows: string[] = [];
  for (const id of pageIds(pages)) {
    const p = pages[id];
    busy ||= p.request !== 0;
    more ||= !p.loaded || !!p.cursor;
    newer ||= !!p.previous;
    if (p.error) errors.push(`${machineOf(hosts, id)}: ${p.error}`);
    if (p.loaded) {
      single = p.position >= 0 && p.rows.length ? `${p.position + 1}–${p.position + p.rows.length} of ${p.total}` : `${p.rows.length} shown of ${p.total}`;
      windows.push(`${machineOf(hosts, id)}: ${single}`);
    }
    shown += p.rows.length;
  }
  const summary = busy ? "Loading archive…" : !hosts.length ? "Loaded sessions" : hosts.length === 1 && single ? single
    : totalsKnownOf(pages, hosts) ? `${shown} loaded of ${totalOf(pages)}` : `${shown} loaded`;
  const errorHint = errors.join("\n");
  return {
    summary, hint: windows.join("\n"), error: errorHint.slice(0, ERROR_CHARS), errorHint,
    older: { visible: hosts.length > 0 && (more || busy), enabled: valid && !busy },
    newer: { visible: newer, enabled: valid && !busy },
    refreshEnabled: !busy,
  };
}

/** The Older or Newer text: none below NARROW_PX (ArchiveBrowser::resizeEvent). */
export const buttonText = (width: number, text: "Older" | "Newer") => (width < NARROW_PX ? "" : text);

/** Archived rows take no local marks: the host's values as they are (Qt keeps them out of FleetState). */
const HOST_VALUES: Pick<RowMarks, "overlay"> = {
  overlay: (s) => ({ status: s.status, caption: s.caption, attention: s.attention, unread: s.unread, reviewLater: s.reviewLater, summary: s.summary, muted: s.muted, notificationConflict: s.notificationConflict }),
};

/** One archived row as a Session: keyed `archive:<archive id>` on its computer, named as the paging computer names it. */
export function archivedSession(computer: ComputerId, machine: string, row: ViewRow): ArchivedSession {
  const s = toSession(computer, `archive:${row.archive_id ?? ""}`, { ...row, machine: row.machine || machine }, HOST_VALUES);
  return Object.assign(s, { archivedAt: typeof row.archived_at === "number" ? row.archived_at : 0 });
}

/** Newest first; then by computer, name and archive id (Qt's entry key). */
function newestFirst(a: ArchivedSession, b: ArchivedSession): number {
  if (a.archivedAt !== b.archivedAt) return b.archivedAt - a.archivedAt;
  const x = `${a.computer}\n${a.name}\n${a.archiveId ?? ""}`, y = `${b.computer}\n${b.name}\n${b.archiveId ?? ""}`;
  return x < y ? -1 : x > y ? 1 : 0;
}

/**
 * The paging computers: every computer whose stream is live and whose machine
 * is inside the machine filter, named as its rows name it (by id otherwise).
 * `rows` is read only until every live computer has a name.
 */
export function pagingComputers(computers: Readonly<Record<string, ComputerState>>, rows: Iterable<Pick<Session, "computer" | "machine">>, machines: readonly string[]): PagingComputer[] {
  const live = Object.keys(computers).filter((id) => computers[id]?.status === "live").sort();
  const wanted = new Set(live);
  const names = new Map<string, string>();
  if (wanted.size) {
    for (const r of rows) {
      if (!wanted.has(r.computer) || names.has(r.computer)) continue;
      names.set(r.computer, r.machine);
      if (names.size === wanted.size) break;
    }
  }
  const out: PagingComputer[] = [];
  for (const computer of live) {
    const machine = names.get(computer) || computer || "this computer";
    if (!machines.length || machines.includes(machine)) out.push({ computer, machine });
  }
  return out;
}

/** What the Archive filter's list and count follow besides the pages. */
export interface ListScope {
  machines: readonly string[];
  projects: readonly string[];
  /** Inside the date range (ArchiveBrowser::includes). */
  includes(archivedAt: number): boolean;
  compact(key: string): boolean;
}

/** In the machine and project filters (what Qt's Archive count follows). */
export const inScope = (s: Pick<Session, "machine" | "project">, scope: Pick<ListScope, "machines" | "projects">) =>
  (!scope.machines.length || scope.machines.includes(s.machine)) && (!scope.projects.length || scope.projects.includes(s.project));

/**
 * The Archive filter's rows (SessionsWindowRows.cpp 270-271): flat cards in
 * the order given, inside the date range and the filters. Qt groups them by
 * project; that is zerus-5wmh.40.
 */
export function archiveListRows(rows: readonly ArchivedSession[], scope: ListScope): Row[] {
  const out: Row[] = [];
  for (const s of rows) {
    if (!scope.includes(s.archivedAt) || !inScope(s, scope)) continue;
    out.push({ type: "card", id: s.key, session: s, expandable: false, expanded: false, compactRow: scope.compact(s.key) });
  }
  return out;
}

export interface ArchiveDeps {
  /**
   * One page of a computer's archive; `request` is the store's request
   * number (its token). Rejects with Qt's text, "superseded" or the hgs failure.
   */
  read(computer: ComputerId, from: number, until: number, cursor: string, query: string, request: number): Promise<ArchivePage>;
  /**
   * Ends every read numbered `through` or lower, also one not started yet
   * (Qt's cancelArchivePages); their answers are discarded by token anyway.
   */
  cancel(through: number): void;
  /** The Archive filter shows the list. */
  active(): boolean;
  /** The search text; the archive query is it trimmed. */
  query(): string;
  computers(): PagingComputer[];
  now?(): Date;
  /** Request numbers start after this; the app passes its start time, so a reload never reuses a cancelled number. */
  firstRequest?: number;
}

const sameHosts = (a: readonly PagingComputer[], b: readonly PagingComputer[]) =>
  a.length === b.length && a.every((h, i) => h.computer === b[i].computer && h.machine === b[i].machine);
const errorText = (error: unknown) => (typeof error === "string" ? error : error instanceof Error ? error.message : String(error));

/** The archive store; call it inside a reactive root. */
export function createArchive(deps: ArchiveDeps) {
  const now = deps.now ?? (() => new Date());
  const [pages, setPages] = createSignal<Pages>({});
  const [hosts, setHosts] = createSignal<readonly PagingComputer[]>([]);
  const [range, setRangeValue] = createSignal<DateRange>(initialRange(now()));
  /** Counts accepted pages (Qt's windowChanged: the list scrolls to the top). */
  const [windowChanges, setWindowChanges] = createSignal(0);
  let active = false;
  let query = "";
  let delay: ReturnType<typeof setTimeout> | undefined;
  /** The highest request number issued (Qt's m_archiveRequest). */
  let requests = deps.firstRequest ?? 0;

  const bounds = () => rangeBounds(range(), now());
  const includes = (at: number) => { const [from, until] = bounds(); return at >= from && at < until; };
  const put = (computer: string, page: Page) => setPages((all) => ({ ...all, [computer]: page }));
  /** Drops every page; their answers in flight no longer match. */
  const dropPages = () => setPages({});

  /** Older (false) or Newer (true) for every paging computer that has a page that way. */
  function navigate(newer: boolean) {
    untrack(() => {
      if (!active) return;
      const [from, until] = bounds();
      if (from >= until) return;
      const all = { ...pages() };
      const calls: [ComputerId, number, string][] = [];
      for (const { computer } of hosts()) {
        const p = all[computer] ?? emptyPage();
        all[computer] = p;
        const cursor = newer ? p.previous : p.cursor;
        if (p.request || (newer && !cursor) || (!newer && p.loaded && !cursor)) continue;
        const id = ++requests;
        all[computer] = { ...p, error: "", newer, request: id };
        calls.push([computer, id, cursor]);
      }
      setPages(all);
      for (const [computer, id, cursor] of calls) {
        let reply: Promise<ArchivePage>;
        try { reply = deps.read(computer, from, until, cursor, query, id); } catch (error) { reply = Promise.reject(error); }
        reply.then((page) => answer(computer, id, page), (error) => fail(computer, id, error));
      }
    });
  }

  function answer(computer: ComputerId, id: number, page: ArchivePage) {
    const p = untrack(pages)[computer];
    if (!p || p.request !== id) return;
    const machine = machineOf(untrack(hosts), computer);
    const rows = page.rows.map((row) => archivedSession(computer, machine, row));
    if (!rows.length && p.rows.length) { put(computer, { ...p, request: 0, error: ARCHIVE_CHANGED }); return; }
    batch(() => {
      put(computer, {
        ...p, request: 0, error: "", loaded: true, rows, cursor: page.next_cursor ?? "", previous: page.previous_cursor ?? "",
        total: page.total ?? 0, position: typeof page.position === "number" ? page.position : -1,
      });
      setWindowChanges((n) => n + 1);
    });
  }

  function fail(computer: ComputerId, id: number, error: unknown) {
    const p = untrack(pages)[computer];
    if (!p || p.request !== id) return;
    const text = errorText(error);
    put(computer, { ...p, request: 0, error: text === SUPERSEDED ? "" : text });
  }

  function refresh() {
    clearTimeout(delay);
    delay = undefined;
    deps.cancel(requests);
    dropPages();
    navigate(false);
  }

  function setQuery(text: string) {
    if (text === query) return;
    query = text;
    deps.cancel(requests);
    dropPages();
    clearTimeout(delay);
    delay = setTimeout(() => { delay = undefined; refresh(); }, QUERY_DELAY_MS);
  }

  /**
   * ArchiveBrowser::configure: the paging computers and whether the Archive
   * filter shows the list. A computer or a machine name that changed drops
   * every page, so no row keeps an old name.
   */
  function configure(next: readonly PagingComputer[], show: boolean) {
    const different = !sameHosts(next, untrack(hosts));
    batch(() => {
      if (!show && active) {
        deps.cancel(requests);
        setPages((all) => Object.fromEntries(Object.entries(all).map(([id, p]) => [id, p.request ? { ...p, request: 0 } : p])));
      }
      active = show;
      if (different) {
        deps.cancel(requests);
        setHosts(next);
        dropPages();
      }
    });
    const all = untrack(pages);
    const initial = next.some((h) => { const p = all[h.computer]; return !p || (!p.loaded && !p.request && !p.error); });
    if (active && delay === undefined && initial) navigate(false);
  }

  /** Apply in the date picker: a changed range reloads (ArchiveDatePicker::apply). */
  function setRange(next: DateRange) {
    const current = untrack(range);
    const changed = current.period !== next.period || (next.period === 4 && (current.first !== next.first || current.last !== next.last));
    setRangeValue(next);
    if (changed) refresh();
  }

  createEffect(on(() => deps.query().trim(), (text) => setQuery(text)));
  const scope = createMemo(() => deps.computers(), [], { equals: sameHosts });
  const showing = createMemo(() => deps.active());
  createEffect(on([scope, showing], ([next, show]) => configure(next, show)));

  const rows = createMemo(() => {
    const all = pages();
    const list: ArchivedSession[] = [];
    for (const id in all) list.push(...all[id].rows);
    return list.sort(newestFirst);
  });
  const byKey = createMemo(() => new Map(rows().map((s) => [s.key, s])));
  const status = createMemo(() => { const [from, until] = bounds(); return statusOf(pages(), hosts(), from < until); });

  return {
    pages,
    /** Every loaded archived row, newest first. */
    rows,
    /** A loaded archived row by key. */
    row: (key: string): ArchivedSession | undefined => byKey().get(key),
    range,
    setRange,
    bounds,
    includes,
    refresh,
    older: () => navigate(false),
    newer: () => navigate(true),
    status,
    total: () => totalOf(pages()),
    totalsKnown: () => totalsKnownOf(pages(), hosts()),
    hasMore: () => hasMoreOf(pages(), hosts()),
    /** Some computer pages its archive. */
    paged: () => hosts().length > 0,
    countText: (count: number) => countText(count, pages(), hosts()),
    tipText: (caption: string, count: number) => tipText(caption, count, pages(), hosts()),
    window: windowChanges,
    computers: hosts,
  };
}

/** The live rows, in display order, for the machine names. */
function* catalogRows(): Iterable<Session> {
  for (const id of catalog.order) {
    const row = catalog.rows[id];
    if (row) yield row;
  }
}

/**
 * The app's archive: pages while the Archive filter shows the Sessions list,
 * the search text as the query, the live computers inside the machine filter.
 * `archiveRows` is the Archive filter's list (model.ts), `archiveCount` its
 * count chip and `archiveTip` the chip's tooltip; the selection and the
 * actions also find the loaded archived rows.
 */
export const { archive, archiveRows, archiveCount, archiveTip } = createRoot(() => {
  const archive = createArchive({
    read: (computer, from, until, cursor, query, request) => bridge.archivePage(computer, from, until, cursor, query, request),
    cancel: (through) => { void bridge.cancelArchivePages(through); },
    firstRequest: Date.now(),
    active: () => stateFilter() === "archived" && page() === "sessions",
    query: search,
    computers: () => pagingComputers(catalog.computers, catalogRows(), machineFilter()),
  });
  const archiveRows = createMemo(() => archiveListRows(archive.rows(), {
    machines: machineFilter(), projects: projectFilter(), includes: archive.includes, compact: (key) => !!compactRows[key],
  }));
  const counted = createMemo(() => {
    const scope = { machines: machineFilter(), projects: projectFilter() };
    return archive.rows().filter((s) => inScope(s, scope)).length;
  });
  return { archive, archiveRows, archiveCount: () => archive.countText(counted()), archiveTip: () => archive.tipText("Archive", counted()) };
});

let retained: (key: string) => Session | undefined = () => undefined;
/**
 * Hands the selection the archived rows of pending restores (store/restores.ts),
 * which stay the selected row while the archive reloads (Qt's selected()).
 * The actions find loaded rows only.
 */
export function bindRetainedRows(lookup: (key: string) => Session | undefined) { retained = lookup; }

bindSelectedRows((key) => archive.row(key) ?? retained(key));
bindActionRows(archive.row);
