import { tr } from "../../i18n";
import type { TeamPipelineRun, TeamProfileView, TeamTestsView } from "../../types";
import type { PlacedNode } from "./plan";
import { taskHeat } from "./heat";

/** The plan's seven modes, as Qt's TeamWorkspace toolbar names them; Builds is Tests' own page (task #462). */
export type PlanMode = "graph" | "board" | "layers" | "lanes" | "profile" | "tests" | "builds";

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

export function testStatus(status: string): "passed" | "flaky" | "failed" | "timed-out" | "reused" | "queued" {
  if (status === "passed") return "passed";
  // Task 199 (Qt TeamTests statusName): a run that passed on a retry is not a failure.
  if (status === "flaky") return "flaky";
  if (status === "timed_out" || status === "timed-out") return "timed-out";
  if (status === "reused") return "reused";
  if (status === "queued" || status === "waiting" || status === "running") return "queued";
  return "failed";
}

/** A run's status in words (Qt TeamTests statusName). */
export function runStatusLabel(status: string): string {
  const kind = testStatus(status);
  if (kind === "passed") return tr("Passed");
  if (kind === "flaky") return tr("Passed on retry");
  if (kind === "timed-out") return tr("Timed out");
  if (kind === "reused") return tr("Reused");
  if (kind === "queued") return tr("Queued");
  return tr("Failed");
}

/** Qt's run time: 45s, 3m 05s, 1h 20m. */
export function runTime(ms: number): string {
  if (!Number.isFinite(ms) || ms <= 0) return "-";
  const seconds = Math.round(ms / 1000);
  if (seconds < 60) return `${seconds}s`;
  if (seconds < 3600) return `${Math.floor(seconds / 60)}m ${(seconds % 60).toString().padStart(2, "0")}s`;
  return `${Math.floor(seconds / 3600)}h ${Math.floor((seconds % 3600) / 60).toString().padStart(2, "0")}m`;
}

/** Task 197: the heat mode a team starts with: this browser's own stored
 * choice ("time", "tokens", "off"; "1" from before task 197 is lateness),
 * else Qt's imported mode (`teams/heatModes`), else Off. Tokens stays Tokens. */
export function heatModeSeed(team: string, stored: string | null, modes: string): "off" | "time" | "tokens" {
  if (stored === "1" || stored === "time") return "time";
  if (stored === "tokens" || stored === "off") return stored;
  if (stored !== null) return "off";
  try {
    const mode = (JSON.parse(modes) as Record<string, unknown>)[team];
    return mode === "time" || mode === "tokens" ? mode : "off";
  } catch {
    return "off";
  }
}

/** Task 103: what a team's Lateness switch starts with: this browser's own
 * stored choice first, else Qt's imported heat mode (`teams/heatModes`, a
 * bounded team -> "time" | "tokens" map from the settings store). */
export function heatSeed(team: string, stored: string | null, modes: string): boolean {
  if (stored !== null) return stored === "1";
  try {
    const map = JSON.parse(modes) as Record<string, unknown>;
    return map[team] === "time" || map[team] === "tokens";
  } catch {
    return false;
  }
}

/** Task 88: "41 passed, 2 failed, 1 skipped" for a run's report (Qt Run::countsText). */
export function reportCounts(report: { passed: number; failed: number; skipped: number; errors: number } | null | undefined): string {
  if (!report) return "";
  const parts: string[] = [];
  if (report.passed) parts.push(tr("%1 passed", report.passed));
  if (report.failed) parts.push(tr("%1 failed", report.failed));
  if (report.errors) parts.push(tr("%1 errors", report.errors));
  if (report.skipped) parts.push(tr("%1 skipped", report.skipped));
  return parts.length ? parts.join(", ") : tr("no tests reported");
}

/** Task 88: a test's name as the Tests mode shows it: suite::name unless the name already starts with its suite. */
export function caseName(test: { suite: string; name: string }): string {
  return !test.suite || test.name.startsWith(test.suite) ? test.name : `${test.suite}::${test.name}`;
}

/** Task 199 (Qt TeamTests): the suites seen in the queue and the history, by name. */
export function testSuites(tests: TeamTestsView | null | undefined): string[] {
  if (!tests) return [];
  return [...new Set([...tests.queue.map((item) => item.suite), ...tests.runs.map((run) => run.suite)].filter(Boolean))].sort();
}

/** A suite's latest run and its last ten statuses, newest first. */
export function suiteRuns(tests: TeamTestsView | null | undefined, suite: string): TeamTestsView["runs"] {
  return (tests?.runs ?? []).filter((run) => run.suite === suite).slice(0, 10);
}

/** Force run's state for a suite: queued as forced already, or free to force. */
export function forcedWaiting(tests: TeamTestsView | null | undefined, suite: string): boolean {
  return !!tests?.queue.some((item) => item.suite === suite && item.forced);
}

/** Task 88: whether a test failed (a failure or an error). */
export const caseFailed = (test: { status: string }): boolean => test.status === "failed" || test.status === "error";
