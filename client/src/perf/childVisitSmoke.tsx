// The real Activity, child store, composer and catalog stores in WebKit, with
// synthetic rows and child pages. zerus-isrzi: a brief child visit keeps the
// parent's handoff receipt, without showing it in a child. zerus-cpt2k: going
// offline during a child's read shows why at once, and starts no second read.
// No CLI, native call or session is used. Serve Vite, then:
//   python3 scripts/menu-smoke.py --title "Zerus child Activity visit regression (synthetic)" \
//     --url http://127.0.0.1:<port>/scripts/child-visit-smoke.html
// runs in a private Xvfb.
import { createEffect, createRoot, createSignal } from "solid-js";
import { render } from "solid-js/web";
import { ActivityView } from "../ui/sessions/Activity";
import { loadMarkdown } from "../ui/sessions/markdown";
import { applyBatch, computerOffline, toSession } from "../store/catalog";
import { composer } from "../store/composer";
import { draftKey } from "../store/draftKeys";
import { createSubagentActivity, SUBAGENT_POLL_MS, type SubagentActivityTarget } from "../store/subagentActivity";
import { selected, setSelectedKey, setSubagentOpen, subagentOpen } from "../store/ui";
import type { ActivitySource } from "../store/activitySource";
import { emptyBatch, type ActivityEvent, type ComposerView, type LocalMessage } from "../types";
import type { SubagentInspection } from "../bridge";
import { applySkin, skin } from "../skin/runtime";
import "../ui/styles/base.css";
import "../ui/styles/app.css";
import "../ui/styles/detail.css";
import "../ui/styles/client.css";

const failures: string[] = [];
const check = (ok: unknown, message: string) => { if (!ok) failures.push(message); };
const until = async (ok: () => unknown, message: string, ms = 2000) => {
  const deadline = performance.now() + ms;
  while (!ok()) {
    if (performance.now() > deadline) { failures.push(`${message}: ${JSON.stringify(document.querySelector(".activity")?.textContent?.slice(-240))}`); return; }
    await new Promise((resolve) => requestAnimationFrame(resolve));
  }
};
const settle = () => new Promise((resolve) => setTimeout(resolve, 120));

const key = "run:run-1", run = "run-1", conversation = "conversation-1";
const row = { name: "codex/example/review", agent: "codex", state: "running", status: "ready", run_id: run, conversation_id: conversation, cwd: "/example/review" };
const session = toSession("", key, row);
const live = (status: string) => applyBatch({ ...emptyBatch(), computers: [{ computer: "", status, attempt: 0 }] } as Parameters<typeof applyBatch>[0]);

const receipt = (id: string, text: string): LocalMessage => ({ id, text, status: "sent", attachments: [], run_id: run, conversation_id: conversation,
  submitted_at: 1_793_000_000, error: "", uncertain: false, message_id: null, submitted_text: null, source: null, voice: null });
const reference = (child?: string) => ({ computer: session.computer, key: session.catalogKey, name: session.name, archive_id: null, session_id: session.sessionId ?? null,
  ...(child ? { subagent: { agent_id: child, parent_conversation_id: conversation } } : {}) });
const composerView = (local: LocalMessage[], child?: string) => ({ handle: 1, key: draftKey(reference(child)), loaded: true, rev: 1, text_epoch: 1, text: "", position: 0, anchor: 0,
  question: null, input_visible: !child, queue: null, attachments: [], removed: null, attaching: false, sending: false, waiting: null, uncertain: false, delivery_pending: false,
  status: "", status_error: false, can_send: false, send_label: { source: "Send" }, available: true, unavailable_reason: "", block_reason: null, attachments_available: true,
  attachments_reason: "", session_status: "", session_status_error: false, session_blocks_input: false, inspect_error: false,
  cache: { visible: false, tone: "quiet", label: "", short_label: "" }, cache_warning: "", cache_details: "",
  context: { visible: false, text: "", tooltip: "", warning: false, tone: "quiet", percent: null, accessible_name: "" }, local,
  cancel_send: false, compacting: false, subagent: child ? { agent_id: child, send_supported: true, hint: "" } : undefined,
  clear: { enabled: false, dismiss: false, title: "", text: "", confirm: "", run, conversation, cache_enabled: false, running: false, reason: "", cache_reason: "" } } as unknown as ComposerView);

