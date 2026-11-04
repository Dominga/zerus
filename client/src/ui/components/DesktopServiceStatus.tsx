import { Show, createMemo, createSignal } from "solid-js";
import { Portal } from "solid-js/web";
import { desktopService, serviceStatus, servicePaused, retryDesktopService, ownershipActions, pausedSettingsBody, firstChoicePending } from "../../store/desktopService";
import { catalog } from "../../store/catalog";
import { toast } from "../../store/ui";
import { tr } from "../../i18n";
import { MigrationReportDialog } from "./MigrationReportDialog";
import { TaskMessageInputsDialog } from "./TaskMessageInputsDialog";

/** A connected passive service that lists no computers yet; the main area explains it. */
export const servicePausedEmpty = () => (!!servicePaused(desktopService.state()) || firstChoicePending(desktopService.state()))
  && !Object.keys(catalog.computers).length;

const resume = () => void ownershipActions.rearm().catch(error => toast(tr("Background service"), String(error), "error", true));
const resumeBlocked = () => ownershipActions.busy() || !!desktopService.state()?.notification_ownership?.running;
/** Settings > About shows the service state (and, after an interrupted change, its review). */
const openServiceSettings = () => void import("../screens/settings/sections").then(({ openSettings }) => openSettings("about"));

/**
 * The empty main area while background work is paused. Its one action is
 * Resume where the native helper offers it, otherwise the Settings page that
 * shows the service state (Windows).
 */
export function PausedServiceDetail() {
  const settings = () => servicePaused(desktopService.state()) === "settings" || firstChoicePending(desktopService.state());
  return (
    <div class="detail-empty paused-service">
      <h2>{tr("Background work is paused")}</h2>
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
  const [reviewOrigin, setReviewOrigin] = createSignal<HTMLElement>();
  const [inputsOrigin, setInputsOrigin] = createSignal<HTMLElement>();
  const [focused, setFocused] = createSignal(false);
  let summary: HTMLSpanElement | undefined;
  const status = createMemo(() => serviceStatus(desktopService.state()));
  const counts = () => desktopService.state()?.attention_counts?.aggregate;
  const details = () => [status()?.details, props.counts ? `${tr("Shared host attention (excludes this device's review marks)")}: ${counts()?.attention ?? tr("Unknown")}${counts()?.complete ? "" : `; ${tr("Incomplete")}`}` : ""].filter(Boolean).join("\n");
  return <Show when={status()}>{(view) => <>
    <div class="desktop-service-status" role="status" classList={{ healthy: view().healthy }}>
      <span ref={summary} class="service-summary" title={details()} aria-label={details()} tabIndex={0}
        aria-describedby={focused() ? "service-status-details" : undefined}
        onFocus={() => setFocused(true)} onBlur={() => setFocused(false)}
        onKeyDown={(event) => { if (event.key === "Escape") setFocused(false); }}>
        <span class="state-dot" classList={{ ready: view().healthy, attention: !view().healthy }} />{view().label}
      </span>
      <Show when={view().review}>
        <button type="button" class="btn small ghost" aria-label={tr("Review migration report")} onClick={(event) => setReviewOrigin(event.currentTarget)}>{tr("Review")}</button>
      </Show>
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
    <Show when={reviewOrigin()}>{(origin) => <MigrationReportDialog origin={origin()} close={() => setReviewOrigin(undefined)} />}</Show>
    <Show when={inputsOrigin()}>{(origin) => <TaskMessageInputsDialog origin={origin()} fallback={() => summary} close={() => setInputsOrigin(undefined)} />}</Show>
  </>}</Show>;
}
