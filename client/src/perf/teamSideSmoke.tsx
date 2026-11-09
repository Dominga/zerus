// Task 199: the Team tab's side column (member cards, their menu, the side
// lists), the Tests tab and the team settings on synthetic data only (no CLI,
// agent, installed app or owner data). Served by Vite outside the app entry;
// a headless browser with its own profile captures each view over CDP.
import { render } from "solid-js/web";
import { For, Show } from "solid-js";
import { MemberCard } from "../ui/teams/MemberCard";
import { BuildsPanel, TestsPanel } from "../ui/teams/plan-panels";
import { TeamSettingsDialog } from "../ui/teams/TeamSettingsDialog";
import { memberTree } from "../ui/teams/members";
import { sideLists } from "../ui/teams/sideLists";
import { memberStatus } from "../ui/teams/team-model";
import { applySkin, skins } from "../skin/runtime";
import type { TeamBuildsView, TeamMemberView, TeamTaskView, TeamTestsView, TeamView } from "../types";
import "../ui/styles/base.css";
import "../ui/styles/app.css";
import "../ui/styles/teams.css";
import "../ui/styles/plan.css";

const now = Date.now() / 1000;
const session = (name: string) => ({ host: "", name, run_id: `run-${name}`, conversation_id: `conv-${name}` });
const member = (id: string, role: string, extra: Partial<TeamMemberView> = {}): TeamMemberView => ({
  id, role, agent: "claude", model: "claude-opus", effort: "high", account: "", computer: "", folder: "", state: "active", activity: "idle", phase: "idle",
  error: "", lead: false, session: session(`claude/example/${id}`), live: true, stuck: false, question: false, queue: [], minutes_without_progress: 0,
  host_shared: false, ...extra,
});
const task = (id: number, title: string, assignee: string, status = "pending", extra: Partial<TeamTaskView> = {}): TeamTaskView => ({
  id, title, status, assignee, phase: 1, deps: [], estimate_minutes: 30, attempts: status === "pending" ? 0 : 1, attempt_state: "", decided_by: "", pinned: false,
  output: [], waits: "", ...extra,
} as TeamTaskView);

const team: TeamView = {
  id: "smoke-side", name: "Checkout (synthetic)", status: "running", project: "example", folder: "C:/work/example", goal: "", plan_mode: "standard", lead: "lead",
  revision: 9, created: now - 7200, updated: now, waiting_for_lead: false, pending_phase: null, settings: { max_members: 6, build_broker: true },
  members: [
    member("lead", "Lead", { lead: true, session_account: "native-claude" }),
    member("arch", "Architect", { sub: true, queue: [2] }),
    member("api", "Backend", { parent: "arch", agent: "codex", model: "gpt-5", account: "work", session_account: "work", queue: [1, 3], question: true,
      permission: { question_id: "codex-approval:ab", question_hash: "ab", tool: "Bash", command: "cargo test --locked -p checkout", reason: "Run the suite before the report",
        answer: { question_id: "approval", allow: "1", deny: "3" } } }),
    member("ui", "Frontend", { parent: "arch", draining: true, limited: { at: now - 600, resets_at: now + 3600, detail: "5-hour limit" }, limit_resume: true,
      services: [{ name: "socraticode", since: now - 3000, command: "npx socraticode" }], budget: { label: "vite build", state: "amber", minutes: 15, red_minutes: 30, elapsed_minutes: 18 } }),
    member("qa", "Tester", { agent: "kimi", model: "kimi-k2", session: null, host_shared: false }),
  ],
  tasks: [
    task(1, "Saved cards API", "api", "running", { questions: [{ id: "q1", text: "Which payment API version?", at: now - 300 }] }),
    task(2, "Checkout architecture", "arch", "running"),
    task(3, "Refund webhooks", "api", "pending"),
    task(4, "Receipt emails", "ui", "accepted"),
  ],
  phases: [{ n: 1, title: "Build", state: "running", status: "in_work", done: 1, total: 4, barrier: false, auto: false, acceptance: "", paused: false, health: "" },
    { n: 2, title: "Polish", state: "draft", status: "draft", done: 0, total: 0, barrier: false, auto: false, acceptance: "", paused: false, health: "" }],
  proposals: [], attention: [{ kind: "lead_stalled", summary: "The lead has not planned the next phase for 25 min", task: 2 }],
  intakes: [{ id: "k-7f3a", label: "health watcher", created: now - 86400 }],
  validation: { errors: [], warnings: [] }, health: { critical_path: 0, parallelism: 0, warnings: [] }, ready: [3], cycle: [], draft: null, truncated: false,
};

