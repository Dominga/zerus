import { createSignal } from "solid-js";
import { tr } from "../i18n";
import type { TeamConfirmation } from "../types";

// The owner's confirmation of a team action (task 11), as Qt's TeamPanel::
// confirm asks it: a warning with the action's own button and Cancel, where
// Cancel is the default and the escape choice. One question at a time; a
// second waits for the first. The dialog (ui/teams/TeamConfirmDialog.tsx)
// draws `asking()` and focuses the button marked `initial`.

export interface ConfirmButton { id: "cancel" | "confirm"; text: string; danger: boolean; initial: boolean }

export interface Asking { confirmation: TeamConfirmation; buttons: ConfirmButton[] }

/** The dialog's buttons: the action first, Cancel last and focused. */
export function confirmButtons(confirmation: TeamConfirmation): ConfirmButton[] {
  return [
    { id: "confirm", text: confirmation.action || tr("Confirm"), danger: true, initial: false },
    { id: "cancel", text: tr("Cancel"), danger: false, initial: true },
  ];
}

export function createConfirmer() {
  const [asking, setAsking] = createSignal<Asking | undefined>();
  const queue: { confirmation: TeamConfirmation; done: (yes: boolean) => void }[] = [];

  const next = () => {
    const first = queue[0];
    setAsking(first ? { confirmation: first.confirmation, buttons: confirmButtons(first.confirmation) } : undefined);
  };

  /** Asks the owner; true only for the action's own button. */
  function ask(confirmation: TeamConfirmation): Promise<boolean> {
    return new Promise((done) => {
      queue.push({ confirmation, done });
      if (queue.length === 1) next();
    });
  }

  /** The owner's choice; any other way out of the dialog is Cancel. */
  function choose(id: ConfirmButton["id"] | undefined) {
    const first = queue.shift();
    if (!first) return;
    first.done(id === "confirm");
    next();
  }

  /** A key in the dialog: Escape cancels, Enter takes the focused button (Cancel unless the owner moved). */
  function key(name: string, focused: ConfirmButton["id"] = "cancel") {
    if (name === "Escape") choose("cancel");
    else if (name === "Enter") choose(focused);
  }

  return { asking, ask, choose, key };
}

export const teamConfirm = createConfirmer();
