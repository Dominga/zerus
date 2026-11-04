import { Show } from "solid-js";
import { desktopService, firstChoiceActions, ownershipActions, ownershipNotice } from "../../../store/desktopService";
import { toast } from "../../../store/ui";
import { tr } from "../../../i18n";
import { Actions, Group, Status } from "./ui";

/**
 * The first explicit background choice on a plain passive Windows registration
 * (zerus-yfbd.1). Shown only when the controller offers it and ownership needs
 * nothing; an interrupted or reviewed change keeps B2's group instead. The
 * button only opens the native confirmation; consent and checks are native.
 */
export function ServiceFirstChoice() {
  const ownership = () => desktopService.state()?.notification_ownership;
  const activation = () => ownership()?.activation;
  const own = () => { const id = firstChoiceActions.request(); return !!id && activation()?.request_id === id; };
  const pending = () => own() && !!activation()?.pending;
  const notStarted = () => own() && activation()?.code === "activation_not_started";
  const offer = () => (activation()?.first && !ownershipNotice(ownership())) ? activation()!.first! : undefined;
  const visible = () => !activation()?.review && (!!offer() || pending() || notStarted());
  const busy = () => firstChoiceActions.busy() || !!activation()?.running || !!ownership()?.running;
  const choose = () => {
    const display = offer();
    if (display) void firstChoiceActions.submit(display).catch(() =>
      toast(tr("Background service"), tr("The mode action could not be confirmed. Check the original result before making another choice."), "error", true));
  };
  return <Show when={visible()}>
    <Group title={tr("Background service")}>
      <Status kind="warn">{tr("Background work is paused on this computer.")}</Status>
      <p class="set-group-hint">{tr("Turn on background mode to let Zerus run your sessions through the background service on this computer.")}</p>
      <Show when={pending()}>
        <p class="set-group-hint">{tr("The new mode choice is unconfirmed. Check its original result; do not make another choice.")}</p>
      </Show>
      <Show when={notStarted()}>
        <p class="set-group-hint">{tr("The new mode choice did not start. Check the registration and background access before confirming another choice.")}</p>
      </Show>
      <Actions>
        <Show when={offer() && !pending()}>
          <button type="button" class="btn" disabled={busy()} onClick={choose}>{tr("Use background mode…")}</button>
        </Show>
        <Show when={pending()}>
          <button type="button" class="btn" disabled={busy() || ownershipActions.busy()}
            onClick={() => void ownershipActions.check().catch(() =>
              toast(tr("Background service"), tr("The original mode result is unavailable. Keep its request ID and check again."), "error", true))}>{tr("Check result")}</button>
        </Show>
      </Actions>
    </Group>
  </Show>;
}
