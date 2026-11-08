// Task 197: the plan's Graph, Board and Layers in a real layout, beside Qt's
// shots of the same team (tray/tests/support/TeamsUiFixture.h prototypeTeam:
// Analysis done, Build running with the backend's question, Verify & ship a
// draft). Synthetic: no CLI, no session, no owner data; the only IPC answered
// is the settings read. Served by Vite:
//   npx vite --port <free port> --strictPort, then open
//   http://127.0.0.1:<port>/scripts/plan-smoke.html?mode=graph|board|layers[&skin=whitewh-light][&member=tester]
// and capture it (Page.captureScreenshot over CDP).
const native = window as unknown as { __TAURI_INTERNALS__?: unknown };
native.__TAURI_INTERNALS__ = {
  transformCallback: () => 1,
  invoke: async (command: string) => {
    if (command === "settings_get") return { values: {}, revision: 0 };
    if (command.startsWith("plugin:event|")) return 1;
    throw new Error(`synthetic smoke refuses ${command}`);
  },
};

import { render } from "solid-js/web";
import { Show } from "solid-js";
import { PlanView } from "../ui/teams/PlanView";
import { TeamFeed, TeamInbox } from "../ui/teams/TeamDrawers";
import { teams } from "../store/teams";
import { applySkin, skins } from "../skin/runtime";
import type { TeamTaskView, TeamView } from "../types";
import type { PlanGraph } from "../ui/teams/types";
import type { PlanMode } from "../ui/teams/planModes";
import "../ui/styles/base.css";
import "../ui/styles/app.css";
import "../ui/styles/teams.css";

const NOW = 1_800_004_000;
const query = new URLSearchParams(location.search);
const skin = skins.find((found) => found.id === (query.get("skin") ?? "whitewh-dark")) ?? skins[0];
applySkin(skin);

function task(id: number, phase: number, title: string, assignee: string, status: string, deps: number[], extra: Partial<TeamTaskView> = {}): TeamTaskView {
  return { id, title, status, assignee, phase, deps: deps.map((dep) => ({ task: dep, hard: true })), estimate_minutes: 0, attempts: 0,
    attempt_state: "", decided_by: "", pinned: false, output: [], waits: "", ...extra };
}

const tasks: TeamTaskView[] = [
  task(1, 1, "Requirements and acceptance", "lead", "accepted", [], { worked_s: 38 * 60, estimate_minutes: 45, estimate_source: "lead" }),
  task(2, 2, "Saved cards API", "backend", "in_progress", [1], { attempts: 1, attempt_state: "delivered", attempt_begun: true,
    running_since: NOW - 22 * 60, estimate_minutes: 40, estimate_source: "role" }),
  task(3, 2, "Card form UI", "frontend", "in_progress", [1], { attempts: 2, attempt_state: "delivered", attempt_begun: true,
    running_since: NOW - 12 * 60, estimate_minutes: 30, estimate_source: "role" }),
  task(4, 2, "Address validation", "frontend", "submitted", [1], { attempts: 1, attempt_state: "reported", worked_s: 25 * 60, estimate_minutes: 30, estimate_source: "role" }),
  task(5, 2, "Contract tests", "tester", "pending", [2], { wait_for: "deps", wait_on: ["2"], estimate_minutes: 45, estimate_source: "lead" }),
  task(6, 2, "E2E checkout flow", "tester", "pending", [3, 5], { wait_for: "deps", wait_on: ["3", "5"], estimate_minutes: 60, estimate_source: "lead" }),
  task(8, 3, "Rate limits on /v1/cards", "backend", "draft", [2]),
  task(9, 3, "Load test checkout", "tester", "draft", [6]),
  task(7, 3, "Release notes", "lead", "draft", [8, 9]),
];
const layer: Record<number, number> = { 1: 0, 2: 1, 3: 1, 4: 1, 5: 2, 6: 3, 8: 2, 9: 4, 7: 5 };
const phase = (n: number, title: string, state: string, status: string) =>
  ({ n, title, state, status, done: 0, total: 0, barrier: false, auto: false, acceptance: "", paused: false, health: "" });

