import type { TeamLaneView, TeamTaskView } from "../../types";
import { tr } from "../../i18n";

// Task #226: the plan's lanes, as Qt's TeamWorkspace groups them (U27): tasks
// with the same lane.n form one lane; lane 0 is "no lane" and falls back to
// the dependency layout. The helpers here are pure so the view's menus and the
// tests share them.

export interface LaneGroup {
  lane: number;
  label: string;
  tasks: TeamTaskView[];
}

/** One phase's lanes: numbered lanes ascending, the unassigned tasks (0) last. */
export function laneGroups(tasks: readonly TeamTaskView[], phase: number, includeCancelled = true): LaneGroup[] {
  const grouped = new Map<number, TeamTaskView[]>();
  for (const task of tasks) {
    if (task.phase !== phase) continue;
    if (!includeCancelled && task.status === "cancelled") continue;
    const lane = task.lane && task.lane.n > 0 ? task.lane.n : 0;
    const group = grouped.get(lane) ?? [];
    group.push(task);
    grouped.set(lane, group);
  }
  const numbers = [...grouped.keys()].filter((lane) => lane > 0).sort((a, b) => a - b);
  const groups = numbers.map((lane) => ({
    lane,
    label: laneLabel(grouped.get(lane) ?? []),
    tasks: grouped.get(lane) ?? [],
  }));
  const unassigned = grouped.get(0);
  if (unassigned?.length) groups.push({ lane: 0, label: "", tasks: unassigned });
  return groups;
}

/** The label any task of the lane carries (Qt laneLabel). */
export function laneLabel(tasks: readonly TeamTaskView[]): string {
  return tasks.find((task) => task.lane?.label)?.lane?.label ?? "";
}

/** Qt's lane name: "Lane N — label", "Lane N", or "No lane — by dependency". */
export function laneName(lane: number, label = ""): string {
  if (lane <= 0) return tr("No lane — by dependency");
  return label ? tr("Lane %1 — %2", lane, label) : tr("Lane %1", lane);
}

/** The lane header's own line: its name and how many tasks it has. */
export function laneTitle(lane: number, label: string, count: number): string {
  const name = laneName(lane, label);
  return count === 1 ? tr("%1 — 1 task", name) : tr("%1 — %2 tasks", name, count);
}

/** The lane a "New lane…" starts: one past the highest in the phase. */
export function nextLane(tasks: readonly TeamTaskView[], phase: number): number {
  let next = 1;
  for (const task of tasks) {
    if (task.phase === phase && task.lane && task.lane.n >= next) next = task.lane.n + 1;
  }
  return next;
}

/** The default name of a new lane: Qt's "Checkpoint N". */
export function defaultLaneName(lane: number): string {
  return tr("Checkpoint %1", lane);
}

/** The updates that rename a lane: every task in it carries the new label. */
export function laneRenameUpdates(tasks: readonly TeamTaskView[], phase: number, lane: number, label: string): { id: number; lane: TeamLaneView | null }[] {
  if (lane <= 0) return [];
  const name = label.trim().slice(0, 40);
  return tasks
    .filter((task) => task.phase === phase && task.lane?.n === lane)
    .map((task) => ({ id: task.id, lane: { n: lane, label: name } }));
}

/** The update that moves one task to a lane (null: out of every lane). */
export function laneMoveUpdate(task: TeamTaskView, lane: number, label = ""): { id: number; lane: TeamLaneView | null }[] {
  if (lane <= 0) return [{ id: task.id, lane: null }];
  return [{ id: task.id, lane: { n: lane, label: label.trim().slice(0, 40) } }];
}

/** Whether a phase's tasks take lane edits: a planning team, a draft phase, or
 * an accepted phase whose tasks have not started (Qt's acceptedUnstarted). */
export function laneEditable(
  team: { status: string; phases: { n: number; state: string; status: string }[]; tasks?: { phase: number; status: string; attempts?: number }[] },
  phase: number,
): boolean {
  if (team.status === "planning") return true;
  const found = team.phases.find((candidate) => candidate.n === phase);
  if (!found) return false;
  if (found.state === "draft" || found.status === "draft") return true;
  const accepted = found.state === "accepted" || found.status === "accepted";
  if (!accepted) return false;
  const tasks = (team.tasks ?? []).filter((task) => task.phase === phase);
  return tasks.length > 0 && tasks.every((task) => (task.status === "draft" || task.status === "pending") && (task.attempts ?? 0) === 0);
}
