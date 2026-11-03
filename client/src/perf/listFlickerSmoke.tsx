// Native WebKit regression for zerus-bkbb: the end of a session's compaction
// must not repaint, move or replace any other row of the Sessions list, also
// while a native resnapshot rebuilds the service view of its online source.
// Served by Vite, outside the app entry; every native read is refused. Run:
//   npx vite --port <free port> --strictPort, then
//   python3 scripts/menu-smoke.py --title "Zerus list flicker regression" \
//     --url http://127.0.0.1:<port>/scripts/list-flicker-smoke.html
// in its own private Xvfb by default, or with --real-display under
// flock -w 1800 /tmp/zerus-arch-display.lock.
import { render } from "solid-js/web";
import { SessionList } from "../ui/sessions/SessionList";
import { applyBatch } from "../store/catalog";
import { setSelectedKey } from "../store/ui";
import { emptyBatch, type Batch, type ViewRow } from "../types";
import { applySkin, skins } from "../skin/runtime";
import "../ui/styles/base.css";
import "../ui/styles/app.css";
import "../ui/styles/sessions.css";
import "../ui/styles/detail.css";
import "../ui/styles/client.css";
import "../ui/styles/motion.css";

const native = window as unknown as { __TAURI_INTERNALS__?: { invoke: (command: string) => Promise<unknown> } };
native.__TAURI_INTERNALS__ = { invoke: (command) => Promise.reject(new Error(`synthetic smoke refuses ${command}`)) };

const sleep = (ms: number) => new Promise((resolve) => setTimeout(resolve, ms));
const frame = () => new Promise<void>((resolve) => requestAnimationFrame(() => resolve()));
const failures: string[] = [], cases: unknown[] = [];
const check = (condition: unknown, message: string) => { if (!condition) failures.push(message); };
const batchOf = (part: Partial<Batch>): Batch => ({ ...emptyBatch(), ...part });

const COMPACTING = "run:compact";
const now = Math.floor(Date.now() / 1000);
function view(n: number, extra: Partial<ViewRow> = {}): ViewRow {
  return {
    name: `codex/example/session-${n}`, title: `Session ${n}`, agent: n % 3 ? "codex" : "claude", machine: "workstation",
    project: "example", folder: `/srv/example/${n}`, cwd: `/srv/example/${n}`, branch: "main", model: "example-model",
    state: "running", tracked: true, run_id: `run-${n}`, conversation_id: `conversation-${n}`, updated_at: now - n * 60,
    status: n % 4 === 1 ? "working" : "ready", phase: n % 4 === 1 ? "working" : "idle", activity: n % 4 === 1 ? "busy" : "idle",
    turn_started: n % 4 === 1 ? now - 30 : undefined, unread: n % 5 === 2, summary: `Example summary ${n}`, ...extra,
  } as ViewRow;
}
const rows = (compaction: "compacting" | "done") => Array.from({ length: 24 }, (_, n) => {
  const key = n === 3 ? COMPACTING : `run:run-${n}`;
  const extra: Partial<ViewRow> = n !== 3 ? {} : compaction === "compacting"
    ? { run_id: "compact", status: "working", phase: "compacting", activity: "busy", caption: "Compacting", turn_started: now - 90, compaction_started: now - 20 } as Partial<ViewRow>
    : { run_id: "compact", status: "working", phase: "working", activity: "busy", caption: "Working", turn_started: now - 90 };
  return { computer: "", key, row: view(n, extra) };
});

/** What a viewer sees of each row: its node, place and visible state. */
function snapshot() {
  const out = new Map<string, { node: Element; top: number; look: string }>();
  for (const el of document.querySelectorAll<HTMLElement>(".sp-scroll .vrow")) {
    const key = el.dataset.key ?? "";
    const badge = el.querySelector(".sbadge");
    const style = getComputedStyle(el);
    out.set(key, { node: el, top: el.getBoundingClientRect().top,
      look: `${badge?.className ?? ""}|${badge?.textContent ?? ""}|${style.opacity}|${el.querySelector(".card,.rowhit")?.className ?? ""}` });
  }
  return out;
}

