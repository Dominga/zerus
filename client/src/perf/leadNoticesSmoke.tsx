// Task 244: a team lead's session with its team notices in full, folded
// (the default) or hidden (`?mode=full|collapsed|silent`), beside Qt's
// lead-notices shots. Built on the task 208 page below.
// Task 208: the Activity view's system notices beside Qt's activity-whitewh
// shot (tray/tests/test_activityview.cpp whiteWhTranscriptReadsFlat): the
// lead's hand-off, the owner's request, tool rows and the agent's reply, then a
// Zerus doorbell and a member's report. A team or Zerus notice is a grey card
// with its sender and a coloured left bar; the reply is plain text; the
// owner's message keeps its bubble. Synthetic: a read-only source, no CLI, no
// session, no owner data; the only IPC answered is the settings read. Served by Vite:
//   http://127.0.0.1:<port>/scripts/activity-notices-smoke.html?skin=whitewh-dark|whitewh-light
// and captured over CDP (scripts/plan-smoke.mjs <out> <chrome> activity-notices).
const native = window as unknown as { __TAURI_INTERNALS__?: unknown };
native.__TAURI_INTERNALS__ = {
  transformCallback: () => 1,
  invoke: async (command: string) => {
    if (command === "settings_get") return { values: { "teams/leadNotices": new URLSearchParams(location.search).get("mode") ?? "collapsed" }, revision: 0 };
    if (command.startsWith("plugin:event|")) return 1;
    throw new Error(`synthetic smoke refuses ${command}`);
  },
};

// The app's style order (main.tsx): base.css declares the cascade layers first.
import "../ui/styles/base.css";
import "../ui/styles/app.css";
import "../ui/styles/sessions.css";
import "../ui/styles/detail.css";
import "../ui/styles/client.css";
import { createSignal } from "solid-js";
import { render } from "solid-js/web";
import { ActivityView } from "../ui/sessions/Activity";
import { loadMarkdown } from "../ui/sessions/markdown";
import type { ActivitySource } from "../store/activitySource";
import type { ActivityEvent } from "../types";
import { applySkin, skin } from "../skin/runtime";
import { applyBatch, catalog } from "../store/catalog";
import { teams } from "../store/teams";
import { setSetting } from "../store/settings";
import { emptyBatch, type Batch, type ViewRow } from "../types";

const at = 1793050800;
const event = (seq: number, type: string, detail: string, tool?: string, extra: Record<string, unknown> = {}) =>
  ({ seq, type, detail, tool, at: at + seq * 20, ...extra }) as unknown as ActivityEvent;
const handOff = "[Zerus team \u00b7 from: Lead \u00b7 task: #3 \u00b7 attempt: 2]\n"
  + "Team: Checkout. Your role: Backend.\n\nTask #3: Saved cards API\nKeep everything from attempt 1 and fix cursor pagination.";
const events = [
  event(0, "UserPromptSubmit", handOff),
  event(1, "UserPromptSubmit", "Add cursor pagination to GET /cards."),
  event(2, "PreToolUse", "src/api/cards.ts", "Read"), event(3, "PostToolUse", "src/api/cards.ts", "Read"),
  event(4, "PreToolUse", "src/api/cards.ts", "Edit"), event(5, "PostToolUse", "src/api/cards.ts", "Edit"),
  event(6, "PreToolUse", "pnpm test cards", "Bash"), event(7, "PostToolUse", "23 passed", "Bash"),
  event(8, "Stop", "Adding cursor pagination to `GET /cards` and scoping lookups by user.\n\n```ts\nconst page = await cards.list({ cursor, limit: 20 });\n```\n\nTests: 23 passed."),
  event(9, "UserPromptSubmit", "[Zerus team] Your build (task 3 cards) for task #3 Saved cards API has finished. Read its result, then go on with the task."),
  event(10, "UserPromptSubmit", "Message from claude/orbit/tester", undefined, { origin: "peer_message", sender: "claude/orbit/tester",
    report: "[Zerus team \u00b7 from: Tester \u00b7 task: #3 \u00b7 attempt: 2]\nReport for task #3 Saved cards API: the cursor tests pass; one flaky retry on CI.\n\nDecide with hgs team accept" }),
  event(11, "Stop", "Thanks, the flaky retry is the CI cache; nothing to change here."),
];
const [target] = createSignal({ id: "session-notices" });
const source: ActivitySource = {
  target, events: () => events, summary: () => ({ conversation_id: "session-notices" }), loaded: () => true,
  loadingOlder: () => false, olderDone: () => true, ended: () => false, error: () => undefined, readOnly: true,
  loadOlder: () => {},
} as unknown as ActivitySource;

async function run() {
  await document.fonts.ready;
  await loadMarkdown(); applySkin(skin());
  const root = document.getElementById("root")!;
  root.style.cssText = "display:flex;width:760px;height:1100px;--col:100%;--content-scale:1";
  // The lead's own session: the catalog row and its team's list name it the lead.
  const lead = { name: "claude/orbit/lead", title: "Checkout lead", agent: "claude", cmd: "claude", machine: "workstation", folder: "/work/orbit", cwd: "/work/orbit",
    state: "running", tracked: true, run_id: "run-lead", conversation_id: "conversation-lead", status: "idle", activity: "idle", team_lead: true } as ViewRow;
  applyBatch({ ...emptyBatch(), computers: [{ computer: "", status: "live", attempt: 0 }], upserts: [{ computer: "", key: "run:run-lead", row: lead }] } as Batch);
  teams.apply([{ computer: "", key: "list", kind: "list", payload: { truncated: false, teams: [{ id: "t1", name: "Checkout", status: "running", project: "", folder: "", plan_mode: "", lead: "lead",
    members: [{ id: "lead", role: "Lead", agent: "claude", state: "active", lead: true, session: { host: "", name: "claude/orbit/lead", run_id: "run-lead", conversation_id: "" } }],
    tasks: 3, accepted: 1, submitted: 0, in_review: 0, in_test: 0, attention: 0, open_proposals: 0, waiting_for_lead: false, pending_phase: null, updated: 0, revision: 1 }] } }]);
  await setSetting("teams/leadNotices", new URLSearchParams(location.search).get("mode") ?? "collapsed").catch(() => {});
  const row = () => catalog.rows["\nrun:run-lead"];
  render(() => <ActivityView source={source} session={row} />, root);
  const deadline = performance.now() + 10000;
  while (root.querySelector<HTMLElement>(".act-scroll > .act-col")?.style.visibility !== "visible") {
    if (performance.now() > deadline) throw Error("Activity was never placed");
    await new Promise((done) => setTimeout(done, 20));
  }
  // The view follows the newest item; show the transcript from its start, as Qt's shot does.
  root.querySelector<HTMLElement>(".act-scroll")?.scrollTo({ top: 0 });
  await new Promise((done) => setTimeout(done, 400));
  document.title = `${document.title}: ready`;
}
void run().catch((error: Error) => { document.title = `failed: ${String(error)} ${(error.stack ?? "").split("\n").slice(1, 3).join(" | ")}`; });
