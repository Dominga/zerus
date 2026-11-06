// Native WebKit regression: on macOS and Windows the Tasks board and the
// Projects list never received HTML5 drag events (Tauri's native file-drop
// handler answers every drag), so tasks could not be moved, reordered or
// assigned by drag and projects could not be reordered. There they move by a
// pointer drag (ui/lib/pointerDrag.ts). This page forces that path
// (window.__ZERUS_POINTER_DRAG__) in a real layout and drives pointer events:
// a task reorders, moves to Backlog on its tile, is assigned to a session and
// to Any session on its project header, Escape cancels; with `?projects` a
// project moves before another one. Served by Vite; synthetic IPC records the
// edits and refuses everything else. Run:
//   npx vite --port <free port> --strictPort, then
//   python3 scripts/menu-smoke.py --title "Zerus board pointer drag regression (synthetic)" \
//     --url http://127.0.0.1:<port>/scripts/board-pointer-drag-smoke.html[?projects]
const edits: { command: string; args: Record<string, unknown> }[] = [];
const RECORDED = new Set(["task_board_action", "task_assign", "move_project"]);
const native = window as unknown as { __TAURI_INTERNALS__?: unknown };
native.__TAURI_INTERNALS__ = {
  transformCallback: () => 1,
  invoke: async (command: string, args: Record<string, unknown>) => {
    if (RECORDED.has(command)) {
      edits.push({ command, args });
      return command === "move_project" ? { outcome: "done", conflicts: 0 } : null;
    }
    if (command === "tasks_follow" || command === "cancel_content_search") return null;
    if (command === "pending_read_marks") return [];
    if (command === "attention_marks") return { marks: {}, replies: {} };
    if (command === "settings_get") return { values: {}, revision: 0 };
    if (command === "new_session_context") return { branch: "work-example", own_windows_session: null, build_style: "posix", remembered: { agent: "claude", accounts: {}, open_terminal: false }, swarm: "" };
    if (command.startsWith("plugin:event|")) return 1;
    throw new Error(`synthetic smoke refuses ${command}`);
  },
};

import { Suspense } from "solid-js";
import { render } from "solid-js/web";
import { SessionList } from "../ui/sessions/SessionList";
import { TasksPanel } from "../ui/screens/Tasks";
import { receive } from "../store/session";
import { applyGrouping, emptyGrouping } from "../store/organization";
import { setProjectsPage } from "../store/projects";
import { followProjectTasks } from "../store/taskContext";
import { setPage, setSelectedKey } from "../store/ui";
import { notices } from "../store/notices";
import { POINTER_DRAG } from "../ui/lib/pointerDrag";
import { applySkin, skins } from "../skin/runtime";
import { emptyBatch, emptyProjects, type Batch, type Project, type ViewRow } from "../types";
import "../ui/styles/base.css";
import "../ui/styles/app.css";
import "../ui/styles/sessions.css";
import "../ui/styles/screens.css";
import "../ui/styles/client.css";

const sleep = (ms: number) => new Promise((resolve) => setTimeout(resolve, ms));
const failures: string[] = [];
const check = (condition: unknown, message: string) => { if (!condition) failures.push(message); };
const batchOf = (part: Partial<Batch>): Batch => ({ ...emptyBatch(), ...part });
const post = (value: unknown) => (window as unknown as { webkit: { messageHandlers: { result: { postMessage(v: string): void } } } }).webkit.messageHandlers.result.postMessage(JSON.stringify(value));

const centre = (el: Element, dy = 0) => { const r = el.getBoundingClientRect(); return { x: r.left + r.width / 2, y: r.top + r.height / 2 + dy }; };
const pointer = (target: EventTarget, type: string, at: { x: number; y: number }) =>
  target.dispatchEvent(new PointerEvent(type, { bubbles: true, cancelable: true, pointerId: 9, isPrimary: true, button: 0, buttons: type === "pointerup" ? 0 : 1, clientX: at.x, clientY: at.y }));
