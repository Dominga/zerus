// Adapted from WhiteWh's lanes.ts (722f14814): group one bounded page into
// lanes. Upstream lanes are members, not editable metadata on the fork's tasks.
import type { TeamCard, TeamMember, TeamStage, TeamView } from "../../store/teams";
import { closedStage, currentStage, stageAgrees, workerOf } from "./pipeline-view";

export interface LaneTask {
  task: TeamCard;
  stage?: TeamStage;
  state: string;
  worker: string;
}
export interface MemberLane {
  key: string;
  member: TeamMember | null;
  tasks: LaneTask[];
}

// Current work first; a closed result remains visible at the end of its lane.
const STATES = ["in_progress", "reported", "pending", "waiting", "draft", "accepted", "cancelled"];
const compare = (a: string, b: string) => a < b ? -1 : a > b ? 1 : 0;

/** Current verified pipeline work belongs to its stage worker, not the person
 * who implemented the task being reviewed. Other cards keep their own facts. */
export function laneTask(task: TeamCard): LaneTask {
  const candidate = currentStage(task);
  const stage = candidate && stageAgrees(task, candidate)
    && (task.kind === "stage" || (!["accepted", "cancelled"].includes(task.state) && !closedStage(candidate))) ? candidate : undefined;
  const worker = stage?.holder || stage?.assignee || (task.kind === "stage" && candidate && !stage ? "" : task.holder || workerOf(task));
  return { task, stage, worker, state: stage && (stage.holder || stage.assignee) ? stage.state : task.state };
}

/** Page-local hand-out facts: stages by node (oldest first), queue tasks by
 * board rank, then task UUID and node. This does not decide scheduler readiness. */
export function laneOrder(a: LaneTask, b: LaneTask): number {
  const state = (value: string) => { const n = STATES.indexOf(value); return n < 0 ? STATES.length : n; };
  const status = state(a.state) - state(b.state) || compare(a.state, b.state);
  if (status) return status;
  const stage = (entry: LaneTask) => entry.task.kind === "stage" || !!(entry.stage?.holder || entry.stage?.assignee);
  if (stage(a) !== stage(b)) return stage(a) ? -1 : 1;
  if (stage(a)) return (a.stage?.node ?? a.task.node) - (b.stage?.node ?? b.task.node) || a.task.node - b.task.node;
  return compare(a.task.rank, b.task.rank) || compare(a.task.task?.uuid ?? "", b.task.task?.uuid ?? "") || a.task.node - b.task.node;
}

/** One lane for every projected member, including empty lanes, and an
 * unassigned lane last. Unknown members never hide a task or invent a roster. */
export function laneGroups(view: TeamView): MemberLane[] {
  const lanes: MemberLane[] = view.members.map((member) => ({ key: `member:${member.id}`, member, tasks: [] }));
  const byMember = new Map(lanes.map((lane) => [lane.member!.id, lane]));
  const unassigned: MemberLane = { key: "unassigned", member: null, tasks: [] };
  for (const group of view.groups) for (const card of group.cards) {
    const entry = laneTask(card);
    (byMember.get(entry.worker) ?? unassigned).tasks.push(entry);
  }
  lanes.push(unassigned);
  for (const lane of lanes) lane.tasks.sort(laneOrder);
  return lanes;
}
