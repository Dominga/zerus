// Native WebKit + production Activity/end-follow/virtualization. Serve via Vite:
// python3 scripts/menu-smoke.py --title "Zerus Activity benchmark regression (synthetic)" \
//   --url http://127.0.0.1:<port>/scripts/activity-bench-smoke.html
import { render } from "solid-js/web";
import Diagnostics from "./Diagnostics";
import { DEFAULT_PLAN, runScenario, type PacingRun } from "./scenarios";
import { ActivityView, following } from "../ui/sessions/Activity";
import { activityGeometry, activityGeometryValid } from "../ui/sessions/activity-virtual";
import { loadMarkdown } from "../ui/sessions/markdown";
import type { ActivitySource } from "../store/activitySource";
import { applySkin, skin } from "../skin/runtime";
import "../ui/styles/base.css";
import "../ui/styles/app.css";
import "../ui/styles/detail.css";
import "../ui/styles/client.css";

const failures: string[] = [];
const check = (ok: unknown, message: string) => { if (!ok) failures.push(message); };
const sleep = (ms: number) => new Promise(resolve => setTimeout(resolve, ms));
const events = Array.from({ length: 100 }, (_, index) => ({ seq: index + 1, type: "AgentMessage", source: "claude_transcript",
  message_id: `bench-${index}`, at: 1793000000 + index, detail: `## Reply ${index}\n\n` + "Variable-height **Markdown** with words that wrap at the reading column.\n\n".repeat(1 + index % 5) }));
const source: ActivitySource = { target: () => ({ id: "synthetic-benchmark" }), events: () => events,
  summary: () => ({ conversation_id: "synthetic-benchmark" }), loaded: () => true, loadingOlder: () => false,
  olderDone: () => true, ended: () => false, loadOlder: () => {}, error: () => undefined, readOnly: true };
const report = (value: unknown) => (window as unknown as { webkit: { messageHandlers: { result: { postMessage(value: string): void } } } }).webkit.messageHandlers.result.postMessage(JSON.stringify(value));

async function run() {
  await loadMarkdown(); applySkin(skin());
  const root = document.getElementById("root")!;
  root.style.cssText = "display:flex;width:940px;height:600px;--col:100%;--content-scale:1";
  render(() => <ActivityView source={source} />, root);
  const scroller = root.querySelector<HTMLElement>(".act-scroll")!;
  const deadline = performance.now() + 10000;
  while (scroller.querySelector<HTMLElement>(".act-col")?.style.visibility !== "visible") {
    if (performance.now() > deadline) throw Error("Activity placement deadline");
    await sleep(20);
  }
  check(following(), "the control did not start with end-follow active");
  const indexes: number[] = [];
  let released = false, maximumMounted = 0;
  const observe = () => {
    const rows = scroller.querySelectorAll<HTMLElement>(".act-virtual-row");
    const first = rows[0]; if (first) indexes.push(Number(first.dataset.index));
    maximumMounted = Math.max(maximumMounted, rows.length); released ||= !following();
  };
  const observer = new MutationObserver(observe); observer.observe(scroller, { childList: true, subtree: true }); observe();
  const measured = await runScenario("activity-scroll", { scrollActivity: () => scroller, scrollList: () => null,
    activityEvents: () => events.length, openSession: async () => {} }, null as never,
    { ...DEFAULT_PLAN, hz: 144, hzSource: "manual", warmupMs: 500, runMs: 4000, runs: 1 });
  observer.disconnect();
  const travel = (measured.runs[0] as PacingRun | undefined)?.scroll_travel;
  check(!measured.skipped, `Activity measurement was skipped: ${measured.skipped}`);
  check(released, "the benchmark never released Activity end-follow");
  check(travel && travel.span_px > scroller.clientHeight / 2, "the benchmark did not traverse the viewport");
  const frames = (measured.runs[0] as PacingRun | undefined)?.frames ?? 0;
  check(travel && travel.total_px / (frames + 1) >= 14, "CSS smooth scrolling replaced the intended 16 px workload");
  check(Math.max(...indexes) - Math.min(...indexes) >= 3, "the benchmark never traversed the mounted virtual range");
  check(maximumMounted < 50, `the benchmark retained ${maximumMounted} of 100 rows`);
  await sleep(400);
  check(activityGeometryValid(activityGeometry(scroller)), "the benchmark left a gap or overlapping rows");
  // The first native rate can be unavailable before mapping. At Run it is known.
  let probes = 0;
  const diagnostics = document.getElementById("diagnostics")!; diagnostics.style.display = "none";
  render(() => <Diagnostics representative={false} scrollActivity={() => scroller} scrollList={() => null} openSession={async () => {}}
    displayHz={async () => ++probes === 1 ? null : 144} />, diagnostics);
  await sleep(1200);
  const budget = await globalThis.zerusPerf!.run({ ids: ["raf-light"], warmupMs: 0, runMs: 100, runs: 1 });
  check(budget.platform.hz === 144 && budget.platform.hz_source === "native", "a startup estimate replaced the known native budget");
  report({ failures, travel, firstIndex: [Math.min(...indexes), Math.max(...indexes)], maximumMounted, budget: budget.platform });
}
void run().catch(error => report({ failures: [...failures, String(error)] }));
