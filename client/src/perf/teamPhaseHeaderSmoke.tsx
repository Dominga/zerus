// Task 196: the plan's phase headers by state and planning mode, on synthetic
// teams only (no CLI, agent, installed app or owner data). Served by Vite
// outside the app entry; scripts/team-phase-header-smoke.mjs opens it in a
// headless browser with its own profile and captures each view over CDP.
import { render } from "solid-js/web";
import { Show } from "solid-js";
import { applySkin, skins } from "../skin/runtime";
import { PlanView } from "../ui/teams/PlanView";
import { PhaseActions } from "../ui/teams/TeamActions";
import type { TeamPhaseView, TeamTaskView, TeamView } from "../types";
import type { PlanGraph } from "../ui/teams/types";
import "../ui/styles/base.css";
import "../ui/styles/app.css";
import "../ui/styles/teams.css";
import "../ui/styles/plan.css";

const now = Date.now() / 1000;
const member = (id: string, role: string, lead = false) => ({
  id, role, agent: "claude", model: "claude-opus", effort: "high", account: "", computer: "", folder: "", state: "active", activity: "idle",
  phase: "", error: "", lead, queue: [], live: true, stuck: false, question: false, host_shared: false, minutes_without_progress: 0, session: null,
});
const task = (id: number, phase: number, title: string, status = "pending", deps: number[] = [], attempts = 0, extra: Partial<TeamTaskView> = {}): TeamTaskView => ({
  id, title, status, assignee: id % 2 ? "api" : "ui", phase, deps: deps.map((dep) => ({ task: dep, hard: true })), estimate_minutes: 30, attempts,
  attempt_state: attempts ? "working" : "", decided_by: "", pinned: false, output: [], waits: "", ...extra,
} as TeamTaskView);
const phase = (n: number, title: string, state: string, extra: Partial<TeamPhaseView> = {}): TeamPhaseView => ({
  n, title, state, status: state === "running" ? "in_work" : state, done: 0, total: 0, barrier: false, auto: false, acceptance: "", paused: false, health: "", ...extra,
});

function team(status: string, phases: TeamPhaseView[], tasks: TeamTaskView[]): TeamView {
  return {
    id: `smoke-${status}`, name: "Checkout (synthetic)", status, project: "example", folder: "C:/work/example", goal: "", plan_mode: "standard", lead: "lead",
    revision: 12, created: now - 7200, updated: now, waiting_for_lead: false, pending_phase: null, settings: {},
    members: [member("lead", "Lead", true), member("api", "Backend"), member("ui", "Frontend")] as never,
    tasks, phases, proposals: [], attention: [], validation: { errors: [], warnings: [] }, health: { critical_path: 0, parallelism: 0, warnings: [] },
    ready: [], cycle: [], draft: null, truncated: false,
  };
}

function graph(view: TeamView): PlanGraph {
  const layer = new Map<number, number>();
  for (const item of view.tasks) layer.set(item.id, item.deps.reduce((most, dep) => Math.max(most, (layer.get(dep.task) ?? 0) + 1), 0));
  return {
    team: view.id, layers: Math.max(1, ...layer.values()) + 1,
    nodes: view.tasks.map((item) => ({ id: item.id, title: item.title, status: item.status, assignee: item.assignee, role: item.assignee === "api" ? "Backend" : "Frontend",
      phase: item.phase, layer: layer.get(item.id) ?? 0, ready: false, in_cycle: false })),
    edges: view.tasks.flatMap((item) => item.deps.map((dep) => ({ from: dep.task, to: item.id, hard: dep.hard }))),
  };
}

// The running team: each header state Qt draws (TeamWorkspace.cpp 2709-3060).
const running = team("running", [
  phase(1, "Foundation", "running", { accepted_by_auto: true, note: "Ship the API first; the UI follows it." }),
  phase(2, "Checkout", "awaiting_acceptance"),
  phase(3, "Payments", "queued", { waiting: "Starts after phase 2 is accepted", owner_accepts: true }),
  phase(4, "Polish", "queued", { paused: true, pause_owner: true, pause_reason: "after the release" }),
  phase(5, "Docs", "draft", { added_by: "Lead", added_at: now - 600, changes: 2 }),
  phase(6, "Telemetry", "rejected", { rejection: "Split the dashboards from the alerts" }),
], [
  task(1, 1, "Order API", "accepted", [], 1), task(2, 1, "Cart UI", "running", [1], 1, { held: true }), task(3, 1, "Old banner", "cancelled"),
  task(4, 2, "Checkout form", "accepted", [], 1), task(5, 2, "Receipts", "accepted", [4], 1),
  task(6, 3, "Card payments", "pending"), task(7, 3, "Refunds", "pending", [6]),
  task(8, 4, "Empty states", "pending"),
  task(9, 5, "User guide", "draft"), task(10, 5, "API reference", "draft", [9]),
  task(11, 6, "Dashboards and alerts", "draft"),
]);

// The planning team: "+ Task", a card's edit, drag to link, an edge's removal.
const planning = team("planning", [phase(1, "Plan", "draft"), phase(2, "Rollout", "draft")], [
  task(1, 1, "Schema", "draft", [], 0, { description: "Tables for orders and carts.", criteria: ["Migrations run twice"] } as never),
  task(2, 1, "Order API", "draft", [1]), task(3, 1, "Cart UI", "draft", [2]),
  task(4, 2, "Feature flag", "draft"), task(5, 2, "Gradual rollout", "draft", [4]),
]);

const params = new URLSearchParams(location.search);
const which = params.get("view") ?? "running";
const shown = which === "planning" ? planning : running;
if (params.get("note") === "0") shown.phases = shown.phases.map((item) => ({ ...item, note: "" }));
applySkin(skins.find((skin) => skin.id === params.get("skin")) ?? skins[0]);
document.documentElement.dataset.platform = "windows";
render(() => (
  <div style={{ height: "100vh", display: "flex", "flex-direction": "column" }}>
    <Show when={which === "planning"}>
      <div class="team-plan-tools" role="toolbar"><button type="button" class="btn small">+ Task</button>
        <span class="dim">Click a task to edit it; drag from a node's right edge onto a task to connect them; click a dependency to remove it.</span></div>
    </Show>
    <div style={{ flex: "1", "min-height": "0", position: "relative" }}>
      <PlanView team={shown} graph={graph(shown)}
        phaseActions={params.get("actions") === "0" ? undefined : (n) => <PhaseActions computer={"" as never} team={shown} phase={n} />}
        editing={which === "planning" ? { canLink: () => true, link: () => {}, unlink: () => {}, editTask: () => {} } : undefined} />
    </div>
  </div>
), document.getElementById("root")!);