async function drag(from: Element, to: { x: number; y: number }, escape = false) {
  const start = centre(from);
  pointer(from, "pointerdown", start);
  for (let i = 1; i <= 6; i++) {
    pointer(window, "pointermove", { x: start.x + ((to.x - start.x) * i) / 6, y: start.y + ((to.y - start.y) * i) / 6 });
    await sleep(16);
  }
  if (escape) window.dispatchEvent(new KeyboardEvent("keydown", { key: "Escape", bubbles: true, cancelable: true }));
  else pointer(window, "pointerup", to);
  await sleep(60);
}
const last = (command: string) => [...edits].reverse().find((e) => e.command === command)?.args;

const PROJECT = "project-1";
const now = Math.floor(Date.now() / 1000);
const agent = { host: "", kind: "agent", machine: "machine-1", resolved: true, session: "claude/example/worker", session_id: "launch_id:worker" };
const task = (uuid: string, id: string, rank: string) => ({ uuid, id, ref: `example/${id}`, project: PROJECT, stage: "todo", text: `Task ${id}\n`, summary: "",
  cancel_reason: "", cancelled_at: null, created_at: 1793202519280, stage_at: 1793216348387, dispatch_id: "", rank,
  target: { kind: "any" }, session: null, author: agent, writable: true, history_revision: "r1" });
const view = (n: number): ViewRow => ({ name: `claude/example/worker-${n}`, title: `Worker ${n}`, agent: "claude", machine: "workstation", project: "example", folder: "/srv/example",
  cwd: "/srv/example", branch: "main", model: "example-model", state: "running", tracked: true, run_id: `run-${n}`, conversation_id: `conversation-${n}`,
  launch_id: `worker-${n}`, updated_at: now + n, status: "ready", phase: "idle", activity: "idle", unread: false, summary: "Example summary" } as ViewRow);
// Two projects, so the session list shows its project headers.
const grouping = { ...emptyGrouping(), loaded: true, ready: true, reason: null, default_project: PROJECT,
  headers: [{ id: PROJECT, name: "example", color: null, accessible: true, collapsed: false, is_default: true, palette: 0 },
    { id: "project-2", name: "other", color: null, accessible: true, collapsed: false, is_default: false, palette: 1 }] };

async function tasksBoard() {
  receive(batchOf({ computers: [{ computer: "", status: "live", attempt: 0 }], upserts: [1, 2, 3].map((n) => ({ computer: "", key: `run:run-${n}`, row: view(n) })),
    groups: [1, 2, 3].map((n) => ({ computer: "", key: `run:run-${n}`, group: n === 3 ? "project-2" : PROJECT, rank: n })), grouping }));
  setSelectedKey("\nrun:run-1");
  render(() => { followProjectTasks(); return <div style={{ display: "flex", height: "100%" }}><div style={{ width: "380px", display: "flex" }}><SessionList /></div>
    <div style={{ width: "460px", display: "flex", "flex-direction": "column" }}><Suspense><TasksPanel /></Suspense></div></div>; }, document.getElementById("root")!);
  receive(batchOf({ tasks: [{ computer: "", key: PROJECT, kind: "list", payload: { project: { id: PROJECT, name: "example" },
    tasks: [task("task-a", "T-aaaaaa", "1"), task("task-b", "T-bbbbbb", "2"), task("task-c", "T-cccccc", "3")] } }] }));
  await sleep(300);
  document.querySelector<HTMLElement>('.tk-tile[data-filter="todo"]')?.click();
  await sleep(200);
  const card = (uuid: string) => document.querySelector<HTMLElement>(`.tk-card[data-task="${uuid}"]`);
  check(card("task-a") && card("task-c"), "the board did not show its To do tasks");
  check(card("task-c")?.getAttribute("draggable") === "false" || card("task-c")?.getAttribute("draggable") === null, "a pointer-drag card must not start an HTML5 drag");
  // Reorder: C before A within To do.
  await drag(card("task-c")!, centre(card("task-a")!, -10));
  check(JSON.stringify(last("task_board_action")) === JSON.stringify({ project: PROJECT, task: "task-c", expectedStage: "todo", action: { kind: "reorder", anchor: "task-a", before: true } }),
    `reorder: ${JSON.stringify(last("task_board_action"))}`);
  // Stage: B onto the Backlog tile.
  await drag(card("task-b")!, centre(document.querySelector('.tk-tile[data-filter="backlog"]')!));
  check(JSON.stringify(last("task_board_action")) === JSON.stringify({ project: PROJECT, task: "task-b", expectedStage: "todo", action: { kind: "to_backlog" } }),
    `backlog tile: ${JSON.stringify(last("task_board_action"))}`);
  // Assign: A onto session 2 in the session list, then onto the project header.
  const session = document.querySelector(`.sp-scroll .vrow[data-key="${CSS.escape("\nrun:run-2")}"] .rowhit`)!;
  await drag(card("task-a")!, centre(session));
  const assigned = last("task_assign") as { input?: Record<string, unknown> } | undefined;
  check(assigned?.input?.task === "task-a" && JSON.stringify(assigned.input.target) === JSON.stringify({ computer: "", name: "claude/example/worker-2", run_id: "run-2", conversation_id: "conversation-2" })
    && assigned.input.session_id === "launch_id:worker-2", `assign to a session: ${JSON.stringify(assigned)}`);
  await drag(card("task-a")!, centre(document.querySelector(`.sp-scroll .vrow[data-key="h:${PROJECT}"] .group-band`)!));
  const any = last("task_assign") as { input?: Record<string, unknown> } | undefined;
  check(edits.filter((e) => e.command === "task_assign").length === 2 && any?.input?.target === null, `assign to Any session: ${JSON.stringify(any)}`);
  // A session of another project is not an assignment target, as for an HTML5 drop.
  const assigns = edits.length;
  await drag(card("task-a")!, centre(document.querySelector(`.sp-scroll .vrow[data-key="${CSS.escape("\nrun:run-3")}"] .rowhit`)!));
  check(edits.length === assigns && !notices().some((n) => n.tone === "error"), "a session of another project was taken as an assignment target");
  // Escape cancels.
  const before = edits.length;
  await drag(card("task-c")!, centre(card("task-a")!, -10), true);
  check(edits.length === before, "Escape still changed a task");
  check(!document.querySelector(".tk-dropline.on"), "the board's drop line stayed after Escape");
}

