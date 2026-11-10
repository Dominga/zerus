import { Show, createMemo, createSignal } from "solid-js";
import { Portal } from "solid-js/web";
import { desktopService, serviceStatus, servicePaused, retryDesktopService, ownershipActions, pausedSettingsBody, firstChoicePending, takeoverRefusal } from "../../store/desktopService";
import { catalog } from "../../store/catalog";
import { toast } from "../../store/ui";
import { tr } from "../../i18n";
import { Icon } from "../lib/icons";
import { lazyView } from "../lazy-view";
import { LazyBoundary } from "../lazy-boundary";
// Opened on demand, outside the entry chunk (zerus-ycycu).
const TaskMessageInputsDialog = lazyView(() => import("./TaskMessageInputsDialog"), (m) => m.TaskMessageInputsDialog);

/** A connected passive service that lists no computers yet; the main area explains it. */
export const servicePausedEmpty = () => (!!servicePaused(desktopService.state()) || firstChoicePending(desktopService.state()))
  && !Object.keys(catalog.computers).length;

const resume = () => void ownershipActions.rearm().catch(error => toast(tr("Background service"), String(error), "error", true));
const resumeBlocked = () => ownershipActions.busy() || !!desktopService.state()?.notification_ownership?.running;
/** Settings > About shows the service state (and, after an interrupted change, its review). */
const openServiceSettings = () => void import("../screens/settings/sections").then(({ openSettings }) => openSettings("about"));

/** Task 176: hgs refused the session state folder and the repair did not fix it. */
export const stateRefusal = () => desktopService.state()?.state_refusal || "";

/**
 * The session list while hgs refuses the session state folder: the refusal in
 * hgs's own words (folder, access entry, repair) and Retry, never loading rows.
 */
export function StateRefusalDetail() {
  return (
    <div class="empty sp-empty sp-refused" role="alert">
      <Icon name="triangle-alert" size={30} />
      <h3>{tr("Sessions cannot load")}</h3>
      <p class="sp-refusal">{stateRefusal()}</p>
      <p>{tr("Zerus makes a folder you own private when the background service starts. A folder another account owns must be fixed by its owner; then retry.")}</p>
      <button type="button" class="btn small" onClick={() => void retryDesktopService().catch(error => toast(tr("Background service"), String(error), "error", true))}>{tr("Retry now")}</button>
    </div>
  );
}

/** Task 180: the check that refused the native takeover, with its next step. */
export const takeoverRefused = () => takeoverRefusal(desktopService.state()?.notification_ownership);

/**
 * Task 180 (owner, 2026-11-07, local build 152: the list stayed empty): the
 * session list while the native takeover is refused: which check, why, and
 * the next step, never an empty list or a bare "paused".
 */
export function TakeoverRefusalDetail() {
  return (
    <Show when={takeoverRefused()}>{(refused) => (
      <div class="empty sp-empty sp-refused" role="alert">
        <Icon name="triangle-alert" size={30} />
        <h3>{tr("Zerus could not take over its sessions")}</h3>
        <p class="sp-refusal"><strong>{refused().check}</strong>: {refused().reason}</p>
        <p>{refused().next}</p>
      </div>
    )}</Show>
  );
}

/**
 * The empty main area while background work is paused. Its one action is
 * Resume where the native helper offers it, otherwise the Settings page that
 * shows the service state (Windows).
 */
export function PausedServiceDetail() {
  const settings = () => servicePaused(desktopService.state()) === "settings" || firstChoicePending(desktopService.state());
  return (
    <div class="detail-empty paused-service">
      <h2>{firstChoicePending(desktopService.state()) ? tr("Turning on background mode") : tr("Background work is paused")}</h2>
      <Show when={takeoverRefused()}>{(refused) => (
        <p class="paused-reason" role="alert"><strong>{refused().check}</strong>: {refused().reason} {refused().next}</p>
      )}</Show>
      <Show when={settings()} fallback={<>
        <p>{tr("Zerus runs your sessions through a background service, which stays paused until you resume it.")}</p>
        <button type="button" class="btn primary" disabled={resumeBlocked()} onClick={resume}>{tr("Resume background work")}</button>
      </>}>
        <p>{pausedSettingsBody(desktopService.state())}</p>
        <button type="button" class="btn primary" onClick={openServiceSettings}>{tr("Open background service settings")}</button>
      </Show>
    </div>
  );
}

export function DesktopServiceStatus(props: { counts?: boolean }) {
  const [inputsOrigin, setInputsOrigin] = createSignal<HTMLElement>();
  const [focused, setFocused] = createSignal(false);
  let summary: HTMLSpanElement | undefined;
  const status = createMemo(() => serviceStatus(desktopService.state()));
  const counts = () => desktopService.state()?.attention_counts?.aggregate;
  const details = () => [status()?.details, props.counts ? `${tr("Shared host attention (excludes this device's review marks)")}: ${counts()?.attention ?? tr("Unknown")}${counts()?.complete ? "" : `; ${tr("Incomplete")}`}` : ""].filter(Boolean).join("\n");
  return <Show when={status()}>{(view) => <>
    <Show when={props.counts || !view().healthy || view().inputsReview || view().ownership || view().manualRefresh || view().modeReview}>
    <div class="desktop-service-status" role="status" classList={{ healthy: view().healthy }}>
      <span ref={summary} class="service-summary" title={details()} aria-label={details()} tabIndex={0}
        aria-describedby={focused() ? "service-status-details" : undefined}
        onFocus={() => setFocused(true)} onBlur={() => setFocused(false)}
        onKeyDown={(event) => { if (event.key === "Escape") setFocused(false); }}>
        <span class="state-dot" classList={{ ready: view().healthy, attention: !view().healthy }} />{view().label}
      </span>
      <Show when={view().inputsReview}>
        <button type="button" class="btn small ghost" onClick={(event) => setInputsOrigin(event.currentTarget)}>{tr("Review message copies")}</button>
      </Show>
      <Show when={view().manualRefresh && !props.counts}>
        <button type="button" class="btn small ghost" onClick={openServiceSettings}>{tr("Review service update")}</button>
      </Show>
      <Show when={view().modeReview && !props.counts}>
        <button type="button" class="btn small ghost" onClick={() => {
          void import("../screens/settings/sections").then(({ openSettings }) => openSettings("about"));
        }}>{tr("Review interrupted mode change")}</button>
      </Show>
      <Show when={view().retry}><button type="button" class="btn small ghost" onClick={() => void retryDesktopService().catch(error => toast(tr("Background service"), String(error), "error", true))}>{tr("Retry now")}</button></Show>
      <Show when={view().ownership === "check"} fallback={
        <Show when={view().ownership === "rearm"}>
          <button type="button" class="btn small ghost" disabled={resumeBlocked()} onClick={resume}>{tr("Resume background work")}</button>
        </Show>
      }>
        <button type="button" class="btn small ghost"
          disabled={ownershipActions.busy() || desktopService.state()?.notification_ownership?.running}
          onClick={() => void ownershipActions.check().catch(error => toast(tr("Background service"), String(error), "error", true))}>{tr("Check result")}</button>
      </Show>
    </div>
    <Show when={focused()}><Portal><div id="service-status-details" class="service-status-details" role="tooltip">{details()}</div></Portal></Show>
    </Show>
    <Show when={inputsOrigin()}>{(origin) => <LazyBoundary floating><TaskMessageInputsDialog origin={origin()} fallback={() => summary} close={() => setInputsOrigin(undefined)} /></LazyBoundary>}</Show>
  </>}</Show>;
}
