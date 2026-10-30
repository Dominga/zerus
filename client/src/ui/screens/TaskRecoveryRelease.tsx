import { onCleanup, onMount } from "solid-js";
import { Icon } from "../lib/icons";

/** TaskDialogs' task-release-recovery confirmation, with Cancel focused. */
export default function TaskRecoveryRelease(props: { close: () => void; release: () => void; origin: HTMLElement }) {
  let dialog!: HTMLDialogElement, cancel!: HTMLButtonElement;
  onMount(() => { dialog.showModal(); queueMicrotask(() => cancel.focus()); });
  onCleanup(() => { if (dialog.open) dialog.close(); queueMicrotask(() => { if (props.origin.isConnected) props.origin.focus(); }); });
  return <dialog ref={dialog} class="dlg" aria-labelledby="task-release-recovery-title" style={{ width: "460px" }} onCancel={props.close} onClose={props.close}>
    <div class="dlg-head"><Icon name="triangle-alert" size={18} class="accent" /><h2 id="task-release-recovery-title">Release message recovery</h2><span class="grow" /><button class="icon-btn" aria-label="Close" onClick={props.close}><Icon name="x" size={16} /></button></div>
    <div class="dlg-body"><div class="confirm-body"><p>The previous message may already have reached the executor. Releasing this local record neither cancels nor resends it. Check the task and executor Activity before sending another message.</p></div></div>
    <div class="dlg-foot"><span class="spacer" /><button ref={cancel} class="btn" autofocus onClick={props.close}>Cancel</button><button class="btn solid-danger" onClick={props.release}>Release record</button></div>
  </dialog>;
}
