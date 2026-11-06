import { Show, createSignal, onCleanup, onMount } from "solid-js";
import * as bridge from "../../bridge";
import { applyCleanup } from "../../store/terminationCleanup";
import { notify } from "../../store/actions";
import { tr } from "../../i18n";
import { Icon } from "../lib/icons";
import { TerminationPrograms } from "./TerminationPrograms";
import { readForceOffer } from "./force-offer";

/** Only this fresh confirmation can consume the native Force offer. Closing
 * leaves the known normal-stop result; no timeout or error confirms Force. */
export function ForceStopDialog(props: { id: string; label: string; close(): void }) {
  let dialog!: HTMLDialogElement;
  let closed = false, consumed = false;
  const [offer, setOffer] = createSignal<bridge.ProgramForceOffer>();
  const [error, setError] = createSignal(false);
  onMount(() => {
    dialog.showModal();
    void readForceOffer(props.id, { offer: bridge.terminationProgramForceOffer, cancel: bridge.terminationProgramForceCancel }, () => !closed)
      .then((value) => { if (!closed) setOffer(value); })
      .catch(() => { if (!closed) setError(true); });
  });
  onCleanup(() => {
    closed = true; dialog.close();
    const value = offer();
    if (value && !consumed) void bridge.terminationProgramForceCancel(props.id, value.confirmation).then(applyCleanup).catch(() => {});
  });
  const confirm = () => {
    const value = offer();
    if (!value || consumed) return;
    consumed = true;
    props.close();
    void bridge.terminationProgramForceConfirm(props.id, value.confirmation).then(applyCleanup)
      .catch((reason) => notify("error", String(reason), true));
  };
  return <dialog ref={dialog} class="dlg" aria-labelledby="force-stop-title" onCancel={props.close}>
    <div class="dlg-head"><Icon name="triangle-alert" size={18} /><h2 id="force-stop-title">{tr("Force stop programs?")}</h2></div>
    <div class="dlg-body"><p>{tr("These programs did not stop normally. Force stop them before removing %1?", props.label)}</p>
      <Show when={offer()} fallback={<p class="hint">{error() ? tr("The program list changed or could not be checked. Review the working folder again.") : tr("Checking…")}</p>}>{(value) => <TerminationPrograms force programs={value().programs} />}</Show>
    </div>
    <div class="dlg-foot"><span class="grow" /><button class="btn solid-danger" disabled={!offer()} onClick={confirm}>{tr("Force stop")}</button><button class="btn primary" autofocus onClick={props.close}>{tr("Cancel")}</button></div>
  </dialog>;
}
