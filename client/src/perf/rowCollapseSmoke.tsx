// Native WebKit regression for zerus-bkbb: collapsing a session row ("Compact
// session row") never shows the expanded layout again once the morph began,
// while the live service keeps upserting rows. The collapsing card is painted
// magenta and the other cards cyan, so menu-smoke.py --paint-column can check
// every presented frame; each frame's layout is also checked here.
// Served by Vite, outside the app entry; IPC is refused. Run:
//   npx vite --port <free port> --strictPort, then
//   python3 scripts/menu-smoke.py --title "Zerus row collapse regression" --paint-column 26 \
//     --url http://127.0.0.1:<port>/scripts/row-collapse-smoke.html
// in its own private Xvfb by default, or with --real-display under
// flock -w 1800 /tmp/zerus-arch-display.lock (the owner's vblank-paced
// WebKitGTK at 144 Hz showed the stale frame; stock WebKitGTK did not).
const native = window as unknown as { __TAURI_INTERNALS__?: { invoke: (command: string) => Promise<unknown> } };
native.__TAURI_INTERNALS__ = { invoke: (command) => Promise.reject(new Error(`synthetic smoke refuses ${command}`)) };

import { render } from "solid-js/web";
import { SessionList } from "../ui/sessions/SessionList";
import { applyBatch } from "../store/catalog";
import "../store/session";
import { emptyBatch, type Batch, type ViewRow } from "../types";
import { applySkin, skins } from "../skin/runtime";
import "../ui/styles/base.css";
import "../ui/styles/app.css";
import "../ui/styles/sessions.css";
import "../ui/styles/detail.css";
import "../ui/styles/client.css";
import "../ui/styles/motion.css";

const sleep = (ms: number) => new Promise((resolve) => setTimeout(resolve, ms));
const frame = () => new Promise<void>((resolve) => requestAnimationFrame(() => resolve()));
const batchOf = (part: Partial<Batch>): Batch => ({ ...emptyBatch(), ...part });
const failures: string[] = [], cases: unknown[] = [];
const KEY = "run:run-3", ID = `\n${KEY}`;
const t0 = Math.floor(Date.now() / 1000) - 600;
const row = (n: number, extra: Partial<ViewRow> = {}): ViewRow => ({ name: `codex/example/session-${n}`, title: `Session ${n}`, agent: "codex", cmd: "codex",
  machine: "workstation", project: "example", folder: `/srv/example/${n}`, cwd: `/srv/example/${n}`, branch: "main", model: "example-model", state: "running",
  tracked: true, run_id: `run-${n}`, conversation_id: `conversation-${n}`, created: t0 + n, status: "working", phase: "working", activity: "busy",
  turn_started: t0 + 300, updated_at: t0 + 100 + n, summary: `Example summary ${n}`, ...extra } as ViewRow);

const style = document.createElement("style");
style.textContent = `
  .sp-scroll .scard { background: #00ffff !important; border-color: #00ffff !important; }
  .sp-scroll .vrow[data-key="${CSS.escape(ID)}"] .scard { background: #ff00ff !important; border-color: #ff00ff !important; }
  .sp-scroll .scard * { color: transparent !important; background: transparent !important; border-color: transparent !important; box-shadow: none !important; }
  .sp-scroll .scard::before { display: none !important; }
`;
document.head.append(style);

const vrow = () => document.querySelector<HTMLElement>(`.sp-scroll .vrow[data-key="${CSS.escape(ID)}"]`)!;
const toggle = () => vrow().querySelector<HTMLElement>(".rowhit")!.dispatchEvent(new MouseEvent("dblclick", { bubbles: true }));

/** The card's height and the next card's top, as laid out in this frame. */
function layout() {
  const card = vrow().querySelector<HTMLElement>(".scard")!.getBoundingClientRect();
  const next = [...document.querySelectorAll<HTMLElement>(".sp-scroll .vrow")].map((el) => el.getBoundingClientRect().top).filter((top) => top > card.top + 1).sort((a, b) => a - b)[0];
  return { height: card.height, next: next ?? 0 };
}

