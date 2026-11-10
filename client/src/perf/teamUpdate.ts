// Task #9: what one update of a large team costs the Plan view, applied as
// the store did before (each read replaced the view's object, so every <For>
// keyed by identity rebuilt its rows) and as it does now (reconciled by id).
// Shared by the browser smoke (teamUpdateSmoke.tsx) and the happy-dom run
// (teamUpdate.perf.test.ts); synthetic data only.
import { batch, createComponent, createRoot } from "solid-js";
import { createStore } from "solid-js/store";
import { render } from "solid-js/web";
import { createTeams } from "../store/teamsView";
import type { TeamView, TeamsOut } from "../types";
import type { PlanGraph } from "../ui/teams/types";
import { PlanView } from "../ui/teams/PlanView";
import type { PlanMode } from "../ui/teams/planModes";

export const TASKS = 465;
const PHASES = 6;
const MEMBERS = 10;
const STATES = ["accepted", "accepted", "in_progress", "assigned", "submitted", "pending", "pending", "draft"];

/** A 465-task team in six phases with ten members; `tick` changes one member's context, `moved` one task's status. */
export function team465(tick = 0, moved = 0): { team: TeamView; graph: PlanGraph } {
  const members = Array.from({ length: MEMBERS }, (_, index) => ({
    id: index === 0 ? "lead" : `dev${index}`, role: index === 0 ? "Lead" : `Developer ${index}`, agent: ["claude", "codex", "kimi"][index % 3],
    model: "model", effort: "high", account: "", computer: "", folder: "", state: "active", activity: "idle", phase: "idle", error: "", lead: index === 0,
    session: { host: "", name: `claude/example/m${index}`, run_id: `run-${index}`, conversation_id: `conv-${index}` }, live: true, stuck: false,
    question: false, queue: [], minutes_without_progress: 0, host_shared: false,
    context_percent: index === 3 ? 20 + (tick % 70) : 30,
  }));
  const perPhase = Math.ceil(TASKS / PHASES);
  const tasks = Array.from({ length: TASKS }, (_, index) => {
    const id = index + 1, phase = Math.floor(index / perPhase) + 1, inPhase = index % perPhase;
    const status = id === 7 && moved % 2 === 1 ? "submitted" : STATES[(id * 7 + phase) % STATES.length];
    return {
      id, title: `Task ${id}: synthetic work item`, status, assignee: members[1 + (id % (MEMBERS - 1))].id, phase,
      deps: inPhase >= 3 ? [{ task: id - 3, hard: true }] : [], estimate_minutes: 30, attempts: status === "pending" ? 0 : 1, attempt_state: "",
      decided_by: "", pinned: false, output: [], waits: "", layer: Math.floor(inPhase / 3),
    };
  });
  const phases = Array.from({ length: PHASES }, (_, index) => {
    const n = index + 1, own = tasks.filter((task) => task.phase === n);
    return { n, title: `Phase ${n}`, state: "approved", status: n === 1 ? "in_work" : "approved", done: own.filter((task) => task.status === "accepted").length,
      total: own.length, barrier: false, auto: false, acceptance: "", paused: false, health: "" };
  });
  const team = {
    id: "perf-465", name: "Large team (synthetic)", status: "running", project: "example", folder: "C:/work/example", goal: "", plan_mode: "standard", lead: "lead",
    revision: 1 + tick + moved, created: 0, updated: tick, waiting_for_lead: false, pending_phase: null, settings: {},
    members, tasks: tasks.map(({ layer: _layer, ...task }) => task), phases, proposals: [], attention: [], intakes: [],
    validation: { errors: [], warnings: [] }, health: { critical_path: 0, parallelism: 0, warnings: [] }, ready: [], cycle: [], draft: null, truncated: false,
  } as unknown as TeamView;
  const graph: PlanGraph = {
    team: team.id, layers: Math.ceil(perPhase / 3),
    nodes: tasks.map((task) => ({ id: task.id, title: task.title, status: task.status, assignee: task.assignee, role: "", phase: task.phase,
      layer: task.layer, ready: false, in_cycle: false })),
    edges: tasks.flatMap((task) => task.deps.map((dep) => ({ from: dep.task, to: task.id, hard: dep.hard }))),
  };
  return { team, graph };
}

