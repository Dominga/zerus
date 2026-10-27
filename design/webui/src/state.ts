import { createSignal, createMemo, batch } from "solid-js";
import { createStore, reconcile } from "solid-js/store";
import { sessions as seedSessions, archived, stressSessions } from "./mock/data";
import type { Session, Status } from "./mock/types";

// UI state for the mockup. In the Tauri client the catalog store is fed by
// whole-row upserts from the Rust backend (`reconcile` keyed by `key`), so a
// status change touches only the DOM nodes bound to that row.

export type MenuEntry =
  | { label: string; icon?: string; kbd?: string; danger?: boolean; disabled?: boolean; checked?: boolean; action?: () => void }
  | { separator: true }
  | { heading: string };

export type Page = "dashboard" | "sessions" | "projects" | "machines" | "accounts" | "settings";
export type DetailTab = "activity" | "terminal" | "processes" | "native" | "taskview";
export type InspectorTab = "plan" | "details" | "worktrees" | "tasks";
export type StateFilter = "all" | "attention" | "active" | "paused" | "archived";

// URL parameters open a given screen directly (used for review links and
// headless screenshots): ?page=settings&dialog=new-session&tab=taskview&itab=tasks&select=ws:1
const params = new URLSearchParams(location.search);
const param = <T extends string>(name: string, fallback: T): T => (params.get(name) as T | null) ?? fallback;

export const [page, setPage] = createSignal<Page>(param<Page>("page", "sessions"));
export const [catalog, setCatalog] = createStore<{ rows: Session[] }>({ rows: seedSessions });
export const [selectedKey, setSelectedKey] = createSignal<string | undefined>(params.get("select") === "none" ? undefined : param("select", "ws:1"));
export const [detailTab, setDetailTab] = createSignal<DetailTab>(param<DetailTab>("tab", "activity"));
export const [inspectorTab, setInspectorTab] = createSignal<InspectorTab>(param<InspectorTab>("itab", "plan"));
export const [inspectorOpen, setInspectorOpen] = createSignal(params.get("inspector") !== "0");
export const [stateFilter, setStateFilter] = createSignal<StateFilter>(param<StateFilter>("filter", "all"));
export const [machineFilter, setMachineFilter] = createSignal<string[]>([]);
export const [projectFilter, setProjectFilter] = createSignal<string[]>([]);
export const [accountFilter, setAccountFilter] = createSignal<string[]>([]);
export const [multi, setMulti] = createStore<Record<string, boolean>>({});
export const [peek, setPeek] = createSignal(false);
export const [subagentOpen, setSubagentOpen] = createSignal<string | undefined>();
export const [search, setSearch] = createSignal(params.get("q") ?? "");
export const [collapsed, setCollapsed] = createStore<Record<string, boolean>>({});
export const [expandedChildren, setExpandedChildren] = createStore<Record<string, boolean>>({ "ws:1": true });
export const [compactRows, setCompactRows] = createStore<Record<string, boolean>>({ "ws:8": true });
export const [stripMode, setStripMode] = createSignal(params.get("strip") === "1");
export const [menu, setMenu] = createSignal<{ x: number; y: number; items: MenuEntry[] } | undefined>();
export const [dialog, setDialog] = createSignal<string | undefined>(params.get("dialog") ?? undefined);
export const [stress, setStress] = createSignal(false);
export const [demoState, setDemoState] = createSignal<"normal" | "empty" | "error" | "loading">(param("demo", "normal"));
export const [contentScale, setContentScale] = createSignal(1);
export const [toasts, setToasts] = createStore<{ items: { id: number; title: string; text: string }[] }>({ items: [] });
export const [selectedTask, setSelectedTask] = createSignal<string | undefined>(param("task", "T-4k2a9q"));

// Docked session list width (QSplitter default 310, minimum 270). Remembered per
// viewer like workspace/sessionsWidth; storage may be unavailable.
const savedWidth = (() => { try { return Number(localStorage.getItem("zerus-mock-sessions-width")) || 0; } catch { return 0; } })();
export const [sessionsWidth, setSessionsWidthRaw] = createSignal(savedWidth >= 270 ? savedWidth : 310);
export function setSessionsWidth(width: number) {
  setSessionsWidthRaw(width);
  try { localStorage.setItem("zerus-mock-sessions-width", String(width)); } catch { /* ignore */ }
}

/** Mockup-only choice between Zerus mark animations (review toolbar). */
export type LogoMotion = "qt" | "blades" | "ripple";
export const [logoMotion, setLogoMotion] = createSignal<LogoMotion>(param<LogoMotion>("logo", "blades"));

let toastId = 0;
export function toast(title: string, text: string) {
  const id = ++toastId;
  setToasts("items", (items) => [...items, { id, title, text }]);
  setTimeout(() => setToasts("items", (items) => items.filter((t) => t.id !== id)), 4200);
}

export function setStressMode(on: boolean) {
  batch(() => {
    setStress(on);
    setCatalog("rows", reconcile(on ? [...seedSessions, ...stressSessions(20000)] : seedSessions, { key: "key" }));
  });
}

export const selected = createMemo(() => catalog.rows.find((s) => s.key === selectedKey()) ?? archived.find((s) => s.key === selectedKey()));

/** Needs attention: actionable, unread reply or review later (muted rows are listed, not counted). */
export const needsAttention = (s: Session) => s.status === "attention" || (s.unread && s.status !== "offline") || !!s.reviewLater;

/** Active = current work OR Scheduled OR Needs attention OR a local draft. */
export const isActive = (s: Session) =>
  s.status === "working" || s.status === "background" || s.status === "scheduled" || s.status === "waiting" || needsAttention(s) || s.draft;

export function matchesFilter(s: Session, f: StateFilter): boolean {
  switch (f) {
    case "all":
      return true;
    case "attention":
      return needsAttention(s);
    case "active":
      return isActive(s);
    case "paused":
      return s.status === "paused";
    case "archived":
      return false;
  }
}

export const counts = createMemo(() => {
  const rows = catalog.rows;
  let active = 0, attention = 0, working = 0, paused = 0;
  for (const s of rows) {
    if (isActive(s)) active++;
    if (needsAttention(s) && !s.muted) attention++;
    if (s.status === "working" || s.status === "background") working++;
    if (s.status === "paused") paused++;
  }
  return { all: rows.length, active, attention, working, paused, archived: archived.length };
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
  starting: "Starting…",
  ended: "Archived",
};

// Simulated live updates: one row changes per tick, like a catalog delta.
let tick = 0;
setInterval(() => {
  tick++;
  const row = catalog.rows.findIndex((s) => s.key === "ws:6");
  if (row >= 0) {
    const tools = ["Edit: images/runner/Containerfile", "Bash: podman build -t runner:26.10 .", "Read: images/runner/README.md", "Bash: ./scripts/smoke-test.sh"];
    setCatalog("rows", row, { summary: tools[tick % tools.length], turnStartedMin: 2 + Math.floor(tick / 6) });
  }
}, 2500);
