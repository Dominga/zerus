import { For, Show } from "solid-js";
import type { JSX } from "solid-js";
import { tr } from "../../i18n";
import { Icon } from "../lib/icons";

// Task #8 (team ede724c4): New team's plan mode, which the owner chooses and
// the lead may offer, and the page's short how-to.

export type PlanModeChoice = "standard" | "free";

const PLAN_MODES: { value: PlanModeChoice; label: () => string; hint: () => string }[] = [
  { value: "standard", label: () => tr("Standard planning"),
    hint: () => tr("You approve the phases one after another; the team ends with a release.") },
  { value: "free", label: () => tr("Free planning"),
    hint: () => tr("Tasks come as a stream and are decided one by one; you close the phase when it is done.") },
];

const modeName = (mode: string) => PLAN_MODES.find((choice) => choice.value === mode)?.label() ?? mode;

export interface PlanModeFieldProps {
  /** The owner's choice: what Create team and the plan approval use. */
  mode: PlanModeChoice;
  /** The lead's offer ("" for none) and its one-line reason. */
  offer?: string;
  reason?: string;
  /** The lead changed the offer and the owner has not looked at it yet. */
  unseen?: boolean;
  onChoose: (mode: PlanModeChoice) => void;
  onSeen?: () => void;
}

/** The plan mode as an owner-editable field, before and after Start planning. */
export function PlanModeField(props: PlanModeFieldProps): JSX.Element {
  const offer = () => (props.offer === "standard" || props.offer === "free" ? props.offer : "");
  return (
    <section class="nt-plan-mode" data-plan-mode={props.mode} data-unseen={props.unseen ? "true" : undefined} classList={{ "nt-changed": !!props.unseen }}>
      <div class="nt-inline">
        <b>{tr("Plan mode")}</b>
        <Show when={props.unseen}>
          <button type="button" class="nt-unseen" data-unseen-mark="plan_mode" title={tr("Changed by the lead, not yet seen: click when you have seen it")}
            onClick={() => props.onSeen?.()}>●</button>
        </Show>
      </div>
      <div class="nt-plan-modes" role="radiogroup" aria-label={tr("Plan mode")}>
        <For each={PLAN_MODES}>{(choice) => (
          <label class="nt-plan-mode-choice" classList={{ on: props.mode === choice.value }} data-plan-mode-choice={choice.value}>
            <input type="radio" name="nt-plan-mode" value={choice.value} checked={props.mode === choice.value} onChange={() => props.onChoose(choice.value)} />
            <span class="nt-plan-mode-text">
              <span class="nt-plan-mode-label">{choice.label()}</span>
              <span class="nt-hint">{choice.hint()}</span>
            </span>
          </label>
        )}</For>
      </div>
      <Show when={offer()}>
        <div class="nt-inline nt-plan-offer" data-plan-offer={offer()}>
          <span class="nt-hint grow">{props.reason?.trim() ? tr("The lead offers %1: %2", modeName(offer()), props.reason.trim()) : tr("The lead offers %1", modeName(offer()))}</span>
          <Show when={offer() !== props.mode}>
            <button type="button" class="btn small" data-use-offer onClick={() => props.onChoose(offer() as PlanModeChoice)}>{tr("Use the lead's offer")}</button>
          </Show>
        </div>
      </Show>
    </section>
  );
}

export interface NewTeamHowToProps {
  open: boolean;
  onToggle: (open: boolean) => void;
}

/** What happens at each step of New team; collapsible, the form remembers it. */
export function NewTeamHowTo(props: NewTeamHowToProps): JSX.Element {
  const steps = () => [
    tr("Fill in the folder and the goal."),
    tr("Start planning starts only the lead, in the team folder."),
    tr("Talk to the lead in its chat to shape the plan."),
    tr("Fields the lead changed are marked ● until you look at them."),
    tr("Create team starts the members and their worktrees; nothing runs before that."),
  ];
  return (
    <section class="nt-howto" data-howto={props.open ? "open" : "closed"}>
      <button type="button" class="nt-howto-toggle" aria-expanded={props.open} onClick={() => props.onToggle(!props.open)}>
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
