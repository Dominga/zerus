import { createSignal } from "solid-js";
import { tr } from "../i18n";
import type { TeamChoice, TeamConfirmation } from "../types";

// The owner's confirmation of a team action (task 11), as Qt's TeamPanel::
// confirm asks it: a warning with the action's own button and Cancel, where
// Cancel is the default and the escape choice. One question at a time; a
// second waits for the first. The dialog (ui/teams/TeamConfirmDialog.tsx)
// draws `asking()` and focuses the button marked `initial`. Task 52: a
// question may offer several answers (Qt's subteamChoice: Stop sub-team,
// Keep it running), still with Cancel last and focused.

export interface ConfirmButton { id: string; text: string; danger: boolean; initial: boolean }

export interface Asking { confirmation: TeamConfirmation; buttons: ConfirmButton[] }

const cancelButton = (): ConfirmButton => ({ id: "cancel", text: tr("Cancel"), danger: false, initial: true });

/** The dialog's buttons: the action first, Cancel last and focused. */
export function confirmButtons(confirmation: TeamConfirmation): ConfirmButton[] {
  return [{ id: "confirm", text: confirmation.action || tr("Confirm"), danger: true, initial: false }, cancelButton()];
}

/** A choice's buttons: its answers in order, Cancel last and focused. */
export function choiceButtons(choice: TeamChoice): ConfirmButton[] {
  const focused = choice.choices.some((answer) => answer.id === choice.initial);
  return [...choice.choices.map((answer) => ({ id: answer.id, text: answer.text, danger: !!answer.danger, initial: answer.id === choice.initial })),
    { ...cancelButton(), initial: !focused }];
}

export function createConfirmer() {
  const [asking, setAsking] = createSignal<Asking | undefined>();
  const queue: { asking: Asking; done: (id: string | undefined) => void }[] = [];

  const next = () => setAsking(queue[0]?.asking);
  const push = (entry: { asking: Asking; done: (id: string | undefined) => void }) => {
    queue.push(entry);
    if (queue.length === 1) next();
  };

  /** Asks the owner; true only for the action's own button. */
  function ask(confirmation: TeamConfirmation): Promise<boolean> {
    return new Promise((done) => push({ asking: { confirmation, buttons: confirmButtons(confirmation) }, done: (id) => done(id === "confirm") }));
  }

  /** Asks the owner to choose; the answer's id, or undefined for Cancel and any other way out. */
  function askChoice(choice: TeamChoice): Promise<string | undefined> {
    const confirmation = { name: choice.name, title: choice.title, text: choice.text, action: "" };
    return new Promise((done) => push({ asking: { confirmation, buttons: choiceButtons(choice) }, done: (id) => done(id && id !== "cancel" ? id : undefined) }));
  }

  /** The owner's choice; any other way out of the dialog is Cancel. */
  function choose(id: string | undefined) {
    const first = queue.shift();
    if (!first) return;
    const known = first.asking.buttons.some((button) => button.id === id);
    first.done(known ? id : "cancel");
    next();
  }

  /** A key in the dialog: Escape cancels, Enter takes the focused button (Cancel unless the owner moved). */
  function key(name: string, focused: string = "cancel") {
    if (name === "Escape") choose("cancel");
    else if (name === "Enter") choose(focused);
  }

  return { asking, ask, askChoice, choose, key };
}

export const teamConfirm = createConfirmer();
