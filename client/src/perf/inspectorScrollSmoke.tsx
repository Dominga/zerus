// Measurement for zerus-5ffek (owner task T-ug5hxx): the real Inspector's Plan &
// agents tab for a synthetic session with many subagents, scrolled 16 px each
// frame while a few subagents change state every 250 ms, as a live session's
// catalog updates do. Reports frame pacing and how many subagent rows render.
// Served by Vite, outside the app entry; every native read is refused. Run:
//   npx vite --port <free port> --strictPort, then
//   python3 scripts/menu-smoke.py --title "Zerus Plan and agents scroll measurement (synthetic)" \
//     --url "http://127.0.0.1:<port>/scripts/inspector-scroll-smoke.html?agents=300"
// in its own private Xvfb.
import { render } from "solid-js/web";
import { Inspector } from "../ui/sessions/Inspector";
import { applyBatch } from "../store/catalog";
import { setSelectedKey } from "../store/ui";
import { setInspectorOpen, setInspectorTab } from "../store/inspector";
import { emptyBatch, type Batch, type ViewRow } from "../types";
import { applySkin, skins } from "../skin/runtime";
import { recordFrames, stats } from "./pacing";
import "../ui/styles/base.css";
import "../ui/styles/app.css";
import "../ui/styles/detail.css";
import "../ui/styles/client.css";
import "../ui/styles/inspector.css";

const native = window as unknown as { __TAURI_INTERNALS__?: { invoke: (command: string) => Promise<unknown> } };
native.__TAURI_INTERNALS__ = { invoke: (command) => Promise.reject(new Error(`synthetic smoke refuses ${command}`)) };

const params = new URLSearchParams(location.search);
const AGENTS = Number(params.get("agents") ?? 300);
const SECONDS = Number(params.get("seconds") ?? 8);
// One subagent in EVERY works; the others finished, a few with errors.
const EVERY = Number(params.get("every") ?? 6);
const sleep = (ms: number) => new Promise((resolve) => setTimeout(resolve, ms));
const batchOf = (part: Partial<Batch>): Batch => ({ ...emptyBatch(), ...part });
const now = Math.floor(Date.now() / 1000);

/** The lead's subagents at one update: a few change state with each tick. */
function subagents(tick: number) {
  const map: Record<string, unknown> = {};
  for (let i = 0; i < AGENTS; i++) {
    const changing = i % 37 === tick % 37;
    map[`agent-${String(i).padStart(4, "0")}`] = {
      label: "default", state: changing || i % EVERY === EVERY - 1 ? "working" : i % 9 === 3 ? "error" : "completed",
      detail: `Synthetic subagent ${i}: reviewed client/src/ui/sessions/Inspector.tsx and reported the result ${tick % 7}`,
    };
  }
  return map;
}
const view = (tick: number): ViewRow => ({
  name: "codex/example/lead", title: "lead", agent: "codex", machine: "workstation", project: "example",
  folder: "/srv/example", cwd: "/srv/example", branch: "main", model: "example-model", state: "running", tracked: true,
  run_id: "run-lead", conversation_id: "conversation-lead", updated_at: now + tick, status: "working", phase: "working",
  activity: "busy", summary: "Coordinating subagents", subagents: subagents(tick), subagent_counts_complete: true,
} as unknown as ViewRow);

async function measure(scroller: HTMLElement, ticking: boolean) {
  scroller.scrollTop = 0;
  let direction = 1, tick = 0, replaced = 0;
  // Rows the updates rebuilt: removed row nodes while measuring.
  const card = document.getElementById("icard-plan-agents");
  const observer = new MutationObserver((records) => { for (const r of records) r.removedNodes.forEach((n) => { if (n instanceof HTMLElement && n.classList.contains("irow")) replaced++; }); });
  if (card) observer.observe(card, { childList: true, subtree: true });
  const timer = ticking ? setInterval(() => applyBatch(batchOf({ upserts: [{ computer: "", key: "run:lead", row: view(++tick) }] })), 250) : undefined;
  const deltas = await recordFrames(SECONDS * 1000, {
    onFrame: () => {
      const max = scroller.scrollHeight - scroller.clientHeight;
      if (scroller.scrollTop + 16 * direction > max || scroller.scrollTop + 16 * direction < 0) direction = -direction;
      scroller.scrollTop += 16 * direction;
    },
  });
  if (timer) clearInterval(timer);
  observer.disconnect();
  const s = stats(deltas, 1000 / 60);
  const round = (n: number) => Math.round(n * 100) / 100;
  return { ticking, updates: tick, rows_rebuilt: replaced, frames: s.count, mean: round(s.mean), median: round(s.median), p95: round(s.p95), p99: round(s.p99),
    over_1_5T: round(s.over1_5T * 100), over_2_5T: round(s.over2_5T * 100) };
}

async function run() {
  const failures: string[] = [];
  await document.fonts.ready;
  applySkin(skins[0]);
  document.documentElement.dataset.platform = "linux";
  render(() => <div class="app" style={{ height: "760px", display: "flex", "justify-content": "flex-end" }}><Inspector available={1000} /></div>, document.getElementById("root")!);
  applyBatch(batchOf({ computers: [{ computer: "", status: "live", attempt: 0 }], upserts: [{ computer: "", key: "run:lead", row: view(0) }] }));
  setSelectedKey("\nrun:lead");
  setInspectorTab("plan");
  setInspectorOpen(true);
  await sleep(600);
  const scroller = document.querySelector<HTMLElement>(".insp-scroll");
  if (!scroller) failures.push("the Inspector did not render");
  const rows = () => document.querySelectorAll("#icard-plan-agents .irow").length;
  const report = { agents: AGENTS, rendered_rows: rows(), scroll_height: scroller?.scrollHeight ?? 0, still: {}, ticking: {}, failures };
  if (scroller) {
    report.still = await measure(scroller, false);
    report.ticking = await measure(scroller, true);
    // The fold, where it exists: unfold shows one step, the next shows more.
    const more = () => [...document.querySelectorAll<HTMLButtonElement>("#icard-plan-agents .isub-more")];
    if (more().length) {
      const folded = rows();
      more()[0].click(); await sleep(100);
      const first = rows();
      if (first - folded !== Math.min(50, AGENTS - folded)) failures.push(`unfolding showed ${first - folded} rows`);
      if (AGENTS - folded > 50) { more().at(-1)!.click(); await sleep(100); if (rows() <= first) failures.push("Show more added no rows"); }
      Object.assign(report, { folded_rows: folded, unfolded_rows: first, after_more_rows: rows() });
    }
  }
  (window as unknown as { webkit: { messageHandlers: { result: { postMessage: (text: string) => void } } } }).webkit.messageHandlers.result.postMessage(JSON.stringify(report));
}
void run().catch((error) => (window as unknown as { webkit: { messageHandlers: { result: { postMessage: (text: string) => void } } } }).webkit.messageHandlers.result.postMessage(JSON.stringify({ failures: [String(error)] })));
