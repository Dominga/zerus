// Run with menu-smoke.py --url pointing to scripts/migration-review-smoke.html.
// Two read-only report commands return synthetic records; all other IPC is forbidden.
import { render } from "solid-js/web";
import { desktopService } from "../store/desktopService";
import { DesktopServiceStatus } from "../ui/components/DesktopServiceStatus";
import { MachinesStatus } from "../ui/components/MachinesStatus";
import { applyBatch } from "../store/catalog";
import type { DesktopService } from "../types";
import { applySkin, skin } from "../skin/runtime";
import "../ui/styles/base.css";
import "../ui/styles/app.css";

const failures: string[] = [];
const layouts: unknown[] = [];
const check = (condition: unknown, message: string) => { if (!condition) failures.push(message); };
let calls = 0;
const commands: string[] = [];
const reads: string[] = [];
Object.assign(window, { __TAURI_INTERNALS__: { invoke: (command: string) => {
  // The current report dialog reads these bounded native metadata records.
  // No native backend exists here, and neither fixture admits a write.
  if (command === "settings_qt_preferences") { reads.push(command); return Promise.resolve({ families: [], unused: [] }); }
  if (command === "settings_legacy_projects") { reads.push(command); return Promise.resolve({ family: "legacy-projects", state: "empty", keys: [], reasons: [] }); }
  calls++; commands.push(command); throw Error("Review attempted unexpected native IPC");
} } });
const report = { required: true, complete: false, running: false, code: "retained_import_qt_unavailable", phase: null };
const state: DesktopService = { connected: true, epoch: "fixture", generation: 1,
  background: Object.fromEntries(["fleet", "presence", "reads", "attention", "approvals"].map((key) => [key, { ready: true, code: "ready" }])),
  attention_counts: null, automatic_approvals: false, details_available: true, code: "", retained_source_import: report };
desktopService.apply(state);
applyBatch({ upserts: [], removes: [], replaces: [], activity: [], computers: [{ computer: "", status: "reconnecting", attempt: 2 }, { computer: "build-validation-01", status: "reconnecting", attempt: 2 }] });
applySkin(skin());
const root = document.getElementById("root")!;
const dispose = render(() => <div class="app" style={{ width: "900px" }}><nav class="rail" /><main class="page">Synthetic page</main>
  <footer class="statusbar"><MachinesStatus mode={() => "replay"} /><DesktopServiceStatus />
    <span class="grow" /><span class="session-usage-strip" style={{ width: "200px" }}>Synthetic account usage</span></footer></div>, root);
