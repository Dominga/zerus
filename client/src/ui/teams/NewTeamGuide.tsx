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

// Task #31 (team ede724c4): one guide. Before a draft the intro tells how a
// team works and what Start working with a team does; the form's how-to
// (after it) adds only what the intro does not say.

/** The intro's steps, before a draft (owner, 2026-11-09). */
export function NewTeamIntroSteps(): JSX.Element {
  return (
    <div class="nt-intro-guide" data-intro-steps>
      <h3>{tr("How a team works")}</h3>
      <ol>
        <li>{tr("A lead plans the work: it reads your goal, drafts the members, their roles and the tasks, and asks you what it needs.")}</li>
        <li>{tr("You decide: nothing runs until you approve the team and its plan.")}</li>
        <li>{tr("Members work in their own folders or branches; reviewers check their work and mergers land it.")}</li>
        <li>{tr("You follow it in the Team tab: progress, questions waiting for you, reports and the plan.")}</li>
        <li>{tr("Everything stays editable: members, tasks and settings can change while the team works.")}</li>
      </ol>
    </div>
  );
}

/** What Start working with a team does, under its button. */
export function NewTeamIntroHint(): JSX.Element {
  return <p class="nt-hint" data-intro-hint>{tr("This creates a draft and starts the lead in the team folder.")}</p>;
}

export interface NewTeamHowToProps {
  open: boolean;
  onToggle: (open: boolean) => void;
}

/** What the draft's form adds to the intro; collapsible, closed by default,
 * the form remembers it. */
export function NewTeamHowTo(props: NewTeamHowToProps): JSX.Element {
  const steps = () => [
    tr("Fields the lead changed are marked ● until you look at them."),
    tr("Create team starts the members and their worktrees."),
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

// Task #13 (team ede724c4): the owner's settings the lead set in the draft
// stay the owner's; the lead's values are offers applied with one click.
const OFFER_LABELS: Record<string, () => string> = {
  base_branch: () => tr("Base branch"),
  branch_per_task: () => tr("Branch per task"),
  max_members: () => tr("Max members"),
  approval: () => tr("Approval"),
};

const offerValue = (value: unknown): string => {
  if (value === true) return tr("Yes");
  if (value === false) return tr("No");
  if (value === null || value === undefined || value === "") return tr("not set");
  return typeof value === "string" ? value : JSON.stringify(value);
};

export interface SettingOffersProps {
  offers?: Record<string, unknown>;
  onApply: (field: string, value: unknown) => void;
}

/** The lead's offers for the owner's settings, each with Apply. */
export function SettingOffers(props: SettingOffersProps): JSX.Element {
  const entries = () => Object.entries(props.offers ?? {});
  return (
    <Show when={entries().length > 0}>
      <section class="nt-setting-offers" data-setting-offers>
        <b>{tr("The lead offers settings only you set")}</b>
        <For each={entries()}>{([field, value]) => (
          <div class="nt-inline" data-setting-offer={field}>
            <span class="nt-hint grow">{tr("%1: %2", OFFER_LABELS[field]?.() ?? field, offerValue(value))}</span>
            <button type="button" class="btn small" onClick={() => props.onApply(field, value)}>{tr("Apply")}</button>
          </div>
        )}</For>
      </section>
    </Show>
  );
}
