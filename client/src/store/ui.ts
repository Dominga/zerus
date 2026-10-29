import { createMemo, createSignal } from "solid-js";
import { createStore } from "solid-js/store";
import { catalog, computerFresh, orderedRows } from "./catalog";
import { documentVisibility } from "./visibility";
import { isActive, isWorking, needsAttention } from "./attention";
import type { Session, Status } from "../ui/sessions/types";
export { isActive, needsAttention };

// Interface state of the first screen: page, selection, filters, list layout,
// menus and toasts. Data lives in store/catalog.ts and store/activity.ts.

export type MenuEntry =
  | { label: string; icon?: string; kbd?: string; danger?: boolean; disabled?: boolean; hint?: string; checked?: boolean; action?: () => void }
  | { separator: true }
  | { heading: string };

export type Page = "dashboard" | "sessions" | "projects" | "machines" | "accounts" | "settings";
export type StateFilter = "all" | "attention" | "active" | "paused" | "archived";
/** The Zerus mark's motion; the owner chose the two-turn spin-up (2026-10-27). */
export type LogoMotion = "spin";

const params = new URLSearchParams(location.search);

const PAGES: Page[] = ["dashboard", "sessions", "projects", "machines", "accounts", "settings"];
// `?page=settings` opens a screen (headless screenshots), as in the mockup.
export const [page, setPage] = createSignal<Page>(PAGES.find((p) => p === params.get("page")) ?? "sessions");
export const [selectedKey, setSelectedKey] = createSignal<string | undefined>();
export const [stateFilter, setStateFilter] = createSignal<StateFilter>("all");
export const [machineFilter, setMachineFilter] = createSignal<string[]>([]);
export const [projectFilter, setProjectFilter] = createSignal<string[]>([]);
export const [multi, setMulti] = createStore<Record<string, boolean>>({});
export const [peek, setPeek] = createSignal(false);
export type DetailTab = "activity" | "terminal" | "native" | "processes";
export const [detailTab, setDetailTab] = createSignal<DetailTab>("activity");
export const [subagentOpen, setSubagentOpen] = createSignal<string | undefined>();
export const [search, setSearch] = createSignal(params.get("q") ?? "");
export const [expandedChildren, setExpandedChildren] = createStore<Record<string, boolean>>({});
export const [compactRows, setCompactRows] = createStore<Record<string, boolean>>({});
export const [stripMode, setStripMode] = createSignal(params.get("strip") === "1");
export const [menu, setMenu] = createSignal<{ x: number; y: number; items: MenuEntry[] } | undefined>();
export const [contentScale, setContentScale] = createSignal(1);
export const [toasts, setToasts] = createStore<{ items: { id: number; title: string; text: string }[] }>({ items: [] });
/** A row the session list scrolls to once it lists it (Qt's `revealRow`); a new selection drops it. */
export const [revealing, setRevealing] = createSignal<string | undefined>();
export const revealRow = (id: string) => setRevealing(id);
export const logoMotion = (): LogoMotion => "spin";

/**
 * Seconds since the Unix epoch, ticking once a second: only Working clocks read
 * it. The clock stops while the window is hidden or minimised, as Qt stops its
 * elapsed timer in hideEvent, and catches up when the window shows again.
 */
export const [now, setNow] = createSignal(Date.now() / 1000);
let clock: ReturnType<typeof setInterval> | undefined;
const tick = () => setNow(Date.now() / 1000);
const syncClock = () => {
  if (documentVisibility.hidden()) { clearInterval(clock); clock = undefined; }
  else if (clock === undefined) { tick(); clock = setInterval(tick, 1000); }
};
syncClock();
documentVisibility.onChange(syncClock);

// Docked session list width (QSplitter default 310, minimum 270), remembered per viewer.
const SESSIONS_WIDTH_KEY = "zerus-client-sessions-width";
const savedWidth = (() => { try { return Number(localStorage.getItem(SESSIONS_WIDTH_KEY)) || 0; } catch { return 0; } })();
export const [sessionsWidth, setSessionsWidthRaw] = createSignal(savedWidth >= 270 ? savedWidth : 310);
export function setSessionsWidth(width: number) {
  setSessionsWidthRaw(width);
  try { localStorage.setItem(SESSIONS_WIDTH_KEY, String(width)); } catch { /* storage may be unavailable */ }
}

let toastId = 0;
export function toast(title: string, text: string) {
  const id = ++toastId;
  setToasts("items", (items) => [...items, { id, title, text }]);
  setTimeout(() => setToasts("items", (items) => items.filter((t) => t.id !== id)), 4200);
}

let archivedRow: (key: string) => Session | undefined = () => undefined;
/** Hands the selection the loaded archived rows (store/archive.ts), which this module cannot import: archive.ts reads the filters here. */
export function bindArchivedRows(lookup: (key: string) => Session | undefined) { archivedRow = lookup; }

export const selected = createMemo<Session | undefined>(() => {
  const key = selectedKey();
  return key ? catalog.rows[key] ?? archivedRow(key) : undefined;
});

/** The window's online state of a row: its computer's rows are current (Qt's `entry.online`). */
const online = (s: Session) => computerFresh(s.computer);

export function matchesFilter(s: Session, f: StateFilter): boolean {
  switch (f) {
    case "all": return true;
    case "attention": return needsAttention(s, online(s));
    case "active": return isActive(s, online(s));
    case "paused": return s.status === "paused";
    case "archived": return s.state === "archived";
  }
}

export const counts = createMemo(() => {
  let all = 0, active = 0, attention = 0, working = 0, paused = 0;
  for (const s of orderedRows()) {
    all++;
    const up = online(s);
    if (isActive(s, up)) active++;
    if (needsAttention(s, up) && !s.muted) attention++;
    if (isWorking(s, up)) working++;
    if (s.status === "paused") paused++;
  }
  return { all, active, attention, working, paused };
});

export const STATUS_LABEL: Record<Status, string> = {
  attention: "Needs attention",
  working: "Working",
  background: "Background",
  scheduled: "Scheduled",
  ready: "Ready",
  error: "Error",
  offline: "Offline",
  paused: "Paused",
  waiting: "Waiting for result",
  starting: "Starting",
  ended: "Archived",
};
