import type { TeamPipelineRun, TeamProfileView, TeamTestsView } from "../../types";
import type { PlacedNode } from "./plan";
import { taskHeat } from "./heat";

/** The plan's five modes, as Qt's TeamWorkspace toolbar names them. */
export type PlanMode = "graph" | "board" | "layers" | "profile" | "tests";

export interface PlanColumn {
  key: string;
  statuses: string[];
  nodes: PlacedNode[];
}

const COLUMNS: { key: string; statuses: string[] }[] = [
  { key: "todo", statuses: ["backlog", "todo", "pending"] },
  { key: "active", statuses: ["in_progress", "working"] },
  { key: "review", statuses: ["submitted", "in_review"] },
  { key: "test", statuses: ["in_test"] },
  { key: "done", statuses: ["accepted", "done"] },
  { key: "cancelled", statuses: ["cancelled", "rejected"] },
];

/** Task #60: the task a follow-up was spawned from, from the graph or the task. */
export function spawnedFrom(node: { origin?: number | null }, task?: { origin?: number | null }): number | undefined {
  const origin = node.origin ?? task?.origin ?? 0;
  return origin > 0 ? origin : undefined;
}

/** The tasks of every phase as one list, cancelled ones only when shown. */
export function placedTasks(phases: { nodes: PlacedNode[] }[], includeCancelled: boolean): PlacedNode[] {
  return phases.flatMap((phase) => phase.nodes).filter((placed) => includeCancelled || placed.node.status !== "cancelled");
}

/** Board columns by the task's state; a column is empty rather than hidden. */
export function boardColumns(phases: { nodes: PlacedNode[] }[], includeCancelled: boolean): PlanColumn[] {
  const tasks = placedTasks(phases, includeCancelled);
  return COLUMNS.filter((column) => includeCancelled || column.key !== "cancelled").map((column) => ({
    ...column,
    nodes: tasks.filter((placed) => column.statuses.includes(placed.node.status)),
  }));
}

/** Layers across every phase: one row per graph layer, phase order inside it. */
export function layerRows(phases: { nodes: PlacedNode[] }[], includeCancelled: boolean): { layer: number; nodes: PlacedNode[] }[] {
  const rows = new Map<number, PlacedNode[]>();
  for (const placed of placedTasks(phases, includeCancelled)) {
    const row = rows.get(placed.node.layer) ?? [];
    row.push(placed);
    rows.set(placed.node.layer, row);
  }
  return [...rows.entries()].sort(([a], [b]) => a - b).map(([layer, nodes]) => ({ layer, nodes }));
}

/** The stage's name: Qt's stages::stageName. */
export function stageName(kind: string): string {
  if (kind === "review") return "Review";
  if (kind === "test") return "Test";
  if (kind === "command") return "Command";
  if (kind === "implement") return "Implement";
  if (kind === "land") return "Land";
  return kind;
}

/** The newest run of each stage kind, in pipeline order. */
export function stageChips(run: TeamPipelineRun | null | undefined): { kind: string; state: string; attempt: number }[] {
  if (!run) return [];
  const seen = new Set<string>();
  return run.stages
    .filter((kind) => {
      if (seen.has(kind)) return false;
      seen.add(kind);
      return true;
    })
    .map((kind) => ({
      kind,
      state: run.open?.kind === kind ? run.open.state : "",
      attempt: 0,
    }));
}

/** Task 82: a task's tint, 0..1, by its own lateness against the plan (2 h
 * tints fully); null when it is not late by its own (inherited lateness, on
 * schedule, or no baseline): the card stays untinted. */
export function heatShare(profile: TeamProfileView | null | undefined, task: number): number | null {
  const heat = taskHeat(profile, task);
  return heat && heat.tint > 0 ? heat.tint : null;
}

/** The loss text for a tile, or empty when unknown. */
export function lossText(profile: TeamProfileView | null | undefined, task: number): string {
  if (!profile) return "";
  return profile.losses
    .filter((loss) => loss.task === task && loss.seconds > 0)
    .map((loss) => `${loss.kind} ${formatSeconds(loss.seconds)}`)
    .join(" ");
}

export function formatSeconds(seconds: number): string {
  if (!Number.isFinite(seconds) || seconds <= 0) return "-";
  if (seconds < 60) return `${Math.round(seconds)}s`;
  if (seconds < 3600) return `${Math.floor(seconds / 60)}m ${Math.round(seconds % 60).toString().padStart(2, "0")}s`;
  return `${Math.floor(seconds / 3600)}h ${Math.floor((seconds % 3600) / 60).toString().padStart(2, "0")}m`;
}

export function formatTokens(tokens: number | null): string {
  if (tokens === null || !Number.isFinite(tokens)) return "unknown";
  if (tokens >= 1_000_000) return `${(tokens / 1_000_000).toFixed(1)}M`;
  if (tokens >= 1_000) return `${(tokens / 1_000).toFixed(1)}k`;
  return String(tokens);
}

/** The Tests mode's rows: running first, then waiting, then the latest runs. */
export interface TestsRows {
  running: TeamTestsView["queue"];
  waiting: TeamTestsView["queue"];
  runs: TeamTestsView["runs"];
}

export function testsRows(tests: TeamTestsView | null | undefined): TestsRows {
  if (!tests) return { running: [], waiting: [], runs: [] };
  return {
    running: tests.queue.filter((item) => item.state === "running"),
    waiting: tests.queue.filter((item) => item.state !== "running"),
    runs: tests.runs,
  };
}

export function testStatus(status: string): "passed" | "failed" | "timed-out" | "reused" | "queued" {
  if (status === "passed") return "passed";
  if (status === "timed_out" || status === "timed-out") return "timed-out";
  if (status === "reused") return "reused";
  if (status === "queued" || status === "waiting" || status === "running") return "queued";
  return "failed";
}

/** Qt's run time: 45s, 3m 05s, 1h 20m. */
export function runTime(ms: number): string {
  if (!Number.isFinite(ms) || ms <= 0) return "-";
  const seconds = Math.round(ms / 1000);
  if (seconds < 60) return `${seconds}s`;
  if (seconds < 3600) return `${Math.floor(seconds / 60)}m ${(seconds % 60).toString().padStart(2, "0")}s`;
  return `${Math.floor(seconds / 3600)}h ${Math.floor((seconds % 3600) / 60).toString().padStart(2, "0")}m`;
}
