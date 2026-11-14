/** Saved replica evidence, never live runtime, a queue position or authority.
 * Only Core's bounded projection is accepted; old/future/unsafe values stay
 * unavailable. Copy scalars explicitly so raw native input/history is not kept.
 */
export type ExecutionPhase = "submitted" | "unknown" | "running" | "paused" | "ready" | "cancelled" | "not_started" | "unconfirmed";
type Origin = "send" | "start" | "handoff" | "reopen";
export interface SavedExecutor { machine: string; session: string; session_id: string; host: string | null; resolved: boolean }
export interface SavedAttempt {
  id: string; generation: number; origin: Origin; executor: SavedExecutor | null; request_id: string;
  requirement: number; phase: ExecutionPhase; at: number; began_at: number | null; progress_at: number | null; legacy: boolean;
}
export interface SavedExecution {
  requirement: number; attempts: number; current: SavedAttempt | null;
  conflict: boolean; has_unknown: boolean; has_unexplained: boolean;
}
const record = (v: unknown): Record<string, unknown> | undefined => v && typeof v === "object" && !Array.isArray(v) ? v as Record<string, unknown> : undefined;
const text = (v: unknown): v is string => typeof v === "string" && new TextEncoder().encode(v).length <= 512 && !/[\u0000-\u001f\u007f-\u009f]/u.test(v);
const natural = (v: unknown): v is number => typeof v === "number" && Number.isSafeInteger(v) && v >= 0;
const time = (v: unknown): v is number | null => v === null || typeof v === "number" && Number.isSafeInteger(v) && Math.abs(v) <= 8.64e15;

function executor(value: unknown): SavedExecutor | undefined {
  const v = record(value);
  if (!v || !text(v.machine) || !text(v.session) || !text(v.session_id) || !(v.host === null || text(v.host)) || typeof v.resolved !== "boolean") return;
  return { machine: v.machine, session: v.session, session_id: v.session_id, host: v.host, resolved: v.resolved && v.host !== null };
}
function attempt(value: unknown): SavedAttempt | undefined {
  const v = record(value);
  if (!v || !text(v.id) || !natural(v.generation) || !natural(v.requirement) || !text(v.request_id) || typeof v.legacy !== "boolean"
    || typeof v.origin !== "string" || !["send", "start", "handoff", "reopen"].includes(v.origin)
    || typeof v.phase !== "string" || !["submitted", "unknown", "running", "paused", "ready", "cancelled", "not_started", "unconfirmed"].includes(v.phase)
    || !time(v.at) || v.at === null || !time(v.began_at) || !time(v.progress_at)) return;
  if (v.legacy ? v.id !== "" || v.generation !== 0 : !/^[0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{12}$/.test(v.id) || v.generation === 0) return;
  if (v.phase === "running" && v.began_at === null) return;
  const peer = v.executor === null && v.legacy ? null : executor(v.executor);
  if (peer === undefined) return;
  return { id: v.id, generation: v.generation, origin: v.origin as Origin, executor: peer, request_id: v.request_id,
    requirement: v.requirement, phase: v.phase as ExecutionPhase, at: v.at, began_at: v.began_at, progress_at: v.progress_at, legacy: v.legacy };
}
export function savedExecution(value: unknown): SavedExecution | undefined {
  const v = record(value);
  if (!v || !natural(v.requirement) || !natural(v.attempts) || v.attempts > 256
    || typeof v.conflict !== "boolean" || typeof v.has_unknown !== "boolean" || typeof v.has_unexplained !== "boolean") return;
  const current = v.current === null ? null : attempt(v.current);
  if (current === undefined) return;
  return { requirement: v.requirement, attempts: v.attempts, current, conflict: v.conflict, has_unknown: v.has_unknown, has_unexplained: v.has_unexplained };
}
