import { createSignal } from "solid-js";
import { invoke } from "@tauri-apps/api/core";
import type { DesktopService, NotificationOwnership } from "../types";
import { tr } from "../i18n";

/** Presence of this projection is the startup mode. It never grants writer authority. */
export function createDesktopService() {
  const [state, setState] = createSignal<DesktopService>();
  return {
    state, apply: (next: DesktopService) => setState(next),
    enabled: () => state() !== undefined,
    ready: (component: string) => !!state()?.connected && state()?.background[component]?.ready === true,
  };
}
export const desktopService = createDesktopService();
export const retryDesktopService = () => invoke<void>("desktop_service_retry");

export function pendingReadNotice(state: DesktopService | undefined): string {
  const transfer = state?.pending_read_import;
  if (!transfer?.required || transfer.complete) return "";
  return transfer.running
    ? tr("Transferring saved read marks. Marking sessions as read is paused.")
    : tr("Saved read marks could not be transferred. Marking sessions as read is paused. Retry to continue.");
}

export function retainedSourceNotice(state: DesktopService | undefined): string {
  const report = state?.retained_source_import;
  if (report?.creations_capture?.paused && report.creations_capture.code === "creation_source_private_slot") return tr("Session creation is paused. Check that the journal archive folder is available and private (0700 on Unix), then retry.");
  if (report?.creations_capture?.paused && report.creations_capture.code === "creation_source_data_directory") return tr("Session creation is paused. Check that the desktop data folder is available and owned by your user, then retry.");
  if (report?.creations_capture?.paused && report.creations_capture.code === "creation_source_unavailable") return tr("Session creation is paused. Check journal and archive access, ownership and private file permissions (0600 on Unix); review retained partial files before retrying.");
  if (report?.creations_capture?.paused) return tr("Session creation is paused while its original journal is preserved. Review migration status or retry.");
  const creations = report?.creations_report;
  if (creations?.required && !creations.complete) {
    if (creations.running) return tr("Preparing the saved creation report. Your sessions remain available.");
    if (["creations_import_waiting", "creations_import_source_resources_pending", "creations_import_handoff_busy", "creations_import_before_record"].includes(creations.code)) return tr("The saved creation report is deferred. Your sessions remain available; Retry keeps the original report identity.");
    return tr("The saved creation report needs review. An unconfirmed publication will only be looked up, never resent.");
  }
  if (!report?.required || !report.code || report.code === "retained_import_staged") return "";
  if (report.running) return tr("Preparing the migration report. Your sessions remain available.");
  if (report.code === "retained_import_handoff_busy") return tr("The migration report is deferred while background work is running.");
  return tr("The migration report needs review. Your sessions remain available.");
}

export interface MigrationReportDetail { reason: string; next: string; code: string; phase: string | null }
export type MigrationReportDetails = Partial<Record<"settings" | "creations" | "capture", MigrationReportDetail>>;

/** Independent native report reasons. Reviewing never retries or publishes. */
export function retainedSourceDetails(state: DesktopService | undefined): MigrationReportDetails | undefined {
  const report = state?.retained_source_import;
  if (!report) return;
  const details: MigrationReportDetails = {};
  const capture = report.creations_capture;
  if (capture?.paused) details.capture = {
    reason: retainedSourceNotice(state), next: tr("Check the original journal and archive access before retrying."),
    code: capture.code, phase: null,
  };
  const code = report.code;
  let reason: string, next: string;
  if (code === "retained_import_qt_unavailable") {
    reason = tr("The Qt settings file could not be read safely.");
    next = tr("Check that the file belongs to your user and that other users cannot write to it. Symbolic links and files with multiple links cannot be imported.");
  } else if (code === "retained_import_settings_unavailable") {
    reason = tr("Zerus settings could not be captured for the migration report.");
    next = tr("Finish pending settings changes and check access to the Zerus data folder, then retry.");
  } else if (["retained_import_original_result_unconfirmed", "retained_import_result_unavailable"].includes(code)) {
    reason = tr("The original migration report result is unconfirmed.");
    next = tr("Retry checks the original report result. It does not publish another copy of an unconfirmed report.");
  } else if (["retained_import_source_kept_for_review", "retained_import_source_changed"].includes(code)) {
    reason = tr("The original settings source changed or could not be verified.");
    next = tr("Keep the original settings and report for review before attempting another migration.");
  } else if (code === "retained_import_handoff_busy") {
    reason = tr("The migration report is deferred while background work is running.");
    next = tr("Wait for background work to finish, then retry.");
  } else if (report.running) {
    reason = tr("The migration report is being prepared.");
    next = tr("Your sessions remain available while the report is prepared.");
  } else if (report.complete) {
    reason = tr("The migration report has been saved for review.");
    next = tr("A saved report is historical evidence; it does not grant background access.");
  } else {
    reason = tr("The migration report could not be prepared.");
    next = tr("Check access to your Qt settings and Zerus data folders, then retry. Keep any existing report for review.");
  }
  if (report.required && (report.code || report.running || report.phase !== null || report.complete))
    details.settings = { reason, next, code, phase: report.phase };
  const creations = report.creations_report;
  if (creations?.required) {
    if (creations.phase === "unknown" || ["creations_import_original_result_unconfirmed", "creations_import_result_unavailable"].includes(creations.code)) {
      reason = tr("The original migration report result is unconfirmed.");
      next = tr("Retry checks the original report result. It does not publish another copy of an unconfirmed report.");
    } else if (creations.running) {
      reason = tr("Preparing the saved creation report. Your sessions remain available.");
      next = tr("Your sessions remain available while the report is prepared.");
    } else if (creations.phase === "before_record" && creations.code === "creations_import_before_record") {
      reason = tr("The saved creation report was not sent because local preparation failed.");
      next = tr("Check access to the original journal and the Zerus data folder, then use Retry. The original report identity is preserved.");
    } else if (creations.complete) {
      reason = tr("The migration report has been saved for review.");
      next = tr("A saved report is historical evidence; it does not grant background access.");
    } else if (["creations_import_waiting", "creations_import_source_resources_pending", "creations_import_handoff_busy"].includes(creations.code)) {
      reason = tr("The saved creation report is deferred. Your sessions remain available; Retry keeps the original report identity.");
      next = tr("Wait for background work to finish, then retry.");
    } else {
      reason = tr("The saved creation report needs review. An unconfirmed publication will only be looked up, never resent.");
      next = tr("Keep the original settings and report for review before attempting another migration.");
    }
    details.creations = { reason, next, code: creations.code, phase: creations.phase };
  }
  return Object.keys(details).length ? details : undefined;
}

