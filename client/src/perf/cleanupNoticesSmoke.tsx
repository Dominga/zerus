// Actual Solid/WebKit presentation, synthetic native phases only. Serve Vite;
// scripts/menu-smoke.py --url http://127.0.0.1:<port>/scripts/cleanup-notices-smoke.html
// runs in a private Xvfb. No CLI, agent, native termination or removal is invoked.
import { render } from "solid-js/web";
import { Toasts } from "../ui/components/Overlays";
import { BackgroundTasksChip } from "../ui/screens/BackgroundTasks";
import { CleanupChip } from "../ui/screens/CleanupChip";
import { createEndActions } from "../store/endActions";
import { bus, NOTICE_MS } from "../store/notices";
import { applyCleanup, cleanup, cleanupDialog, closeCleanup, emptyCleanup, type CleanupRow } from "../store/terminationCleanup";
import { applySkin, skins } from "../skin/runtime";
import "../ui/styles/base.css";
import "../ui/styles/detail.css";
import "../ui/styles/motion.css";

const failures: string[] = [], phases: unknown[] = [];
const check = (condition: unknown, message: string) => { if (!condition) failures.push(message); };
const frame = () => new Promise<void>((resolve) => requestAnimationFrame(() => resolve()));
const settle = async () => { await frame(); await frame(); };
const target = { computer: "", name: "codex/example/original", run_id: "original-run", conversation_id: "original-conversation" };
let revision = 0;
const row = (status: "checking" | "removing" | "kept"): CleanupRow => ({
  id: "original-cleanup", label: "example worktree", path: "/srv/private/example-work", machine: "workstation", computer: "", branch: "feature/example", size: "1 KiB",
  status, reason: "private inspection details", details: "private inspection details", can_check: status === "kept", ready: false, busy: status !== "kept", source: target,
  notice: status === "kept" ? "Working folder kept for review." : null, restore: null,
});
const phase = (status: "checking" | "removing" | "kept") => applyCleanup({ ...emptyCleanup(), revision: ++revision,
  rows: [row(status)], busy: status !== "kept", kept: status === "kept" ? 1 : 0, title: "Working folders", text: status === "kept" ? "1 folder kept" : "Checking folders…" });
const sample = (phase: string) => phases.push({ phase, notices: bus.notices().map((n) => ({ title: n.title, busy: !!n.busy })), chip: !!document.querySelector("button.chip-accent") });
const report = () => (window as unknown as { webkit: { messageHandlers: { result: { postMessage(text: string): void } } } }).webkit.messageHandlers.result.postMessage(JSON.stringify({ failures, phases, userAgent: navigator.userAgent }));
async function run() {
  applySkin(skins[0]);
  const dispose = render(() => <><Toasts /><BackgroundTasksChip /><CleanupChip /></>, document.getElementById("root")!);
  let ready!: (value: boolean) => void, finished!: () => void, calls = 0;
  const actions = createEndActions({ begin: async () => {}, wait: () => new Promise((resolve) => { ready = resolve; }), cancel: async () => {} }, bus);
  const job = actions.schedule([{ target, action: { kind: "terminate" }, state: "running" }], () => {
    calls++; return new Promise<void>((resolve) => { finished = resolve; });
  }, { remove: true });
  await settle(); ready(true); await settle();
  check(!!document.querySelector('.toast[data-busy="true"]'), "actual pending action has no progress");
  check(document.querySelector(".toast b")?.textContent === "Terminating session", "termination phase missing");
  check(!document.querySelector(".toast-actions"), "Undo remained after dispatch began");
  sample("terminating");
  phase("checking"); await settle(); finished(); await job; await settle();
  check(document.querySelector(".toast b")?.textContent === "Checking working folder…", "handoff erased native checking phase");
  sample("checking");
  phase("removing"); await settle();
  check(document.querySelectorAll(".toast").length === 1, "phase handoff duplicated progress");
  check(document.querySelector(".toast b")?.textContent === "Removing working folder…", "native removing phase missing");
  const arc = document.querySelector(".toast .busy-arc");
  check(!!arc && getComputedStyle(arc).borderTopWidth === "2px", "no visible busy indicator");
  document.documentElement.dataset.reduceMotion = "1"; await settle();
  check(!!arc && getComputedStyle(arc).animationName === "none", "busy indicator ignores reduced motion");
  delete document.documentElement.dataset.reduceMotion;
  sample("removing");
  phase("kept"); await settle();
  check(document.querySelectorAll(".toast").length === 1 && !document.querySelector('.toast[data-busy="true"]'), "result did not replace progress");
  check(!document.body.textContent?.includes("/srv/private"), "private raw path leaked into notice");
  (document.querySelector(".toast-actions button") as HTMLButtonElement)?.click();
  check(cleanupDialog()?.selected === "original-cleanup", "result Review lost its original row"); closeCleanup();
  sample("kept");
  const dark = getComputedStyle(document.querySelector(".toast")!).backgroundColor;
  applySkin(skins.find((skin) => skin.appearance === "light")!); await settle();
  check(getComputedStyle(document.querySelector(".toast")!).backgroundColor !== dark, "light appearance did not repaint the result");
  sample("kept-light");
  await new Promise((resolve) => setTimeout(resolve, NOTICE_MS + 150));
  check(!document.querySelector(".toast"), "result did not expire");
  (document.querySelector("button.chip-accent") as HTMLButtonElement)?.click();
  check(!!cleanupDialog() && cleanup().rows[0]?.status === "kept", "global Review was lost after expiry"); closeCleanup();
  sample("expired");
  check(calls === 1, "presentation replayed the original operation");
  actions.close(); dispose(); report();
}
void run().catch((error) => { failures.push(String(error)); report(); });
