// Synthetic WebKit regression with the real Activity virtualizer and Markdown.
// Serve with Vite, then run scripts/menu-smoke.py --title "Zerus Activity reading position regression"
// --url http://127.0.0.1:<port>/scripts/activity-position-smoke.html on its private Xvfb.
import { batch, createSignal } from "solid-js";
import { render } from "solid-js/web";
import { ActivityView, following, jumpToLatest } from "../ui/sessions/Activity";
import { activityReadingRows, type ReadingAnchor } from "../ui/sessions/activity-anchor";
import { loadMarkdown } from "../ui/sessions/markdown";
import type { ActivitySource } from "../store/activitySource";
import type { ActivityEvent } from "../types";
import { applySkin, skin } from "../skin/runtime";
import "../ui/styles/base.css";
import "../ui/styles/app.css";
import "../ui/styles/detail.css";
import "../ui/styles/client.css";

const failures: string[] = [], cases: unknown[] = [];
const check = (ok: unknown, message: string) => { if (!ok) failures.push(message); };
const sleep = (ms: number) => new Promise(resolve => setTimeout(resolve, ms));
const page = (start: number, count = 100): ActivityEvent[] => Array.from({ length: count }, (_, i) => ({
  seq: start + i, type: "AgentMessage", source: "claude_transcript", at: 1793000000 + start + i,
  detail: `## Reply ${start + i}\n\n` + "Variable height **Markdown** content that wraps while reading.\n\n".repeat(1 + i % 7),
}));
const [target, setTarget] = createSignal({ id: "first" });
const [events, setEvents] = createSignal(page(1));
const [loaded, setLoaded] = createSignal(true);
const [loadingOlder, setLoadingOlder] = createSignal(false);
let reads = 0;
const source: ActivitySource = {
  target, events, loaded, loadingOlder, summary: () => undefined,
  olderDone: () => false, ended: () => false, error: () => undefined,
  loadOlder: after => {
    reads++; setLoadingOlder(true);
    setTimeout(() => batch(() => { setEvents([...page((after ?? 0) + 1), ...events()]); setLoadingOlder(false); }), 100);
  },
};

async function run() {
  await loadMarkdown(); applySkin(skin());
  const root = document.getElementById("root")!;
  root.style.cssText = "display:flex;width:940px;height:740px;--col:100%;--content-scale:1";
  let dispose = render(() => <ActivityView source={source} />, root);
  const scroller = () => root.querySelector<HTMLElement>(".act-scroll")!;
  const placed = async () => {
    const deadline = performance.now() + 5000;
    while (scroller().querySelector<HTMLElement>(".act-col")?.style.visibility !== "visible") {
      if (performance.now() > deadline) throw Error("Activity placement deadline");
      await sleep(20);
    }
    await sleep(300);
  };
  const visit = async (id: string, start: number, count = 100) => {
    batch(() => { setTarget({ id }); setEvents([]); setLoaded(false); });
    await sleep(60);
    batch(() => { setEvents(page(start, count)); setLoaded(true); });
    await placed();
  };
  const read = async (top: number) => {
    scroller().dispatchEvent(new WheelEvent("wheel", { deltaY: -500, bubbles: true }));
    scroller().scrollTo({ top, behavior: "instant" });
    await sleep(650);
    const anchor = activityReadingRows(scroller()).visible()[0];
    if (!anchor) throw Error("No reading anchor");
    return anchor;
  };
  const verify = (label: string, anchor: ReadingAnchor) => {
    const offset = activityReadingRows(scroller()).offset(anchor.key);
    cases.push({ label, key: anchor.key, before: anchor.offset, after: offset });
    check(offset !== undefined && Math.abs(offset - anchor.offset) <= 1.5, `${label}: reading anchor moved (${anchor.offset} to ${offset})`);
    check(!following(), `${label}: unexpectedly following`);
  };
  await placed();
  const first = await read(1800);
  await visit("second", 501);
  const second = await read(3100);
  await visit("first", 1, 110); verify("first return with new replies", first);
  setEvents(page(1, 115)); await sleep(400); verify("streaming after return", first);
  await visit("second", 501); verify("second return", second);
  await visit("first", 1001); verify("saved window outside tail", first);
  check(reads === 1, `Expected one saved-window read, got ${reads}`);
  dispose(); root.replaceChildren();
  dispose = render(() => <ActivityView source={source} />, root);
  await placed(); verify("Activity tab remount", first);
  jumpToLatest(); await sleep(400);
  check(following() && scroller().scrollHeight - scroller().scrollTop - scroller().clientHeight <= 1, "Latest did not restore end follow");
  await visit("second", 501);
  await visit("first", 1, 120);
  check(following() && scroller().scrollHeight - scroller().scrollTop - scroller().clientHeight <= 1, "A pinned session did not follow its new end");
  dispose();
  return { failures, cases, reads };
}
const report = (value: unknown) => (window as unknown as { webkit: { messageHandlers: { result: { postMessage(value: string): void } } } }).webkit.messageHandlers.result.postMessage(JSON.stringify(value));
void run().then(report, error => report({ failures: [...failures, String(error)], cases }));
