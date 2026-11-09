import { createSignal } from "solid-js";
import { invoke } from "@tauri-apps/api/core";
import type { DesktopService, NotificationOwnership, ModeReview, FirstChoiceOffer } from "../types";
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
  if (report?.downloads_capture?.code === "download_capture_unreadable") return tr("Changes to the downloaded files folder are refused because its earlier index record could not be verified. Existing files are kept.");
  if (report?.local_read_capture?.paused) return tr("Local read marks are paused while their original file is preserved. Review the migration report and retry.");
  const captureNotice = creationCaptureNotice(report);
  if (captureNotice) return captureNotice;
  if (drafts?.paused) return tr("Importing drafts from Qt is paused because the earlier drafts could not be recorded. New drafts are saved.");
  if (report?.worktree_cache_capture?.paused) return tr("Saving folder catalogs is paused while their original cache is preserved. Fresh folder queries and session creation remain available.");
  // Persistence pauses above remain first. Among reports, review and active
  // preparation take precedence over a deferred sibling. Details keep every
  // source's independent reason, regardless of which notice is shown.
  const notices: { text: string; priority: number }[] = [];
  const creations = report?.creations_report;
  if (creations?.required && !creations.complete) {
    if (creations.running) notices.push({ text: tr("Preparing the saved creation report. Your sessions remain available."), priority: 1 });
    else if (creations.phase !== "unknown" && ["creations_import_waiting", "creations_import_source_resources_pending", "creations_import_handoff_busy", "creations_import_before_record"].includes(creations.code)) notices.push({ text: tr("The saved creation report is deferred. Your sessions remain available; Retry keeps the original report identity."), priority: 0 });
    else notices.push({ text: tr("The saved creation report needs review. An unconfirmed publication will only be looked up, never resent."), priority: 2 });
  }
  if (report?.required && report.code && report.code !== "retained_import_staged") {
    if (report.running) notices.push({ text: tr("Preparing the migration report. Your sessions remain available."), priority: 1 });
    else if (report.phase !== "unknown" && ["retained_import_handoff_busy", "retained_import_source_resources_pending"].includes(report.code)) notices.push({ text: tr("The migration report is deferred while background work is running."), priority: 0 });
    else notices.push({ text: tr("The migration report needs review. Your sessions remain available."), priority: 2 });
  }
  const local = report?.local_reads_report;
  if (local?.required && !local.complete) {
    if (local.running) notices.push({ text: tr("Preparing the historical local read-mark report. Your sessions remain available."), priority: 1 });
    else if (local.phase !== "unknown" && ["local_reads_import_waiting", "local_reads_import_source_resources_pending", "local_reads_import_handoff_busy", "local_reads_import_before_record"].includes(local.code)) notices.push({ text: tr("The historical local read-mark report is deferred. Retry keeps the original report identity."), priority: 0 });
    else notices.push({ text: tr("The historical local read-mark report needs review. An unconfirmed publication will only be looked up, never resent."), priority: 2 });
  }
  const cache = report?.worktree_cache_report;
  if (cache?.required && !cache.complete) {
    if (cache.running) notices.push({ text: tr("Preparing the historical folder catalog report. Your sessions remain available."), priority: 1 });
    else if (cache.phase !== "unknown" && ["worktree_cache_import_waiting", "worktree_cache_import_source_resources_pending", "worktree_cache_import_handoff_busy", "worktree_cache_import_before_record"].includes(cache.code)) notices.push({ text: tr("The historical folder catalog report is deferred. Retry keeps the original report identity."), priority: 0 });
    else notices.push({ text: tr("The historical folder catalog report needs review. An unconfirmed publication will only be looked up, never resent."), priority: 2 });
  }
  const downloadsReport = report?.downloads_report;
  if (downloadsReport?.required && !downloadsReport.complete) {
    if (downloadsReport.running) notices.push({ text: tr("Preparing the historical downloaded files report. Your sessions remain available."), priority: 1 });
    else if (downloadsReport.phase !== "unknown" && ["downloads_import_waiting", "downloads_import_source_resources_pending", "downloads_import_handoff_busy", "downloads_import_before_record"].includes(downloadsReport.code)) notices.push({ text: tr("The historical downloaded files report is deferred. Retry keeps the original report identity."), priority: 0 });
    else notices.push({ text: tr("The historical downloaded files report needs review. An unconfirmed publication will only be looked up, never resent."), priority: 2 });
  }
  // A not-captured or partial drafts report is permanent and shown in the
  // Migration report; it needs no banner of its own. Before the capture's
  // outcome is final the report has not started, and the Drafts notices
  // above already explain a refused capture.
  const draftsReport = draftsReportStarted(report) ? report?.drafts_report : undefined;
  if (draftsReport?.required && !draftsReport.complete) {
    if (draftsReport.running) notices.push({ text: tr("Preparing the historical drafts report. Your sessions remain available."), priority: 1 });
    else if (draftsReport.phase !== "unknown" && DRAFTS_REPORT_DEFERRED.includes(draftsReport.code)) notices.push({ text: tr("The historical drafts report is deferred. Retry keeps the original report identity."), priority: 0 });
    else if (!draftsReport.partial && draftsReport.code !== "drafts_import_capture_unavailable") notices.push({ text: tr("The historical drafts report needs review. An unconfirmed publication will only be looked up, never resent."), priority: 2 });
  }
  return notices.find(notice => notice.priority === 2)?.text
    ?? notices.find(notice => notice.priority === 1)?.text
    ?? notices[0]?.text ?? "";
}