/** Collapse while other rows (and optionally this row) keep changing. */
async function collapse(name: string, sameRow: boolean, morphs: number[]) {
  let n = 0;
  const stop = performance.now() + 700;
  const busy = () => {
    if (performance.now() > stop) return;
    n++;
    applyBatch(batchOf({ upserts: [0, 1, 2, 4, 5, 6].map((k) => ({ computer: "", key: `run:run-${k}`, row: row(k, { summary: `Busy ${n}`, updated_at: t0 + 1000 + n }) })) }));
    requestAnimationFrame(busy);
  };
  requestAnimationFrame(busy);
  let last = layout(), lowest = last, shrinking = false, taller = 0, frames = 0, running = true;
  const start = last.next, nexts = new Set<number>();
  const sample = () => {
    const now = layout();
    frames++;
    nexts.add(Math.round(now.next));
    if (now.height < last.height - 0.5) shrinking = true;
    if (shrinking && (now.height > lowest.height + 0.5 || now.next > lowest.next + 0.5)) taller++;
    lowest = { height: Math.min(lowest.height, now.height), next: Math.min(lowest.next, now.next) };
    last = now;
    if (running) requestAnimationFrame(sample);
  };
  requestAnimationFrame(sample);
  morphs.push(Date.now());
  toggle();
  if (sameRow) { await sleep(90); applyBatch(batchOf({ upserts: [{ computer: "", key: KEY, row: row(3, { summary: "A much longer preview that arrives mid-morph", phase: "tool", updated_at: t0 + 2000 }) }] })); }
  await sleep(900);
  running = false;
  await frame();
  const compact = !!vrow().querySelector(".scard.compact");
  // The rows below glide: several laid-out places between start and end.
  const glided = [...nexts].filter((y) => y < Math.round(start) - 1 && y > Math.round(last.next) + 1).length;
  cases.push({ name, frames, taller, glided, compact, height: Math.round(last.height), next: Math.round(last.next) });
  if (!compact) failures.push(`${name}: the row did not collapse`);
  if (glided < 3) failures.push(`${name}: the rows below snapped (${glided} intermediate places)`);
  if (taller) failures.push(`${name}: ${taller} laid-out frames were taller after the collapse began`);
  toggle(); // Expand again (also a morph) before the next case.
  await sleep(1000);
}

async function run() {
  await document.fonts.ready;
  applySkin(skins[0]);
  document.documentElement.dataset.platform = "linux";
  render(() => <div class="app"><main class="page"><div class="sessions-page"><SessionList /></div></main></div>, document.getElementById("root")!);
  applyBatch(batchOf({ computers: [{ computer: "", status: "live", attempt: 0 }], upserts: Array.from({ length: 14 }, (_, n) => ({ computer: "", key: `run:run-${n}`, row: row(n) })) }));
  await sleep(1500);
  const card = vrow().querySelector<HTMLElement>(".scard")!.getBoundingClientRect();
  const morphs: number[] = [];
  for (let i = 0; i < 3; i++) await collapse(`collapse ${i} while other rows update`, false, morphs);
  for (let i = 0; i < 2; i++) await collapse(`collapse ${i} with a same-row update mid-morph`, true, morphs);
  const report = { failures, cases, morphs, card: { x: card.x, y: card.y, width: card.width, height: card.height }, userAgent: navigator.userAgent };
  (window as unknown as { webkit: { messageHandlers: { result: { postMessage: (text: string) => void } } } }).webkit.messageHandlers.result.postMessage(JSON.stringify(report));
}
void run().catch((error) => {
  (window as unknown as { webkit: { messageHandlers: { result: { postMessage: (text: string) => void } } } }).webkit.messageHandlers.result.postMessage(JSON.stringify({ failures: [String(error)] }));
});
