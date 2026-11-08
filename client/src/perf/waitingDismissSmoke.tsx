// Task 304: Waiting for you with hgs #246's Dismiss and Show dismissed
// (Restore, "still blocks") and hgs #286's escalation and grouped digest, on
// the page and in the Activity strip, beside Qt's task304 test shots.
// Synthetic: no CLI; the Waiting view goes into the teams store directly.
//   http://127.0.0.1:<port>/scripts/waiting-dismiss-smoke.html?skin=whitewh-dark
const native = window as unknown as { __TAURI_INTERNALS__?: unknown };
native.__TAURI_INTERNALS__ = {
  transformCallback: () => 1,
  invoke: async (command: string) => {
    if (command === "settings_get") return { values: {}, revision: 0 };
    if (command.startsWith("plugin:event|")) return 1;
    if (command === "teams_follow") return null;
    throw new Error(`synthetic smoke refuses ${command}`);
  },
};

import "../ui/styles/base.css";
import "../ui/styles/app.css";
import "../ui/styles/sessions.css";
import "../ui/styles/detail.css";
import "../ui/styles/teams.css";
import "../ui/styles/client.css";
import { render } from "solid-js/web";
import { WaitingBanner, WaitingPage } from "../ui/teams/Waiting";
import { teams } from "../store/teams";
import { applySkin, skin } from "../skin/runtime";
import type { TeamWaitingItem, TeamWaitingView } from "../types";

const now = Date.now() / 1000;
const item = (over: Partial<TeamWaitingItem> & Pick<TeamWaitingItem, "id" | "kind" | "title">): TeamWaitingItem =>
  ({ team: "t1", team_name: "Checkout", action: "open_team", since: now - 300, proposal: null, ...over } as TeamWaitingItem);
const digest = item({ id: "t1/lead_changes/digest", kind: "lead_changes", since: now - 60, state: "digest:1",
  title: "The lead changed the plan 4 times inside the approved goal, newest: added #9 API contract tests",
  detail: { changes: [{ id: "lc4", summary: "added #9 API contract tests" }], groups: [
    { kind: "added", count: 2, newest: [{ summary: "added #9 API contract tests" }, { summary: "added #8 card form errors" }] },
    { kind: "moved", count: 1, newest: [{ summary: "moved #3 to phase 2" }] },
    { kind: "cancelled", count: 1, newest: [{ summary: "cancelled #5 legacy export" }] },
  ] } });
const escalated = item({ id: "t1/task_wait_escalated/escalated-4", kind: "task_wait_escalated", since: now - 600, state: "escalated:4",
  title: "#4 Saved cards API still waits after 60 min: unknown model gpt-x (Lead has not acted)" });
const paused = item({ id: "t2/team_paused/1", kind: "team_paused", team: "t2", team_name: "Orbit", action: "resume_team", since: now - 1800, state: "paused:100.0",
  title: "Team paused since 09:12 by You: members get no tasks" });
const plan = item({ id: "t2/plan_draft/plan", kind: "plan_draft", team: "t2", team_name: "Orbit", action: "approve_plan", since: now - 3600, state: "plan:7",
  title: "Plan of 6 tasks waits for Approve & Run", detail: { blocking: true, blocks: "Orbit" } });
const view: TeamWaitingView = { items: [digest, escalated, paused], total: 3, more: false, next: null, dismissed: [plan], dismissed_total: 1 };

async function run() {
  await document.fonts.ready;
  applySkin(skin());
  teams.apply([{ computer: "", key: "waiting", kind: "waiting", payload: view }] as never);
  render(() => (
    <div style={{ display: "grid", "grid-template-rows": "auto 1fr", height: "100vh", background: "var(--z-color-page)" }}>
      <div style={{ padding: "8px 12px", "border-bottom": "1px solid var(--z-color-divider)" }}><WaitingBanner /></div>
      <WaitingPage />
    </div>
  ), document.getElementById("root")!);
  await new Promise((done) => setTimeout(done, 400));
  document.querySelector<HTMLElement>("[data-show-dismissed]")?.click();
  await new Promise((done) => setTimeout(done, 300));
  (window as unknown as { __waiting: unknown }).__waiting = {
    dismiss: document.querySelectorAll("[data-waiting-dismiss]").length,
    groups: [...document.querySelectorAll<HTMLElement>("[data-group]")].map((chip) => chip.textContent),
    restore: document.querySelectorAll("[data-waiting-restore]").length,
    blocks: document.querySelector("[data-still-blocks]")?.textContent ?? "",
    stripHead: document.querySelector(".team-waiting-banner-head .ell")?.textContent ?? "",
  };
  document.title = `${document.title}: ready`;
}
void run().catch((error: Error) => { document.title = `failed: ${String(error)} ${(error.stack ?? "").split("\n").slice(1, 3).join(" | ")}`; });
