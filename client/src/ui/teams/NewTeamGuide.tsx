import { For, Show } from "solid-js";
import type { JSX } from "solid-js";
import { tr } from "../../i18n";
import { Icon } from "../lib/icons";

// New team's short guide for M1: how a standing team works, and what Create
// team does. The fork's plan modes and setting offers belong to planned work
// (M2) and are not here.

/** The intro's steps. */
export function NewTeamIntroSteps(props: { planning?: boolean } = {}): JSX.Element {
  return (
    <div class="nt-intro-guide" data-intro-steps>
      <h3>{tr("How a team works")}</h3>
      <ol>
        <li>{props.planning ? tr("The lead starts in its own worktree to prepare your plan.") : tr("A lead takes the tasks you send and hands them to the members with the right role.")}</li>
        <li>{tr("Every task runs through its pipeline, for example an implementation and a review.")}</li>
        <li>{tr("Accepted work is merged into one integration branch; you follow it in the Team tab.")}</li>
        <li>{props.planning ? tr("Nothing starts until you press Start planning.") : tr("Nothing runs before you press Create team.")}</li>
      </ol>
    </div>
  );
}

/** What Create team does, under the form. */
export function NewTeamIntroHint(): JSX.Element {
  return <p class="nt-hint" data-intro-hint>{tr("This creates the team and its members' sessions in the project folder.")}</p>;
}

export interface NewTeamHowToProps {
  open: boolean;
  onToggle: (open: boolean) => void;
}

/** The form's short how-to: collapsible, closed by default. */
export function NewTeamHowTo(props: NewTeamHowToProps): JSX.Element {
  const steps = () => [
    tr("The template brings the roles, the pipelines and the team's settings."),
    tr("The folder must be a Git folder: the members work in their own worktrees."),
    tr("Save as template keeps the template with your name for it."),
  ];
  return (
    <section class="nt-howto" data-howto={props.open ? "open" : "closed"}>
      <button type="button" class="btn ghost small nt-howto-toggle" aria-expanded={props.open} onClick={() => props.onToggle(!props.open)}>
        <Icon name={props.open ? "chevron-down" : "chevron-right"} size={13} />
        <span>{tr("How it works")}</span>
      </button>
      <Show when={props.open}>
        <ol class="nt-howto-steps">
          <For each={steps()}>{(step) => <li>{step}</li>}</For>
        </ol>
      </Show>
    </section>
  );
}
