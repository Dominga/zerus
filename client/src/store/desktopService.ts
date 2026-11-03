import { createSignal } from "solid-js";
import { invoke } from "@tauri-apps/api/core";
import type { DesktopService, NotificationOwnership } from "../types";
import { tr } from "../i18n";

/** Presence of this projection is the startup mode. It never grants writer authority. */
export function createDesktopService() {
  const [state, setState] = createSignal<DesktopService>();
  let localReadReady: (() => void) | undefined, announced: string | undefined;
  const notifyLocalReadReady = (next: DesktopService | undefined) => {
    const capture = next?.retained_source_import?.local_read_capture;
    if (capture?.paused || capture?.view_ready !== true) { announced = undefined; return; }
    const view = `${capture.capture_id}/${capture.view_revision ?? "0"}`;
    if (capture.capture_id && view !== announced && localReadReady) {
      announced = view;
      localReadReady();
    }
  };
  return {
    state, apply: (next: DesktopService) => { setState(next); notifyLocalReadReady(next); },
    enabled: () => state() !== undefined,
    ready: (component: string) => !!state()?.connected && state()?.background[component]?.ready === true,
    /** One bounded presentation reload after the actual native model is ready. */
    onLocalReadReady: (listener: () => void) => { localReadReady = listener; notifyLocalReadReady(state()); },
  };
}
export const desktopService = createDesktopService();
export const retryDesktopService = () => invoke<void>("desktop_service_retry");
export const checkDesktopServiceStatus = (epoch: string) => invoke<void>("desktop_service_check_status", { epoch });

export function pendingReadNotice(state: DesktopService | undefined): string {
  const transfer = state?.pending_read_import;
  if (!transfer?.required || transfer.complete) return "";
  return transfer.running
    ? tr("Transferring saved read marks. Marking sessions as read is paused.")
    : tr("Saved read marks could not be transferred. Marking sessions as read is paused. Retry to continue.");
}

function creationCaptureNotice(report: DesktopService["retained_source_import"]): string {
  if (report?.creations_capture?.paused && report.creations_capture.code === "creation_source_private_slot") return tr("Session creation is paused. Check that the journal archive folder is available and private (0700 on Unix), then retry.");
  if (report?.creations_capture?.paused && report.creations_capture.code === "creation_source_data_directory") return tr("Session creation is paused. Check that the desktop data folder is available and owned by your user, then retry.");
  if (report?.creations_capture?.paused && report.creations_capture.code === "creation_source_unavailable") return tr("Session creation is paused. Check journal and archive access, ownership and private file permissions (0600 on Unix); review retained partial files before retrying.");
  if (report?.creations_capture?.paused) return tr("Session creation is paused while its original journal is preserved. Review migration status or retry.");
  return "";
}

