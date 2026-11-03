// Native WebKit regression for zerus-oavj: when an older Activity page lands
// above the reading position, no presented frame shows the reading anchor
// anywhere but where the reader left it. The anchor row is painted magenta,
// the other rows cyan, on black, so menu-smoke.py --paint-column reads the
// anchor's top in every presented frame; each frame's layout is checked here.
// Served by Vite, outside the app entry. Run:
//   npx vite --port <free port> --strictPort, then
//   python3 scripts/menu-smoke.py --title "Zerus Activity older page regression" --paint-column 470 \
//     --url http://127.0.0.1:<port>/scripts/activity-older-smoke.html
// in its own private Xvfb by default, or with --real-display under
// flock -w 1800 /tmp/zerus-arch-display.lock.
import { batch, createSignal } from "solid-js";
import { render } from "solid-js/web";
import { ActivityView } from "../ui/sessions/Activity";
import { loadMarkdown } from "../ui/sessions/markdown";
import { activityReadingRows } from "../ui/sessions/activity-anchor";
import type { ActivitySource } from "../store/activitySource";
import type { ActivityEvent } from "../types";
import { applySkin, skin } from "../skin/runtime";
import "../ui/styles/base.css";
import "../ui/styles/app.css";
import "../ui/styles/detail.css";
import "../ui/styles/client.css";

const failures: string[] = [];
const check = (ok: unknown, message: string) => { if (!ok) failures.push(message); };
const sleep = (ms: number) => new Promise((resolve) => setTimeout(resolve, ms));
const PAGE = 50, TOTAL = 200;
const all = Array.from({ length: TOTAL }, (_, index) => ({ seq: index + 1, type: "AgentMessage", source: "claude_transcript",
  message_id: `older-${index}`, at: 1793000000 + index, detail: `## Response ${index}\n\n` + Array.from({ length: 1 + index % 7 }, (_, block) =>
    `Paragraph ${block}: a variable-height Markdown reply with **emphasis** and enough words to wrap across the reading column.\n\n`
    + (block % 3 === 0 ? "```rust\nfn fixture() {\n    let value = 42;\n}\n```\n\n" : "")).join("") })) as unknown as ActivityEvent[];
const [events, setEvents] = createSignal(all.slice(TOTAL - PAGE));
const [loadingOlder, setLoadingOlder] = createSignal(false);
/** When each older page landed, for the painted check. */
const landings: number[] = [];
const source: ActivitySource = {
  target: () => ({ id: "synthetic-older" }), events, summary: () => ({ conversation_id: "synthetic-older" }), loaded: () => true,
  loadingOlder, olderDone: () => events().length >= TOTAL, ended: () => false, error: () => undefined, readOnly: true,
  loadOlder: () => {
    if (loadingOlder()) return;
    setLoadingOlder(true);
    // An IPC answer arrives as a task, a little later.
    setTimeout(() => {
      landings.push(Date.now());
      batch(() => { setEvents(all.slice(Math.max(0, TOTAL - events().length - PAGE))); setLoadingOlder(false); });
    }, 150);
  },
};

const style = document.createElement("style");
document.head.append(style);
/** Paint the anchor row magenta and the other rows cyan, on black, text hidden. */
const paint = (key: string) => {
  style.textContent = `
    html, body, #root, .act-scroll, .act-col { background: #000 !important; }
    .act-scroll * { color: transparent !important; background: transparent !important; border-color: transparent !important; box-shadow: none !important; text-decoration-color: transparent !important; }
    .act-scroll img, .act-scroll svg, .act-scroll video { visibility: hidden !important; }
    .act-scroll .act-virtual-row { background: #00ffff !important; }
    .act-scroll .act-virtual-row:has(#${CSS.escape(key)}) { background: #ff00ff !important; }
  `;
};

async function run() {
  await loadMarkdown(); applySkin(skin());
  const root = document.getElementById("root")!;
  root.style.cssText = "display:flex;width:940px;height:740px;--col:100%;--content-scale:1";
  render(() => <ActivityView source={source} />, root);
  const scroller = root.querySelector<HTMLElement>(".act-scroll")!;
  const deadline = performance.now() + 10000;
  while (scroller.querySelector<HTMLElement>(".act-col")?.style.visibility !== "visible") {
    if (performance.now() > deadline) throw Error("Activity initial placement deadline");
    await sleep(20);
  }
  const cases: unknown[] = [];
  for (let page = 0; page < (TOTAL - PAGE) / PAGE; page++) {
    // Read near the top, above the automatic older edge, with the end pin released.
    scroller.dispatchEvent(new WheelEvent("wheel", { deltaY: -500 }));
    scroller.scrollTo({ top: 400, behavior: "instant" });
    await sleep(700);
    const anchor = activityReadingRows(scroller).visible()[1];
    check(anchor, `page ${page}: no reading anchor`);
    if (!anchor) break;
    paint(anchor.key);
    await sleep(300);
    const before = activityReadingRows(scroller).offset(anchor.key)!;
    const count = events().length;
    let moved = 0, frames = 0, worst = 0, sampling = true;
    // Sample what each frame painted: after its animation-frame callbacks
    // (Activity's own corrections run there) and the paint, before any other task.
    const sample = () => {
      const offset = activityReadingRows(scroller).offset(anchor.key);
      frames++;
      if (offset === undefined || Math.abs(offset - before) > 1.5) { moved++; worst = Math.max(worst, offset === undefined ? 99999 : Math.abs(offset - before)); }
      if (sampling) requestAnimationFrame(() => setTimeout(sample, 0));
    };
    requestAnimationFrame(() => setTimeout(sample, 0));
    scroller.querySelector<HTMLElement>(".act-read-older")!.click();
    while (events().length === count) await sleep(20);
    await sleep(900);
    sampling = false;
    const after = activityReadingRows(scroller).offset(anchor.key);
    cases.push({ page, anchor: anchor.key, before: Math.round(before), after: after === undefined ? null : Math.round(after), frames, moved, worst });
    check(after !== undefined && Math.abs(after - before) <= 1.5, `page ${page}: the reading anchor moved from ${before} to ${after}`);
    check(moved === 0, `page ${page}: ${moved} laid-out frames showed the anchor away from its place (worst ${worst} px)`);
  }
  const result = { failures, cases, landings, events: events().length };
  (window as unknown as { webkit: { messageHandlers: { result: { postMessage(value: string): void } } } }).webkit.messageHandlers.result.postMessage(JSON.stringify(result));
}
void run().catch((error) => {
  (window as unknown as { webkit: { messageHandlers: { result: { postMessage(value: string): void } } } }).webkit.messageHandlers.result.postMessage(JSON.stringify({ failures: [...failures, String(error)] }));
});