async function projectsList() {
  const projects: Project[] = ["Example", "Other", "Third"].map((name, index) => ({ id: name.toLowerCase(), name, color: null, accessible: true, is_default: index === 0, palette: index, folders: [] }));
  applyGrouping(batchOf({ grouping: { ...emptyGrouping(), loaded: true, ready: true, reason: null, default_project: "example", headers: projects.map((p) => ({ ...p, collapsed: false })) } }));
  setProjectsPage({ ...emptyProjects(), loaded: true, projects, status: "" });
  setPage("projects");
  const { ProjectsScreen } = await import("../ui/screens/Projects");
  render(() => <div style={{ width: "860px", height: "720px" }}><ProjectsScreen /></div>, document.getElementById("root")!);
  await sleep(300);
  const item = (id: string) => document.querySelector<HTMLElement>(`.proj-list li[data-id="${id}"] .proj-item`);
  check(item("third") && item("example"), "the Projects list did not render");
  check(item("third")?.getAttribute("draggable") === "false" || item("third")?.getAttribute("draggable") === null, "a pointer-drag project must not start an HTML5 drag");
  await drag(item("third")!, centre(item("example")!, -6));
  check(JSON.stringify(last("move_project")) === JSON.stringify({ project: "third", before: "example" }), `project before another: ${JSON.stringify(last("move_project"))}`);
  check(!document.querySelector(".proj-list li.drop-before"), "the drop marker stayed after the drop");
  const before = edits.length;
  await drag(item("other")!, centre(item("example")!, -6), true);
  check(edits.length === before, "Escape still moved a project");
  // Released outside the list: no target, nothing moves.
  await drag(item("other")!, { x: 700, y: 60 });
  check(edits.length === before, "a project moved on a release outside the list");
}

async function run() {
  applySkin(skins[0]);
  document.documentElement.dataset.platform = "macos";
  check(POINTER_DRAG, "the page did not force the pointer path");
  if (location.search.includes("projects")) await projectsList();
  else await tasksBoard();
  post({ failures, edits, mode: location.search.includes("projects") ? "projects" : "tasks" });
}
void run().catch((error) => post({ failures: [String(error)] }));
