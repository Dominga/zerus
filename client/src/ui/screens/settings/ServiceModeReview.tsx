import { Show, createMemo } from "solid-js";
import { desktopService, modeReviewActions, ownershipActions } from "../../../store/desktopService";
import { toast } from "../../../store/ui";
import { tr } from "../../../i18n";
import type { ModeReview } from "../../../types";
import type { ModeReviewAction } from "../../../store/desktopService";

export function ServiceModeReview() {
  const activation = () => desktopService.state()?.notification_ownership?.activation;
  const original = () => activation()?.review;
  const manual = () => desktopService.state()?.executable_upgrade?.state === "manual_refresh_required";
  const busy = () => modeReviewActions.busy() || activation()?.running;
  const accessPending = () => desktopService.state()?.notification_ownership?.pending;
  const choosing = () => busy() || desktopService.state()?.notification_ownership?.running;
  const choicePending = createMemo(() => !!activation()?.pending && !!activation()?.request_id
    && activation()?.request_id !== original()?.request_id);
  const act = (display: ModeReview, action: ModeReviewAction) => {
    void modeReviewActions.submit(display, action).catch(error =>
      toast(tr("Background service"), String(error).includes("activation_helper_update_required")
        ? tr("Update Zerus and its background service, then reopen Zerus to review this change.")
        : tr("The mode action could not be confirmed. Check the original result before making another choice."), "error", true));
  };
  return <Show when={original()}>{(display) => <section aria-label={tr("Interrupted service mode change")}>
    <h2>{tr("Interrupted service mode change")}</h2>
    <p>{tr("The original manager result stays Unknown. Review does not stop or start the service, change its mode or resume background access.")}</p>
    <dl class="selectable" style={{ "overflow-wrap": "anywhere" }}>
      <dt>{tr("Original request")}</dt><dd>{display().request_id}</dd>
      <dt>{tr("Configuration")}</dt><dd>{display().configuration}</dd>
    </dl>
    <Show when={display().disposition === "reboot_required"}>
      <p>{tr("Use Restart, not Shut down, then open Zerus and choose Review interrupted mode change again. Fast Startup can keep the previous kernel session.")}</p>
    </Show>
    <Show when={display().disposition === "reviewed_unknown"} fallback={
      <button type="button" class="btn" disabled={busy()}
        onClick={() => act(display(), "review")}>{tr("Review interrupted mode change")}</button>
    }>
      <p>{tr("The interrupted change was reviewed. Automatic activation stays off. A new mode choice needs a separate native confirmation and fresh checks.")}</p>
      <Show when={manual()}>
        <p>{tr("A CLI update also needs attention. Follow the service update guidance before choosing a mode.")}</p>
      </Show>
      <Show when={choicePending() || accessPending()} fallback={<>
        <button type="button" class="btn" disabled={choosing() || manual()}
          onClick={() => act(display(), "background")}>{tr("Use background mode")}</button>
        <Show when={display().passive_available}>
          <button type="button" class="btn" disabled={choosing() || manual()}
            onClick={() => act(display(), "passive")}>{tr("Use passive mode")}</button>
        </Show>
      </>}>
        <p>{accessPending() ? tr("Background access is unconfirmed. Check the result before making another change.")
          : tr("The new mode choice is unconfirmed. Check its original result; do not make another choice.")}</p>
      </Show>
    </Show>
    <Show when={activation()?.request_id && activation()?.request_id !== display().request_id}>
      <p class="selectable">{tr("New mode request")}: {activation()?.request_id}</p>
    </Show>
    <Show when={activation()?.code === "activation_not_started"}>
      <p>{tr("The new mode choice did not start. Check the registration and background access before confirming another choice.")}</p>
    </Show>
    <button type="button" class="btn" disabled={busy() || ownershipActions.busy()}
      onClick={() => void ownershipActions.check().catch(() =>
        toast(tr("Background service"), tr("The original mode result is unavailable. Keep its request ID and check again."), "error", true))}>{tr("Check result")}</button>
  </section>}</Show>;
}
