import type { GraphEdge, GraphNode, PlanGraph, TaskView, TeamView, PhaseView } from "./types";

// The plan graph's geometry, shared by the view and its tests. Cards are fixed
// size like the prototype's 232x60 nodes; layers run left to right and rows
// top to bottom, and the virtualizer windows whole layer rows.
export const CARD_W = 232;
export const CARD_H = 60;
export const GAP_X = 26;
export const GAP_Y = 22;
export const HEADER_H = 44;
/** Task 196: the lead's note under an open phase's header (Qt teamPhaseNote). */
export const NOTE_H = 26;
export const ROW_H = CARD_H + GAP_Y;

export interface PlacedNode {
  node: GraphNode;
  task: TaskView | undefined;
  x: number;
  y: number;
}

export interface PlacedLayer {
  layer: number;
  nodes: PlacedNode[];
}

export interface PlanPhase {
  phase: PhaseView;
  tasks: TaskView[];
  nodes: PlacedNode[];
  layers: PlacedLayer[];
  edges: GraphEdge[];
  width: number;
  height: number;
}

export interface PlanRow {
  key: string;
  kind: "header" | "layer";
  phase: PlanPhase;
  layer: number;
  top: number;
  height: number;
  nodes: PlacedNode[];
  /** The last visual row of a phase, so its band closes with a border. */
  end: boolean;
}

const byId = (tasks: TaskView[]) => new Map(tasks.map((task) => [task.id, task]));

/** A task and every task it depends on, directly or through another task. */
export function ancestors(tasks: TaskView[], id: number): Set<number> {
  const found = new Set<number>([id]);
  const pending = [id];
  const tasksById = byId(tasks);
  while (pending.length) {
    const task = tasksById.get(pending.pop()!);
    for (const dep of task?.deps ?? []) {
      if (!found.has(dep.task)) {
        found.add(dep.task);
        pending.push(dep.task);
      }
    }
  }
  return found;
}

/** Every task that depends on `id`, directly or through another task. */
export function descendants(tasks: TaskView[], id: number): Set<number> {
  const found = new Set<number>([id]);
  const pending = [id];
  while (pending.length) {
    const current = pending.pop()!;
    for (const task of tasks) {
      if (found.has(task.id)) continue;
      if (task.deps.some((dep) => dep.task === current)) {
        found.add(task.id);
        pending.push(task.id);
      }
    }
  }
  return found;
}

/**
 * The hover chain of a task: itself, its ancestors and its descendants. A
 * cycle in a hand-edited plan still ends, because every visited task is added
 * before it is walked.
 */
export function chain(tasks: TaskView[], id: number): Set<number> {
  return new Set([...ancestors(tasks, id), ...descendants(tasks, id)]);
}

/** The tasks blocking `id` that are not dependencies in the same phase. */
export function externalDeps(phase: number, task: TaskView, tasksById: Map<number, TaskView>): number[] {
  return task.deps
    .map((dep) => dep.task)
    .filter((id) => (tasksById.get(id)?.phase ?? phase) !== phase);
}

/** The tasks that wait for `id` and are not in its phase. */
export function externalBlocks(phase: number, id: number, tasks: TaskView[]): number[] {
  return tasks.filter((task) => task.phase !== phase && task.deps.some((dep) => dep.task === id)).map((task) => task.id);
}

function phaseRecord(team: TeamView, n: number): PhaseView {
  return (
    team.phases.find((phase) => phase.n === n) ?? {
      n,
      title: `Phase ${n}`,
      state: "",
      status: "",
      done: 0,
      total: 0,
      barrier: false,
      auto: false,
      acceptance: "",
      paused: false,
      health: "",
    }
  );
}

/**
 * One phase's graph: tasks grouped by hgs layer, each layer a row of cards
 * left to right. Phases without tasks still get their header.
 */
export function phasePlans(team: TeamView, graph: PlanGraph): PlanPhase[] {
  const tasksById = byId(team.tasks);
  const numbers = new Set<number>(team.phases.map((phase) => phase.n));
  for (const task of team.tasks) numbers.add(task.phase);
  return [...numbers]
    .sort((a, b) => a - b)
    .map((n) => {
      const tasks = team.tasks.filter((task) => task.phase === n);
      const known = new Set(tasks.map((task) => task.id));
      const nodes = graph.nodes
        .filter((node) => node.phase === n)
        .map((node) => ({
          node,
          task: tasksById.get(node.id),
          x: 0,
          y: 0,
        }));
      const graphed = new Set(nodes.map((placed) => placed.node.id));
      for (const task of tasks.filter((task) => task.archived && !graphed.has(task.id))) {
        nodes.push({ node: { id: task.id, title: task.title, status: task.status, assignee: task.assignee,
          role: "", phase: n, layer: 0, ready: false, in_cycle: false }, task, x: 0, y: 0 });
      }
      const layers = new Map<number, PlacedNode[]>();
      for (const placed of nodes) {
        const layer = layers.get(placed.node.layer) ?? [];
        layer.push(placed);
        layers.set(placed.node.layer, layer);
      }
      const ordered = [...layers.entries()]
        .sort(([a], [b]) => a - b)
        .map(([layer, list], row) => {
          const placed = list
            .sort((a, b) => a.node.id - b.node.id)
            .map((item, column) => ({ ...item, x: column * (CARD_W + GAP_X), y: row * ROW_H }));
          return { layer, nodes: placed };
        });
      const placed = ordered.flatMap((layer) => layer.nodes);
      const columns = Math.max(0, ...ordered.map((layer) => layer.nodes.length));
      return {
        // Task 196 (P10): a cancelled task is not work the phase still owes, as
        // Qt counts it: a Done phase with one cancelled task reads 7/7, not 7/8.
        // An archived phase keeps only some of its tasks live: its record's total stands.
        phase: { ...phaseRecord(team, n), done: phaseRecord(team, n).done || tasks.filter((task) => task.status === "accepted" || task.status === "done").length, total: phaseRecord(team, n).archived ? Math.max(phaseRecord(team, n).total, tasks.length) : tasks.filter((task) => task.status !== "cancelled").length },
        tasks,
        nodes: placed,
        layers: ordered,
        edges: graph.edges.filter((edge) => known.has(edge.from) && known.has(edge.to)),
        width: columns === 0 ? 0 : columns * CARD_W + (columns - 1) * GAP_X,
        height: ordered.length * ROW_H,
      };
    });
}

