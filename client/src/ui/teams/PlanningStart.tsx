import { Show, createUniqueId } from "solid-js";
import { tr } from "../../i18n";

export function PlanningStart(props: { planning: boolean; disabled: boolean; change: (value: boolean) => void }) {
  const name = createUniqueId();
  return <fieldset class="nt-section" disabled={props.disabled}>
    <legend>{tr("How to start")}</legend>
    <label><input type="radio" name={name} checked={!props.planning} onChange={() => props.change(false)} /> {tr("Start with a task queue")}</label>
    <label><input type="radio" name={name} checked={props.planning} onChange={() => props.change(true)} data-start-planning /> {tr("Plan a goal first")}</label>
    <Show when={props.planning}><p class="nt-hint">{tr("Only the lead starts. Other members and the integration branch wait for your approval of the initial plan.")}</p></Show>
  </fieldset>;
}

export function PlanningNotice(props: { status: string; goal: string; planning?: { known: boolean; paused: boolean; goal: string } | null }) {
  return <Show when={props.status === "planning"}><div class="notice" data-planning-notice>
    <b>{props.planning?.known ? props.planning.paused ? tr("Planning is paused. The lead's current work may continue.") : tr("The lead is preparing the initial plan.") : tr("Planning state could not be confirmed. Read the team again before resuming.")}</b>
    <p class="phase-description">{props.planning?.goal ?? props.goal}</p>
    <p>{tr("Other members and the integration branch wait for your approval of the initial plan.")}</p>
  </div></Show>;
}
