// Native WebKit regression: on macOS and Windows Tauri's native file-drop
// handler answers every drag itself, so the session list's HTML5 drag never
// reached the page (a copy cursor, no insertion line, nothing on release).
// There the list organises by a pointer drag (ui/lib/pointerDrag.ts). This
// page forces that path (window.__ZERUS_POINTER_DRAG__) in a real layout and
// drives it with pointer events: a session moves before another one, a project
// before another one, Escape cancels, and the release selects nothing. With
// `?html5` (Linux) the rows stay HTML5-draggable and a pointer drag does nothing.
// Served by Vite, outside the app entry; synthetic IPC records the edits. Run:
//   npx vite --port <free port> --strictPort, then
//   python3 scripts/menu-smoke.py --title "Zerus session pointer drag regression (synthetic)" \
//     --url http://127.0.0.1:<port>/scripts/session-pointer-drag-smoke.html
const edits: { command: string; args: Record<string, unknown> }[] = [];
const native = window as unknown as { __TAURI_INTERNALS__?: unknown };
native.__TAURI_INTERNALS__ = {
  transformCallback: () => 1,
  invoke: async (command: string, args: Record<string, unknown>) => {
    if (command === "move_to_project" || command === "move_project") {
      edits.push({ command, args });
      return { outcome: "done", conflicts: 0 };
    }
    if (command.startsWith("plugin:event|")) return 1;
    throw new Error(`synthetic smoke refuses ${command}`);
  },
};

import { render } from "solid-js/web";
import { SessionList } from "../ui/sessions/SessionList";
import { applyBatch } from "../store/catalog";
import { applyGrouping, emptyGrouping } from "../store/organization";
import { selectedKey } from "../store/ui";
import { POINTER_DRAG } from "../ui/lib/pointerDrag";
import "../store/session";
import { emptyBatch, type Batch, type ViewRow } from "../types";
import { applySkin, skins } from "../skin/runtime";
import "../ui/styles/base.css";
import "../ui/styles/app.css";
import "../ui/styles/sessions.css";
import "../ui/styles/client.css";

const sleep = (ms: number) => new Promise((resolve) => setTimeout(resolve, ms));
const failures: string[] = [];
const check = (condition: unknown, message: string) => { if (!condition) failures.push(message); };
const batchOf = (part: Partial<Batch>): Batch => ({ ...emptyBatch(), ...part });
const t0 = Math.floor(Date.now() / 1000) - 600;
const row = (n: number): ViewRow => ({ name: `codex/example/session-${n}`, title: `Session ${n}`, agent: "codex", cmd: "codex",
  machine: "workstation", project: "example", folder: `/srv/example/${n}`, cwd: `/srv/example/${n}`, branch: "main", model: "example-model", state: "running",
  tracked: true, run_id: `run-${n}`, conversation_id: `conversation-${n}`, created: t0 + n, status: "ready", phase: "idle", activity: "idle",
  updated_at: t0 + 100 + n, summary: `Example summary ${n}` } as ViewRow);
const header = (id: string, name: string, palette: number) => ({ id, name, color: null, accessible: true, collapsed: false, is_default: id === "alpha", palette });

const card = (n: number) => document.querySelector<HTMLElement>(`.sp-scroll .vrow[data-key="${CSS.escape(`\nrun:run-${n}`)}"] .rowhit`);
const band = (id: string) => document.querySelector<HTMLElement>(`.sp-scroll .vrow[data-key="${CSS.escape(`h:${id}`)}"] .group-band`);
const centre = (el: HTMLElement, dy = 0) => { const r = el.getBoundingClientRect(); return { x: r.left + r.width / 2, y: r.top + r.height / 2 + dy }; };
const pointer = (target: EventTarget, type: string, at: { x: number; y: number }) =>
  target.dispatchEvent(new PointerEvent(type, { bubbles: true, cancelable: true, pointerId: 7, isPrimary: true, button: 0, buttons: type === "pointerup" ? 0 : 1, clientX: at.x, clientY: at.y }));
