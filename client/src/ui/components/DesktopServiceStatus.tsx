import { Show, createMemo, createSignal } from "solid-js";
import { Portal } from "solid-js/web";
import { desktopService, serviceStatus, retryDesktopService, ownershipActions } from "../../store/desktopService";
import { toast } from "../../store/ui";
import { tr } from "../../i18n";
import { MigrationReportDialog } from "./MigrationReportDialog";

export function DesktopServiceStatus(props: { counts?: boolean }) {
  const [reviewOrigin, setReviewOrigin] = createSignal<HTMLElement>();
  const [focused, setFocused] = createSignal(false);
  const status = createMemo(() => serviceStatus(desktopService.state()));
  const counts = () => desktopService.state()?.attention_counts?.aggregate;
  const details = () => [status()?.details, props.counts ? `${tr("Shared host attention (excludes this device's review marks)")}: ${counts()?.attention ?? tr("Unknown")}${counts()?.complete ? "" : `; ${tr("Incomplete")}`}` : ""].filter(Boolean).join("\n");
  return <Show when={status()}>{(view) => <>
    <div class="desktop-service-status" role="status" classList={{ healthy: view().healthy }}>
      <span class="service-summary" title={details()} aria-label={details()} tabIndex={0}
        aria-describedby={focused() ? "service-status-details" : undefined}
        onFocus={() => setFocused(true)} onBlur={() => setFocused(false)}
        onKeyDown={(event) => { if (event.key === "Escape") setFocused(false); }}>
        <span class="state-dot" classList={{ ready: view().healthy, attention: !view().healthy }} />{view().label}
      </span>
      <Show when={view().review}>
        <button type="button" class="btn small ghost" aria-label={tr("Review migration report")} onClick={(event) => setReviewOrigin(event.currentTarget)}>{tr("Review")}</button>
      </Show>
      <Show when={view().retry}><button type="button" class="btn small ghost" onClick={() => void retryDesktopService().catch(error => toast(tr("Background service"), String(error), "error", true))}>{tr("Retry now")}</button></Show>
      <Show when={view().ownership === "check"} fallback={
        <Show when={view().ownership === "rearm"}>
          <button type="button" class="btn small ghost"
            disabled={ownershipActions.busy() || desktopService.state()?.notification_ownership?.running}
            onClick={() => void ownershipActions.rearm().catch(error => toast(tr("Background service"), String(error), "error", true))}>{tr("Resume background work")}</button>
        </Show>
      }>
        <button type="button" class="btn small ghost"
          disabled={ownershipActions.busy() || desktopService.state()?.notification_ownership?.running}
          onClick={() => void ownershipActions.check().catch(error => toast(tr("Background service"), String(error), "error", true))}>{tr("Check result")}</button>
      </Show>
    </div>
    <Show when={focused()}><Portal><div id="service-status-details" class="service-status-details" role="tooltip">{details()}</div></Portal></Show>
    <Show when={reviewOrigin()}>{(origin) => <MigrationReportDialog origin={origin()} close={() => setReviewOrigin(undefined)} />}</Show>
  </>}</Show>;
}
