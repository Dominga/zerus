import type { Followed } from "../../store/actions";
import type { Usage } from "../../types";
import { clock, dayTime, limitRecoveryNotice } from "../lib/usage";
import type { Session } from "./types";

// RecoveryUi::Panel and SessionsWindow's provider-error wiring, without a DOM.
export type Recovery = Record<string, unknown>;
const object = (value: unknown): Recovery => value && typeof value === "object" && !Array.isArray(value) ? value as Recovery : {};
const text = (value: unknown) => typeof value === "string" ? value : "";
const number = (value: unknown) => typeof value === "number" && Number.isFinite(value) ? value : 0;
const flag = (value: unknown) => value === true;
const list = (value: unknown) => Array.isArray(value) ? value : [];

export const resumed = (job: Recovery) => text(job.state) === "retrying" && flag(job.acknowledged) && number(job.resumed_at) > 0;

export function recoveryStatus(job: Recovery, now: number) {
  const state = text(job.state);
  if (state === "waiting") {
    if (job.class === "session_limit" && job.session_limit_mode === "reset") return `Waiting for reset at ${dayTime(number(job.due_at))}`;
    const remaining = Math.max(0, Math.ceil(number(job.due_at) - now));
    return remaining > 0 ? `Retry in ${remaining} s` : "Waiting to retry";
  }
  if (state === "dispatching") return "Sending recovery request…";
  if (state === "retrying") return !flag(job.acknowledged) ? "Waiting for delivery confirmation…" : resumed(job) ? "Agent resumed after recovery" : "Recovery in progress";
  if (state === "exhausted") return "Automatic attempts exhausted";
  if (state === "uncertain") return "Check the agent before retrying";
  if (state === "blocked") return "Automatic recovery needs attention";
  if (state === "cancelled") return "Automatic recovery cancelled";
  if (state === "succeeded") return "Recovered";
  return "Automatic recovery is not scheduled";
}

export function providerFailure(error: Recovery) {
  switch (text(error.error_kind)) {
    case "session_limit": return "Session limit reached";
    case "quota": return "Usage limit reached";
    case "rate_limit": return "Rate limit reached";
    case "capacity": return "Model at capacity";
    case "authentication": return "Sign-in failed";
    case "provider_policy": return "Request blocked by provider";
    case "context_limit": return "Context limit reached";
    case "model_unavailable": return "Model unavailable";
    default: return "Provider error";
  }
}

export interface RecoveryAction { visible: boolean; enabled: boolean }
export interface RecoveryAttempt { at: string; label: string; attempts: number; reason: string }
export interface RecoveryView {
  active: boolean;
  failure: boolean;
  label: string;
  shortLabel: string;
  tone: "warning" | "danger";
  status: string;
  detail: string;
  detailFull: string;
  help: string;
  now: RecoveryAction;
  cancel: RecoveryAction;
  history: RecoveryAction;
  terminal: RecoveryAction;
  usage: RecoveryAction;
  settingsVisible: boolean;
  attempts: RecoveryAttempt[];
  jobId: string;
  name: string;
}

const history = (job: Recovery, now: number): RecoveryAttempt[] => list(job.history).map((raw) => {
  const entry = object(raw);
  const seconds = number(entry.at);
  const at = new Date(seconds * 1000);
  const two = (n: number) => String(n).padStart(2, "0");
  const stamp = `${at.getDate()} ${dayTime(seconds).split(" ").slice(1, 3).join(" ")}:${two(at.getSeconds())}`;
  return {
    at: stamp,
    label: text(entry.state) === "waiting" ? "Retry scheduled" : recoveryStatus(entry, now),
    attempts: number(entry.attempt),
    reason: text(entry.reason),
  };
});

