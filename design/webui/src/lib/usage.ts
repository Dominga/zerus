import type { Account } from "../mock/types";

// Account limit windows and their pace, ported from tray/src/AccountUsage.h.
// Pace compares used quota with the share of the window that has passed:
// spending faster than that share exhausts the limit before it resets.

export interface UsageWindow {
  /** Window length in minutes (300 = 5 h, 10080 = 7 d). */
  minutes: number;
  /** Used share, 0..100. */
  used: number;
  /** Minutes until reset; not positive when the window already ended. */
  resetsIn: number;
}

export type Tone = "normal" | "warning" | "critical";

export interface Pace {
  /** The window length and a future reset are known. */
  known: boolean;
  /** The first 8% of a window is too short to extrapolate. */
  early: boolean;
  used: number;
  /** Share of the window that has passed, 0..1. */
  elapsed: number;
  minutesLeft: number;
  /** Minutes until 100% at the average pace; -1 when it lasts until reset. */
  runsOutIn: number;
  /** Percent at reset at the average pace; -1 when unknown. */
  projected: number;
  tone: Tone;
}

const thresholdTone = (used: number): Tone => (used >= 90 ? "critical" : used >= 70 ? "warning" : "normal");

export const current = (w: UsageWindow) => w.resetsIn > 0;

export function pace(w: UsageWindow): Pace {
  const used = Math.max(0, Math.min(100, w.used));
  const result: Pace = { known: false, early: false, used, elapsed: 0, minutesLeft: -1, runsOutIn: -1, projected: -1, tone: thresholdTone(used) };
  if (w.minutes <= 0 || w.resetsIn <= 0) return result;
  result.known = true;
  result.minutesLeft = w.resetsIn;
  const passed = Math.max(0, w.minutes - w.resetsIn);
  result.elapsed = passed / w.minutes;
  result.early = result.elapsed < 0.08;
  // Exhausted and fresh windows keep the fixed 70/90% thresholds.
  if (used >= 100 || result.early) return result;
  result.projected = used / result.elapsed;
  if (result.projected > 100) result.runsOutIn = ((100 - used) * passed) / used;
  result.tone = result.runsOutIn < 0 ? "normal" : result.runsOutIn >= result.minutesLeft / 2 ? "warning" : "critical";
  if (used >= 90 && result.tone === "normal") result.tone = "warning";
  return result;
}

export const period = (w: UsageWindow) =>
  w.minutes === 10080 ? "7d" : w.minutes % 1440 === 0 ? `${w.minutes / 1440}d` : w.minutes % 60 === 0 ? `${w.minutes / 60}h` : `${w.minutes}m`;

export function remaining(minutes: number) {
  const m = Math.max(0, Math.ceil(minutes));
  if (!m) return "refresh needed";
  if (m >= 1440) return `${Math.floor(m / 1440)}d ${Math.floor((m % 1440) / 60)}h`;
  if (m >= 60) return `${Math.floor(m / 60)}h ${m % 60}m`;
  return `${m}m`;
}

/** Footer countdown: only the largest whole unit fits next to the percentage. */
export function countdown(minutes: number) {
  const m = Math.max(1, Math.ceil(minutes));
  return m < 60 ? `${m}m` : m < 2880 ? `${Math.floor(m / 60)}h` : `${Math.floor(m / 1440)}d`;
}

export function windowText(w: UsageWindow, withReset = true) {
  if (!current(w)) return `${period(w)} refresh`;
  return `${period(w)} ${Math.round(w.used)}%${withReset && w.used >= 100 ? ` (${remaining(w.resetsIn)})` : ""}`;
}

export function approximately(minutes: number) {
  const m = Math.max(1, Math.floor(minutes)), d = Math.floor(m / 1440), h = Math.floor((m % 1440) / 60), r = m % 60;
  if (d) return h ? `${d}d ${h}h` : `${d}d`;
  if (h) return r ? `${h}h ${r}m` : `${h}h`;
  return `${m}m`;
}

export function forecast(w: UsageWindow) {
  if (!current(w)) return "";
  const p = pace(w);
  if (!p.known || p.early || p.used >= 100) return "";
  return p.runsOutIn >= 0 ? `At the current pace, the limit runs out in about ${approximately(p.runsOutIn)}`
    : `At the current pace, about ${Math.round(p.projected)}% by reset`;
}

/** The synthetic windows of a mock account (5 h and 7 d), or none when the provider reports no limits. */
export function accountWindows(a: Account | undefined): UsageWindow[] {
  if (!a || (!a.fiveHour && !a.week)) return [];
  const week = { minutes: 10080, used: a.week, resetsIn: a.weekResetMin ?? 5880 };
  return a.weekOnly ? [week] : [{ minutes: 300, used: a.fiveHour, resetsIn: a.fiveHourResetMin ?? 130 }, week];
}

/** AccountUsage::tooltip: provider, account, each window with its reset and forecast. */
export function usageTooltip(provider: string, label: string, windows: UsageWindow[]) {
  const lines = [provider, label];
  for (const w of windows) {
    lines.push(`${period(w)}: ${Math.round(w.used)}% used`);
    if (current(w)) lines.push(`Resets in ${remaining(w.resetsIn)}`);
    const f = forecast(w);
    if (f) lines.push(f);
  }
  lines.push("Updated 11:58");
  return lines.join("\n");
}
