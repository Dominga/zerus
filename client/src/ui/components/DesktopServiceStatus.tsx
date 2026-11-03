import { Show, createMemo, createSignal } from "solid-js";
import { desktopService, pendingReadNotice, retainedSourceNotice, retryDesktopService, ownershipActions, ownershipNotice } from "../../store/desktopService";
import { toast } from "../../store/ui";
import { tr } from "../../i18n";
import { MigrationReportDialog } from "./MigrationReportDialog";

export function DesktopServiceStatus(props: { counts?: boolean }) {
  const [reviewOrigin, setReviewOrigin] = createSignal<HTMLElement>();
  const notice = createMemo(() => {
    const state = desktopService.state();
    if (!state?.connected) return state?.reconnecting
      ? tr("Background service unavailable. Reconnecting automatically; showing last known sessions.")
      : tr("Background service unavailable. Retry to reconnect.");
    if (state.code === "explicit_handoff_required") return tr("Background service is waiting for an explicit handoff.");
    if (state.code || !["fleet", "presence", "reads", "attention", "approvals"].every(desktopService.ready)) return tr("Background service is not ready. Some actions are unavailable.");
    if (!state.details_available) return tr("Activity, processes and project tasks are unavailable with this background service connection.");
    return "";
  });
  const counts = () => desktopService.state()?.attention_counts?.aggregate;
  return <Show when={desktopService.enabled()}>
    <div class="desktop-service-status" role="status">
      <Show when={notice()} fallback={<span>{tr("Background service connected")}</span>}>{(text) => <span>{text()}</span>}</Show>
      <Show when={pendingReadNotice(desktopService.state())}>{(text) => <span>{text()}</span>}</Show>
      <Show when={retainedSourceNotice(desktopService.state())}>{(text) => <>
        <span>{text()}</span>
        <button type="button" class="btn small ghost" aria-label={tr("Review migration report")} onClick={(event) => setReviewOrigin(event.currentTarget)}>{tr("Review")}</button>
      </>}</Show>
      <button type="button" class="btn small ghost" onClick={() => void retryDesktopService().catch(error => toast(tr("Background service"), String(error), "error", true))}>{tr("Retry now")}</button>
      <Show when={ownershipNotice(desktopService.state()?.notification_ownership)}>{(text) => <span>{text()}</span>}</Show>
      <Show when={desktopService.state()?.notification_ownership?.pending} fallback={
        <Show when={desktopService.state()?.notification_ownership?.status?.rearm_required}>
          <button type="button" class="btn small ghost"
            disabled={ownershipActions.busy() || desktopService.state()?.notification_ownership?.running}
            onClick={() => void ownershipActions.rearm().catch(error => toast(tr("Background service"), String(error), "error", true))}>{tr("Resume background work")}</button>
        </Show>
      }>
        <button type="button" class="btn small ghost"
          disabled={ownershipActions.busy() || desktopService.state()?.notification_ownership?.running}
          onClick={() => void ownershipActions.check().catch(error => toast(tr("Background service"), String(error), "error", true))}>{tr("Check result")}</button>
      </Show>
      <Show when={props.counts}>
        <span>{tr("Shared host attention (excludes this device's review marks)")}: {counts()?.attention ?? tr("Unknown")}</span>
        <Show when={!counts()?.complete}><span>{tr("Incomplete")}</span></Show>
      </Show>
    </div>
    <Show when={reviewOrigin()}>{(origin) => <MigrationReportDialog origin={origin()} close={() => setReviewOrigin(undefined)} />}</Show>
  </Show>;
}