/** Draft capture outcomes that recorded the earlier drafts (or found none). */
const DRAFTS_RECORDED = ["draft_capture_waiting", "draft_capture_captured", "draft_capture_not_applicable"];
/** Draft capture states before a final outcome: the drafts report waits. */
const DRAFTS_PENDING = ["draft_capture_waiting", "draft_capture_private_slot"];

/** The drafts report runs only once the drafts capture's outcome is final. */
function draftsReportStarted(report: DesktopService["retained_source_import"]): boolean {
  return !DRAFTS_PENDING.includes(report?.drafts_capture?.code ?? "");
}

/** Drafts report codes that wait for background work. */
const DRAFTS_REPORT_DEFERRED = ["drafts_import_waiting", "drafts_import_source_resources_pending", "drafts_import_handoff_busy", "drafts_import_before_record"];

export interface MigrationReportDetail { reason: string; next: string; code: string; phase: string | null }
export type MigrationReportDetails = Partial<Record<"settings" | "creations" | "capture" | "localReads" | "localReadsReport" | "drafts" | "draftsReport" | "downloads" | "downloadsReport" | "worktreeCache" | "worktreeCacheReport", MigrationReportDetail>>;

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
  if (report.worktree_cache_capture?.paused) details.worktreeCache = {
    reason: tr("Saving folder catalogs is paused while their original cache is preserved. Fresh folder queries and session creation remain available."),
    next: report.worktree_cache_capture.code === "worktree_cache_source_owner_unavailable"
      ? tr("The folder-cache worker could not run. Restart Zerus to retry; the original cache is unchanged.")
      : tr("Check access to the folder cache and its private archive, then retry. Keep unsupported or changed originals for review; Retry never replaces a saved original."),
    code: report.worktree_cache_capture.code, phase: null,
  };
  const downloadsReport = report.downloads_report;
  if (downloadsReport?.required) {
    let reason: string, next: string;
    if (downloadsReport.phase === "unknown" || ["downloads_import_original_result_unconfirmed", "downloads_import_result_unavailable"].includes(downloadsReport.code)) {
      reason = tr("The original migration report result is unconfirmed.");
      next = tr("Retry checks the original report result. It does not publish another copy of an unconfirmed report.");
    } else if (downloadsReport.running) {
      reason = tr("Preparing the historical downloaded files report. Your sessions remain available.");
      next = tr("Your sessions remain available while the report is prepared.");
    } else if (downloadsReport.phase === "before_record" && downloadsReport.code === "downloads_import_before_record") {
      reason = tr("The historical downloaded files report was not sent because local preparation failed.");
      next = tr("Check access to the original journal and the Zerus data folder, then use Retry. The original report identity is preserved.");
    } else if (downloadsReport.complete) {
      reason = tr("The migration report has been saved for review.");
      next = tr("This historical downloaded files report is evidence only. It never downloads, removes or cleans up files.");
    } else if (["downloads_import_waiting", "downloads_import_source_resources_pending", "downloads_import_handoff_busy"].includes(downloadsReport.code)) {
      reason = tr("The historical downloaded files report is deferred. Retry keeps the original report identity.");
      next = tr("Wait for background work to finish, then retry.");
    } else {
      reason = tr("The historical downloaded files report needs review. An unconfirmed publication will only be looked up, never resent.");
      next = tr("Keep the original settings and report for review before attempting another migration.");
    }
    details.downloadsReport = { reason, next, code: downloadsReport.code, phase: downloadsReport.phase };
  }
  const draftsReport = draftsReportStarted(report) ? report.drafts_report : undefined;
  if (draftsReport?.required) {
    let reason: string, next: string;
    if (draftsReport.phase === "unknown" || ["drafts_import_original_result_unconfirmed", "drafts_import_result_unavailable"].includes(draftsReport.code)) {
      reason = tr("The original migration report result is unconfirmed.");
      next = tr("Retry checks the original report result. It does not publish another copy of an unconfirmed report.");
    } else if (draftsReport.running) {
      reason = tr("Preparing the historical drafts report. Your sessions remain available.");
      next = tr("Your sessions remain available while the report is prepared.");
    } else if (draftsReport.phase === "before_record" && draftsReport.code === "drafts_import_before_record") {
      reason = tr("The historical drafts report was not sent because local preparation failed.");
      next = tr("Check access to the original journal and the Zerus data folder, then use Retry. The original report identity is preserved.");
    } else if (draftsReport.partial && draftsReport.phase === "published") {
      reason = tr("The historical drafts report was saved without earlier versions of the drafts already in Zerus. Your drafts are kept and nothing is lost.");
      next = tr("This part of the migration report stays incomplete. Retry never records the earlier drafts later.");
    } else if (draftsReport.complete) {
      reason = tr("The migration report has been saved for review.");
      next = tr("This historical drafts report is evidence only. It never sends, imports or deletes drafts or their attachments.");
    } else if (draftsReport.code === "drafts_import_capture_unavailable") {
      reason = tr("The recorded earlier drafts are unavailable, so the historical drafts report was not sent.");
      next = tr("This part of the migration report stays incomplete. Retry never records the earlier drafts later.");
    } else if (DRAFTS_REPORT_DEFERRED.includes(draftsReport.code)) {
      reason = tr("The historical drafts report is deferred. Retry keeps the original report identity.");
      next = tr("Wait for background work to finish, then retry.");
    } else {
      reason = tr("The historical drafts report needs review. An unconfirmed publication will only be looked up, never resent.");
      next = tr("Keep the original settings and report for review before attempting another migration.");
    }
    details.draftsReport = { reason, next, code: draftsReport.code, phase: draftsReport.phase };
  }
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
  const cache = report.worktree_cache_report;
  if (cache?.required) {
    let reason: string, next: string;
    if (cache.phase === "unknown" || ["worktree_cache_import_original_result_unconfirmed", "worktree_cache_import_result_unavailable"].includes(cache.code)) {
      reason = tr("The original migration report result is unconfirmed.");
      next = tr("Retry checks the original report result. It does not publish another copy of an unconfirmed report.");
    } else if (cache.running) {
      reason = tr("Preparing the historical folder catalog report. Your sessions remain available.");
      next = tr("Your sessions remain available while the report is prepared.");
    } else if (cache.phase === "before_record" && cache.code === "worktree_cache_import_before_record") {
      reason = tr("The historical folder catalog report was not sent because local preparation failed.");
      next = tr("Check access to the original journal and the Zerus data folder, then use Retry. The original report identity is preserved.");
    } else if (cache.complete) {
      reason = tr("The migration report has been saved for review.");
      next = tr("This historical folder catalog report does not verify today's checkouts or authorize worktree creation or cleanup.");
    } else if (["worktree_cache_import_waiting", "worktree_cache_import_source_resources_pending", "worktree_cache_import_handoff_busy"].includes(cache.code)) {
      reason = tr("The historical folder catalog report is deferred. Retry keeps the original report identity.");
      next = tr("Wait for background work to finish, then retry.");
    } else {
      reason = tr("The historical folder catalog report needs review. An unconfirmed publication will only be looked up, never resent.");
      next = tr("Keep the original settings and report for review before attempting another migration.");
    }
    details.worktreeCacheReport = { reason, next, code: cache.code, phase: cache.phase };
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
          ? tr("Your drafts are kept and nothing is lost. Earlier versions of the drafts already in Zerus could not be recorded for the migration report, because an earlier run changed them first.")
          : tr("The earlier drafts could not be recorded for the migration report. Drafts are saved and imported normally."),
    next: !drafts.writes
      ? tr("Check that the Zerus data folder is available and owned by your user. Retry records the earlier drafts before any draft is saved.")
      : drafts.paused
        ? tr("Retry imports the drafts from Qt. The earlier drafts stay unrecorded; Retry never records them later.")
        : tr("This part of the migration report stays incomplete. Retry never records the earlier drafts later."),
    code: drafts.code, phase: null,
  };
  // The download folder's evidence: shown only when it is incomplete. It is
  // never consent to download, remove or clean up anything.
  const downloads = report.downloads_capture;
  if (downloads && ["download_capture_unsupported", "download_capture_unreadable"].includes(downloads.code)) details.downloads = {
    reason: downloads.code === "download_capture_unsupported" ? tr("The downloaded files folder is in a location where its earlier index cannot be recorded, so it is not part of the migration report. Downloads keep working.") : tr("Changes to the downloaded files folder are refused because its earlier index record could not be verified. Existing files are kept."),
    next: downloads.code === "download_capture_unsupported" ? tr("This part of the migration report stays incomplete.") : tr("Check that the folder and its .zerus-files-preload-v1 folder belong to your user and are private. Nothing is recaptured or overwritten."),
    code: downloads.code, phase: null,
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

/** One switch or category change; `categories` (complete set) only for a category change. */
type ApprovalRequest = { id: string; enabled: boolean; categories?: string[] };
const sameIds = (a: unknown, b: readonly string[]) => Array.isArray(a) && a.length === b.length && a.every((id, n) => id === b[n]);

/** The switch is changed by the existing opt-in confirmation only; a category
 * change carries the committed switch value, so it can never turn approvals
 * on. An uncertain request is not replayed. */
export function createServiceApprovalIntent(
  send: (id: string, enabled: boolean, categories?: string[]) => Promise<unknown>,
  id: () => string = () => crypto.randomUUID(),
  read?: () => Promise<unknown>,
) {
  const [uncertain, setUncertain] = createSignal<ApprovalRequest>();
  const [checking, setChecking] = createSignal(false);
  const settle = (result: Record<string, unknown>, request: ApprovalRequest) => {
    if (result?.request_id === request.id && result.op === "automatic_approvals"
      && result.accepted === false && result.durable === false && result.pending !== true) {
      setUncertain(undefined);
      if (result.code === "frontend_request_not_applied") throw Error(tr("The previous change was not applied."));
      if (result.code === "approval_policy_changed") throw Error(tr("Automatic approvals were turned on or off elsewhere. Check the switch, then change the categories again."));
      throw Error("Automatic approval settings are unavailable with this background service connection.");
    }
    if (result?.request_id !== request.id || result.op !== "automatic_approvals" || result.accepted !== true
      || result.durable !== true || result.enabled !== request.enabled || result.pending === true
      || (request.categories && !sameIds(result.categories, request.categories))) {
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
    /** The complete category set, with the switch value the service has committed. */
    async submitCategories(enabled: boolean, categories: string[]) {
      if (uncertain()) throw Error("The previous approval setting is unconfirmed. Check the background service before making another change.");
      const request = { id: id(), enabled, categories: [...categories] };
      setUncertain(request);
      const result = await send(request.id, enabled, request.categories) as Record<string, unknown>;
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
          if (Array.isArray(result.categories) && result.categories.every((c) => typeof c === "string")) request.categories = result.categories as string[];
          setUncertain(request);
        }
        if (!request) throw Error("The background service has not confirmed this approval setting.");
        settle(result, request);
      } finally { setChecking(false); }
    },
  };
}
export const serviceApprovalIntent = createServiceApprovalIntent(
  (request_id, enabled, categories) => invoke("desktop_service_automatic_approvals", { request_id, enabled, categories: categories ?? null }),
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
export type ModeReviewAction = "review" | "background" | "passive";
/** Only a click invokes this command. Native confirmation and stale-selection
 * checks happen in Rust; neither the view nor these arguments supply consent. */
export function createModeReviewActions(
  run: (display: ModeReview, action: ModeReviewAction) => Promise<unknown>,
) {
  const [busy, setBusy] = createSignal(false);
  return { busy, async submit(display: ModeReview, action: ModeReviewAction) {
    if (busy()) return;
    const selected = { ...display };
    setBusy(true);
    try { return await run(selected, action); } finally { setBusy(false); }
  } };
}
export const modeReviewActions = createModeReviewActions(
  (display, action) => invoke("desktop_service_mode_action", { display, action }),
);
/** The first explicit background choice (Windows). Only a click invokes it; the
 * native confirmation and every check happen in Rust. The returned original ID
 * is kept only to show this choice's own pending or not-started outcome. */
export function createFirstChoiceActions(
  run: (display: FirstChoiceOffer) => Promise<{ request_id?: string } | null>,
) {
  const [busy, setBusy] = createSignal(false);
  const [request, setRequest] = createSignal<string>();
  return { busy, request, async submit(display: FirstChoiceOffer) {
    if (busy()) return;
    setBusy(true);
    try {
      const result = await run({ ...display });
      if (result?.request_id) setRequest(result.request_id);
      return result;
    } finally { setBusy(false); }
  } };
}
export const firstChoiceActions = createFirstChoiceActions(
  (display) => invoke("desktop_service_first_choice", { display }),
);
const HELPER_UNAVAILABLE = ["activation_helper_unavailable", "activation_helper_update_required", "activation_helper_changed"];
const COMPONENTS = ["fleet", "presence", "reads", "attention", "approvals"];
/**
 * Task 180 (owner, 2026-11-07): a refused native takeover says which check
 * refused it and the next step, never an empty list or a bare "paused".
 */
export interface TakeoverRefusal { check: string; reason: string; next: string }

export function takeoverRefusal(view: NotificationOwnership | undefined): TakeoverRefusal | undefined {
  const refused = view?.native_recovery ? view.native_recovery_reason : undefined;
  if (!refused) return undefined;
  const reinstall = tr("Install Zerus again from this portable folder so hgs records this app, then restart Zerus.");
  switch (refused.code) {
    case "frontend_binding_unavailable":
      return { check: tr("App binding"), reason: tr("hgs has no record of this app: its binding (frontend.json) is missing or unreadable."), next: reinstall };
    case "frontend_binding_other_cli":
      return { check: tr("App binding"), reason: tr("hgs's record of this app names another hgs installation."), next: reinstall };
    case "frontend_not_bound_application":
    case "frontend_not_installed_application":
      return { check: tr("App binding"), reason: tr("This app is not the one hgs recorded."), next: tr("Start Zerus from its installed folder, or install this copy again; then restart Zerus.") };
    case "frontend_folders_not_private":
      return { check: tr("Folder privacy"), reason: tr("Another account can change a folder that holds hgs's record of this app (.local/share/hgs/host or a folder above it)."),
        next: tr("Make that folder changeable only by you (other accounts may keep read access to the folders above it), then restart Zerus.") };
    case "agent_descended_frontend_refused":
      return { check: tr("Process ancestry"), reason: tr("Zerus was started from an agent's session."), next: tr("Close it and start Zerus from the Start menu or its folder.") };    // Task #421 (2026-11-09, package 167): the takeover's hgs child failed
    // because the desktop service could not open its store; "Restart Zerus"
    // never helped. Starting the service once shows why, or migrates it.
    case "child_failed":
      return { check: tr("Desktop service"), reason: tr("The desktop service did not start, so Zerus could not take over its sessions."),
        next: tr("Run hgs desktop serve once in a terminal: it starts the service or says why it cannot. Then restart Zerus.") };
  }
  switch (refused.check) {
    case "ownership_journal":
      return { check: tr("Ownership journal"), reason: tr("The background service has an ownership change that is not settled."), next: tr("Review it in Settings > About, then resume background work.") };
    case "identity":
      return { check: tr("App identity"), reason: tr("hgs could not verify this app's process."), next: tr("Restart Zerus.") };
    default:
      return { check: tr("Takeover"), reason: tr("The takeover was not confirmed (%1).", refused.code), next: tr("Restart Zerus. If it stays, send hgs-tray.log to the team.") };
  }
}

export function ownershipNotice(view: NotificationOwnership | undefined): string {
  if (!view) return "";
  if (view.pending) return tr("Background access is unconfirmed. Check the result before making another change.");
  if (view.activation?.review) {
    if (view.activation.review.disposition !== "reviewed_unknown")
      return tr("An earlier change of the background service mode needs review in Settings > About.");
    if (!view.activation.business_ready || !view.activation.active || view.activation.pending || view.status?.state !== "service" || !view.status.identity_current || view.status.rearm_required)
      return tr("The earlier mode change was reviewed. Choose how the service should work in Settings > About.");
  }
  if (view.activation?.pending) return tr("Background activation is unconfirmed. Check the original result before making another change.");
  if (view.activation?.needs_resume) {
    if (HELPER_UNAVAILABLE.includes(view.activation.code))
      return tr("Background activation is unavailable. Update Zerus and its desktop service, then Resume to check again.");
    return tr("Background work is paused. Resume to activate the service explicitly.");
  }
  // Task #440: the window restarts itself to finish starting; said ahead.
  if (view.native_recovery && view.native_recovery_restart_at)
    return tr("Zerus restarts its window to finish starting. Your drafts are saved.");
  // Task 180: the refused takeover names its check and the next step.
  const refused = takeoverRefusal(view);
  if (refused) return `${tr("Zerus could not take over its sessions")} (${refused.check}): ${refused.reason} ${refused.next}`;
  if (view.native_recovery) return tr("Background service is not ready. Some actions are unavailable.");
  if (view.status?.rearm_required) return tr("Background access was revoked. Resume it to let the service work while Zerus is closed.");
  if (view.code || view.status?.state !== "service" || !view.status.identity_current)
    return tr("Background access is unavailable. Retry to check the service.");
  return "";
}

/** Held task message copies the service reports for review; only a connected status counts. */
export function heldInputsNotice(state: DesktopService | undefined): string {
  const held = state?.connected ? state.task_message_inputs : undefined;
  if (!held || !(held.review > 0)) return "";
  return held.full
    ? tr("Large task messages are paused: some held message copies need review.")
    : tr("Some held task message copies need review.");
}

/**
 * A connected service that runs passive: it reports none of its background work
 * started. "resume": the native activation helper offers the explicit Resume
 * (Linux, macOS). "settings": the platform has no native activation route
 * (activation_platform_manual, Windows) and ownership needs nothing; Settings >
 * About shows the service state. Presentation only; never authority.
 */
export type PausedAction = "resume" | "settings";
export function servicePaused(state: DesktopService | undefined): PausedAction | undefined {
  const ownership = state?.notification_ownership, activation = ownership?.activation;
  if (!state?.connected || state.executable_upgrade?.state === "manual_refresh_required"
    || !COMPONENTS.every((name) => state.background[name]?.code === "not_started")
    || !activation || ownership?.pending || activation.pending
    || activation.review || activation.automatic_blocked) return undefined;
  if (activation.needs_resume) return HELPER_UNAVAILABLE.includes(activation.code) ? undefined : "resume";
  return activation.code === "activation_platform_manual" && !ownershipNotice(ownership) ? "settings" : undefined;
}

/** THIS first background choice is unconfirmed: its own original ID is
 * pending and no review or manual refresh has taken over. Presentation only. */
export function firstChoicePending(state: DesktopService | undefined, request = firstChoiceActions.request()): boolean {
  const activation = state?.notification_ownership?.activation;
  return !!request && !!activation?.pending && activation.request_id === request && !activation.review
    && state?.executable_upgrade?.state !== "manual_refresh_required";
}

/** The Windows paused body: it promises turning background mode on in
 * Settings only while that first choice is actually offered. */
export function pausedSettingsBody(state: DesktopService | undefined): string {
  if (firstChoicePending(state)) return tr("Zerus is turning on background mode on this computer. Settings shows its progress.");
  return state?.notification_ownership?.activation?.first
    ? tr("Zerus runs your sessions through a background service, which stays paused until you turn it on in Settings.")
    : tr("Zerus runs your sessions through a background service, which is paused on this computer. Settings shows its current state.");
}

/** Compact footer presentation; full reasons stay in its accessible tooltip. */
export function serviceStatus(state: DesktopService | undefined) {
  if (!state) return;
  // An explicit mode change in flight stays the headline with only its
  // original-ID Check, never a reconnect Retry, even while the service
  // restarts. Independent reasons keep their own text and review links.
  if (firstChoicePending(state)) {
    const ownership = state.notification_ownership;
    const reads = pendingReadNotice(state), retained = retainedSourceNotice(state), inputs = heldInputsNotice(state);
    const access = ownership?.pending ? ownershipNotice(ownership) : "";
    // The same readiness rule as below: no held-copy Review while not ready.
    const inputsReview = !!inputs && state.connected && !state.code && COMPONENTS.every((name) => state.background[name]?.ready);
    return { label: tr("Turning on background mode"),
      details: [tr("The new mode choice is unconfirmed. Check its original result; do not make another choice."), reads, retained, inputs, access]
        .filter(Boolean).join("\n"),
      healthy: false, retry: false, review: !!retained, inputsReview, ownership: "check" as const, manualRefresh: false, modeReview: false };
  }
  const manualRefresh = state.executable_upgrade?.state === "manual_refresh_required";
  const paused = servicePaused(state);
  let notice = "", label = tr("Background service");
  if (manualRefresh) {
    label = tr("Service refresh required");
    notice = tr("The CLI was updated. The Windows background service needs a manual refresh. See Settings > About.");
  } else if (state.state_refusal) {
    // Task 176: hgs's own words name the folder, the access entry and the repair.
    label = tr("Session state folder refused");
    notice = state.state_refusal;
  } else if (!state.connected) {
    label = tr("Background service unavailable");
    notice = state.reconnecting
      ? tr("Background service unavailable. Reconnecting automatically; showing last known sessions.")
      : tr("Background service unavailable. Retry to reconnect.");
  } else if (state.code === "explicit_handoff_required") {
    label = tr("Background handoff required");
    notice = tr("Background service is waiting for an explicit handoff.");
  } else if (paused) {
    // Passive by selection, not a fault. With "resume" the ownership notice
    // explains it and offers Resume; with "settings" its own reason below.
  } else if (state.code || !COMPONENTS.every((name) => state.background[name]?.ready)) {
    label = tr("Background service not ready");
    notice = tr("Background service is not ready. Some actions are unavailable.");
  } else if (!state.details_available) {
    label = tr("Service views unavailable");
    notice = tr("Activity, processes and project tasks are unavailable with this background service connection.");
  }
  const pending = pendingReadNotice(state), retained = retainedSourceNotice(state), inputs = heldInputsNotice(state), owner = ownershipNotice(state.notification_ownership);
  // Held copies are listed and released by the running background service:
  // a paused, not-ready or handoff-waiting one cannot, so it offers no review.
  // The notice still tells the user that copies are held.
  const reviewable = !!inputs && !manualRefresh && !paused && state.connected && !state.code
    && COMPONENTS.every((name) => state.background[name]?.ready);
  const mode = state.notification_ownership?.activation, access = state.notification_ownership?.status;
  const modeReview = !!mode?.review && !(mode.review.disposition === "reviewed_unknown" && mode.business_ready && mode.active
    && !mode.pending && !state.notification_ownership?.pending && !state.notification_ownership?.native_recovery
    && access?.state === "service" && access.identity_current && !access.rearm_required);
  const modeBlocked = !!mode?.review || mode?.automatic_blocked;
  const ownership = state.notification_ownership?.pending ? "check"
    : !modeBlocked && state.notification_ownership?.activation?.pending ? "check"
    : !manualRefresh && !modeBlocked && (state.notification_ownership?.native_recovery || state.notification_ownership?.status?.rearm_required
      || state.notification_ownership?.activation?.needs_resume) ? "rearm" : undefined;
  if (!notice) {
    if (pending) label = state.pending_read_import?.running ? tr("Transferring read marks") : tr("Read marks need attention");
    else if (retained) label = tr("Migration report");
    else if (reviewable) label = tr("Held message copies");
    else if (owner) label = ownership === "check" ? tr("Background access unconfirmed") : ownership === "rearm" ? tr("Background access paused") : tr("Background access unavailable");
    else if (paused === "settings") label = tr("Background access paused");
  }
  // A reason without Retry or Resume: no in-app action turns background mode on
  // here yet (zerus-5wmh.66.8.1.15); Settings shows the current state.
  const settings = paused === "settings" ? pausedSettingsBody(state) : "";
  const reasons = [notice, pending, retained, inputs, manualRefresh ? "" : owner, settings].filter(Boolean);
  return { label, details: reasons.join("\n") || tr("Background service connected"), healthy: !reasons.length,
    retry: !manualRefresh && !!(notice || pending || retained || (owner && !ownership)), review: !!retained, inputsReview: reviewable, ownership, manualRefresh, modeReview };
}