// The parent: a live source whose journal this page controls.
const [journal, setJournal] = createSignal<ActivityEvent[]>([]);
const parent: ActivitySource = { target: () => ({ id: session.key }), events: journal, summary: () => ({}), loaded: () => true,
  loadingOlder: () => false, olderDone: () => true, ended: () => false, error: () => undefined, loadOlder: () => {} };

// The child: the real store, as session.ts wires it, with its reads and timers held by this page.
const page = (agent: string, detail: string, seq = 1): SubagentInspection => ({ agent_id: agent, conversation_id: `${conversation}/${agent}`, parent_conversation_id: conversation, run_id: run,
  send_supported: true, history_scope: "Native subagent history", history_truncated: false, events: [{ seq, type: "Stop", source: "subagent_summary", detail, at: 1_793_000_000 + seq }] });
const reads: Array<{ agent: string; answer(value: SubagentInspection): void }> = [];
const timers: Array<{ run: () => void; ms: number }> = [];
const later = (callback: () => void, ms: number) => { const timer = { run: callback, ms }; timers.push(timer); return () => { const at = timers.indexOf(timer); if (at >= 0) timers.splice(at, 1); }; };
const firePoll = () => { const at = timers.findIndex((timer) => timer.ms === SUBAGENT_POLL_MS); check(at >= 0, "no poll was scheduled"); if (at >= 0) timers.splice(at, 1)[0].run(); };
const child = createRoot(() => {
  const store = createSubagentActivity({
    read: (target) => new Promise<SubagentInspection>((answer) => { reads.push({ agent: target.agentId, answer }); }),
    showing: () => true, online: (computer) => !computerOffline(computer), later,
  });
  createEffect(() => {
    const open = subagentOpen(), s = selected();
    const target: SubagentActivityTarget | undefined = s && open ? { id: s.key, computer: s.computer, name: s.name, runId: s.runId ?? "", conversationId: s.conversationId ?? "", archiveId: null, agentId: open } : undefined;
    store.follow(target);
  });
  return store;
});

const transcript = () => document.querySelector(".act-virtual")?.textContent ?? "";
/** The column stays hidden until placed, so only a reason the person can see counts. */
const reason = () => {
  const notice = document.querySelector<HTMLElement>(".activity .notice.danger");
  return notice && getComputedStyle(notice).visibility === "visible" ? notice.textContent ?? "" : "";
};
const overlay = () => document.querySelector(".activity > [role=status]");
const reading = () => document.querySelector(".activity")?.textContent?.includes("Reading activity…");
const open = async (agent: string | undefined, local: LocalMessage[]) => {
  setSubagentOpen(agent);
  await settle(); // The real composer's own open answers null here; what Rust would send comes after it.
  composer.apply(composerView(local, agent));
};

async function visitKeepsTheParentReceipt() {
  await open(undefined, [receipt("parent-1", "parent receipt")]);
  await until(() => transcript().includes("parent receipt"), "the parent's local row did not show");
  // Rust drops the row one batch before its journal event.
  composer.apply(composerView([]));
  await settle();
  check(transcript().includes("parent receipt"), "the handoff lost the row before any visit");

  await open("child-a", []);
  await until(() => reads.length === 1, "the child issued no read");
  reads.shift()!.answer(page("child-a", "child a page"));
  await until(() => transcript().includes("child a page"), "the child's page did not show");
  check(!transcript().includes("parent receipt"), "the parent's receipt shows in a child");

  await open(undefined, []);
  await until(() => !overlay(), "the parent did not place after the visit");
  check(transcript().includes("parent receipt"), "the visit hid the parent's handoff receipt");
  setJournal([{ seq: 5, type: "UserPromptSubmit", detail: "parent receipt", at: 1_793_000_000 }]);
  await settle();
  check(transcript().split("parent receipt").length === 2, `the receipt and its journal event are not one row: ${JSON.stringify(transcript())}`);
}