/** Called by the existing opt-in confirmation only. An uncertain request is not replayed. */
export function createServiceApprovalIntent(
  send: (id: string, enabled: boolean) => Promise<unknown>,
  id: () => string = () => crypto.randomUUID(),
  read?: () => Promise<unknown>,
) {
  const [uncertain, setUncertain] = createSignal<{ id: string; enabled: boolean }>();
  const [checking, setChecking] = createSignal(false);
  const settle = (result: Record<string, unknown>, request: { id: string; enabled: boolean }) => {
    if (result?.request_id === request.id && result.op === "automatic_approvals"
      && result.accepted === false && result.durable === false && result.pending !== true) {
      setUncertain(undefined);
      if (result.code === "frontend_request_not_applied") throw Error(tr("The previous change was not applied."));
      throw Error("Automatic approval settings are unavailable with this background service connection.");
    }
    if (result?.request_id !== request.id || result.op !== "automatic_approvals" || result.accepted !== true
      || result.durable !== true || result.enabled !== request.enabled || result.pending === true) {
      throw Error("The background service has not confirmed this approval setting.");
    }
    setUncertain(undefined);
  };
  return {
    pending: uncertain, checking,
    async submit(enabled: boolean) {
      if (uncertain()) throw Error("The previous approval setting is unconfirmed. Check the background service before making another change.");
      const request = { id: id(), enabled };
      setUncertain(request);
      const result = await send(request.id, enabled) as Record<string, unknown>;
      settle(result, request);
    },
    async check() {
      if (!read || checking()) return;
      setChecking(true);
      try {
        const result = await read() as Record<string, unknown>;
        let request = uncertain();
        if (!request && result?.op === "automatic_approvals" && result.pending === false
          && (result.request_id == null || result.accepted === false)) return;
        if (!request && result?.op === "automatic_approvals" && typeof result.request_id === "string" && typeof result.enabled === "boolean") {
          request = { id: result.request_id, enabled: result.enabled };
          setUncertain(request);
        }
        if (!request) throw Error("The background service has not confirmed this approval setting.");
        settle(result, request);
      } finally { setChecking(false); }
    },
  };
}
export const serviceApprovalIntent = createServiceApprovalIntent(
  (request_id, enabled) => invoke("desktop_service_automatic_approvals", { request_id, enabled }),
  () => crypto.randomUUID(),
  () => invoke("desktop_service_frontend_result", { operation: "automatic_approvals" }),
);


/** The native controller retains the original intent across connection changes.
 * Mount/Check only look it up; Rearm is an explicit user action. */
export function createOwnershipActions(
  rearm: () => Promise<NotificationOwnership>,
  read: () => Promise<NotificationOwnership>,
) {
  const [busy, setBusy] = createSignal(false);
  const act = async (call: () => Promise<NotificationOwnership>) => {
    if (busy()) return;
    setBusy(true);
    try { return await call(); } finally { setBusy(false); }
  };
  return { busy, rearm: () => act(rearm), check: () => act(read) };
}
export const ownershipActions = createOwnershipActions(
  () => invoke("desktop_service_ownership_rearm"),
  () => invoke("desktop_service_ownership_result"),
);
export function ownershipNotice(view: NotificationOwnership | undefined): string {
  if (!view) return "";
  if (view.pending) return tr("Background access is unconfirmed. Check the result before making another change.");
  if (view.status?.rearm_required) return tr("Background access was revoked. Resume it to let the service work while Zerus is closed.");
  if (view.code || view.status?.state !== "service" || !view.status.identity_current)
    return tr("Background access is unavailable. Retry to check the service.");
  return "";
}
