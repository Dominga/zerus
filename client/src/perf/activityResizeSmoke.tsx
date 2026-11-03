// menu-smoke.py --real-display --url .../scripts/activity-resize-smoke.html
// Change only Activity's reading width; native allocation is a separate fixture.
import { render } from "solid-js/web";
import { ActivityView } from "../ui/sessions/Activity";
import { loadMarkdown } from "../ui/sessions/markdown";
import { activityReadingRows } from "../ui/sessions/activity-anchor";
import { activityGeometry, activityGeometryValid } from "../ui/sessions/activity-virtual";
import type { ActivitySource } from "../store/activitySource";
import { applySkin, skin } from "../skin/runtime";
import "../ui/styles/base.css";
import "../ui/styles/app.css";
import "../ui/styles/detail.css";
import "../ui/styles/client.css";

const enforce = new URLSearchParams(location.search).has("enforce");
const failures: string[] = [];
const check = (ok: unknown, message: string) => { if (!ok) failures.push(message); };
const frame = () => new Promise<number>(resolve => requestAnimationFrame(resolve));
const sleep = (ms: number) => new Promise(resolve => setTimeout(resolve, ms));
const events = Array.from({ length: 100 }, (_, index) => ({ seq: index + 1, type: "AgentMessage", source: "claude_transcript",
  message_id: `resize-${index}`, at: 1793000000 + index, detail: `## Response ${index}\n\n` + Array.from({ length: 3 + index % 9 }, (_, block) =>
    `Paragraph ${block}: a variable-height Markdown reply with **emphasis**, a [fixture link](https://example.org/) and enough words to wrap across the reading column.\n\n1. First list entry with another line that wraps in the reading column.\n2. Second entry\n   - Nested entry ${index}\n\n` + "```rust\nfn fixture() {\n    let value = 42;\n}\n```\n\n").join("") }));
const source: ActivitySource = { target: () => ({ id: "synthetic-resize" }), events: () => events,
  summary: () => ({ conversation_id: "synthetic-resize" }), loaded: () => true, loadingOlder: () => false,
  olderDone: () => true, ended: () => false, loadOlder: () => {}, error: () => undefined, readOnly: true };

async function run() {
  await loadMarkdown(); applySkin(skin());
  const root = document.getElementById("root")!;
  root.style.cssText = "display:flex;width:940px;height:740px;--col:100%;--content-scale:1";
  const dispose = render(() => <ActivityView source={source} />, root);
  const scroller = root.querySelector<HTMLElement>(".act-scroll")!;
  const deadline = performance.now() + 10000;
  while (scroller.querySelector<HTMLElement>(".act-col")?.style.visibility !== "visible") {
    if (performance.now() > deadline) throw Error("Activity initial placement deadline");
    await sleep(20);
  }
  scroller.dispatchEvent(new WheelEvent("wheel", { deltaY: -500 }));
  scroller.scrollTo({ top: scroller.scrollHeight * .45, behavior: "instant" });
  await sleep(600);
  const anchor = activityReadingRows(scroller).visible()[0];
  check(anchor, "no mid-history reading anchor");
  let maximumMounted = 0, rowReads = 0;
  const rect = Element.prototype.getBoundingClientRect;
  Element.prototype.getBoundingClientRect = function () {
    if (this.classList.contains("act-virtual-row")) rowReads++;
    return rect.call(this);
  };
  const mounted = () => { maximumMounted = Math.max(maximumMounted, scroller.querySelectorAll(".act-virtual-row").length); };
  const observer = new MutationObserver(mounted); observer.observe(scroller, { subtree: true, childList: true });
  const intervals: number[] = [], transitions: unknown[] = [];
  let previous = await frame(), watching = true;
  const observe = (now: number) => { intervals.push(now - previous); previous = now; if (watching) requestAnimationFrame(observe); };
  requestAnimationFrame(observe);
  for (const width of [850, 760, 660, 760, 850, 940, 850, 760, 660, 760, 850, 940]) {
    const before = rowReads;
    root.style.width = `${width}px`;
    await frame(); await frame(); await frame(); mounted();
    transitions.push({ width, rowReads: rowReads - before, mounted: scroller.querySelectorAll(".act-virtual-row").length,
      anchorDrift: anchor ? (activityReadingRows(scroller).offset(anchor.key) ?? Infinity) - anchor.offset : null });
  }
  await sleep(300); watching = false; observer.disconnect();
  const measuredFrames = intervals.slice(), measuredReads = rowReads;
  const finalOffset = anchor && activityReadingRows(scroller).offset(anchor.key);
  const geometry = activityGeometry(scroller);
  check(anchor && finalOffset !== undefined && Math.abs(finalOffset - anchor.offset) <= 1.5, "resize lost the named reading anchor");
  check(activityGeometryValid(geometry), "resize left overlapping rows or a viewport gap");
  if (enforce) check(maximumMounted <= 24, `resize mounted ${maximumMounted} rows instead of the visible window`);
  // Visit a previously off-screen range at a new width: its old heights are
  // estimates until it mounts, then its reading anchor must survive a return.
  root.style.width = "660px"; await sleep(350);
  scroller.dispatchEvent(new WheelEvent("wheel", { deltaY: -500 }));
  scroller.scrollTo({ top: scroller.scrollHeight * .1, behavior: "instant" });
  await sleep(600);
  const lazy = activityReadingRows(scroller).visible()[0];
  check(activityGeometryValid(activityGeometry(scroller)), "lazy measurements left a viewport gap or overlap");
  root.style.width = "940px"; await sleep(350);
  const lazyOffset = lazy && activityReadingRows(scroller).offset(lazy.key);
  check(lazy && lazyOffset !== undefined && Math.abs(lazyOffset - lazy.offset) <= 1.5, "returning to a cached width moved the lazy reading anchor");
  // Settings/column drag can resize the text without changing the viewport.
  root.style.setProperty("--col", "480px"); await sleep(350);
  check(activityGeometryValid(activityGeometry(scroller)), "column-only resize broke virtual geometry");
  Element.prototype.getBoundingClientRect = rect;
  const sorted = measuredFrames.slice().sort((a, b) => a - b);
  dispose();
  const result = { failures, observerWarnings: (window as unknown as { activityResizeObserverWarnings: number }).activityResizeObserverWarnings, rows: 100, maximumMounted, rowReads: measuredReads, transitions, frames: measuredFrames.length,
    frameMs: { p50: sorted[Math.floor(sorted.length * .5)], p95: sorted[Math.floor(sorted.length * .95)], max: sorted.at(-1) } };
  (window as unknown as { webkit: { messageHandlers: { result: { postMessage(value: string): void } } } }).webkit.messageHandlers.result.postMessage(JSON.stringify(result));
}
void run();