async function childrenStayApart() {
  await open("child-a", [receipt("child-a-1", "child a receipt")]);
  await until(() => reads.length === 1, "the reopened child issued no read");
  reads.shift()!.answer(page("child-a", "child a page"));
  await until(() => transcript().includes("child a receipt"), "the child's own receipt did not show");
  composer.apply(composerView([], "child-a"));
  await settle();
  await open("child-b", []);
  await until(() => reads.length === 1, "the other child issued no read");
  reads.shift()!.answer(page("child-b", "child b page"));
  await until(() => transcript().includes("child b page"), "the other child's page did not show");
  check(!transcript().includes("child a receipt"), "one child's receipt shows in another");
  await open(undefined, []);
  await until(() => !overlay(), "the parent did not place after the children");
  check(!transcript().includes("child a receipt"), "a child's receipt shows in its parent");
}

async function offlineDuringTheFirstRead() {
  await open("child-c", []);
  await until(() => reads.length === 1, "the first read was not issued");
  await until(() => reading(), "the first read did not show as reading");
  live("reconnecting");
  await until(() => reason().includes("offline"), "going offline during the first read showed no reason");
  check(!reading(), "the panel still says Reading activity while offline");
  check(reads.length === 1, "going offline started a second read");
  // Back online with the read still out: the reason goes, and it stays one read.
  live("live");
  await until(() => !reason(), "the offline reason stayed after the computer returned");
  check(reads.length === 1, "returning online started a parallel read");
  reads.shift()!.answer(page("child-c", "first page"));
  await until(() => transcript().includes("first page"), "the outstanding read's page did not show");
}

async function offlineDuringARefresh() {
  firePoll();
  await until(() => reads.length === 1, "the refresh was not issued");
  live("reconnecting");
  await until(() => reason().includes("offline"), "going offline during a refresh showed no reason");
  check(transcript().includes("first page"), "going offline lost the page");
  // Its answer is still the child's page, but the computer is still offline.
  reads.shift()!.answer(page("child-c", "newer page", 2));
  await until(() => transcript().includes("newer page"), "the refresh's answer was not adopted");
  await settle();
  check(reason().includes("offline"), "an answer cleared the reason while still offline");
  firePoll();
  check(reads.length === 0, "a poll read while offline");
  live("live");
  await until(() => reads.length === 1, "no read followed the computer's return");
  check(!reason(), "the offline reason stayed after the computer returned");
  reads.shift()!.answer(page("child-c", "newest page", 3));
  await until(() => transcript().includes("newest page"), "the read after the return did not show");
}

async function walk() {
  await loadMarkdown(); applySkin(skin());
  applyBatch({ ...emptyBatch(), computers: [{ computer: "", status: "live", attempt: 0 }], upserts: [{ computer: "", key, row }] } as Parameters<typeof applyBatch>[0]);
  setSelectedKey(session.key);
  const root = document.getElementById("root")!;
  root.style.cssText = "display:flex;width:940px;height:740px;--col:100%;--content-scale:1";
  render(() => <ActivityView source={subagentOpen() ? child : parent} />, root);
  await visitKeepsTheParentReceipt();
  await childrenStayApart();
  await offlineDuringTheFirstRead();
  await offlineDuringARefresh();
}
const report = () => (window as unknown as { webkit: { messageHandlers: { result: { postMessage(text: string): void } } } }).webkit.messageHandlers.result.postMessage(JSON.stringify({ failures }));
void walk().catch((error) => { failures.push(String(error)); }).finally(report);
