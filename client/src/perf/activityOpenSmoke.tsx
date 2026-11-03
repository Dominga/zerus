// Native WebKit regression for zerus-ig5q: while a session opens, its
// Activity heading is drawn once. The content column stays hidden until it is
// placed, but its heading set `visibility: visible` on itself, which a hidden
// parent does not hide: the loading overlay's heading and the content's were
// both drawn, a few pixels apart. The loading heading is painted magenta and
// the content heading's text box cyan, on black, so menu-smoke.py
// --paint-column fails any presented frame that shows both. Each animation
// frame's layout is checked here too.
// Served by Vite, outside the app entry. Run:
//   npx vite --port <free port> --strictPort, then
//   python3 scripts/menu-smoke.py --title "Zerus Activity open regression" --paint-column 100 \
//     --url http://127.0.0.1:<port>/scripts/activity-open-smoke.html
// in its own private Xvfb by default, or with --real-display under
// flock -w 1800 /tmp/zerus-arch-display.lock (busy loops in parallel give the
// owner's "under load").
import { batch, createSignal } from "solid-js";
import { render } from "solid-js/web";
import { ActivityView } from "../ui/sessions/Activity";
import { loadMarkdown } from "../ui/sessions/markdown";
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
const OPENS = 12;
const page = (session: string) => Array.from({ length: 2 }, (_, index) => ({ seq: index + 1, type: "AgentMessage",
  source: "claude_transcript", message_id: `${session}-${index}`, at: 1793000000 + index,
  detail: `## ${session} reply ${index}\n\nA reply with **emphasis** and enough words to wrap across the reading column.` })) as unknown as ActivityEvent[];
const [target, setTarget] = createSignal({ id: "session-0" });
const [events, setEvents] = createSignal<ActivityEvent[]>(page("session-0"));
const [loaded, setLoaded] = createSignal(true);
const source: ActivitySource = {
  target, events, summary: () => ({ conversation_id: target().id }), loaded,
  loadingOlder: () => false, olderDone: () => true, ended: () => false, error: () => undefined, readOnly: true,
  loadOlder: () => {},
};
/** When each session opened, for the painted check. */
const opens: number[] = [];

const style = document.createElement("style");
style.textContent = `
  html, body, #root, .activity, .act-scroll, .act-col { background: #000 !important; }
  .activity * { color: transparent !important; background: transparent !important; border-color: transparent !important; box-shadow: none !important; text-decoration-color: transparent !important; }
  .activity img, .activity svg, .activity video { visibility: hidden !important; }
  .activity > .act-col > .act-label { background: #ff00ff !important; }
  .act-scroll .act-history-header .act-label > span:first-child { background: #00ffff !important; }
`;

async function run() {
  await loadMarkdown(); applySkin(skin());
  document.head.append(style);
  const root = document.getElementById("root")!;
  root.style.cssText = "display:flex;width:940px;height:740px;--col:100%;--content-scale:1";
  render(() => <ActivityView source={source} />, root);
  const activity = root.querySelector<HTMLElement>(".activity")!;
  const content = () => activity.querySelector<HTMLElement>(".act-scroll > .act-col");
  const placed = () => content()?.style.visibility === "visible";
  const deadline = performance.now() + 10000;
  while (!placed()) {
    if (performance.now() > deadline) throw Error("Activity initial placement deadline");
    await sleep(20);
  }
  // In each animation frame, after Activity's own frame callbacks: both
  // headings visible at once in the layout itself.
  let frames = 0, both = 0, sampling = true;
  const heading = () => activity.querySelector<HTMLElement>(".act-scroll .act-history-header .act-label > span");
  const sample = () => {
    const overlay = activity.querySelector(":scope > .act-col > .act-label");
    const label = heading();
    const shown = !!label && getComputedStyle(label).visibility === "visible";
    frames++;
    if (overlay && shown) both++;
    if (sampling) requestAnimationFrame(() => setTimeout(sample, 0));
  };
  requestAnimationFrame(() => setTimeout(sample, 0));
  for (let open = 1; open <= OPENS; open++) {
    const session = `session-${open}`;
    opens.push(Date.now());
    // A selection switches the target at once; its first read answers later,
    // slower under load, as the owner's "Reading activity…" showed.
    batch(() => { setTarget({ id: session }); setEvents([]); setLoaded(false); });
    await sleep(open % 2 ? 60 : 240);
    batch(() => { setEvents(page(session)); setLoaded(true); });
    const placedBy = performance.now() + 5000;
    while (!placed()) {
      check(performance.now() < placedBy, `open ${open}: never placed`);
      if (performance.now() >= placedBy) break;
      await sleep(10);
    }
    await sleep(400);
  }
  sampling = false;
  check(both === 0, `${both} laid-out frames showed the loading heading and the placed heading together`);
  const result = { failures, opens, frames, layout_both: both };
  (window as unknown as { webkit: { messageHandlers: { result: { postMessage(value: string): void } } } }).webkit.messageHandlers.result.postMessage(JSON.stringify(result));
}
void run().catch((error) => {
  (window as unknown as { webkit: { messageHandlers: { result: { postMessage(value: string): void } } } }).webkit.messageHandlers.result.postMessage(JSON.stringify({ failures: [...failures, String(error)] }));
});
