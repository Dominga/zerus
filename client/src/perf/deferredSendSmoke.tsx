// The real Composer in a deferred send's two new states (zerus-5wmh.66.4.4),
// with synthetic views only: state 5, a send the desktop service could not
// confirm (uncertain, Allow retry), and state 6, a send it still holds (a
// normal status, no Allow retry). No CLI, service, session or file is used.
// Served by Vite, outside the app entry:
//   npx vite --port <port> --strictPort, then load
//   http://127.0.0.1:<port>/scripts/deferred-send-smoke.html?state=5&skin=zerus-dark
// in WebKitGTK; the page posts "ready" (or its failures) to `result`.
import { render } from "solid-js/web";
import { applyBatch, toSession } from "../store/catalog";
import { setSelectedKey } from "../store/ui";
import { emptyBatch, type ComposerView } from "../types";
import { applySkin, skin } from "../skin/runtime";
import "../ui/styles/base.css";
import "../ui/styles/app.css";
import "../ui/styles/detail.css";
import "../ui/styles/client.css";

const UNCONFIRMED = "Delivery is not confirmed by the desktop service. Check Terminal before retrying.";
const STILL = "The desktop service is still sending this message. Its result appears here when it finishes.";
const state = new URLSearchParams(location.search).get("state") === "6" ? 6 : 5;
const report = (value: unknown) => (window as unknown as { webkit?: { messageHandlers?: { result?: { postMessage(text: string): void } } } })
  .webkit?.messageHandlers?.result?.postMessage(JSON.stringify(value));
const row = { name: "claude/example/review", agent: "claude", state: "running", status: "ready", run_id: "run-1", conversation_id: "conversation-1", cwd: "/example/review" };
const key = "run:run-1";
const view: ComposerView = {
  handle: 1, key: "\nclaude/example/review", loaded: true, rev: 1, text_epoch: 1,
  text: "Please rerun the failing integration test and summarise the cause.", position: 0, anchor: 0,
  question: null, input_visible: true, queue: null, attachments: [], removed: null, attaching: false, sending: false, waiting: null,
  uncertain: true, delivery_pending: state === 6,
  status: state === 6 ? STILL : UNCONFIRMED,
  status_error: state === 5, can_send: false, send_label: { source: "Send" }, available: true, unavailable_reason: "", block_reason: null,
  attachments_available: true, attachments_reason: "", session_status: "", session_status_error: false, session_blocks_input: false,
  inspect_error: false, cache: { visible: false, tone: "quiet", label: "", short_label: "" }, cache_warning: "", cache_details: "",
  context: { visible: false, text: "", tooltip: "", warning: false, tone: "quiet", percent: null, accessible_name: "" }, local: [],
  cancel_send: false, compacting: false,
  clear: { enabled: true, dismiss: false, title: "Clear session?", text: "", confirm: "Clear session", run: "run-1", conversation: "conversation-1",
    cache_enabled: true, running: false, reason: "", cache_reason: "" },
} as unknown as ComposerView;
// Only reads answer; nothing is written anywhere.
Object.assign(window, { __TAURI_INTERNALS__: { invoke: async (command: string) => {
  if (command === "composer_open") return view;
  return null;
} } });

async function run() {
  applySkin(skin());
  applyBatch({ ...emptyBatch(), computers: [{ computer: "", status: "live", attempt: 0 }], upserts: [{ computer: "", key, row }] });
  setSelectedKey(toSession("", key, row).key);
  const { composer } = await import("../store/composer");
  composer.apply(view);
  const { Composer } = await import("../ui/sessions/Composer");
  const root = document.getElementById("root")!;
  root.style.cssText = "width:880px;padding:16px;box-sizing:border-box";
  render(() => <div class="cmp-col"><Composer /></div>, root);
  await new Promise((resolve) => setTimeout(resolve, 600));
  const retry = [...document.querySelectorAll("button")].some((b) => b.textContent?.includes("Allow retry"));
  const status = document.querySelector(".cmp-status");
  const failures: string[] = [];
  if (retry !== (state === 5)) failures.push(`Allow retry shown=${retry}`);
  if (!status || status.classList.contains("error") !== (state === 5)) failures.push("status style");
  report({ ready: true, state, failures, status: status?.textContent ?? null });
}
run().catch((error) => report({ ready: false, failures: [String(error)] }));
