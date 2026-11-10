import type { RetryReport } from "../types";

// A connection supervisor's restart schedule on this view's monotonic clock.
// Pure: the catalog (this app's own supervisors) and Machines (the service's)
// anchor reports with it.

/** A reported restart schedule anchored on `performance.now()`. */
export interface RetryState {
  phase: "idle" | "waiting" | "attempting";
  /** The supervisor the schedule belongs to: its service incarnation, or "" for this app's own. */
  source: string;
  serial: number;
  attempt: number;
  delayMs?: number;
  /** `performance.now()` when the attempt is due. */
  dueAt?: number;
}

const PHASES = new Set(["idle", "waiting", "attempting"]);

/**
 * Anchors a reported schedule. A wait is anchored when its serial is first
 * seen; a later report of the same wait (a heartbeat, a reloaded view) may
 * only bring it earlier, so it never restarts or extends, and wall-clock
 * changes cannot move it.
 */
export function anchorRetry(previous: RetryState | undefined, report: RetryReport | null | undefined, now: number, source = ""): RetryState | undefined {
  if (!report || !PHASES.has(report.phase)) return undefined;
  const phase = report.phase as RetryState["phase"];
  const serial = report.serial ?? 0;
  const attempt = report.attempt ?? 0;
  if (phase !== "waiting") return { phase, source, serial, attempt };
  const remaining = Math.max(0, report.remaining_ms ?? 0);
  const dueAt = now + remaining;
  const same = previous?.phase === "waiting" && previous.serial === serial && previous.source === source && previous.dueAt !== undefined;
  return { phase, source, serial, attempt, delayMs: report.delay_ms ?? remaining, dueAt: same ? Math.min(previous!.dueAt!, dueAt) : dueAt };
}

/** Whole seconds left until a waiting attempt at `now`; 0 once it is due. */
export function secondsLeft(retry: RetryState | undefined, now: number): number {
  if (retry?.phase !== "waiting" || retry.dueAt === undefined) return 0;
  return Math.max(0, Math.ceil((retry.dueAt - now) / 1000));
}
