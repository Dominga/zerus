// Adapted from WhiteWh/zerus planTasks.ts to upstream node states and guards.
import { tr } from "../../i18n";
import type { TeamCard } from "../../store/teams";

export function taskState(task: TeamCard): string {
  if (task.held) return tr("Held");
  switch (task.state) {
    case "draft": return tr("Draft");
    case "waiting": return tr("Waiting");
    case "pending": return tr("Pending");
    case "in_progress": return tr("In progress");
    case "reported": return tr("Submitted");
    case "accepted": return tr("Accepted");
    case "cancelled": return tr("Cancelled");
    default: return task.state;
  }
}

export function canSetDeps(task: TeamCard, status: string, busy: boolean): boolean {
  return !busy && ["running", "paused"].includes(status) && task.kind !== "stage"
    && ["draft", "waiting", "pending", "in_progress", "reported"].includes(task.state) && !task.pending_decision;
}
