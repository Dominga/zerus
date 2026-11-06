// menu-smoke.py --real-display --url .../scripts/activity-resize-smoke.html
// Change only Activity's reading width; native allocation is a separate fixture.
import { render } from "solid-js/web";
import { ActivityView, jumpToLatest } from "../ui/sessions/Activity";
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
// Sample after the frame's ResizeObserver/anchor corrections and paint.
const painted = () => new Promise<void>(resolve => requestAnimationFrame(() => setTimeout(resolve, 0)));
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
  // Keep only the last 22 px of a row visible at the top. Zero resize overscan
  // used to drop this row, leaving a small blank band that a 96 px tolerance hid.
  const top = geometry.visible[0];
  scroller.dispatchEvent(new WheelEvent("wheel", { deltaY: -500 }));
  scroller.scrollTo({ top: scroller.scrollTop + top.bottom - scroller.getBoundingClientRect().top - 22, behavior: "instant" });
  await sleep(600);
  const straddler = activityGeometry(scroller).visible[0];
  check(straddler?.key === top.key && Math.abs(straddler.bottom - scroller.getBoundingClientRect().top - 22) <= 1,
    "continuous drag did not start with a partly visible top row");
  let dragFrames = 0, missingTop = 0, gapFrames = 0, maximumTopGap = 0;
  const sampleDrag = () => {
    const report = activityGeometry(scroller);
    dragFrames++;
    if (!report.visible.some(row => row.key === top.key)) missingTop++;
    if (!activityGeometryValid(report)) gapFrames++;
    maximumTopGap = Math.max(maximumTopGap, report.firstVisibleGap ?? Infinity);
  };
  for (let width = 936; width >= 600; width -= 4) { root.style.width = `${width}px`; await painted(); sampleDrag(); }
  for (let width = 604; width <= 940; width += 4) { root.style.width = `${width}px`; await painted(); sampleDrag(); }
  const settleUntil = performance.now() + 350;
  while (performance.now() < settleUntil) { await painted(); sampleDrag(); }
  check(missingTop === 0, `continuous drag unmounted the partly visible top row in ${missingTop}/${dragFrames} frames`);
  check(gapFrames === 0 && maximumTopGap <= 1, `continuous drag painted ${gapFrames} invalid frames (top gap ${maximumTopGap}px)`);
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
  // Composer/banner height changes must keep ordinary scroll overscan. Count
  // actual DOM row churn through both the resize and the 250 ms quiet period.
  const heightOnly: unknown[] = [];
  for (const position of ["bottom", "middle"]) {
    if (position === "bottom") jumpToLatest();
    else {
      scroller.dispatchEvent(new WheelEvent("wheel", { deltaY: -500 }));
      scroller.scrollTo({ top: scroller.scrollHeight * .45, behavior: "instant" });
    }
    await sleep(600);
    const reading = activityReadingRows(scroller).visible()[0];
    for (const height of [700, 740]) {
      let removed = 0, added = 0;
      const churn = new MutationObserver(records => {
        for (const record of records) {
          for (const node of record.removedNodes) if (node instanceof Element && node.matches(".act-virtual-row")) removed++;
          for (const node of record.addedNodes) if (node instanceof Element && node.matches(".act-virtual-row")) added++;
        }
      });
      churn.observe(scroller, { subtree: true, childList: true });
      root.style.height = `${height}px`;
      await sleep(600); churn.disconnect();
      heightOnly.push({ position, height, removed, added });
      // At most one boundary row may enter/leave the actual visible range.
      check(removed <= 1 && added <= 1, `${position} height-only resize churned ${removed}/${added} rows`);
      check(activityGeometryValid(activityGeometry(scroller)), `${position} height-only resize broke geometry`);
      if (position === "bottom") check(scroller.scrollHeight - scroller.scrollTop - scroller.clientHeight <= 1, "height-only resize lost the end pin");
      else check(reading && Math.abs((activityReadingRows(scroller).offset(reading.key) ?? Infinity) - reading.offset) <= 1.5,
        "height-only resize moved the reading anchor");
    }
  }
  // Settings/column drag can resize the text without changing the viewport.
  root.style.setProperty("--col", "480px"); await sleep(350);
  check(activityGeometryValid(activityGeometry(scroller)), "column-only resize broke virtual geometry");
  // A capped text column still lives in a changing viewport. It must not
  // restore scroll overscan while that viewport is being resized.
  let cappedMaximum = 0;
  const cappedBefore = scroller.querySelectorAll(".act-virtual-row").length;
  for (let n = 1; n <= 8; n++) {
    root.style.width = `${940 - n * 20}px`;
    root.style.height = `${740 - n * 5}px`;
    await frame(); await frame();
    const mounted = [...scroller.querySelectorAll<HTMLElement>(".act-virtual-row")];
    const edge = scroller.getBoundingClientRect();
    const visible = mounted.filter(row => { const r = row.getBoundingClientRect(); return r.bottom > edge.top && r.top < edge.bottom; }).length;
    cappedMaximum = Math.max(cappedMaximum, mounted.length);
    check(mounted.length <= visible + 6, "capped column restored normal overscan during viewport resize");
    check(activityGeometryValid(activityGeometry(scroller)), "capped-column viewport resize broke geometry");
    await sleep(80);
  }
  await sleep(350);
  const cappedAfter = scroller.querySelectorAll(".act-virtual-row").length;
  check(cappedAfter > cappedMaximum, "scroll overscan did not return after viewport resize settled");
  Element.prototype.getBoundingClientRect = rect;
  const sorted = measuredFrames.slice().sort((a, b) => a - b);
  dispose();
  root.style.setProperty("--col", "100%");
  const voiceSource: ActivitySource = { ...source, target: () => ({ id: "synthetic-voice-resize" }),
    events: () => [{ seq: 1, type: "UserPromptSubmit", source: "hgs_voice", message_id: "voice-resize", at: 1793000000,
      voice: { id: "voice-resize", state: "submitted", audio: { duration_ms: 60000, waveform: Array(64).fill(128) }, transcript: "Synthetic transcript" } }],
    summary: () => ({ conversation_id: "synthetic-voice-resize" }) };
  const closeVoice = render(() => <ActivityView source={voiceSource} />, root);
  const voiceDeadline = performance.now() + 5000;
  while (!root.querySelector(".vp")) { if (performance.now() > voiceDeadline) throw Error("voice geometry deadline"); await sleep(20); }
  const voiceWidths: unknown[] = [];
  for (const scale of [0.75, 1, 2]) for (const width of [240, 320, 480, 760, 940]) {
    root.style.width = `${width}px`; root.style.setProperty("--content-scale", String(scale));
    await frame(); await frame();
    const column = root.querySelector<HTMLElement>(".act-col")!.getBoundingClientRect().width;
    const player = root.querySelector<HTMLElement>(".vp")!.getBoundingClientRect().width;
    const expected = Math.max(80, Math.min(420, column - 112));
    voiceWidths.push({ scale, width, column, player, expected });
    check(Math.abs(player - expected) <= 1, `voice player changed geometry at ${width}px / ${scale}`);
  }
  closeVoice();
  const result = { failures, drag: { frames: dragFrames, missingTop, gapFrames, maximumTopGap }, heightOnly, cappedMaximum, cappedBefore, cappedAfter, voiceWidths, observerWarnings: (window as unknown as { activityResizeObserverWarnings: number }).activityResizeObserverWarnings, rows: 100, maximumMounted, rowReads: measuredReads, transitions, frames: measuredFrames.length,
    frameMs: { p50: sorted[Math.floor(sorted.length * .5)], p95: sorted[Math.floor(sorted.length * .95)], max: sorted.at(-1) } };
  (window as unknown as { webkit: { messageHandlers: { result: { postMessage(value: string): void } } } }).webkit.messageHandlers.result.postMessage(JSON.stringify(result));
}
void run();