/** Presses `from`, moves in steps to `to`, then releases (or presses Escape there). */
async function drag(from: HTMLElement, to: { x: number; y: number }, escape = false) {
  const start = centre(from);
  pointer(from, "pointerdown", start);
  for (let i = 1; i <= 6; i++) {
    pointer(window, "pointermove", { x: start.x + ((to.x - start.x) * i) / 6, y: start.y + ((to.y - start.y) * i) / 6 });
    await sleep(16);
  }
  const line = !!document.querySelector(".sp-scroll .drop-line, .sp-scroll .drop-box");
  if (escape) window.dispatchEvent(new KeyboardEvent("keydown", { key: "Escape", bubbles: true, cancelable: true }));
  else pointer(window, "pointerup", to);
  // The click a release sends must not select what was dragged.
  from.dispatchEvent(new MouseEvent("click", { bubbles: true, cancelable: true, clientX: to.x, clientY: to.y }));
  await sleep(50);
  return line;
}

async function run() {
  applySkin(skins[0]);
  document.documentElement.dataset.platform = "macos";
  const html5 = location.search.includes("html5");
  check(POINTER_DRAG === !html5, "the page did not choose its drag path");
  const batch = batchOf({ upserts: [1, 2, 3, 4].map((n) => ({ computer: "", key: `run:run-${n}`, row: row(n) })) });
  batch.grouping = { ...emptyGrouping(), loaded: true, ready: true, reason: null, default_project: "alpha", headers: [header("alpha", "Alpha", 0), header("beta", "Beta", 1)] };
  batch.groups = [1, 2, 3].map((n) => ({ computer: "", key: `run:run-${n}`, group: "alpha", rank: n })).concat([{ computer: "", key: "run:run-4", group: "beta", rank: 1 }]);
  applyBatch(batch);
  applyGrouping(batch);
  render(() => <SessionList />, document.getElementById("root")!);
  await sleep(400);
  check(card(3) && card(1) && band("beta") && band("alpha"), "the list did not render its rows");
  if (html5) {
    check(card(3)?.getAttribute("draggable") === "true" && band("beta")?.getAttribute("draggable") === "true", "the Linux rows lost their HTML5 drag");
    await drag(card(3)!, centre(card(1)!, -8));
    check(edits.length === 0, "a pointer drag moved a session on the HTML5 path");
    post({ failures, edits, path: "html5" });
    return;
  }
  check(card(3)?.getAttribute("draggable") === null, "a pointer-drag row must not start an HTML5 drag");
  // Session 3 before session 1 (upper half of session 1's card).
  const lineShown = await drag(card(3)!, centre(card(1)!, -8));
  check(lineShown, "no insertion line while dragging a session");
  check(!document.querySelector(".sp-scroll .drop-line, .sp-scroll .drop-box"), "the insertion line stayed after the drop");
  check(selectedKey() !== "\nrun:run-3", "the release selected the dragged session");
  const moved = edits.find((e) => e.command === "move_to_project");
  check(moved && JSON.stringify(moved.args) === JSON.stringify({ members: ["workstation\ncodex/example/session-3"], project: "alpha", before: "workstation\ncodex/example/session-1" }),
    `session move: ${JSON.stringify(moved?.args)}`);
  // Project Beta before Alpha (upper half of Alpha's header).
  await drag(band("beta")!, centre(band("alpha")!, -6));
  const project = edits.find((e) => e.command === "move_project");
  check(project && JSON.stringify(project.args) === JSON.stringify({ project: "beta", before: "alpha" }), `project move: ${JSON.stringify(project?.args)}`);
  // Escape cancels: no edit, no line.
  const before = edits.length;
  await drag(card(2)!, centre(card(1)!, -8), true);
  check(edits.length === before, "Escape still moved a session");
  check(!document.querySelector(".sp-scroll .drop-line, .sp-scroll .drop-box"), "the insertion line stayed after Escape");
  // A plain click is still a click.
  card(2)!.dispatchEvent(new MouseEvent("click", { bubbles: true, cancelable: true }));
  await sleep(50);
  check(selectedKey() === "\nrun:run-2", "a plain click no longer selects a session");
  post({ failures, edits, path: "pointer" });
}
const post = (value: unknown) => (window as unknown as { webkit: { messageHandlers: { result: { postMessage(v: string): void } } } }).webkit.messageHandlers.result.postMessage(JSON.stringify(value));
void run().catch((error) => post({ failures: [String(error)] }));
