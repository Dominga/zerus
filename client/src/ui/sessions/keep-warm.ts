/**
 * Keep cache warm (Details > Session): what the host reported, merged with a
 * change this window confirmed, and the words the row shows. The host decides
 * eligibility and timing (docs/reference.md, Keep cache warm); this only
 * presents it. Values are never separated with dots.
 */
import { tr, trNoop, uiLanguage } from "../../i18n";
import type { KeepWarmChoice } from "../../bridge";
import type { KeepWarmState } from "../../types";

/** The slider's last position: Always. */
export const ALWAYS = 8;

// The host's English reasons, translated by the catalog where they are known.
trNoop("The agent is working.");
trNoop("The agent is not running.");
trNoop("Background work is running.");
trNoop("The agent has scheduled jobs.");
trNoop("Automatic recovery is active.");
trNoop("Someone is typing in Terminal.");
trNoop("The session is no longer available.");
trNoop("Keep cache warm ended at the chosen time.");
trNoop("The cache went cold. Keeping it warm resumes after your next message.");
trNoop("The last keep-warm ping failed. Keeping it warm resumes after your next message.");
trNoop("No reply to the last keep-warm ping was seen. Keeping it warm resumes after your next message.");
trNoop("A keep-warm ping may have been typed when its worker stopped. Keeping it warm resumes after your next message.");
trNoop("Waiting for Claude to report its cache lifetime with its next reply.");
trNoop("Waiting for Codex to report its model.");
trNoop("Waiting for Kimi to report its model.");
trNoop("Claude uses a 5-minute cache on this account. Keeping it warm would cost about as much as rebuilding it.");
trNoop("OpenAI keeps a 30-minute cache only for GPT-5.6 and later. This model's cache lasts about 5 to 10 minutes.");
trNoop("Kimi reports no cache lifetime of at least 15 minutes for this model.");
trNoop("DeepSeek keeps its context cache for hours without pings.");
trNoop("This agent reports no prompt cache that pings can keep warm.");
trNoop("Terminal input activity is unknown.");
trNoop("The keep-warm state of this session is unreadable. Nothing is sent until your next message.");
trNoop("Keep cache warm was turned off; nothing was submitted");
trNoop("Keep cache warm has expired; nothing was submitted");
trNoop("This keep-warm ping is no longer current; nothing was submitted");
trNoop("The agent is not idle; nothing was submitted");
trNoop("Background work is running or unknown; nothing was submitted");
trNoop("The agent has scheduled jobs; nothing was submitted");
trNoop("Automatic recovery owns the next message; nothing was submitted");
trNoop("Terminal input activity is unknown; nothing was submitted");
trNoop("Someone typed in Terminal; nothing was submitted");
trNoop("The cache may already have expired; nothing was submitted");
trNoop("Keep cache warm settings are unreadable; nothing was submitted");
trNoop("model or effort settings are pending; no keep-warm ping was typed");
// The core's refusals of a change (client/core/src/keep_warm.rs).
trNoop("Keep cache warm changed on another device. Refresh the session and try again.");
trNoop("At most 64 sessions on one computer can keep their cache warm.");
trNoop("The session changed. Refresh it and try again.");
trNoop("The computer did not confirm the keep-warm change. Refresh the session.");
trNoop("The session has no confirmed conversation yet.");
trNoop("Choose 1 to 7 hours or Always.");

const finite = (value: unknown): value is number => typeof value === "number" && Number.isFinite(value);
const optionalNumber = (value: unknown) => (finite(value) ? value : null);
const string = (value: unknown) => (typeof value === "string" ? value : "");

/** A host `keep_warm` object, or undefined when it is not one. */
export function keepWarmOf(value: unknown): KeepWarmState | undefined {
  if (!value || typeof value !== "object") return undefined;
  const v = value as Record<string, unknown>;
  if (typeof v.enabled !== "boolean" || !finite(v.revision) || v.revision < 0 || !Number.isInteger(v.revision)) return undefined;
  const state: KeepWarmState = {
    enabled: v.enabled, until: optionalNumber(v.until), revision: v.revision, state: string(v.state) || (v.enabled ? "waiting" : "off"),
    next_at: optionalNumber(v.next_at), reason: string(v.reason), turn_active: v.turn_active === true,
  };
  if (typeof v.supported === "boolean") {
    state.supported = v.supported;
    state.reason_code = string(v.reason_code);
    state.ttl_seconds = optionalNumber(v.ttl_seconds);
    state.ttl_source = string(v.ttl_source);
    state.interval_seconds = optionalNumber(v.interval_seconds);
    state.cache_expires_at = optionalNumber(v.cache_expires_at);
  }
  return state;
}

/**
 * The newest state by revision; on a tie the listing (the freshest stream)
 * wins, then the Details read, then this window's confirmed change. The
 * Details-only fields come from the newest source that has them.
 */
export function mergeKeepWarm(listing?: KeepWarmState, inspected?: KeepWarmState, confirmed?: KeepWarmState): KeepWarmState | undefined {
  const sources = [listing, inspected, confirmed].filter((s): s is KeepWarmState => !!s);
  if (!sources.length) return undefined;
  const newest = (list: KeepWarmState[]) => list.reduce((best, s) => (s.revision > best.revision ? s : best));
  const best = newest(sources);
  const detailed = sources.filter((s) => s.supported !== undefined);
  if (!detailed.length) return { ...best };
  const details = newest(detailed);
  return {
    ...best, supported: details.supported, reason_code: details.reason_code, ttl_seconds: details.ttl_seconds,
    ttl_source: details.ttl_source, interval_seconds: details.interval_seconds, cache_expires_at: details.cache_expires_at,
    // Why the agent cannot keep its cache warm is the Details read's, not the worker's.
    reason: details.supported === false ? details.reason : best.reason,
  };
}