export function recoveryView(details: Recovery, online: boolean, now: number, pending: boolean, actionError: string, usageNotice: string): RecoveryView {
  const job = object(details.recovery);
  const failureObject = details.phase === "error" ? object(details.provider_error) : {};
  const state = text(job.state);
  const failure = Object.keys(failureObject).length > 0 && (Object.keys(job).length === 0 || state === "succeeded" || resumed(job)
    || failureObject.error_kind === "quota" || (failureObject.error_kind === "session_limit" && job.class !== "session_limit"));
  const active = (Object.keys(job).length > 0 && state !== "succeeded" && !resumed(job)) || Object.keys(failureObject).length > 0;
  const sessionLimit = failureObject.error_kind === "session_limit";
  const waiting = state === "waiting";
  const reset = job.class === "session_limit" && job.session_limit_mode === "reset";
  let label = "Automatic recovery is not scheduled", shortLabel = "Error", tone: "warning" | "danger" = "danger";
  if (failure) { label = "Provider error"; shortLabel = "Error"; }
  else if (waiting || state === "dispatching" || state === "retrying") {
    label = recoveryStatus(job, now);
    tone = "warning";
    if (waiting && reset) shortLabel = `Reset ${clock(number(job.due_at))}`;
    else {
      const remaining = Math.max(0, Math.ceil(number(job.due_at) - now));
      shortLabel = waiting && remaining > 0 ? `${remaining} s` : "…";
    }
  } else if (state === "cancelled") { label = recoveryStatus(job, now); shortLabel = "Off"; tone = "warning"; }
  else label = recoveryStatus(job, now);

  let status: string, detail: string, detailFull: string, help = "";
  if (failure) {
    status = providerFailure(failureObject);
    detailFull = text(failureObject.detail);
    detail = detailFull.length > 500 ? `${detailFull.slice(0, 500)}…` : detailFull;
    help = sessionLimit
      ? "The session usage limit stopped this turn. Automatic recovery can wait for reset or retry on a schedule when enabled in Settings. Your draft is kept."
      : failureObject.error_kind === "quota"
      ? "The provider stopped this turn. Wait for the limit to reset or restore account access, then refresh usage and send a message to continue. Your draft is kept; Zerus will not retry automatically."
      : "The provider stopped this turn. Check Terminal and resolve the error, then send a message to continue. Your draft is kept.";
    if (usageNotice && (sessionLimit || failureObject.error_kind === "quota")) help = `${usageNotice}\n${help}`;
  } else {
    const delays = list(job.delays);
    const infinite = delays.length > 0 && number(delays[delays.length - 1]) === 0;
    const count = delays.length - (delays.length > 0 && number(delays[delays.length - 1]) <= 0 ? 1 : 0);
    const attempt = typeof job.class_attempt === "number" && Number.isFinite(job.class_attempt) ? number(job.class_attempt) : number(job.attempt);
    const suffix = reset ? ` (${number(job.attempt)} attempts)` : infinite ? ` (${number(job.attempt)} attempts, repeats until stopped)` : ` (${attempt}/${count} attempts)`;
    status = `${recoveryStatus(job, now)}${suffix}`;
    detail = actionError || text(job.reason) || (job.action === "retry_request" ? "Retry the failed provider request" : "Send a continuation message in this conversation");
    detailFull = detail;
  }
  const attempts = history(job, now);
  return {
    active, failure, label, shortLabel, tone, status, detail, detailFull, help,
    now: { visible: !failure && waiting, enabled: !failure && waiting && online && !pending && number(job.not_before) <= now },
    cancel: { visible: waiting, enabled: waiting && online && !pending },
    history: { visible: !failure, enabled: !failure && attempts.length > 0 },
    terminal: { visible: failure, enabled: failure && online },
    usage: { visible: failure && (failureObject.error_kind === "quota" || sessionLimit), enabled: failure && online },
    settingsVisible: !failure || sessionLimit,
    attempts, jobId: text(job.id), name: text(job.name),
  };
}

/** Only the current live row's followed inspection may show recovery. */
export function recoveryInspection(s: Session, followed: Followed): Recovery {
  if (s.state === "archived") return {};
  const details = followed.value ?? {};
  return text(details.run_id) === (s.runId ?? "") && text(details.conversation_id) === (s.conversationId ?? "") ? details : {};
}

/** A fresh usage snapshot can explain an early Claude reset, not clear the error. */
export function recoveryUsageNotice(s: Session, details: Recovery, usage: Usage, online: boolean, now: number) {
  if (!online || s.cmd !== "claude") return "";
  const failure = object(details.provider_error);
  if (details.phase !== "error" && !text(details.attention_id).startsWith("claude-usage-limit:")) return "";
  return failure.error_kind === "quota" || failure.error_kind === "session_limit" ? limitRecoveryNotice(usage, number(failure.at), now) : "";
}
