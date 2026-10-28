import type { Account, AccountsView, MemberState, Profile, ProjectedWindow } from "../../types";
import { current, dayTime, forecast, fromProjected, pace, period, thresholdTone, type Tone } from "../lib/usage";

// Pure texts and selection rules of the Accounts screen (AccountsPage.cpp).

/** The status line's tone (the mockup's statusText tones). */
export const tone = (s: MemberState): "ok" | "busy" | "muted" | "warn" =>
  s === "ready" || s === "apikey" ? "ok" : s === "checking" ? "busy" : s === "offline" ? "muted" : "warn";

/** A member's state as the "+N more" list shows it; none for a ready member. */
export function memberStateText(s: MemberState): string {
  switch (s) {
    case "ready": return "";
    case "offline": return "Offline";
    case "not-installed": return "Not installed";
    case "apikey": return "API key";
    case "signed-out": return "Sign-in needed";
    case "keychain": return "Keychain locked";
    case "desktop": return "Desktop sign-in unavailable";
    case "unavailable": return "Sign-in unusable";
    case "expired": return "Sign-in expired";
    case "checking": return "Checking…";
    default: return "Unknown";
  }
}

/** The sign-in button (AccountsPage.cpp:639-641); none for OpenCode or an agent that is not installed. */
export function loginLabel(p: Profile): string | undefined {
  if (p.provider === "opencode" || !p.installed) return undefined;
  if (p.provider === "dsh") return p.usage.status === "configured" ? `Change API key on ${p.machine}…` : `Set API key on ${p.machine}…`;
  return p.signed_in ? `Sign in again on ${p.machine}…` : `Sign in on ${p.machine}…`;
}

/** The member the detail shows (AccountsPage::selected): the chosen profile, else the filtered machine's, else the first. */
export function memberOf(a: Account, key: string | undefined, machine: string): Profile | undefined {
  return a.members.find((m) => m.key === key) ?? a.members.find((m) => m.host === machine) ?? a.members[0];
}

/** At most `max` machine chips, keeping the selected one (spec round 6); the rest show as "+N more". */
export function visibleMembers(a: Account, selectedKey: string | undefined, max = 4): Profile[] {
  const all = a.members;
  if (all.length <= max) return all;
  const first = all.slice(0, max);
  const chosen = all.find((m) => m.key === selectedKey);
  return chosen && !first.includes(chosen) ? [...first.slice(0, max - 1), chosen] : first;
}

/** The accounts with a member on `machine` ("*" is every machine). */
export const visibleAccounts = (view: AccountsView, machine: string): Account[] =>
  machine === "*" ? view.accounts : view.accounts.filter((a) => a.members.some((m) => m.host === machine));

/** updateRefreshState (AccountsPage.cpp:440-447) once the first snapshot is in and hosts exist; `visible` counts the listed accounts. */
export function summaryLine(view: AccountsView, visible: number): string {
  const accounts = visible === 1 ? "1 account" : `${visible} accounts`;
  const machines = view.loaded_machines === 1 ? "1 machine loaded" : `${view.loaded_machines} machines loaded`;
  return `${accounts}, ${machines}${view.pending ? ` (Updating… ${view.pending} remaining)` : ""}`;
}

export interface PanelWindow {
  period: string;
  usedText: string;
  /** The bar's share, 0 once the window ended. */
  used: number;
  /** Qt's threshold colour (AccountUsage::color). */
  tone: Tone;
  /** The elapsed share in percent, when the length and reset are known. */
  elapsed?: number;
  resets?: string;
  forecast: string;
}

/** One window of the usage panel (AccountUsage::Panel::setData, AccountUsage.h:368-384, plus the forecast). */
export function panelWindow(p: ProjectedWindow, now: number): PanelWindow {
  const w = fromProjected(p, now);
  const fresh = current(w);
  const known = pace(w);
  return {
    period: period(w),
    usedText: fresh ? `${Math.round(w.used)}% used` : "Window ended; refresh for current usage",
    used: fresh ? Math.max(0, Math.min(100, w.used)) : 0,
    tone: thresholdTone(w.used),
    elapsed: known.known ? known.elapsed * 100 : undefined,
    resets: p.resets_at !== null ? `Resets ${dayTime(p.resets_at)}` : undefined,
    forecast: forecast(w),
  };
}

/** AccountUsage::credits and the panel's credits line. */
export function creditsText(credits: { balance: string; unlimited: boolean } | null): string | undefined {
  if (!credits) return undefined;
  if (credits.unlimited) return "Credits: unlimited";
  const n = Number(credits.balance);
  const text = credits.balance.trim() !== "" && Number.isFinite(n)
    ? n.toLocaleString(undefined, { minimumFractionDigits: 2, maximumFractionDigits: 2 })
    : "—";
  return `Credits available: ${text}`;
}
