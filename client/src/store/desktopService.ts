import { createSignal } from "solid-js";
import { invoke } from "@tauri-apps/api/core";
import type { DesktopService, NotificationOwnership, ModeReview, FirstChoiceOffer } from "../types";
import { tr } from "../i18n";

/** Presence of this projection is the startup mode. It never grants writer authority. */
export function createDesktopService() {
  const [state, setState] = createSignal<DesktopService>();
  return {
    state, apply: (next: DesktopService) => { setState(next); },
    enabled: () => state() !== undefined,
    ready: (component: string) => !!state()?.connected && state()?.background[component]?.ready === true,
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
      return { check: tr("Process ancestry"), reason: tr("Zerus was started from an agent's session."), next: tr("Close it and start Zerus from the Start menu or its folder.") };
    // Task #53 (owner, 2026-11-10, build 175): why the install before the
    // takeover recorded no binding for this app, with the step that fixes it.
    case "frontend_parent_not_beside":
    case "frontend_ancestry_unverified":
      return { check: tr("Process ancestry"), reason: tr("Zerus was started by another program (an installer or a terminal), so hgs could not record this app."),
        next: tr("Close Zerus and start it from the Start menu or Explorer.") };
    case "frontend_parent_gone":
      return { check: tr("Process ancestry"), reason: tr("The program that started Zerus ended while hgs recorded this app."),
        next: tr("Close Zerus and start it from the Start menu or Explorer.") };
    case "frontend_record_timed_out":
      return { check: tr("App binding"), reason: tr("Recording this app took too long: the computer was busy."), next: tr("Restart Zerus when the computer is less busy.") };
    case "frontend_cli_pin_changed":
    case "frontend_cli_image_unreadable":
    case "frontend_app_unreadable":
    case "frontend_record_refused":
      return { check: tr("App binding"), reason: tr("hgs could not read or pin the program files while it recorded this app."), next: reinstall };    // Task #421 (2026-11-09, package 167): the takeover's hgs child failed
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
    if (HELPER_UNAVAILABLE.includes(view.activation.code)) {
      const text = tr("Background activation is unavailable. Update Zerus and its desktop service, then Resume to check again.");
      // Task #59: the refused check's own words, e.g. why the helper could not be pinned.
      return view.activation.reason ? `${text}\n${tr("Why: %1", view.activation.reason)}` : text;
    }
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
    const reads = pendingReadNotice(state), inputs = heldInputsNotice(state);
    const access = ownership?.pending ? ownershipNotice(ownership) : "";
    // The same readiness rule as below: no held-copy Review while not ready.
    const inputsReview = !!inputs && state.connected && !state.code && COMPONENTS.every((name) => state.background[name]?.ready);
    return { label: tr("Turning on background mode"),
      details: [tr("The new mode choice is unconfirmed. Check its original result; do not make another choice."), reads, inputs, access]
        .filter(Boolean).join("\n"),
      healthy: false, retry: false, inputsReview, ownership: "check" as const, manualRefresh: false, modeReview: false };
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
  const pending = pendingReadNotice(state), inputs = heldInputsNotice(state), owner = ownershipNotice(state.notification_ownership);
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
    else if (reviewable) label = tr("Held message copies");
    else if (owner) label = ownership === "check" ? tr("Background access unconfirmed") : ownership === "rearm" ? tr("Background access paused") : tr("Background access unavailable");
    else if (paused === "settings") label = tr("Background access paused");
  }
  // A reason without Retry or Resume: no in-app action turns background mode on
  // here yet (zerus-5wmh.66.8.1.15); Settings shows the current state.
  const settings = paused === "settings" ? pausedSettingsBody(state) : "";
  const reasons = [notice, pending, inputs, manualRefresh ? "" : owner, settings].filter(Boolean);
  return { label, details: reasons.join("\n") || tr("Background service connected"), healthy: !reasons.length,
    retry: !manualRefresh && !!(notice || pending || (owner && !ownership)), inputsReview: reviewable, ownership, manualRefresh, modeReview };
}
