// Toolbar filtering and bounded zoom adapted from WhiteWh's PlanView/planTasks.
// The upstream queue's facts are the only inputs; no attempt or member inference.
import type { PlanPreferences } from "../../store/teamPlan";
import type { TeamCard } from "../../store/teams";

export function planProblem(task: TeamCard): boolean {
  return task.held || ["cancel_refused", "key_clear_refused", "pending"].includes(task.stop_outcome?.code ?? "");
}

export function planMatches(task: TeamCard, state: PlanPreferences): boolean {
  if (task.state === "cancelled" && !state.showCancelled) return false;
  if (task.state === "accepted" && state.hideFinished) return false;
  if (state.filter === "active") return ["in_progress", "reported"].includes(task.state);
  if (state.filter === "problems") return planProblem(task);
  return true;
}

export type PlanTone = "done" | "work" | "review" | "notstarted" | "cancelled";
export function planTone(task: TeamCard): PlanTone {
  if (task.state === "accepted") return "done";
  if (task.state === "in_progress") return "work";
  if (task.state === "reported") return "review";
  return task.state === "cancelled" ? "cancelled" : "notstarted";
}

export function fitPlanZoom(room: number, widths: readonly number[]): number | undefined {
  if (room <= 0 || widths.length === 0) return undefined;
  return Math.max(0.4, Math.min(1, (room - 2) / Math.max(1, ...widths)));
}

/** Origin links are explanatory, never editable dependencies. Only same-page,
 * same-section endpoints are routed; other origins stay named on the card. */
export function planOrigins(tasks: readonly TeamCard[]) {
  const ids = new Set(tasks.map((task) => task.node));
  return tasks.flatMap((task) => task.origin != null && task.origin !== task.node && ids.has(task.origin)
    ? [{ from: task.origin, to: task.node }] : []);
}