/** The virtualized vertical list: a phase header, then one row per layer. */
export function buildRows(phases: PlanPhase[], collapsed: ReadonlySet<number>): PlanRow[] {
  const rows: PlanRow[] = [];
  let top = 0;
  for (const phase of phases) {
    const isCollapsed = collapsed.has(phase.phase.n);
    const height = HEADER_H + (!isCollapsed && phase.phase.note?.trim() ? NOTE_H : 0);
    rows.push({ key: `phase:${phase.phase.n}`, kind: "header", phase, layer: 0, top, height, nodes: [], end: isCollapsed || phase.nodes.length === 0 });
    top += height;
    if (isCollapsed || phase.nodes.length === 0) continue;
    phase.layers.forEach((layer, index) => {
      rows.push({ key: `phase:${phase.phase.n}:layer:${layer.layer}`, kind: "layer", phase, layer: layer.layer, top, height: ROW_H, nodes: layer.nodes, end: index === phase.layers.length - 1 });
      top += ROW_H;
    });
  }
  return rows;
}

/** The phase chip's kind and text, as the plan header shows it. */
export function phaseStatus(phase: PhaseView): { kind: string; label: string } {
  // Task 196: Qt's phase states (TeamPhaseScroll.h phaseStateBadge).
  if (phase.state === "cancelled") return { kind: "cancelled", label: "Cancelled" };
  if (phase.state === "rejected") return { kind: "rejected", label: "Sent back" };
  if (phase.state === "awaiting_acceptance") return { kind: "awaiting", label: "Awaiting your acceptance" };
  if (phase.state === "planned") return { kind: "planned", label: "In the plan" };
  if (phase.paused) return { kind: "paused", label: "Paused" };
  if (phase.state === "queued") return { kind: "queued", label: "Accepted, queued" };
  if (phase.state === "draft" || phase.status === "draft") return { kind: "draft", label: "Draft" };
  if (phase.status === "done") return { kind: "done", label: "Done" };
  if (phase.status === "in_work") return { kind: "running", label: "Running" };
  if (phase.status === "approved") return { kind: "approved", label: "Approved" };
  return { kind: "open", label: "Open" };
}

/** Whether a phase still needs the owner's approval. */
export function phaseNeedsApproval(phase: PhaseView): boolean {
  return phase.state === "draft" || phase.status === "draft";
}

export function edgeKey(edge: GraphEdge): string {
  return `${edge.from}>${edge.to}`;
}

/** A chain lights only the edges whose two ends are both on it. */
export function edgeState(edge: GraphEdge, lit: ReadonlySet<number> | null): "lit" | "dim" | "" {
  if (!lit) return "";
  return lit.has(edge.from) && lit.has(edge.to) ? "lit" : "dim";
}

/** The orthogonal-ish curve from a layer's node to a later layer's node. */
export function edgePath(from: PlacedNode, to: PlacedNode, rowTop: number): string {
  const sx = from.x + CARD_W;
  const sy = from.y + CARD_H / 2 - rowTop;
  const tx = to.x;
  const ty = to.y + CARD_H / 2 - rowTop;
  const bend = Math.max(24, Math.abs(tx - sx) / 2);
  return `M ${sx} ${sy} C ${sx + bend} ${sy}, ${tx - bend} ${ty}, ${tx} ${ty}`;
}

/** The origin links whose target is in one layer row: a follow-up's source to
 * its task, drawn apart from dependency edges and never waited for. */
export function rowOrigins(phase: PlanPhase, row: PlanRow): { from: PlacedNode; to: PlacedNode; origin: number }[] {
  if (row.kind !== "layer") return [];
  const sources = new Map(phase.nodes.map((placed) => [placed.node.id, placed]));
  const result: { from: PlacedNode; to: PlacedNode; origin: number }[] = [];
  for (const to of row.nodes) {
    const origin = (to.task?.origin ?? to.node.origin ?? 0) || 0;
    const from = sources.get(origin);
    if (origin > 0 && from && from.node.id !== to.node.id) result.push({ from, to, origin });
  }
  return result;
}
export function rowEdges(phase: PlanPhase, row: PlanRow): { edge: GraphEdge; from: PlacedNode; to: PlacedNode }[] {
  if (row.kind !== "layer") return [];
  const targets = new Map(row.nodes.map((placed) => [placed.node.id, placed]));
  const sources = new Map(phase.nodes.map((placed) => [placed.node.id, placed]));
  const result: { edge: GraphEdge; from: PlacedNode; to: PlacedNode }[] = [];
  for (const edge of phase.edges) {
    const to = targets.get(edge.to);
    const from = sources.get(edge.from);
    if (to && from) result.push({ edge, from, to });
  }
  return result;
}
