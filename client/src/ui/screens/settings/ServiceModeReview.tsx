import { Show, createMemo } from "solid-js";
import { desktopService, modeReviewActions, ownershipActions } from "../../../store/desktopService";
import { toast } from "../../../store/ui";
import { tr } from "../../../i18n";
import { Actions, Group, Status } from "./ui";
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
  return <Show when={original()}>{(display) => <Group title={tr("Interrupted service mode change")}>
    <Status kind="warn">{display().disposition === "reviewed_unknown"
      ? tr("The review has been recorded.") : display().disposition === "reboot_required"
        ? tr("A restart is needed before reviewing again.") : tr("The earlier mode change needs review.")}</Status>
    <p class="set-group-hint">{tr("Zerus could not confirm whether the last change of the background service mode finished. Reviewing it records what Zerus can see now; it does not start or stop the service or change its mode.")}</p>
    <Show when={display().disposition === "reboot_required"}>
      <p class="set-group-hint">{display().passive_available
        ? tr("Restart Windows, then open Zerus and review again. Use Restart, not Shut down: with Fast Startup, Shut down keeps the previous session.")
        : tr("Restart your computer, then open Zerus and review again.")}</p>
    </Show>
    <Show when={display().disposition === "reviewed_unknown"}>
      <p class="set-group-hint">{tr("Review is recorded. Zerus will not change the service mode automatically. Choose a mode below.")}</p>
      <Show when={manual()}>
        <p class="set-group-hint">{tr("Zerus was updated. Follow the service update guidance before choosing a mode.")}</p>
      </Show>
      <Show when={choicePending() || accessPending()}>
        <p class="set-group-hint">{accessPending() ? tr("Background access is unconfirmed. Check the result before making another change.")
          : tr("The new mode choice is unconfirmed. Check its original result; do not make another choice.")}</p>
      </Show>
    </Show>
    <Show when={activation()?.code === "activation_not_started"}>
      <p class="set-group-hint">{tr("The new mode choice did not start. Check the registration and background access before confirming another choice.")}</p>
    </Show>
    <details>
      <summary>{tr("Technical details")}</summary>
      <dl class="selectable" style={{ "overflow-wrap": "anywhere" }}>
        <dt>{tr("Original request")}</dt><dd><code>{display().request_id}</code></dd>
        <dt>{tr("Configuration")}</dt><dd><code title={display().configuration} aria-label={display().configuration} tabIndex={0} onCopy={event => {
          event.clipboardData?.setData("text/plain", display().configuration); event.preventDefault();
        }}>{display().configuration.slice(0, 12)}…</code></dd>
        <Show when={activation()?.request_id && activation()?.request_id !== display().request_id}>
          <dt>{tr("New mode request")}</dt><dd><code>{activation()?.request_id}</code></dd>
        </Show>
      </dl>
    </details>
    <Actions>
      <Show when={display().disposition === "reviewed_unknown"} fallback={
        <button type="button" class="btn" disabled={busy()}
          onClick={() => act(display(), "review")}>{tr("Review interrupted mode change")}</button>
      }>
        <Show when={!choicePending() && !accessPending()}>
          <button type="button" class="btn" disabled={choosing() || manual()}
            onClick={() => act(display(), "background")}>{tr("Use background mode")}</button>
          <Show when={display().passive_available}>
            <button type="button" class="btn" disabled={choosing() || manual()}
              onClick={() => act(display(), "passive")}>{tr("Use passive mode")}</button>
          </Show>
        </Show>
      </Show>
      <button type="button" class="btn" disabled={busy() || ownershipActions.busy()}
        onClick={() => void ownershipActions.check().catch(() =>
          toast(tr("Background service"), tr("The original mode result is unavailable. Keep its request ID and check again."), "error", true))}>{tr("Check result")}</button>
    </Actions>
  </Group>}</Show>;
}
