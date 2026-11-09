// @vitest-environment happy-dom
import { afterEach, describe, expect, it, vi } from "vitest";
import { createComponent, createRoot } from "solid-js";
import { render } from "solid-js/web";
import { createTeams } from "../../store/teamsView";
import type { TeamView } from "../../types";
import type { PlanGraph } from "./types";
import { PlanView } from "./PlanView";
import type { PlanMode } from "./planModes";

// Task #9: a team update reconciles into the view the Team tab shows, so a
// change of one member's field keeps every unchanged task card's element and
// a changed task keeps the others'. Before, each update was a new object and
// every <For> keyed by identity rebuilt all cards.

vi.mock("../../i18n", () => ({ trNoop: (s: string) => s, tr: (s: string, ...args: unknown[]) => s.replace(/%(\d+)/g, (_, n) => String(args[Number(n) - 1])) }));
vi.mock("../../store/settings", () => ({ settings: {}, settingsReady: new Promise(() => {}) }));
vi.mock("../sessions/SessionCard", () => ({ AgentBadge: () => null }));
vi.mock("../../store/catalog", () => ({ agentKind: (agent: string) => agent || "claude" }));
vi.mock("./TimeView", () => ({ ProfileTime: () => null }));
vi.mock("./actions", () => ({ stageName: (kind: string) => kind }));
vi.mock("../../skin/runtime", () => ({ skin: () => ({ appearance: "light" }) }));
vi.mock("../lib/Dropdown", () => ({ Dropdown: () => null }));

const NOW = 1_800_000_000;
let dispose: (() => void) | undefined;
afterEach(() => { dispose?.(); dispose = undefined; document.body.replaceChildren(); });

function payloads(context = 40, status = "in_progress"): { team: TeamView; graph: PlanGraph } {
  const task = (id: number, phase: number, assignee: string, state: string) => ({
    id, title: `Task ${id}`, status: state, assignee, phase, deps: id > 1 && phase === 1 ? [{ task: 1, hard: true }] : [], estimate_minutes: 30,
    attempts: 0, attempt_state: "", decided_by: "", pinned: false, output: [], waits: "",
  });
  const tasks = [task(1, 1, "dev", "accepted"), task(2, 1, "dev", status), task(3, 1, "qa", "assigned"), task(4, 1, "qa", "pending"), task(5, 2, "dev", "pending")];
  const team = {
    id: "t1", name: "Checkout", status: "running", project: "", folder: "", goal: "", plan_mode: "standard", lead: "lead", revision: 1, created: 0, updated: 0,
    waiting_for_lead: false, pending_phase: null, settings: {},
    members: [
      { id: "lead", role: "Lead", agent: "claude", lead: true, context_percent: 10 },
      { id: "dev", role: "Backend", agent: "codex", context_percent: context },
      { id: "qa", role: "Tester", agent: "kimi", context_percent: 5 },
    ],
    tasks,
    phases: [
      { n: 1, title: "Build", state: "approved", status: "in_work", done: 1, total: 4, barrier: false, auto: false, acceptance: "", paused: false, health: "" },
      { n: 2, title: "Ship", state: "approved", status: "approved", done: 0, total: 1, barrier: false, auto: false, acceptance: "", paused: false, health: "" },
    ],
    proposals: [], attention: [], validation: { errors: [], warnings: [] }, health: { critical_path: 0, parallelism: 0, warnings: [] },
    ready: [], cycle: [], draft: null, truncated: false,
  } as unknown as TeamView;
  const layer: Record<number, number> = { 1: 0, 2: 1, 3: 1, 4: 1, 5: 0 };
  const graph: PlanGraph = {
    team: "t1", layers: 2,
    nodes: tasks.map((found) => ({ id: found.id, title: found.title, status: found.status, assignee: found.assignee, role: "", phase: found.phase,
      layer: layer[found.id], ready: false, in_cycle: false })),
    edges: tasks.flatMap((found) => found.deps.map((dep) => ({ from: dep.task, to: found.id, hard: dep.hard }))),
  };
  return { team, graph };
}

function show(mode: PlanMode) {
  const store = createRoot(() => createTeams({ follow: async () => {} }));
  const read = (team: TeamView, graph: PlanGraph) => store.apply([
    { computer: "", key: "team:t1", kind: "team", payload: team }, { computer: "", key: "graph:t1", kind: "graph", payload: graph },
  ]);
  const { team, graph } = payloads();
  read(team, graph);
  const host = document.createElement("div"); document.body.append(host);
  dispose = render(() => createComponent(PlanView, {
    get team() { return store.team("", "t1")!; },
    get graph() { return store.graph("", "t1")!; },
    initialMode: mode, now: () => NOW,
  }), host);
  const card = (id: number) => host.querySelector<HTMLElement>(`[data-task="${id}"]`);
  return { store, read, card };
}

describe("team views update in place (task #9)", () => {
  it.each(["graph", "board", "layers"] as const)("keep every task card's element when another member's field changes (%s)", (mode) => {
    const { store, read, card } = show(mode);
    const before = [1, 2, 3, 4].map(card);
    expect(before.every(Boolean)).toBe(true);
    const shownTeam = store.team("", "t1")!;
    const firstTask = shownTeam.tasks[0];
    const next = payloads(85);
    read(next.team, next.graph);
    // The store kept its objects and took the new value in place.
    expect(store.team("", "t1")).toBe(shownTeam);
    expect(store.team("", "t1")!.tasks[0]).toBe(firstTask);
    expect((store.team("", "t1")!.members[1] as { context_percent?: number }).context_percent).toBe(85);
    // The DOM kept every card.
    [1, 2, 3, 4].forEach((id, index) => expect(card(id), `task ${id}`).toBe(before[index]));
  });

  it("shows a changed task's new status", () => {
    const { store, read, card } = show("board");
    const next = payloads(40, "submitted");
    read(next.team, next.graph);
    expect(store.team("", "t1")!.tasks[1].status).toBe("submitted");
    expect(card(2)).toBeTruthy();
  });
});
