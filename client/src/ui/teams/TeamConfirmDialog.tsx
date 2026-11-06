import { For, Show, onCleanup, onMount } from "solid-js";
import { Icon } from "../lib/icons";
import { teamConfirm, type Asking } from "../../store/teamConfirm";

// The owner's confirmation of a team action (store/teamConfirm.ts): the
// action's own button (or a choice's answers) and Cancel, Cancel focused;
// Escape and closing cancel.

function Dialog(props: { asking: Asking }) {
  let dialog!: HTMLDialogElement;
  const origin = document.activeElement instanceof HTMLElement ? document.activeElement : undefined;
  onMount(() => {
    dialog.showModal();
    queueMicrotask(() => dialog.querySelector<HTMLElement>("[data-initial]")?.focus());
  });
  onCleanup(() => { if (dialog.open) dialog.close(); if (origin?.isConnected) origin.focus(); });
  const focused = () => (document.activeElement instanceof HTMLElement ? document.activeElement.dataset.choice : undefined);
  return (
    <dialog ref={dialog} class="dlg team-confirm" data-name={props.asking.confirmation.name} aria-labelledby="team-confirm-title" aria-describedby="team-confirm-text"
      onCancel={(event) => { event.preventDefault(); teamConfirm.choose("cancel"); }}
      onKeyDown={(event) => { if (event.key === "Enter") { event.preventDefault(); teamConfirm.key("Enter", focused() ?? "cancel"); } }}>
      <div class="dlg-head"><Icon name="triangle-alert" size={18} class="warn" /><h2 id="team-confirm-title">{props.asking.confirmation.title}</h2></div>
      <div class="dlg-body"><p id="team-confirm-text" class="team-confirm-text">{props.asking.confirmation.text}</p></div>
      <div class="dlg-foot"><span class="spacer" />
        <For each={props.asking.buttons}>{(button) => (
          <button type="button" class="btn" classList={{ "solid-danger": button.danger, primary: button.initial }} data-choice={button.id}
            data-initial={button.initial ? "" : undefined} autofocus={button.initial} onClick={() => teamConfirm.choose(button.id)}>{button.text}</button>
        )}</For>
      </div>
    </dialog>
  );
}

export function TeamConfirmDialog() {
  return <Show when={teamConfirm.asking()} keyed>{(asking) => <Dialog asking={asking} />}</Show>;
}
