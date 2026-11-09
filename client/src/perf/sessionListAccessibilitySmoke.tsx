// Actual virtualized list on private Xvfb and accessibility buses; synthetic IPC only.
// python3 scripts/menu-smoke.py --accessibility --title "Zerus session list accessibility regression (synthetic)" \
//   --url http://127.0.0.1:<port>/scripts/session-list-accessibility-smoke.html
import { render } from "solid-js/web";
import { emptyBatch, type CreationCard, type ViewRow } from "../types";
import "../ui/styles/base.css";
import "../ui/styles/app.css";
import "../ui/styles/sessions.css";

const failures: string[] = [];
const creation: CreationCard = { id: "launch-fixture", tag: "Preparing review", state: "saving", caption: "Preparing", error: "",
  created: 100, group: "ungrouped", host: "", machine: "Fixture PC", cmd: "codex", account: "", target: "/fixture/next",
  show_pending: true, launch_dispatched: false, stop_requested: false, worktree_receipts: false, can_resume: false, can_edit: false, bound_session: "" };
Object.assign(window, { __TAURI_INTERNALS__: { transformCallback: () => 1, invoke: async (command: string) => {
  if (command.startsWith("plugin:event|")) return 1;
  if (command === "creations_view") return [creation];
  if (command === "pending_read_marks") return [];
  if (command === "attention_marks") return { marks: {}, replies: {} };
  if (command === "settings_get") return { values: {}, revision: 0 };
  if (command === "cancel_content_search") return null;
  throw Error(`Synthetic smoke refuses ${command}`);
} } });
const frame = () => new Promise<void>((resolve) => requestAnimationFrame(() => resolve()));
const check = (ok: unknown, message: string) => { if (!ok) failures.push(message); };
const key = (id: string) => `\nrun:${id}`;
const row = (id: string, title: string, extra: Partial<ViewRow> = {}): ViewRow => ({ name: `codex/example/${id}`, title, agent: "codex",
  machine: "Fixture PC", project: "Example", folder: "/fixture/project", cwd: "/fixture/project", state: "running", tracked: true, process_state: "alive",
  status: "ready", caption: "Ready", activity: "idle", run_id: id, conversation_id: `conversation-${id}`, ...extra });
async function run() {
  // Install synthetic IPC before loading stores whose startup reads use it.
  const { SessionList } = await import("../ui/sessions/SessionList");
  const { applyBatch } = await import("../store/catalog");
  const { applyGrouping, emptyGrouping } = await import("../store/organization");
  const { setCompactRows, setExpandedChildren, setSelectedKey } = await import("../store/ui");
  const { applySkin, skins } = await import("../skin/runtime");
  applySkin(skins[0]);
  const batch = { ...emptyBatch(), computers: [{ computer: "", status: "live", attempt: 0 }], upserts: [
    { computer: "", key: "run:full", row: row("full", "Full") },
    { computer: "", key: "run:compact", row: row("compact", "Compact", { status: "attention", caption: "Needs input" }) },
    { computer: "", key: "run:parent", row: row("parent", "Parent", { subagents: { helper: { name: "Helper", state: "attention", detail: "Synthetic question" } } }) },
  ] };
  batch.grouping = { ...emptyGrouping(), loaded: true, ready: true, reason: null, default_project: "ungrouped", headers: [] };
  applyBatch(batch); applyGrouping(batch);
  setCompactRows(key("compact"), true); setExpandedChildren(key("parent"), true); setSelectedKey(key("full"));
  render(() => <div style={{ height: "100vh", display: "flex" }}><SessionList /></div>, document.getElementById("root")!);
  for (let i = 0; i < 30 && document.querySelectorAll('.sp-scroll [role="treeitem"]').length < 5; i++) await frame();
  const nodes = [...document.querySelectorAll<HTMLElement>('.sp-scroll [role="treeitem"]')];
  const expected = ["Full, Ready, project, Fixture PC", "Compact, Needs input, project, Fixture PC",
    "Parent, Ready, project, Fixture PC, Subagents: 1", "Helper, Needs input, project, Fixture PC",
    "Preparing review, Fixture PC, Preparing, /fixture/next"];
  check(nodes.length === expected.length, `Expected five mounted tree rows, got ${nodes.length}`);
  for (const name of expected) check(nodes.some((node) => node.getAttribute("aria-label") === name), `Missing row name: ${name}`);
  check(nodes.find((node) => node.dataset.key === key("full"))?.getAttribute("aria-selected") === "true", "Selection semantics retained");
  (window as unknown as { webkit: { messageHandlers: { result: { postMessage(text: string): void } } } }).webkit.messageHandlers.result
    .postMessage(JSON.stringify({ failures, accessibility_tree_items: expected, cases: ["full", "compact", "native parent", "native child", "creation", "selected state"] }));
}
void run().catch((error) => (window as unknown as { webkit: { messageHandlers: { result: { postMessage(text: string): void } } } }).webkit.messageHandlers.result
  .postMessage(JSON.stringify({ failures: [String(error)] })));
