// Native WebKit regression for zerus-agv9: once an opened task's text has
// appeared, Task View must not blink. On the arch WebKit (the vblank-patched
// HGS_LINUX_WEBKIT_PREFIX) the end of each content entrance presented one frame
// near its start opacity: every open blinked about 240 ms after the result
// showed, and each refresh replayed the discussion rows' entrances. The task
// content enters without motion; board and catalog updates that leave the task
// unchanged keep every content node, and a refresh keeps the result, the
// description and the existing discussion rows. The result card is the only
// magenta on black, so menu-smoke.py --paint-column fails any presented frame
// of an open that dims it after it was fully painted (`shows`).
// Served by Vite, outside the app entry; synthetic IPC answers the task reads
// and refuses everything else. Run:
//   npx vite --port <free port> --strictPort, then
//   python3 scripts/menu-smoke.py --title "Zerus Task View flicker regression" --paint-column 100 \
//     --url http://127.0.0.1:<port>/scripts/task-view-flicker-smoke.html
// in its own private Xvfb by default; for the arch WebKit, with LD_LIBRARY_PATH
// and WEBKIT_EXEC_PATH set from that prefix as packaging/desktop/linux-launcher.py sets them.
// `?entrance` restores the mockup's entrance: the detector, which blinks there.
import { Suspense, lazy } from "solid-js";
import { render } from "solid-js/web";
import { receive } from "../store/session";
import { followProjectTasks } from "../store/taskContext";
import { openTask, setSelectedTask } from "../store/taskNavigation";
import { setSelectedKey } from "../store/ui";
import { loadMarkdown } from "../ui/sessions/markdown";
import { applySkin, skins } from "../skin/runtime";
import { emptyBatch, type Batch, type ViewRow } from "../types";
import "../ui/styles/base.css";
import "../ui/styles/app.css";
import "../ui/styles/sessions.css";
import "../ui/styles/detail.css";
import "../ui/styles/client.css";
import "../ui/styles/screens.css";
import "../ui/styles/motion.css";

const TaskView = lazy(() => import("../ui/screens/TaskView"));
const sleep = (ms: number) => new Promise((resolve) => setTimeout(resolve, ms));
const frame = () => new Promise<void>((resolve) => requestAnimationFrame(() => resolve()));
const failures: string[] = [], cases: unknown[] = [], commands: Record<string, number> = {};
const check = (condition: unknown, message: string) => { if (!condition) failures.push(message); };
const batchOf = (part: Partial<Batch>): Batch => ({ ...emptyBatch(), ...part });

const PROJECT = "project-1", TASK = "task-1", OTHER = "task-2", ROW = "run:run-1";
const now = Math.floor(Date.now() / 1000);
const agent = { host: "", kind: "agent", machine: "machine-1", resolved: true, session: "claude/example/worker", session_id: "launch_id:worker" };
const SUMMARY = "Done: read and attention marks are flushed on exit with a bounded wait. Part 1 merged in an earlier change; the remaining call reaches main with its branch.\n";
const TEXT = "Tauri client: flush read and attention marks on exit, bounded. The Exit block calls the cheap read-mark flush and a time-limited attention flush, so a mark changed just before quitting is not lost.\n";
const LOG = [
  { actor: agent, at: 1793202519280, kind: "created", session: null },
  { actor: agent, at: 1793202519310, kind: "started", session: agent },
  { actor: agent, at: 1793202639242, kind: "report", session: null, text: "Not done here. Released the claim.\n" },
  { actor: agent, at: 1793202693565, kind: "stage", session: null, text: "todo" },
  { actor: agent, at: 1793216348387, kind: "report", session: null, text: SUMMARY },
];
const comment = (n: number) => ({ actor: { kind: "person" }, at: 1793216400000 + n, kind: "comment", session: null, text: `A note ${n} on this task.`, delivery: { mode: "", state: "note", error: "" } });
const task = (revision: string, uuid = TASK) => ({ uuid, id: uuid === TASK ? "T-abc123" : "T-def456", ref: "example/T-abc123", project: PROJECT, stage: "review", text: TEXT, summary: SUMMARY,
  cancel_reason: "", cancelled_at: null, created_at: 1793202519280, stage_at: 1793216348387, dispatch_id: "started:1793202693632", rank: "3",
  target: { ...agent, kind: "session" }, session: agent, author: agent, writable: true, history_revision: revision });
let log: unknown[] = LOG;
const detail = (uuid: string) => ({ ...task("", uuid), history_revision: undefined, history_schema: 1, history_next: null, log_total: log.length, log, start_confirmed: true, attachments: [] });
const list = (revision: string) => batchOf({ tasks: [{ computer: "", key: PROJECT, kind: "list", payload: { project: { id: PROJECT, name: "example" }, tasks: [task(revision), task("r1", OTHER)] } }] });
function view(updated: number): ViewRow {
  return { name: "claude/example/worker", title: "Worker", agent: "claude", machine: "workstation", project: "example", folder: "/srv/example",
    cwd: "/srv/example", branch: "main", model: "example-model", state: "running", tracked: true, run_id: "run-1", conversation_id: "conversation-1",
    launch_id: "worker", updated_at: updated, status: "ready", phase: "idle", activity: "idle", unread: false, summary: "Example summary" } as ViewRow;
}
const grouping = { loaded: true, default_project: "ungrouped", ready: true, reason: null, busy: false, status: "", status_error: false, conflicts: 0,
  headers: [{ id: PROJECT, name: "example", color: null, accessible: true, collapsed: false, is_default: false, palette: 0 }] };

