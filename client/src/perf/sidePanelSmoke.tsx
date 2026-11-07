// Task 206: the side panel in the Compact skins (one scroll of Qt's collapsible
// sections) and in the others (tabs), beside Qt's ux-6-inspector shots of the
// same session (tray/tests/test_sessionswindow_inspector1.cpp
// inspectorIsAColumnOfCards: codex/hgs/dashboard in /work/orbit on
// feat/checkout-v2, 72k of 200k context, 2 of 5 plan items done) and a team.
// Synthetic: no CLI, no session, no owner data; the only IPC answered is the
// settings read. Served by Vite:
//   http://127.0.0.1:<port>/scripts/side-panel-smoke.html?skin=whitewh-dark|whitewh-light|zerus-dark[&fold=1]
// and captured over CDP (scripts/plan-smoke.mjs <out> <chrome> side-panel).
const native = window as unknown as { __TAURI_INTERNALS__?: unknown };
native.__TAURI_INTERNALS__ = {
  transformCallback: () => 1,
  invoke: async (command: string) => {
    if (command === "settings_get") return { values: { "workspace/inspectorOpen": true, "workspace/inspectorWidth": 400 }, revision: 0 };
    if (command.startsWith("plugin:event|")) return 1;
    throw new Error(`synthetic smoke refuses ${command}`);
  },
};

// The app's style order (main.tsx): base.css declares the cascade layers first.
import "../ui/styles/base.css";
import "../ui/styles/app.css";
import "../ui/styles/sessions.css";
import "../ui/styles/teams.css";
import { render } from "solid-js/web";
import { Inspector } from "../ui/sessions/Inspector";
import { applyBatch } from "../store/catalog";
import { activity } from "../store/session";
import { teams } from "../store/teams";
import { setSelectedKey } from "../store/ui";
import { applySkin, skin } from "../skin/runtime";
import { emptyBatch, type Batch, type TeamTaskView, type TeamView, type ViewRow } from "../types";

const sleep = (ms: number) => new Promise((done) => setTimeout(done, ms));
const NAME = "codex/hgs/dashboard";
const row = {
  name: NAME, title: "Checkout dashboard", agent: "codex", cmd: "codex", machine: "workstation", project: "orbit", folder: "/work/orbit", cwd: "/work/orbit",
  branch: "feat/checkout-v2", model: "gpt-5.5", effort: "high", state: "running", tracked: true, run_id: "run-1",
  conversation_id: "5f0c2a1e-7d3b-4c8e-9a61-2b7e4d9c0f13", created: 1793040000, status: "working", phase: "working", activity: "busy",
  turn_started: 1793050500, updated_at: 1793050800, summary: "Wiring the saved cards API into the dashboard",
} as ViewRow;

const task = (id: number, phase: number, title: string, assignee: string, status: string): TeamTaskView =>
  ({ id, title, status, assignee, phase, deps: [], estimate_minutes: 0, attempts: 0, attempt_state: "", decided_by: "", pinned: false, output: [], waits: "" });
const phase = (n: number, title: string, state: string, done: number, total: number) =>
  ({ n, title, state, status: state, done, total, barrier: false, auto: false, acceptance: "", paused: false, health: "" });
const session = (name: string) => ({ host: "", name, run_id: "", conversation_id: "" });
const member = (id: string, role: string, agent: string, extra: Record<string, unknown> = {}) =>
  ({ id, role, agent, model: "", effort: "", account: "", computer: "", folder: "", state: "active", activity: "idle", phase: "", error: "", lead: false, live: true, stuck: false, question: false, queue: [], minutes_without_progress: 0, host_shared: false, session: session(`${agent}/orbit/${id}`), ...extra });
const team = {
  id: "team-1", name: "Checkout", status: "running", project: "orbit", folder: "/work/orbit", goal: "", plan_mode: "standard", lead: "lead",
  revision: 20, created: 1793040000, updated: 1793050800, waiting_for_lead: false, pending_phase: 3, settings: {},
  members: [
    member("lead", "Lead", "codex", { lead: true, session: session(NAME), activity: "busy", phase: "working" }),
    member("backend", "Backend", "claude", { activity: "busy", phase: "working" }),
    member("frontend", "Frontend", "kimi"),
    member("tester", "Tester", "deepseek"),
  ],
  tasks: [task(1, 1, "Requirements", "lead", "accepted"), task(2, 2, "Saved cards API", "backend", "in_progress"), task(3, 2, "Card form UI", "frontend", "accepted"),
    task(4, 2, "Address validation", "frontend", "submitted"), task(5, 2, "Contract tests", "tester", "pending"), task(6, 3, "Load test", "tester", "draft")],
  phases: [phase(1, "Analysis", "approved", 1, 1), phase(2, "Build", "approved", 1, 4), phase(3, "Verify & ship", "draft", 0, 1)],
  proposals: [], attention: [], validation: { errors: [], warnings: [] }, health: { critical_path: 0, parallelism: 0, warnings: [] },
  ready: [], cycle: [], draft: null, truncated: false,
} as unknown as TeamView;
const summary = {
  id: team.id, name: team.name, status: team.status, project: team.project, folder: team.folder, plan_mode: team.plan_mode, lead: "lead",
  members: team.members.map((m) => ({ id: m.id, role: m.role, agent: m.agent, state: m.state, lead: m.lead, session: m.session })),
  tasks: 6, accepted: 2, submitted: 1, in_review: 0, in_test: 0, attention: 0, open_proposals: 0, waiting_for_lead: false, pending_phase: 3, updated: team.updated, revision: 20,
};

async function run() {
  await document.fonts.ready;
  applySkin(skin());
  render(() => <Inspector available={1100} />, document.getElementById("root")!);
  applyBatch({ ...emptyBatch(), computers: [{ computer: "", status: "live", attempt: 0 }], upserts: [{ computer: "", key: "run:run-1", row }] } as Batch);
  teams.apply([
    { computer: "", key: "list", kind: "list", payload: { teams: [summary], truncated: false } },
    { computer: "", key: `team:${team.id}`, kind: "team", payload: team },
  ]);
  setSelectedKey("\nrun:run-1");
  for (let i = 0; i < 50 && !activity.target(); i++) await sleep(20);
  const target = activity.target();
  if (target) activity.applyActivity([{ computer: target.computer, key: target.key, kind: "state", payload: { rev: 1, value: {
    conversation_id: row.conversation_id, cwd: "/work/orbit",
    session_usage: { context: { used: 72_000, limit: 200_000 } },
    task_lists: { main: { items: [
      { title: "Plan Checkout v2 with the team", status: "completed" }, { title: "Read the payments service", status: "completed" },
      { title: "Wire the saved cards API", status: "in_progress" }, { title: "Card form states", status: "pending" }, { title: "Release notes", status: "pending" },
    ] } },
  } } }] as never);
  await sleep(400);
  // Qt's ux-6-inspector-collapsed: Environment, Context and Team folded by their headers.
  if (new URLSearchParams(location.search).get("fold") === "1") {
    for (const id of ["sec-environment", "sec-context", "sec-team"]) document.querySelector<HTMLButtonElement>(`[data-section="${id}"] .icard-toggle`)?.click();
    await sleep(300);
  }
  document.title = `${document.title}: ready`;
}
void run().catch((error: Error) => { document.title = `failed: ${String(error)} ${(error.stack ?? "").split("\n").slice(1, 4).join(" | ")}`; });
