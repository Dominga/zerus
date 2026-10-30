import type { Account, AccountsView, MachineMeta } from "../../types";
import type { Connection } from "../../store/overview";
import { dayTime, severest, windowsAt, type Tone } from "../lib/usage";

// Pure texts and levels of the Overview cards (DashboardPage.cpp:29-138,
// 255-279, 341-360), with the owner's round 7-8 choices: percentages are the
// main figures and the sample time shows only when it is not current.

export type Level = "unknown" | "normal" | "warning" | "critical";

/** updateLoadBar (DashboardPage.cpp:60-71): unknown without a live stream or a sample. */
export function level(percent: number | undefined, fresh: boolean): Level {
  if (!fresh || percent === undefined || !Number.isFinite(percent) || percent < 0) return "unknown";
  return percent >= 90 ? "critical" : percent >= 70 ? "warning" : "normal";
}

export function loadTip(percent: number | undefined, fresh: boolean) {
  return percent === undefined ? "No sample available" : !fresh ? "Last known value; waiting for a fresh sample." : "Orange from 70%; red from 90%.";
}

const GIB = 1024 ** 3;
const MIB = 1024 ** 2;
/** DashboardPage.cpp:29-34. */
export const bytes = (value: number) => (value >= GIB ? `${(value / GIB).toFixed(1)} GiB` : `${(value / MIB).toFixed(0)} MiB`);

export interface Figure { value: string; percent?: number; detail: string; tip: string }

/** No metrics at all: an older hgs, or no card. */
const noMetrics = (meta?: MachineMeta) =>
  !meta || (meta.metrics.sampled_at === null && meta.metrics.cpu_state === null && meta.metrics.cpu_percent === null);

export function cpuFigure(meta: MachineMeta | undefined): Figure {
  const m = meta?.metrics;
  if (m && m.cpu_percent !== null) {
    return {
      value: `${Math.round(m.cpu_percent)}%`,
      percent: m.cpu_percent,
      detail: `${m.cpu_count ?? 0} cores, load ${(m.load_1 ?? 0).toFixed(2)}`,
      tip: `Average CPU use over the last ${(m.cpu_interval_seconds ?? 0).toFixed(1)} seconds, across all logical cores.`,
    };
  }
  const detail = noMetrics(meta) ? "Update hgs for metrics" : m?.cpu_state === "unavailable" ? "CPU metrics unavailable" : "Waiting for second sample";
  return { value: "—", detail, tip: "CPU use needs two samples. Refresh again after a moment." };
}

export function memoryFigure(meta: MachineMeta | undefined): Figure {
  const m = meta?.metrics;
  const total = m?.memory_total_bytes ?? 0;
  const used = m?.memory_used_bytes ?? 0;
  const tip = meta?.platform === "macos"
    ? "App, wired and compressed memory. Reclaimable file cache is excluded."
    : "Total memory minus available memory, including reclaimable cache.";
  if (total <= 0) return { value: "—", detail: "Memory unavailable", tip };
  const percent = (100 * used) / total;
  return { value: `${Math.round(percent)}%`, percent, detail: `${bytes(used)} of ${bytes(total)}`, tip };
}

/** The sample time when it is not current; a live stream with a sample under 60 s old is current. */
export function sampleNote(meta: MachineMeta | undefined, c: Connection, now: number): string {
  const at = meta?.metrics.sampled_at ?? null;
  if (c.fresh && at !== null && now - at < 60) return "";
  return at !== null && at > 0 ? `Last sample ${dayTime(at)}` : "No sample yet";
}

export const sampleTip = (c: Connection) =>
  c.fresh ? "Last known value; waiting for a fresh sample." : "Last known values. This machine is unavailable or has not replied recently.";

export const sessionsLabel = (sessions: number, attention: number) =>
  `${sessions === 1 ? "1 session" : `${sessions} sessions`}${attention ? `, ${attention} need attention` : ""}`;

/** The card's identity line (DashboardPage.cpp:263-265). */
export function accountIdentity(a: Account): string {
  const identity = a.usage.identity;
  let text = identity.email || identity.name;
  if (identity.plan) text += `${text ? " / " : ""}${identity.plan}`;
  return text || (a.installed ? "" : "Agent not installed");
}

/** The stripe: the pace tone of the current windows (AccountUsage::severest), grey without one. */
export function accountTone(a: Account, now: number): Tone | "none" {
  return severest(windowsAt(a.usage.windows, now)) ?? "none";
}

/** The Accounts section's text when it shows no card (DashboardPage.cpp:314, 360). */
export function accountsNote(view: AccountsView): string | undefined {
  if (!view.snapshot && !view.accounts.length) return "Loading accounts…";
  if (view.overview.length) return undefined;
  return view.loading ? "Loading account details…" : "Open Manage accounts to view or set up accounts.";
}