const settle = () => new Promise<void>((resolve) => setTimeout(resolve, 30));
async function run() {
  await settle();
  const page = root.querySelector("main")!, footer = root.querySelector("footer")!;
  const geometry = () => [page.getBoundingClientRect().y, page.getBoundingClientRect().height, footer.getBoundingClientRect().height];
  const initial = geometry();
  const review = root.querySelector<HTMLButtonElement>('button[aria-label="Review migration report"]');
  check(review, "banner has no Review action");
  review!.focus(); review!.click(); await settle();
  let dialog = root.querySelector<HTMLDialogElement>("dialog");
  check(dialog?.open, "Review did not open a modal dialog");
  check(dialog?.textContent?.includes("Qt settings file could not be read safely"), "actual Qt refusal reason is missing");
  dialog?.querySelector("details")?.setAttribute("open", "");
  check(dialog?.textContent?.includes("retained_import_qt_unavailable"), "native reason code is missing");
  check(dialog?.textContent?.includes("Not created yet"), "pre-journal status is missing");
  check(dialog?.textContent?.includes("other users cannot write"), "safe next step is missing");
  check(calls === 0 && desktopService.ready("fleet"), "Review changed authority or called native IPC");
  dialog?.querySelector<HTMLButtonElement>("button")?.click(); await settle(); await settle();
  check(!root.querySelector("dialog"), "Close did not dismiss Review");
  check(document.activeElement === review, "Close did not restore the opener focus");
  review!.click(); await settle();
  const technical = root.querySelector<HTMLDetailsElement>("dialog details")!;
  technical.open = true;
  desktopService.apply({ ...state, retained_source_import: { ...report, code: "retained_import_original_result_unconfirmed", phase: "unknown" } });
  await settle();
  dialog = root.querySelector("dialog");
  check(dialog?.querySelector("details") === technical && technical.open, "status update remounted or collapsed technical details");
  check(dialog?.textContent?.includes("checks the original report result"), "unconfirmed report lost its no-replay guidance");
  desktopService.apply({ ...state, retained_source_import: { ...report, creations_report: { ...report, code: "creations_import_original_result_unconfirmed", phase: "unknown" } } });
  await settle();
  check(dialog?.textContent?.includes("retained_import_qt_unavailable") && dialog?.textContent?.includes("creations_import_original_result_unconfirmed"), "one report masked the other report's reason");
  desktopService.apply({ ...state, retained_source_import: { ...report, creations_report: { ...report, code: "creations_import_before_record", phase: "before_record" } } });
  await settle();
  check(dialog?.textContent?.includes("not sent because local preparation failed") && dialog?.textContent?.includes("retained_import_qt_unavailable"), "before_record lost its definitely-unsent reason or masked the Qt reason");
  desktopService.apply({ ...state, retained_source_import: { ...report, creations_report: { ...report, code: "creations_import_before_record", phase: "unknown" } } });
  await settle();
  check(!dialog?.textContent?.includes("not sent because local preparation failed") && dialog?.textContent?.includes("checks the original report"), "a stale preparation code relabeled unknown publication");
  desktopService.apply({ ...state, retained_source_import: { ...report,
    local_read_capture: { code: "local_read_source_unsupported", paused: true, capture_id: "original" },
    local_reads_report: { ...report, code: "local_reads_import_original_result_unconfirmed", phase: "unknown" } } });
  await settle();
  check(dialog?.textContent?.includes("local_read_source_unsupported") && dialog?.textContent?.includes("local_reads_import_original_result_unconfirmed") && dialog?.textContent?.includes("retained_import_qt_unavailable"), "local capture or historical report masked another reason");
  check(calls === 0 && desktopService.ready("fleet"), "historical report review granted authority or invoked native IPC");
  desktopService.apply({ ...state, retained_source_import: { ...report,
    worktree_cache_capture: { code: "worktree_cache_source_changed", paused: true, capture_id: "cache-original" },
    drafts_capture: { code: "draft_capture_next_earlier_run", writes: true, paused: false, capture_id: "draft-original", manifests: 1 } } });
  await settle();
  check(dialog?.textContent?.includes("worktree_cache_source_changed") && dialog?.textContent?.includes("draft_capture_next_earlier_run") && dialog?.textContent?.includes("retained_import_qt_unavailable"), "cache capture masked drafts or settings");
  check(calls === 0 && desktopService.ready("fleet"), "cache review invoked native IPC or changed readiness");
  dialog?.dispatchEvent(new Event("cancel", { cancelable: true })); await settle(); await settle();
  check(!root.querySelector("dialog") && document.activeElement === review, "Escape/cancel did not close and restore focus");
  check(calls === 0 && desktopService.ready("fleet"), "Review or dismissal changed native state");
  const ownership = { pending: false, running: false, code: "", request_id: null,
    status: { abi: "fixture", configuration: "fixture", journal: null, revision: 1, state: "revoked" as const, rearm_required: true, request_id: null, owner_epoch: null, identity_current: true } };
  const baseline: DesktopService = { ...state, retained_source_import: undefined };
  for (const [patch, expected] of [
    [{}, []],
    [{ code: "background_unready" }, ["Retry now"]],
    [{ notification_ownership: ownership }, ["Resume background work"]],
    [{ notification_ownership: { ...ownership, pending: true } }, ["Check result"]],
    [{ code: "background_unready", retained_source_import: report, notification_ownership: ownership }, ["Review", "Retry now", "Resume background work"]],
  ] as const) {
    desktopService.apply({ ...baseline, ...patch }); await settle();
    const serviceGroup = footer.querySelector<HTMLElement>(".desktop-service-status");
    const actions = [...(serviceGroup?.querySelectorAll("button") ?? [])].map((b) => b.textContent);
    layouts.push({ actions, footer: footer.getBoundingClientRect().width, machine: footer.firstElementChild!.getBoundingClientRect().width,
      service: serviceGroup?.getBoundingClientRect().width ?? 0, shrink: serviceGroup ? getComputedStyle(serviceGroup).flexShrink : null,
      buttons: [...(serviceGroup?.querySelectorAll("button") ?? [])].map((b) => [b.textContent, b.getBoundingClientRect().right]) });
    check(JSON.stringify(actions) === JSON.stringify(expected), `wrong footer actions: ${JSON.stringify(actions)}`);
    check(!page.querySelector(".desktop-service-status"), "service status escaped the footer");
    check(JSON.stringify(geometry()) === JSON.stringify(initial) && initial[2] === 28, "service status changed the page or footer geometry");
    if (Object.keys(patch).length === 0) {
      check(!serviceGroup && !footer.querySelector(".service-summary"), "healthy service status did not stay hidden");
      continue;
    }
    check(serviceGroup?.getAttribute("role") === "status", "a service fault or review was hidden");
    check(footer.querySelector(".service-summary")?.getAttribute("title"), "full service explanation is unavailable");
    for (const button of serviceGroup?.querySelectorAll("button") ?? []) {
      const box = button.getBoundingClientRect(), frame = footer.getBoundingClientRect(), group = button.closest(".desktop-service-status")!.getBoundingClientRect();
      check(box.left >= frame.left && box.right <= frame.right && box.right <= 900, `footer action overflowed at 900px: ${button.textContent} right=${box.right}`);
      check(box.left >= group.left && box.right <= group.right, `footer action overflowed its group: ${button.textContent} right=${box.right} group=${group.right}`);
      check(button.contains(document.elementFromPoint(box.x + box.width / 2, box.y + box.height / 2)), `footer action is covered: ${button.textContent}`);
    }
    const summary = footer.querySelector<HTMLElement>(".service-summary")!;
    summary.focus(); await settle();
    const tooltip = document.querySelector<HTMLElement>('[role="tooltip"]');
    check(tooltip?.textContent === summary.getAttribute("title") && tooltip?.getBoundingClientRect().height, "keyboard focus did not disclose the full status reasons");
    check(getComputedStyle(summary).outlineStyle !== "none", "service status has no visible focus indicator");
    summary.dispatchEvent(new KeyboardEvent("keydown", { key: "Escape", bubbles: true })); await settle();
    check(!document.querySelector('[role="tooltip"]') && document.activeElement === summary, "Escape did not dismiss the tooltip while preserving focus");
    summary.blur();
  }
  check(calls === 0, "status presentation attempted native IPC");
  dispose();
  const result = { failures, calls, commands, reads, layouts, cases: ["Qt refusal detail", "read-only review", "unknown no-replay guidance", "close/cancel focus", "healthy/no retry", "not-ready/retry", "rearm/resume", "pending/check", "fixed page/footer geometry", "900px action bounds", "keyboard status reasons and Escape"] };
  (window as unknown as { webkit: { messageHandlers: { result: { postMessage(text: string): void } } } }).webkit.messageHandlers.result.postMessage(JSON.stringify(result));
}
void run().catch((error) => {
  (window as unknown as { webkit: { messageHandlers: { result: { postMessage(text: string): void } } } }).webkit.messageHandlers.result.postMessage(JSON.stringify({ failures: [String(error)] }));
});