const team = {
  id: "team-1", name: "Checkout", status: "running", project: "Orbit platform", folder: "/work/orbit", goal: "", plan_mode: "standard", lead: "lead",
  revision: 20, created: 1000, updated: 5000, waiting_for_lead: false, pending_phase: 3, settings: { auto_accept: false },
  members: [
    { id: "lead", role: "Lead", agent: "claude", model: "claude-opus-4-6", lead: true, activity: "idle" },
    { id: "backend", role: "Backend", agent: "dsh", model: "deepseek-v4-pro", activity: "busy" },
    { id: "frontend", role: "Frontend", agent: "codex", model: "gpt-6.1-sol", activity: "idle" },
    { id: "tester", role: "Tester", agent: "kimi", model: "kimi-k2.5", activity: "idle" },
  ] as never,
  tasks,
  phases: [phase(1, "Analysis", "approved", "done"), phase(2, "Build", "approved", "in_work"), phase(3, "Verify & ship", "draft", "draft")] as never,
  proposals: [], attention: [], validation: { errors: [], warnings: [] }, health: { critical_path: 0, parallelism: 0, warnings: [] },
  ready: [], cycle: [], draft: null, truncated: false,
} as TeamView;

const graph: PlanGraph = {
  team: "team-1", layers: 6,
  nodes: tasks.map((found) => ({ id: found.id, title: found.title, status: found.status, assignee: found.assignee, role: "", phase: found.phase,
    layer: layer[found.id], ready: false, in_cycle: false })),
  edges: tasks.flatMap((found) => found.deps.map((dep) => ({ from: dep.task, to: found.id, hard: dep.hard }))),
};

// prototypeLive: the backend's session asks for an approval.
const problems = new Map([[2, { tone: "warn" as const, text: "Needs your answer" }]]);

// Task 198 (W1): `drawers=1` adds the lead's Inbox and the team's Feed under the plan, as Qt's workspace.
const drawers = query.get("drawers") === "1";
if (drawers) {
  teams.apply([
    { computer: "", key: "inbox:team-1", kind: "inbox", payload: { team: "team-1", truncated: false, items: [
      { kind: "report", member: "frontend", task: 4, summary: "Address validation is ready for review", since: NOW - 600, command: "hgs team accept --json", detail: {} },
      { kind: "interim_report", member: "backend", task: 2, summary: "Cursor pagination done; card storage next", since: NOW - 300, command: "hgs team inbox", detail: {} },
    ] } },
    { computer: "", key: "feed:team-1", kind: "feed", payload: { team: "team-1", more: true, capped: false, events: [
      { seq: 14, at: NOW - 120, kind: "reported", detail: { member: "frontend", by: "Frontend", task: 4, attempt: 1 } },
      { seq: 13, at: NOW - 300, kind: "interim_report", detail: { member: "backend", by: "Backend", task: 2, attempt: 1, text: "Cursor pagination done" } },
      { seq: 12, at: NOW - 900, kind: "started", detail: { member: "frontend", by: "Frontend", task: 3, attempt: 2 } },
      { seq: 11, at: NOW - 1300, kind: "handed_out", detail: { by: "Zerus", to: "Frontend", task: 3, attempt: 2 } },
      { seq: 10, at: NOW - 1500, kind: "unconfirmed", detail: { member: "tester", by: "Tester", task: 5, error: "the Terminal refused the paste" } },
      { seq: 9, at: NOW - 2000, kind: "phase_approved", detail: { by: "Lead", phase: 2, tasks: 5 } },
    ] } },
  ]);
}

render(() => (
  <div class="team-main" style={{ height: "100%" }}>
    <div class="team-canvas">
      <PlanView team={team} graph={graph} problems={problems} now={() => NOW} initialMode={(query.get("mode") ?? "graph") as PlanMode}
        working={(found) => found.assignee === "backend"} member={query.get("member") ?? undefined}
        onApprovePhase={() => {}} onAutoAccept={() => {}} />
    </div>
    <Show when={drawers}><TeamInbox computer="" team={team} /><TeamFeed computer="" team={team} /></Show>
  </div>
), document.getElementById("root")!);
if (drawers) setTimeout(() => {
  document.querySelector<HTMLButtonElement>("[data-inbox-toggle]")?.click();
  document.querySelector<HTMLButtonElement>("[data-feed-toggle]")?.click();
}, 50);
document.title = `Zerus plan smoke ${query.get("mode") ?? "graph"}: ready`;
