import { Show, onCleanup, onMount } from "solid-js";
import { Icon } from "../lib/icons";
import { tr } from "../../i18n";
import { teams } from "../../store/teams";

function advanceText(asking: NonNullable<ReturnType<typeof teams.confirmation>>) {
  return tr("Advance the base of %1 to %2? Future hand-outs use it. Earlier hand-outs keep their assigned base. Branches are unchanged.", asking.call.team_name || asking.call.team, String(asking.call.action.commit));
}

// The person's confirmation of a destructive team action: the core refused to
// send it (`confirm`), so nothing ran yet. The dialog shows the core's own
// title and text; its action button sends the same intent again marked
// confirmed, and Cancel drops it. Escape and closing cancel.

export function TeamConfirmation(props: { name: string; title: string; text: string; action: string; onCancel: () => void; onConfirm: () => void }) {
  let dialog!: HTMLDialogElement;
  const origin = document.activeElement instanceof HTMLElement ? document.activeElement : undefined;
  onMount(() => {
    dialog.showModal();
    queueMicrotask(() => dialog.querySelector<HTMLElement>("[data-initial]")?.focus());
  });
  onCleanup(() => { if (dialog.open) dialog.close(); if (origin?.isConnected) origin.focus(); });
  return (
    <dialog ref={dialog} class="dlg team-confirm" data-name={props.name}
      aria-labelledby="team-confirm-title" aria-describedby="team-confirm-text"
      onCancel={(event) => { event.preventDefault(); props.onCancel(); }}>
      <div class="dlg-head"><Icon name="triangle-alert" size={18} class="warn" /><h2 id="team-confirm-title">{props.title}</h2></div>
      <div class="dlg-body"><p id="team-confirm-text" class="team-confirm-text">{props.text}</p></div>
      <div class="dlg-foot">
        <span class="spacer" />
        <button type="button" class="btn solid-danger" onClick={props.onConfirm}>{props.action}</button>
        <button type="button" class="btn primary" data-initial="" autofocus onClick={props.onCancel}>{tr("Cancel")}</button>
      </div>
    </dialog>
  );
}

export function TeamConfirmDialog(props: { source?: string } = {}) {
  return (
    <Show when={teams.confirmation()} keyed>
      {(asking) => (
        <TeamConfirmation name={asking.confirmation.name} title={asking.call.action.kind === "advance_base" ? tr("Advance base?") : asking.confirmation.title}
          text={asking.call.action.kind === "advance_base" ? advanceText(asking) : asking.confirmation.text} action={asking.call.action.kind === "advance_base" ? tr("Advance base") : asking.confirmation.action} onConfirm={() => void teams.confirm(props.source)} onCancel={() => teams.cancelConfirmation()} />
      )}
    </Show>
  );
}
