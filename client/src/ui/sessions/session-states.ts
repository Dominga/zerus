import { tr } from "../../i18n";
import type { Session } from "./types";

// The session states of the card (tray/src/SessionStates.h, design
// Status/SESSION-STATES.md): the host decides the state (session_state,
// state_reason, last_active, cache_ttl, build); an older host sends none and its
// card keeps its plain presentation. `stateFacts` takes what a card stores,
// `presentState` turns it into the badge, line 3, tooltip and Cooling's heat at
// the moment of painting, so Cooling counts down and turns Cold by itself.
// Texts keep Qt's words; the UI never separates values with dots, so where Qt
// puts a middle dot between two parts of a text, a comma stands here.

export type StateKey = "" | "working" | "user" | "deps" | "build" | "cooling" | "cold" | "stopped" | "offline" | "idle";

export interface StateFacts {
  key: StateKey;
  reasonKind: string;
  reason: string;
  /** What the user state waits for: approval, input, error, you, reply or the host's reason kind. */
  user: string;
  buildLabel: string;
  buildCommand: string;
  buildSince: number;
  lastActive: number;
  ttl: number;
  keptWarm: boolean;
  exited: boolean;
  paused: boolean;
}

export interface Shown {
  key: StateKey;
  badge: string;
  line: string;
  /** hourglass, build, thermo, snow, stop, or empty. */
  icon: StateIcon | "";
  tooltip: string;
  reason: string;
  /** Cooling's remaining warmth, 0.04..1; -1 otherwise. */
  heat: number;
  coldActions: boolean;
}

export type StateIcon = "hourglass" | "build" | "thermo" | "snow" | "stop";

const EMPTY: Shown = { key: "", badge: "", line: "", icon: "", tooltip: "", reason: "", heat: -1, coldActions: false };

/** "41m", "2h 05m": the minutes the design shows (fmtMin). */
export function minutes(seconds: number): string {
  const total = Math.max(0, Math.trunc(seconds / 60));
  if (total < 60) return tr("%1m", total);
  return tr("%1h %2m", Math.trunc(total / 60), String(total % 60).padStart(2, "0"));
}

/** What a card needs to show its state; `online` is false for a machine that does not answer. */
export function stateFacts(s: Session, online: boolean, unreadReply: boolean, keepWarmSeconds = 0): StateFacts {
  let key = (s.sessionState ?? "") as StateKey;
  if (!online) key = "offline";
  if (key === "cooling" || key === "cold") key = "idle";
  const reasonKind = s.stateReason?.kind ?? "";
  let user = reasonKind;
  if (s.phase === "approval") user = "approval";
  else if (s.phase === "input") user = "input";
  else if (s.phase === "error") user = "error";
  else if (reasonKind.startsWith("team")) user = "you";
  else if (!user && unreadReply) user = "reply";
  // Idle time counts from the last native event (keepalive turns included).
  const last = (s.lastActive ?? 0) > 0 ? s.lastActive! : Math.max(s.updatedAt ?? 0, s.replyAt ?? 0);
  const ttl = s.cacheTtl?.seconds ?? 0;
  return {
    key,
    reasonKind,
    reason: s.stateReason?.text ?? "",
    user,
    buildLabel: s.build?.label ?? "",
    buildCommand: s.build?.command ?? "",
    buildSince: s.build?.since ?? 0,
    lastActive: last,
    ttl: keepWarmSeconds > 0 ? keepWarmSeconds : ttl > 0 ? ttl : 300,
    keptWarm: keepWarmSeconds > 0 || !!s.cacheTtl?.kept_warm,
    exited: s.processState === "exited",
    paused: s.state === "paused",
  };
}

export function stateTooltip(key: StateKey): string {
  switch (key) {
    case "working": return tr("Working — the model is writing code or running tools");
    case "user": return tr("Waiting for you — an answer, an approval or a reply to read");
    case "deps": return tr("Waiting for dependencies — subagents, a long operation, other members or the lead");
    case "build": return tr("Building — a build or long test started through Zerus runs; the reply waits for it");
    case "cooling": return tr("Cooling — done and waiting for a new task; the prompt cache is still warm");
    case "cold": return tr("Cold — done and idle; the prompt cache expired, a new session is cheaper");
    case "stopped": return tr("Stopped by you");
    case "offline": return tr("The machine does not answer");
    default: return "";
  }
}

export function presentState(f: StateFacts, now: number): Shown {
  let key = f.key;
  if (!key) return EMPTY;
  const ttl = Math.max(1, f.ttl);
  const idle = Math.max(0, now - f.lastActive);
  if (key === "idle") key = f.lastActive > 0 && idle < ttl ? "cooling" : "cold";
  const shown: Shown = { ...EMPTY, key, tooltip: stateTooltip(key) };
  const reason = f.reason;
  if (key === "user") {
    shown.badge = f.user === "approval" ? tr("Needs approval") : f.user === "input" ? tr("Needs input") : f.user === "you" ? tr("Needs you")
      : f.user === "reply" ? tr("New reply") : f.user === "error" ? tr("Error") : tr("Needs you");
    shown.line = !reason && f.user === "reply" ? tr("Response ready to read") : reason;
  } else if (key === "deps") {
    shown.badge = tr("Waiting");
    shown.icon = "hourglass";
    // The reason is always explicit.
    shown.line = reason || tr("Waiting for background work");
  } else if (key === "build") {
    shown.badge = tr("Building");
    shown.icon = "build";
    const label = f.buildLabel || f.buildCommand;
    shown.line = reason || (f.buildSince > 0 ? tr("Running %1, %2 so far", label, minutes(now - f.buildSince)) : tr("Running %1", label));
  } else if (key === "cooling") {
    const left = ttl - idle;
    shown.badge = tr("Cooling %1", minutes(left));
    shown.icon = "thermo";
    shown.heat = Math.min(1, Math.max(0.04, left / ttl));
    shown.line = tr("Done, waiting for a new task, cache warm for %1", minutes(left)) + (f.keptWarm ? tr(" (kept warm)") : "");
  } else if (key === "cold") {
    shown.badge = tr("Cold");
    shown.icon = "snow";
    shown.line = f.lastActive > 0 ? tr("Cache cold %1", minutes(idle - ttl)) : tr("Cache cold");
    shown.coldActions = true;
  } else if (key === "stopped") {
    shown.badge = f.exited ? tr("Agent exited") : tr("Stopped");
    shown.icon = "stop";
    shown.line = f.exited ? tr("The agent exited, resume to continue") : tr("Stopped by you, resume to continue");
  } else if (key === "working") {
    shown.badge = tr("Working");
  } else if (key === "offline") {
    shown.badge = tr("Offline");
  }
  shown.reason = shown.line || shown.tooltip;
  return shown;
}

/** The state glyphs (SessionStatusBadge::glyph) in a 12 px box, 10 px of it drawn. */
export const STATE_GLYPH: Record<StateIcon, string> = {
  hourglass: `<path d="M2.5 1H9.5M2.5 11H9.5M3.25 1L8.75 11H3.25L8.75 1"/>`,
  thermo: `<path d="M6 1V7.75"/><circle cx="6" cy="8.75" r="1.9" fill="currentColor"/>`,
  snow: `<path d="M1 6H11M3.5 1.67L8.5 10.33M8.5 1.67L3.5 10.33"/>`,
  stop: `<rect x="2.75" y="2.75" width="6.5" height="6.5" rx="1" fill="currentColor"/>`,
  build: `<path d="M2 3.25L5.25 6L2 8.75M6.5 9H10.25"/>`,
};