type Store = { apply(outs: readonly TeamsOut[]): void; team(c: string, id: string): TeamView | undefined; graph(c: string, id: string): PlanGraph | undefined };

/** The store as it applied views before task #9: each read replaced the view's object. */
function replacing(): Store {
  const [views, setViews] = createStore<Record<string, { kind: string; payload: unknown } | undefined>>({});
  return {
    apply: (outs) => batch(() => { for (const out of outs) setViews(`${out.computer}\n${out.key}`, { kind: out.kind, payload: out.payload }); }),
    team: (c, id) => views[`${c}\nteam:${id}`]?.payload as TeamView | undefined,
    graph: (c, id) => views[`${c}\ngraph:${id}`]?.payload as PlanGraph | undefined,
  };
}

export type Way = "replace" | "reconcile";
export type Scenario = "member" | "task";
/** `script_ms`: the median of the apply alone, before the forced layout (task #16). */
export interface Measured { way: Way; scenario: Scenario; mode: PlanMode; tasks: number; updates: number; cards: number; kept: number; median_ms: number; p90_ms: number; total_ms: number; script_ms: number }

const outs = (team: TeamView, graph: PlanGraph): TeamsOut[] => [
  { computer: "", key: `team:${team.id}`, kind: "team", payload: team }, { computer: "", key: `graph:${team.id}`, kind: "graph", payload: graph },
];

/**
 * Renders the Plan view of the 465-task team into `host`, applies `updates`
 * reads that change one member's field (or one task's status) and measures
 * each apply with its synchronous render and a forced layout. `kept` counts
 * the task cards whose element survived every update.
 */
export function measureTeamUpdates(host: HTMLElement, way: Way, scenario: Scenario, mode: PlanMode = "graph", updates = 30): Measured {
  // Only the store is made in a root: updates applied inside a root's body
  // would wait for it to return, so the loop runs outside, as reads do.
  let dispose!: () => void;
  const store: Store = createRoot((done) => { dispose = done; return way === "reconcile" ? createTeams({ follow: async () => {} }) : replacing(); });
  const first = team465();
  store.apply(outs(first.team, first.graph));
  const stop = render(() => createComponent(PlanView, {
    get team() { return store.team("", first.team.id)!; },
    get graph() { return store.graph("", first.team.id)!; },
    initialMode: mode, now: () => 1_800_000_000,
  }), host);
  const cards = () => new Map([...host.querySelectorAll<HTMLElement>("[data-task]")].map((card) => [card.dataset.task!, card]));
  const before = cards();
  const times: number[] = [];
  const scripts: number[] = [];
  for (let n = 1; n <= updates; n++) {
    const next = scenario === "member" ? team465(n, 0) : team465(0, n);
    const started = performance.now();
    store.apply(outs(next.team, next.graph));
    scripts.push(performance.now() - started);
    void host.offsetHeight;
    times.push(performance.now() - started);
  }
  const after = cards();
  let kept = 0;
  for (const [id, card] of before) if (after.get(id) === card) kept++;
  stop(); dispose();
  host.replaceChildren();
  const sorted = [...times].sort((a, b) => a - b);
  const round = (ms: number) => Math.round(ms * 100) / 100;
  const script = [...scripts].sort((a, b) => a - b)[Math.floor(scripts.length / 2)];
  return {
    way, scenario, mode, tasks: TASKS, updates, cards: before.size, kept,
    median_ms: round(sorted[Math.floor(sorted.length / 2)]), p90_ms: round(sorted[Math.floor(sorted.length * 0.9)]), total_ms: round(times.reduce((a, b) => a + b, 0)),
    script_ms: round(script),
  };
}
