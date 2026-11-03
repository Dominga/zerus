// Native WebKit regression for zerus-attm: the session panel's slides (dock to
// strip, peek open and close, strip to docked) never present a frame that goes
// back against the slide, such as the full panel again after a closing slide.
// The panel is painted magenta on black, so menu-smoke.py --paint-row reads
// the panel's right edge in every presented frame.
// Served by Vite, outside the app entry; IPC is refused. Run:
//   npx vite --port <free port> --strictPort, then
//   python3 scripts/menu-smoke.py --title "Zerus panel slide regression" --paint-row 300 \
//     --url http://127.0.0.1:<port>/scripts/panel-slide-smoke.html
// in its own private Xvfb by default, or with --real-display under
// flock -w 1800 /tmp/zerus-arch-display.lock (the owner's vblank-paced
// WebKitGTK at 144 Hz presents frames a stock build does not).
const native = window as unknown as { __TAURI_INTERNALS__?: { invoke: (command: string) => Promise<unknown> } };
native.__TAURI_INTERNALS__ = { invoke: (command) => Promise.reject(new Error(`synthetic smoke refuses ${command}`)) };

import { render } from "solid-js/web";
import { SessionList } from "../ui/sessions/SessionList";
import { applyBatch } from "../store/catalog";
import "../store/session";
import { peek, setPeek, stripMode } from "../store/ui";
import { emptyBatch, type Batch, type ViewRow } from "../types";
import { applySkin, skins } from "../skin/runtime";
import "../ui/styles/base.css";
import "../ui/styles/app.css";
import "../ui/styles/sessions.css";
import "../ui/styles/detail.css";
import "../ui/styles/client.css";
import "../ui/styles/motion.css";

const sleep = (ms: number) => new Promise((resolve) => setTimeout(resolve, ms));
const batchOf = (part: Partial<Batch>): Batch => ({ ...emptyBatch(), ...part });
const failures: string[] = [];
const t0 = Math.floor(Date.now() / 1000) - 600;
const row = (n: number, extra: Partial<ViewRow> = {}): ViewRow => ({ name: `codex/example/session-${n}`, title: `Session ${n}`, agent: "codex", cmd: "codex",
  machine: "workstation", project: "example", folder: `/srv/example/${n}`, cwd: `/srv/example/${n}`, branch: "main", model: "example-model", state: "running",
  tracked: true, run_id: `run-${n}`, conversation_id: `conversation-${n}`, created: t0 + n, status: "working", phase: "working", activity: "busy",
  turn_started: t0 + 300, updated_at: t0 + 100 + n, summary: `Example summary ${n}`, ...extra } as ViewRow);

const style = document.createElement("style");
style.textContent = `
  html, body, .app, .page, .sessions-page, .sessions-pane { background: #000 !important; border-color: #000 !important; }
  .sessions-pane * { color: transparent !important; background: transparent !important; border-color: transparent !important; box-shadow: none !important; }
  .sessions-pane .sp-panel { background: #ff00ff !important; border-color: #ff00ff !important; }
  .sessions-pane .sp-panel::after, .sessions-pane .sp-panel *::before, .sessions-pane .sp-panel *::after { display: none !important; }
  .sessions-pane svg, .sessions-pane img { visibility: hidden !important; }
`;
document.head.append(style);

/** Live upserts to other rows on every frame, as a busy service sends them. */
function busy(until: number) {
  let n = 0;
  const step = () => {
    if (performance.now() > until) return;
    n++;
    applyBatch(batchOf({ upserts: [0, 1, 2].map((k) => ({ computer: "", key: `run:run-${k}`, row: row(k, { summary: `Busy ${n}`, updated_at: t0 + 1000 + n }) })) }));
    requestAnimationFrame(step);
  };
  requestAnimationFrame(step);
}

const click = (selector: string) => {
  const button = document.querySelector<HTMLElement>(selector);
  if (!button) throw new Error(`no ${selector}`);
  button.click();
};

async function run() {
  await document.fonts.ready;
  applySkin(skins[0]);
  document.documentElement.dataset.platform = "linux";
  // The rail's grid column and the conversation beside the list, as App lays them out.
  render(() => <div class="app"><div /><main class="page"><div class="sessions-page"><SessionList /><div style={{ flex: "1" }} /></div></main></div>, document.getElementById("root")!);
  applyBatch(batchOf({ computers: [{ computer: "", status: "live", attempt: 0 }], upserts: Array.from({ length: 10 }, (_, n) => ({ computer: "", key: `run:run-${n}`, row: row(n) })) }));
  await sleep(1500);
  const panel = document.querySelector<HTMLElement>(".sp-panel")!.getBoundingClientRect();
  const slides: { at: number; opening: boolean; name: string }[] = [];
  const slide = async (name: string, opening: boolean, act: () => void) => {
    busy(performance.now() + 600);
    slides.push({ at: Date.now(), opening, name });
    act();
    await sleep(900);
  };
  for (let cycle = 0; cycle < 2; cycle++) {
    await slide(`dock to strip ${cycle}`, false, () => click(".sp-panel .sp-head button:nth-of-type(2)"));
    if (!stripMode()) failures.push(`cycle ${cycle}: the panel did not go to the strip`);
    await slide(`peek open ${cycle}`, true, () => setPeek(true));
    await slide(`peek close ${cycle}`, false, () => setPeek(false));
    if (peek()) failures.push(`cycle ${cycle}: peek stayed open`);
    await slide(`strip to docked ${cycle}`, true, () => click(".strip-toggle"));
    if (stripMode()) failures.push(`cycle ${cycle}: the panel did not dock`);
  }
  const report = { failures, slides, panel: { x: panel.x, y: panel.y, width: panel.width, height: panel.height }, userAgent: navigator.userAgent };
  (window as unknown as { webkit: { messageHandlers: { result: { postMessage: (text: string) => void } } } }).webkit.messageHandlers.result.postMessage(JSON.stringify(report));
}
void run().catch((error) => {
  (window as unknown as { webkit: { messageHandlers: { result: { postMessage: (text: string) => void } } } }).webkit.messageHandlers.result.postMessage(JSON.stringify({ failures: [...failures, String(error)] }));
});
