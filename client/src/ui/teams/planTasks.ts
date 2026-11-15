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

/** The store's attempt/effect fact is required, even on a pending card. */
export function dependencyEditCause(task: TeamCard, status: string): string {
  if (!["running", "paused"].includes(status)) return tr("Dependencies cannot be edited in an inactive team.");
  if (task.kind === "stage") return tr("A pipeline stage's dependencies cannot be edited.");
  if (task.pending_decision) return tr("The previous decision has not settled.");
  if (!["draft", "waiting", "pending"].includes(task.state) || task.deps_editable === false) {
    return tr("Dependencies can only be edited before this task starts or has an attempt or hand-out recorded.");
  }
  if (task.deps_editable !== true) return tr("Dependency editing is unavailable until this task's attempt history is known.");
  return "";
}

export function canSetDeps(task: TeamCard, status: string, busy: boolean): boolean {
  return !busy && dependencyEditCause(task, status) === "";
}