/** Samples every frame while `drive` runs; only the compacting row may change. */
async function sample(name: string, drive: () => Promise<void>, detector = false) {
  await frame(); await frame();
  const before = snapshot();
  let frames = 0, replacedFrames = 0, movedFrames = 0, otherFrames = 0, removals = 0;
  const changedKeys = new Set<string>();
  const observer = new MutationObserver((records) => {
    for (const record of records) for (const n of record.removedNodes) {
      if (n instanceof Element && (n.matches(".vrow, .sbadge, .card") || n.querySelector(".vrow, .card"))) removals++;
    }
  });
  observer.observe(document.querySelector(".sp-scroll")!, { childList: true, subtree: true });
  let running = true;
  const tick = () => {
    const now = snapshot();
    let replaced = false, moved = false, other = false;
    for (const [key, was] of before) {
      const is = now.get(key);
      if (!is || is.node !== was.node) replaced = true;
      else if (Math.abs(is.top - was.top) > 0.1) moved = true;
      if (key !== `\n${COMPACTING}` && is && is.look !== was.look) { other = true; changedKeys.add(key.trim()); }
    }
    frames++; if (replaced) replacedFrames++; if (moved) movedFrames++; if (other) otherFrames++;
    if (running) requestAnimationFrame(tick);
  };
  requestAnimationFrame(tick);
  await drive();
  await sleep(500);
  running = false; observer.disconnect();
  const result = { name, frames, removals, replacedFrames, movedFrames, otherFrames, otherRows: changedKeys.size };
  cases.push(result);
  check(frames >= 10, `${name}: only ${frames} painted frames`);
  check(removals === 0 && replacedFrames === 0, `${name}: removals=${removals}, replaced frames=${replacedFrames}`);
  check(movedFrames === 0, `${name}: ${movedFrames} frames moved rows`);
  // The detector proves this sampler sees a list-wide repaint at all.
  if (detector) check(otherFrames > 0 && changedKeys.size > 4, `${name}: the sampler missed a list-wide repaint`);
  else check(otherFrames === 0, `${name}: ${otherFrames} frames repainted ${changedKeys.size} other rows (${[...changedKeys].slice(0, 4).join(", ")})`);
}

async function run() {
  await document.fonts.ready;
  applySkin(skins[0]);
  document.documentElement.dataset.platform = "linux";
  render(() => <div class="app"><main class="page"><div class="sessions-page"><SessionList /></div></main></div>, document.getElementById("root")!);
  applyBatch(batchOf({ computers: [{ computer: "", status: "live", attempt: 0 }], upserts: rows("compacting") }));
  setSelectedKey(`\n${COMPACTING}`);
  await sleep(400);
  check(document.querySelectorAll(".sp-scroll .vrow").length >= 8, "list rows did not render");

  // A service delta walk: only the compacting row's own upsert.
  await sample("compaction end delta", async () => {
    applyBatch(batchOf({ upserts: rows("done").filter((r) => r.key === COMPACTING) }));
  });
  applyBatch(batchOf({ upserts: rows("compacting").filter((r) => r.key === COMPACTING) }));
  await sleep(300);

  // A native overflow resync at the end of a compaction rebuilds the service
  // view of the online source: a last-known staging walk, then the completed
  // snapshot's baseline, which drops a row that has left. The computer stays
  // Live throughout (desktop_service: an online resnapshot keeps it Live).
  const last = `run:run-23`;
  await sample("compaction end during an online resnapshot", async () => {
    applyBatch(batchOf({ upserts: rows("compacting"), computers: [{ computer: "", status: "live", attempt: 0 }] }));
    await sleep(60);
    applyBatch(batchOf({ upserts: rows("done").filter((r) => r.key !== last) }));
    await frame();
    applyBatch(batchOf({ removes: [{ computer: "", key: last, reason: null }], computers: [{ computer: "", status: "live", attempt: 0 }] }));
  });
  applyBatch(batchOf({ upserts: rows("compacting") }));
  await sleep(300);

  // What the adapter did before: Probing and Reconnecting for that online
  // source repaint every row as offline and back (the reported blink).
  await sample("detector: a brief non-live computer status", async () => {
    applyBatch(batchOf({ computers: [{ computer: "", status: "probing", attempt: 0 }] }));
    await frame();
    applyBatch(batchOf({ computers: [{ computer: "", status: "reconnecting", attempt: 0 }] }));
    await sleep(60);
    applyBatch(batchOf({ upserts: rows("done") }));
    await sleep(60);
    applyBatch(batchOf({ computers: [{ computer: "", status: "live", attempt: 0 }] }));
  }, true);

  const report = { failures, cases, userAgent: navigator.userAgent };
  document.body.textContent = JSON.stringify(report);
  (window as unknown as { webkit: { messageHandlers: { result: { postMessage: (text: string) => void } } } }).webkit.messageHandlers.result.postMessage(JSON.stringify(report));
}
void run().catch((error) => {
  (window as unknown as { webkit: { messageHandlers: { result: { postMessage: (text: string) => void } } } }).webkit.messageHandlers.result.postMessage(JSON.stringify({ failures: [String(error)] }));
});