const OPENS = 8;
/** When each open started (epoch ms), for the painted check. */
const shows: number[] = [], ends: number[] = [];
let entrance = "";
// The result card is the only magenta on black: menu-smoke.py --paint-column
// reads its painted level in every presented frame.
const paint = document.createElement("style");
paint.textContent = `
  html, body, #root, .tab-body, .tv, .tv-scroll { background: #000 !important; }
  .tv *, .tv *::before, .tv *::after { color: transparent !important; background: transparent !important; border-color: transparent !important; box-shadow: none !important; outline: none !important; }
  .tv img, .tv svg { visibility: hidden !important; }
  .tv .tv-result { background: #ff00ff !important; }
`;

const shown = (uuid: string) => document.querySelector(".tv-id")?.textContent === (uuid === TASK ? "T-abc123" : "T-def456") && !!document.querySelector(".tv-result");
let detailsRead = 0;
let callbacks = 0;
const native = window as unknown as { __TAURI_INTERNALS__?: unknown };
native.__TAURI_INTERNALS__ = {
  transformCallback: () => ++callbacks,
  invoke: async (command: string, args: Record<string, unknown>) => {
    commands[command] = (commands[command] ?? 0) + 1;
    if (command === "tasks_follow") return null;
    if (command === "task_details") { detailsRead++; await sleep(90); return detail(String(args.task)); }
    if (command === "task_editor_open") return { project: args.project, task: args.task, mode: args.mode, handle: 1, rev: 0, text: "", position: 0, anchor: 0, files: [], error: "", recovery: false, blocked: false, saved: true };
    if (command === "task_executor_open") return null;
    if (command === "plugin:event|listen") return callbacks;
    if (command === "plugin:event|unlisten") return null;
    throw new Error(`synthetic smoke refuses ${command}`);
  },
};

/** What the viewer sees as one task's content: the nodes, whether they still enter, their opacity. */
const WATCHED = ".tv-result, .tv-doc, .tv-entry, .tv-event, .tv-placeholder, .tv-content";
const describe = (el: Element) => `${el.tagName.toLowerCase()}.${[...el.classList].join(".")}`;
let t0 = 0;
const timeline: string[] = [];
const at = () => Math.round(performance.now() - t0);
document.addEventListener("animationstart", (e) => {
  const el = e.target as Element;
  if (el.closest(".tv-content")) timeline.push(`${at()} animationstart ${describe(el)} ${e.animationName}`);
}, true);

async function sample(name: string, drive: () => Promise<void>, expect: { refresh: boolean; added?: number }) {
  await frame(); await frame();
  const content = document.querySelector(".tv-content");
  // A first note replaces the empty discussion's placeholder, as it should.
  const before = [...document.querySelectorAll(WATCHED)].filter((el) => !expect.added || !el.matches(".tv-placeholder"));
  const reads = detailsRead;
  let frames = 0, dimmedFrames = 0, starts = 0, removed = 0, added = 0, running = true;
  const listener = (e: AnimationEvent) => { if (before.some((el) => el.contains(e.target as Node) && el !== document.querySelector(".tv-content"))) starts++; };
  document.addEventListener("animationstart", listener, true);
  const observer = new MutationObserver((records) => {
    for (const r of records) {
      for (const n of r.removedNodes) if (n instanceof Element && !(expect.added && n.matches(".tv-placeholder")) && (n.matches(WATCHED) || n.querySelector(WATCHED))) { removed++; timeline.push(`${at()} ${name}: removed ${describe(n)}`); }
      for (const n of r.addedNodes) if (n instanceof Element && (n.matches(WATCHED) || n.querySelector(WATCHED))) { added++; timeline.push(`${at()} ${name}: added ${describe(n)}`); }
    }
  });
  observer.observe(document.querySelector(".tv")!, { childList: true, subtree: true });
  // After each paint: an opacity below 1 on content that was already shown is a blink.
  const tick = () => {
    frames++;
    if (before.some((el) => el.isConnected && Number(getComputedStyle(el).opacity) < 0.99)) dimmedFrames++;
    if (running) requestAnimationFrame(() => setTimeout(tick, 0));
  };
  requestAnimationFrame(() => setTimeout(tick, 0));
  await drive();
  await sleep(700);
  running = false; observer.disconnect(); document.removeEventListener("animationstart", listener, true);
  const kept = before.filter((el) => el.isConnected).length;
  const result = { name, frames, reads: detailsRead - reads, kept, of: before.length, removed, added, starts, dimmedFrames, sameContent: content === document.querySelector(".tv-content") };
  cases.push(result);
  check(frames >= 10, `${name}: only ${frames} painted frames`);
  check(expect.refresh ? result.reads === 1 : result.reads === 0, `${name}: ${result.reads} task reads`);
  check(kept === before.length && removed === 0, `${name}: ${before.length - kept} of ${before.length} content nodes replaced (${removed} removals)`);
  check(added === (expect.added ?? 0), `${name}: ${added} content nodes added`);
  check(dimmedFrames === 0, `${name}: ${dimmedFrames} painted frames dimmed content already shown`);
  check(starts === 0, `${name}: ${starts} entrance animations started again on content already shown`);
}