export function retainedSourceNotice(state: DesktopService | undefined): string {
  const report = state?.retained_source_import;
  const drafts = report?.drafts_capture;
  if (drafts && !drafts.writes) return tr("Drafts are not being saved because their earlier versions could not be recorded. Check the Zerus data folder, then retry.");
  if (report?.local_read_capture?.paused) return tr("Local read marks are paused while their original file is preserved. Review the migration report and retry.");
  const captureNotice = creationCaptureNotice(report);
  if (captureNotice) return captureNotice;
  if (drafts?.paused) return tr("Importing drafts from Qt is paused because the earlier drafts could not be recorded. New drafts are saved.");
  const local = report?.local_reads_report;
  if (local?.required && !local.complete) {
    if (local.running) return tr("Preparing the historical local read-mark report. Your sessions remain available.");
    if (local.phase !== "unknown" && ["local_reads_import_waiting", "local_reads_import_source_resources_pending", "local_reads_import_handoff_busy", "local_reads_import_before_record"].includes(local.code)) return tr("The historical local read-mark report is deferred. Retry keeps the original report identity.");
    return tr("The historical local read-mark report needs review. An unconfirmed publication will only be looked up, never resent.");
  }
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

/** Draft capture outcomes that recorded the earlier drafts (or found none). */
const DRAFTS_RECORDED = ["draft_capture_waiting", "draft_capture_captured", "draft_capture_not_applicable"];

export interface MigrationReportDetail { reason: string; next: string; code: string; phase: string | null }
export type MigrationReportDetails = Partial<Record<"settings" | "creations" | "capture" | "localReads" | "localReadsReport" | "drafts", MigrationReportDetail>>;

/** Independent native report reasons. Reviewing never retries or publishes. */
export function retainedSourceDetails(state: DesktopService | undefined): MigrationReportDetails | undefined {
  const report = state?.retained_source_import;
  if (!report) return;
  const details: MigrationReportDetails = {};
  if (report.local_read_capture?.paused) details.localReads = {
    reason: tr("Local read marks are paused while their original file is preserved. Review the migration report and retry."),
    next: report.local_read_capture.code === "local_read_source_owner_unavailable"
      ? tr("The local read-mark writer did not start. Restart Zerus to retry; the original file is unchanged.")
      : tr("Check access to the local read-mark file and its private archive. Keep unsupported or changed originals for review; retry does not replace a saved original."),
    code: report.local_read_capture.code, phase: null,
  };
  const local = report.local_reads_report;
  if (local?.required) {
    let reason: string, next: string;
    if (local.phase === "unknown" || ["local_reads_import_original_result_unconfirmed", "local_reads_import_result_unavailable"].includes(local.code)) {
      reason = tr("The original migration report result is unconfirmed.");
      next = tr("Retry checks the original report result. It does not publish another copy of an unconfirmed report.");
    } else if (local.running) {
      reason = tr("Preparing the historical local read-mark report. Your sessions remain available.");
      next = tr("Your sessions remain available while the report is prepared.");
    } else if (local.phase === "before_record" && local.code === "local_reads_import_before_record") {
      reason = tr("The historical local read-mark report was not sent because local preparation failed.");
      next = tr("Check access to the original journal and the Zerus data folder, then use Retry. The original report identity is preserved.");
    } else if (local.complete) {
      reason = tr("The migration report has been saved for review.");
      next = tr("This historical local read-mark report does not confirm delivery to a host or grant background access.");
    } else if (["local_reads_import_waiting", "local_reads_import_source_resources_pending", "local_reads_import_handoff_busy"].includes(local.code)) {
      reason = tr("The historical local read-mark report is deferred. Retry keeps the original report identity.");
      next = tr("Wait for background work to finish, then retry.");
    } else {
      reason = tr("The historical local read-mark report needs review. An unconfirmed publication will only be looked up, never resent.");
      next = tr("Keep the original settings and report for review before attempting another migration.");
    }
    details.localReadsReport = { reason, next, code: local.code, phase: local.phase };
  }
  const capture = report.creations_capture;
  if (capture?.paused) details.capture = {
    reason: creationCaptureNotice(report), next: tr("Check the original journal and archive access before retrying."),
    code: capture.code, phase: null,
  };
  // A not-captured drafts outcome stays visibly incomplete after Retry
  // resumes the import; any code not known to be recorded counts as one.
  const drafts = report.drafts_capture;
  if (drafts && (!drafts.writes || drafts.paused || !DRAFTS_RECORDED.includes(drafts.code))) details.drafts = {
    reason: !drafts.writes
      ? tr("Drafts are not being saved because their earlier versions could not be recorded. Check the Zerus data folder, then retry.")
      : drafts.paused
        ? tr("Importing drafts from Qt is paused because the earlier drafts could not be recorded. New drafts are saved.")
        : drafts.code === "draft_capture_next_earlier_run"
          ? tr("Your Qt drafts were recorded. Drafts already in Zerus could not be recorded, because an earlier run changed them first.")
          : tr("The earlier drafts could not be recorded for the migration report. Drafts are saved and imported normally."),
    next: !drafts.writes
      ? tr("Check that the Zerus data folder is available and owned by your user. Retry records the earlier drafts before any draft is saved.")
      : drafts.paused
        ? tr("Retry imports the drafts from Qt. The earlier drafts stay unrecorded; Retry never records them later.")
        : tr("This part of the migration report stays incomplete. Retry never records the earlier drafts later."),
    code: drafts.code, phase: null,
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
  if (view.native_recovery) return tr("Background service is not ready. Some actions are unavailable.");
  if (view.status?.rearm_required) return tr("Background access was revoked. Resume it to let the service work while Zerus is closed.");
  if (view.code || view.status?.state !== "service" || !view.status.identity_current)
    return tr("Background access is unavailable. Retry to check the service.");
  return "";
}

/** Compact footer presentation; full reasons stay in its accessible tooltip. */
export function serviceStatus(state: DesktopService | undefined) {
  if (!state) return;
  const manualRefresh = state.executable_upgrade?.state === "manual_refresh_required";
  let notice = "", label = tr("Background service");
  if (manualRefresh) {
    label = tr("Service refresh required");
    notice = tr("The CLI was updated. The Windows background service needs a manual refresh. See Settings > About.");
  } else if (!state.connected) {
    label = tr("Background service unavailable");
    notice = state.reconnecting
      ? tr("Background service unavailable. Reconnecting automatically; showing last known sessions.")
      : tr("Background service unavailable. Retry to reconnect.");
  } else if (state.code === "explicit_handoff_required") {
    label = tr("Background handoff required");
    notice = tr("Background service is waiting for an explicit handoff.");
  } else if (state.code || !["fleet", "presence", "reads", "attention", "approvals"].every((name) => state.background[name]?.ready)) {
    label = tr("Background service not ready");
    notice = tr("Background service is not ready. Some actions are unavailable.");
  } else if (!state.details_available) {
    label = tr("Service views unavailable");
    notice = tr("Activity, processes and project tasks are unavailable with this background service connection.");
  }
  const pending = pendingReadNotice(state), retained = retainedSourceNotice(state), owner = ownershipNotice(state.notification_ownership);
  const ownership = state.notification_ownership?.pending ? "check"
    : !manualRefresh && (state.notification_ownership?.native_recovery || state.notification_ownership?.status?.rearm_required) ? "rearm" : undefined;
  if (!notice) {
    if (pending) label = state.pending_read_import?.running ? tr("Transferring read marks") : tr("Read marks need attention");
    else if (retained) label = tr("Migration report");
    else if (owner) label = ownership === "check" ? tr("Background access unconfirmed") : ownership === "rearm" ? tr("Background access paused") : tr("Background access unavailable");
  }
  const reasons = [notice, pending, retained, manualRefresh ? "" : owner].filter(Boolean);
  return { label, details: reasons.join("\n") || tr("Background service connected"), healthy: !reasons.length,
    retry: !manualRefresh && !!(notice || pending || retained || (owner && !ownership)), review: !!retained, ownership, manualRefresh };
}
