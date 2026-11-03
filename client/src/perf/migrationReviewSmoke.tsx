// Run with menu-smoke.py --url pointing to scripts/migration-review-smoke.html.
// Native IPC is forbidden; this exercises only the existing status projection.
import { render } from "solid-js/web";
import { desktopService } from "../store/desktopService";
import { DesktopServiceStatus } from "../ui/components/DesktopServiceStatus";
import type { DesktopService } from "../types";
import "../ui/styles/base.css";
import "../ui/styles/app.css";

const failures: string[] = [];
const check = (condition: unknown, message: string) => { if (!condition) failures.push(message); };
let calls = 0;
Object.assign(window, { __TAURI_INTERNALS__: { invoke: () => { calls++; throw Error("Review attempted native IPC"); } } });
const report = { required: true, complete: false, running: false, code: "retained_import_qt_unavailable", phase: null };
const state: DesktopService = { connected: true, epoch: "fixture", generation: 1,
  background: Object.fromEntries(["fleet", "presence", "reads", "attention", "approvals"].map((key) => [key, { ready: true, code: "ready" }])),
  attention_counts: null, automatic_approvals: false, details_available: true, code: "", retained_source_import: report };
desktopService.apply(state);
const root = document.getElementById("root")!;
const dispose = render(() => <DesktopServiceStatus />, root);
const settle = () => new Promise<void>((resolve) => setTimeout(resolve, 30));
async function run() {
  await settle();
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
  dialog?.dispatchEvent(new Event("cancel", { cancelable: true })); await settle(); await settle();
  check(!root.querySelector("dialog") && document.activeElement === review, "Escape/cancel did not close and restore focus");
  check(calls === 0 && desktopService.ready("fleet"), "Review or dismissal changed native state");
  dispose();
  const result = { failures, calls, cases: ["Qt refusal detail", "read-only review", "unknown no-replay guidance", "close/cancel focus"] };
  (window as unknown as { webkit: { messageHandlers: { result: { postMessage(text: string): void } } } }).webkit.messageHandlers.result.postMessage(JSON.stringify(result));
}
void run().catch((error) => {
  (window as unknown as { webkit: { messageHandlers: { result: { postMessage(text: string): void } } } }).webkit.messageHandlers.result.postMessage(JSON.stringify({ failures: [String(error)] }));
});