/**
 * How far past a whole hour the time left may be and still show that hour
 * (zerus-ua4bi): the host sets `until` from its own clock, and the row reads
 * its clock every 30 s, so a fresh 1 h choice can be a little over an hour away.
 */
export const POSITION_SLACK_SECONDS = 5 * 60;

/** The slider position: the hours left (rounded up past the slack, 1 to 7), or Always. */
export function positionOf(state: KeepWarmState | undefined, now: number): number {
  if (!state?.enabled) return 2;
  if (state.until === null) return ALWAYS;
  return Math.min(7, Math.max(1, Math.ceil((state.until - now - POSITION_SLACK_SECONDS) / 3600)));
}

/** What a slider position asks the host for: exactly its accepted 1 to 7 hours, or Always. */
export const choiceAt = (position: number): KeepWarmChoice => (position >= ALWAYS ? { kind: "always" } : { kind: "hours", hours: Math.min(7, Math.max(1, Math.round(position))) });

/** A position's name: "3 h" or "Always" (the slider's value text and the mark's tooltip). */
export const durationLabel = (position: number) => (position >= ALWAYS ? tr("Always") : tr("%1 h", position));

/** A position as the row shows it: "3 h", or an infinity sign for Always. */
export const durationMark = (position: number) => (position >= ALWAYS ? "\u221e" : tr("%1 h", position));

/** "1 h", "48 m", "1 h 30 m". */
export function durationText(seconds: number): string {
  const minutes = Math.max(1, Math.round(seconds / 60));
  const hours = Math.floor(minutes / 60), rest = minutes % 60;
  if (!hours) return tr("%1 m", rest);
  return rest ? tr("%1 h %2 m", hours, rest) : tr("%1 h", hours);
}

export function remainingText(state: KeepWarmState | undefined, now: number): string {
  if (!state?.enabled) return "";
  if (state.until === null) return tr("Always");
  const left = state.until - now;
  if (left < 60) return tr("Less than a minute left");
  const hours = Math.floor(left / 3600), minutes = Math.floor((left % 3600) / 60);
  if (!hours) return tr("%1 m left", minutes);
  return minutes ? tr("%1 h %2 m left", hours, minutes) : tr("%1 h left", hours);
}

/** The time left as the header shows it: "2 h 14 m", "45 m", "<1 m", or an infinity sign for Always. */
export function remainingMark(state: KeepWarmState, now: number): string {
  if (state.until === null) return durationMark(ALWAYS);
  const left = state.until - now;
  if (left < 60) return tr("<1 m");
  const hours = Math.floor(left / 3600), minutes = Math.floor((left % 3600) / 60);
  if (!hours) return tr("%1 m", minutes);
  return minutes ? tr("%1 h %2 m", hours, minutes) : tr("%1 h", hours);
}

/** The one line under the row: why it cannot be on, what it is doing, or when it pings next. */
export function statusLine(state: KeepWarmState | undefined, now: number): string {
  if (!state) return "";
  if (state.supported === false) return state.reason ? tr(state.reason) : tr("This agent cannot keep its cache warm.");
  if (!state.enabled) return "";
  if (state.turn_active) return tr("Warming the cache now");
  if (state.reason) return tr(state.reason);
  if (state.state === "warming") return tr("Warming the cache now");
  if (state.state === "waiting" && state.next_at !== null && state.next_at > now) return tr("Next ping in %1", durationText(state.next_at - now));
  return "";
}

const tokens = (count: number) => (count >= 1_000_000 ? `${number(count / 1_000_000)}M` : `${Math.round(count / 1000)}k`);
const number = (value: number) => new Intl.NumberFormat(uiLanguage() === "ru" ? "ru-RU" : "en-US", { maximumFractionDigits: 1 }).format(value);

/** The tooltip: the lifetime and its source, the interval and pings per hour, and the cost of each ping. */
export function tooltipLines(state: KeepWarmState | undefined, contextTokens: number): string[] {
  const lines: string[] = [];
  const ttl = state?.ttl_seconds, interval = state?.interval_seconds;
  if (finite(ttl) && ttl > 0) {
    const life = durationText(ttl);
    lines.push(state?.ttl_source === "native_usage" ? tr("The cache lasts about %1, as the agent reported.", life)
      : state?.ttl_source === "provider_default" ? tr("The cache lasts about %1, the provider's documented default.", life)
      : tr("The cache lasts about %1, as Kimi Code estimates it.", life));
  }
  if (finite(interval) && interval > 0) lines.push(tr("Pings every %1 while idle, about %2 an hour.", durationText(interval), number(3600 / interval)));
  lines.push(contextTokens >= 1000
    ? tr("Each ping reads the whole cached context (about %1 tokens) from this account's usage.", tokens(contextTokens))
    : tr("Each ping reads the whole cached context from this account's usage."));
  return lines;
}

/**
 * The session header's keep-warm mark (zerus-yacns): shown only while it is
 * on, as the time left or an infinity sign; its name and a tooltip with the
 * whole state the Details row shows. Undefined while it is off or unreported.
 */
export function headerKeepWarm(state: KeepWarmState | undefined, now: number, contextTokens: number): { mark: string; label: string; tooltip: string } | undefined {
  if (!state?.enabled) return undefined;
  const label = tr("Keep cache warm: %1", remainingText(state, now));
  const line = statusLine(state, now);
  return { mark: remainingMark(state, now), label, tooltip: [label, ...(line ? [line] : []), ...tooltipLines(state, contextTokens), tr("Click to change it in Details.")].join("\n") };
}
