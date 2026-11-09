// Native WebKit regression for zerus-soudn: the Activity of a session with no
// first page yet shows its pending state where it can be seen and clicked.
// Placement reveals the content column only after a first page, which a shell
// session (it issues no read, zerus-bz54l) and a stalled first read never
// have; their note, Open Terminal and Retry were drawn inside that hidden
// column while "Reading activity…" stayed over it. Here the real ActivityView
// opens a shell row from the real global store, then a stalled live source;
// each state must be visible, its button the element under its own centre,
// and its click must act.
// Served by Vite, outside the app entry. Run:
//   npx vite --port <free port> --strictPort, then
//   python3 scripts/menu-smoke.py --title "Zerus Activity pending regression" \
//     --url http://127.0.0.1:<port>/scripts/activity-pending-smoke.html
// in its own private Xvfb by default.
import { createSignal } from "solid-js";
import { render } from "solid-js/web";
import { ActivityView } from "../ui/sessions/Activity";
import { loadMarkdown, markdownReady } from "../ui/sessions/markdown";
import type { ActivitySource } from "../store/activitySource";
import { applyBatch } from "../store/catalog";
import { detailTab, setDetailTab, setSelectedKey } from "../store/ui";
import { emptyBatch, type ActivityEvent, type ViewRow } from "../types";
import { applySkin, skin } from "../skin/runtime";
import "../ui/styles/base.css";
import "../ui/styles/app.css";
import "../ui/styles/detail.css";
import "../ui/styles/client.css";

const failures: string[] = [];
const check = (ok: unknown, message: string) => { if (!ok) failures.push(message); };
const sleep = (ms: number) => new Promise((resolve) => setTimeout(resolve, ms));
const until = async (done: () => boolean, what: string) => {
  const deadline = performance.now() + 5000;
  while (!done()) {
    if (performance.now() > deadline) throw Error(`${what} did not appear`);
    await sleep(20);
  }
};
const COMPUTER = "fixture-host";

/** Selects one synthetic row of the real catalog, as a click in the list does. */
function select(agent: string, tracked: boolean): string {
  const key = `run:${agent}-${tracked}`;
  const batch = emptyBatch();
  batch.computers = [{ computer: COMPUTER, status: "live", attempt: 0 }];
  batch.upserts = [{ computer: COMPUTER, key, row: { name: `${agent}/fixture/work`, agent, state: "running", status: "ready", tracked, run_id: `${agent}-run`, conversation_id: `${agent}-conversation` } as unknown as ViewRow }];
  applyBatch(batch);
  setSelectedKey(`${COMPUTER}\n${key}`);
  setDetailTab("activity");
  return `${COMPUTER}\n${key}`;
}

function mount(view: () => ReturnType<typeof ActivityView>) {
  const host = document.createElement("div");
  host.style.cssText = "display:flex;width:940px;height:740px;--col:100%;--content-scale:1";
  document.body.append(host);
  const dispose = render(view, host);
  return { host, done: () => { dispose(); host.remove(); } };
}

const text = (host: HTMLElement, wanted: string) => [...host.querySelectorAll<HTMLElement>("b, p, .hint")].find((node) => node.textContent === wanted);
const button = (host: HTMLElement, label: string) => [...host.querySelectorAll<HTMLButtonElement>("button")].find((node) => node.textContent === label && shown(node));
function shown(node: Element | undefined | null): boolean {
  if (!node) return false;
  const style = getComputedStyle(node);
  const box = node.getBoundingClientRect();
  return style.visibility !== "hidden" && style.display !== "none" && box.width > 0 && box.height > 0;
}
/** The button is what a click at its centre reaches. */
function reachable(node: HTMLElement | undefined): boolean {
  if (!node || !shown(node)) return false;
  const box = node.getBoundingClientRect();
  const hit = document.elementFromPoint(box.left + box.width / 2, box.top + box.height / 2);
  return !!hit && node.contains(hit);
}
const reading = (host: HTMLElement) => [...host.querySelectorAll(".hint")].some((node) => node.textContent === "Reading activity…" && shown(node));

async function run() {
  applySkin(skin());
  await loadMarkdown();
  check(markdownReady(), "Markdown never became ready");
  const results: Record<string, unknown> = {};

  // A shell row through the real global live source, which reads nothing.
  select("sh", false);
  const shell = mount(() => <ActivityView />);
  await until(() => !!text(shell.host, "Activity is not recorded for shell sessions. Use Terminal to work in this shell."), "the shell note");
  await sleep(200);
  const terminal = button(shell.host, "Open Terminal");
  results.shell = { note: shown(text(shell.host, "No recorded activity")), reading: reading(shell.host), terminal: reachable(terminal) };
  check(shown(text(shell.host, "No recorded activity")), "the shell note is not visible");
  check(!reading(shell.host), "a shell session still shows Reading activity…");
  check(reachable(terminal), "Open Terminal is not visible and clickable");
  terminal?.click();
  check(detailTab() === "terminal", "Open Terminal did not open the Terminal tab");
  shell.done();

  // A tracked sh row is an agent's: it reads and says so, with no shell note.
  setDetailTab("activity");
  const tracked = select("sh", true);
  const [events, setEvents] = createSignal<readonly ActivityEvent[]>([]);
  const [loaded, setLoaded] = createSignal(false);
  const [stalled, setStalled] = createSignal(false);
  let retries = 0;
  const live: ActivitySource = {
    target: () => ({ id: tracked }), displayKey: () => tracked, events, summary: () => undefined, loaded, stalled,
    retry() { retries++; setStalled(false); setEvents([{ seq: 1, type: "AgentMessage", at: 1, detail: "Synthetic loaded reply" } as unknown as ActivityEvent]); setLoaded(true); },
    loadingOlder: () => false, olderDone: () => true, ended: () => false, loadOlder() {}, error: () => undefined,
  };
  const agent = mount(() => <ActivityView source={live} />);
  await until(() => reading(agent.host), "Reading activity…");
  check(!text(agent.host, "No recorded activity"), "a tracked sh row was treated as a shell session");

  // Its first page does not come: Retry shows and reads again.
  setStalled(true);
  await until(() => !!text(agent.host, "Activity did not load"), "the stalled state");
  await sleep(200);
  const retry = button(agent.host, "Retry");
  results.stalled = { note: shown(text(agent.host, "Activity did not load")), reading: reading(agent.host), retry: reachable(retry) };
  check(shown(text(agent.host, "Activity did not load")), "the stalled state is not visible");
  check(!reading(agent.host), "a stalled read still shows Reading activity…");
  check(reachable(retry), "Retry is not visible and clickable");
  retry?.click();
  check(retries === 1, `Retry ran ${retries} times`);
  await until(() => shown(text(agent.host, "Synthetic loaded reply")), "the loaded reply");
  results.loaded = { reply: true, stalledGone: !text(agent.host, "Activity did not load") };
  check(!shown(text(agent.host, "Activity did not load")), "the stalled state stayed after the page came");
  agent.done();

  post({ failures, results });
}

function post(value: unknown) {
  (window as unknown as { webkit: { messageHandlers: { result: { postMessage(value: string): void } } } }).webkit.messageHandlers.result.postMessage(JSON.stringify(value));
}
void run().catch((error) => post({ failures: [...failures, String(error)] }));
