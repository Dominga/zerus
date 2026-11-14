import { Show, createUniqueId, onCleanup, onMount, type JSX } from "solid-js";
import { tr } from "../../i18n";
export function TeamEditDialog(props: { title: string; action: string; busy: boolean; disabled?: boolean; error: string; close: () => void; submit: () => void; children: JSX.Element }) {
  let dialog!: HTMLDialogElement;
  const id = createUniqueId();
  const origin = document.activeElement instanceof HTMLElement ? document.activeElement : undefined;
  onMount(() => { dialog.showModal(); queueMicrotask(() => dialog.querySelector<HTMLElement>("input,textarea,select")?.focus()); });
  onCleanup(() => { if (dialog.open) dialog.close(); if (origin?.isConnected) origin.focus(); });
  return <dialog ref={dialog} class="dlg team-member-edit" aria-labelledby={id} onCancel={(event) => { event.preventDefault(); if (!props.busy) props.close(); }}>
    <form onSubmit={(e) => { e.preventDefault(); if (!props.busy && !props.disabled) props.submit(); }}>
      <div class="dlg-head"><h2 id={id}>{props.title}</h2></div>
      <div class="dlg-body">{props.children}<Show when={props.error}><p class="danger" role="alert">{props.error}</p></Show></div>
      <div class="dlg-foot"><span class="spacer" /><button type="button" class="btn" disabled={props.busy} onClick={props.close}>{tr("Cancel")}</button>
        <button type="submit" class="btn primary" disabled={props.busy || props.disabled}>{props.action}</button></div>
    </form>
  </dialog>;
}