const run = (id: string, suite: string, status: string, ago: number) => ({ id, suite, status, asked_by: "You", task: 1, at: now - ago, duration_ms: 95000, recorded_ms: 0,
  runner: "", excerpt: "", flags: [], reused_from: "", due: false, over_budget: status === "failed" });
const tests: TeamTestsView = {
  team: team.id, next: 2,
  queue: [{ id: "q1", suite: "client-web", asked_by: "You", state: "waiting", task: 0, forced: true, started: 0, seconds: 40, reason: "" }],
  runs: [run("r1", "client-core", "flaky", 300), run("r2", "client-core", "passed", 1300), run("r3", "client-core", "failed", 2300), run("r4", "hgs", "passed", 900), run("r5", "client-web", "passed", 4000)],
};
const builds: TeamBuildsView = { team: team.id, enabled: true, folders: [{ folder: "C:/work/example", members: ["api", "ui"], build_folders: [{ name: ".", usual_ms: 90000, hit_rate: 0.5,
  running: [{ member: "api", template: "cargo-build", at: now - 30, seconds: 30, usual_ms: 90000, snapshot: true, hit: false, by: "", ms: 0, exit: null, timed_out: false, error: "", coalesced_with: "", waiting_for: [] }],
  queue: [{ member: "ui", template: "cargo-build", at: now - 20, seconds: 20, usual_ms: 0, snapshot: true, hit: false, by: "", ms: 0, exit: null, timed_out: false, error: "", coalesced_with: "api", waiting_for: [] }],
  recent: [{ member: "ui", template: "cargo-build", at: now - 900, seconds: 0, usual_ms: 0, snapshot: true, hit: true, by: "api", ms: 1200, exit: 0, timed_out: false, error: "", coalesced_with: "", waiting_for: [] }] }] }] };

const params = new URLSearchParams(location.search);
const view = params.get("view") ?? "members";
applySkin(skins.find((skin) => skin.id === params.get("skin")) ?? skins[0]);
document.documentElement.dataset.platform = "windows";
const live = { offline: false, attention: false, working: false };
render(() => (
  <div style={{ display: "flex", gap: "16px", padding: "12px", height: "100vh", "box-sizing": "border-box" }}>
    <Show when={view === "members"}>
      <div class="team-side" style={{ width: "420px", "overflow-y": "auto" }}>
        <For each={sideLists(team)}>{(list) => (
          <section class={`team-side-list team-side-${list.id}`} data-list={list.id}>
            <div class="team-section-head team-side-toggle"><b>{list.title}</b><span class="team-side-note warn">{list.note}</span></div>
            <For each={list.rows}>{(row) => (
              <div class={`team-side-row tone-${row.tone}`}><span class="ell grow">{row.text}</span>
                <Show when={row.action}><button type="button" class="btn ghost small">{row.action}</button></Show></div>
            )}</For>
          </section>
        )}</For>
        <div class="team-section-head"><b>Members</b><span class="grow" /><button type="button" class="btn ghost small">+ Member</button></div>
        <div class="team-members-list">
          <For each={memberTree(team).filter((node) => !node.hidden)}>{(node) => (
            <MemberCard computer={"" as never} team={team} member={node.member as TeamMemberView} status={memberStatus(team, node.member as TeamMemberView, live)}
              depth={node.depth} focused={node.member.id === params.get("focus")} onFocus={() => {}} folded={false} onFold={() => {}} onTask={() => {}} />
          )}</For>
        </div>
      </div>
    </Show>
    <Show when={view === "tests"}>
      <div style={{ width: "760px" }}><TestsPanel tests={tests} onMore={() => {}} onForce={async () => ""} /><BuildsPanel builds={builds} /></div>
    </Show>
    <Show when={view === "settings"}>
      <div class="team-tab" style={{ width: "640px", height: "100%" }}><TeamSettingsDialog team={team} onClose={() => {}} /></div>
    </Show>
  </div>
), document.getElementById("root")!);