async function run() {
  await loadMarkdown(); await document.fonts.ready;
  applySkin(skins[0]);
  document.documentElement.dataset.platform = "linux";
  const root = document.getElementById("root")!;
  root.style.cssText = "display:flex;flex-direction:column;width:960px;height:760px";
  receive(batchOf({ computers: [{ computer: "", status: "live", attempt: 0 }], upserts: [{ computer: "", key: ROW, row: view(now) }],
    groups: [{ computer: "", key: ROW, group: PROJECT, rank: 0 }], grouping }));
  setSelectedKey(`\n${ROW}`);
  render(() => { followProjectTasks(); return <div class="tab-body" style={{ display: "flex", "flex-direction": "column", flex: "1", "min-height": "0" }}><Suspense><TaskView /></Suspense></div>; }, root);
  receive(list("r1"));
  await sleep(50);

  // Opens: a card click from an empty Task View, and from another task. Each
  // open's painted frames go to menu-smoke.py (`shows`, --paint-column).
  document.head.append(paint);
  const detector = new URLSearchParams(location.search).has("entrance");
  if (detector) {
    const entrance = document.createElement("style");
    entrance.textContent = ".tv .tv-content .act-msg, .tv .tv-content .tv-doc { animation: rise var(--z-motion-slow) var(--ease) both !important; }";
    document.head.append(entrance);
  }
  let openStarts = 0;
  const opening = (e: AnimationEvent) => { if ((e.target as Element).closest(".tv-content")) openStarts++; };
  document.addEventListener("animationstart", opening, true);
  t0 = performance.now();
  for (let open = 0; open < OPENS; open++) {
    const uuid = open % 2 ? OTHER : TASK;
    if (open % 4 === 2) { setSelectedTask(undefined); await sleep(200); }
    shows.push(Date.now());
    timeline.push(`${at()} open ${open}`);
    openTask(PROJECT, uuid);
    const deadline = performance.now() + 5000;
    while (!shown(uuid)) {
      if (performance.now() > deadline) throw new Error(`open ${open}: the task's result never appeared`);
      await sleep(5);
    }
    timeline.push(`${at()} result shown`);
    await sleep(900);
    ends.push(Date.now());
  }
  document.removeEventListener("animationstart", opening, true);
  if (detector) check(openStarts > 0, "the detector's entrance did not run");
  else check(openStarts === 0, `${openStarts} entrance animations ran in opened task content`);
  if (!shown(TASK)) { openTask(PROJECT, TASK); await sleep(900); }
  const card = document.querySelector(".tv-result");
  check(card?.querySelector(".act-body")?.textContent?.includes("bounded wait"), "the result text is missing");
  entrance = card ? getComputedStyle(card).animationName + " " + getComputedStyle(card).animationDuration : "";

  // Live data that leaves the task as it was.
  await sample("unchanged board list", async () => { for (let i = 0; i < 3; i++) { receive(list("r1")); await sleep(120); } }, { refresh: false });
  await sample("catalog row update", async () => { for (let i = 1; i <= 3; i++) { receive(batchOf({ upserts: [{ computer: "", key: ROW, row: view(now + i) }] })); await sleep(120); } }, { refresh: false });
  await sample("task opened again", async () => { openTask(PROJECT, TASK); await sleep(120); }, { refresh: false });
  await sample("history revision with an unchanged task", async () => { receive(list("r2")); }, { refresh: true });
  log = [...LOG, comment(1)];
  await sample("first discussion note", async () => { receive(list("r3")); }, { refresh: true, added: 1 });
  log = [...LOG, comment(1), comment(2)];
  await sample("second discussion note", async () => { receive(list("r4")); }, { refresh: true, added: 1 });

  const result = { failures, cases, commands, detector, entrance, openCount: OPENS, openStarts, shows, ends, timeline: timeline.slice(0, 120) };
  (window as unknown as { webkit: { messageHandlers: { result: { postMessage(value: string): void } } } }).webkit.messageHandlers.result.postMessage(JSON.stringify(result));
}
void run().catch((error) => {
  (window as unknown as { webkit: { messageHandlers: { result: { postMessage(value: string): void } } } }).webkit.messageHandlers.result.postMessage(JSON.stringify({ failures: [...failures, String(error)], cases, commands, timeline }));
});
