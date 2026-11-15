import { Show } from "solid-js";
import { tr } from "../../i18n";
import type { RecordedFacts } from "../../store/teamDrawers";
import { recordedDate } from "./recorded-time";

export function TeamRecordedTimings(props: { facts: RecordedFacts }) {
  const stamp = (ms: number) => recordedDate(ms)?.text ?? tr("Recorded time unavailable");
  return <Show when={props.facts.effect || props.facts.stage || props.facts.effect_unavailable || props.facts.stage_unavailable}>
    <details class="team-recorded-timings"><summary>{tr("Recorded timing facts")}</summary>
      <Show when={props.facts.effect}>{(effect) => <dl>
        <dt>{tr("Effect")}</dt><dd>{effect().record.request_id}</dd>
        <dt>{tr("Task / generation / member")}</dt><dd>{effect().record.node} / {effect().record.generation} / {effect().record.member}</dd>
        <dt>{tr("Recorded effect state")}</dt><dd>{effect().record.kind}: {effect().record.state}</dd>
        <dt>{tr("Reservation recorded")}</dt><dd>{stamp(effect().record.reserved_ms)}</dd>
        <dt>{tr("First outcome recorded")}</dt><dd>{effect().record.first_outcome_recorded_ms === null ? tr("Not recorded") : stamp(effect().record.first_outcome_recorded_ms!)}</dd>
        <dt>{tr("Delivery bookkeeping elapsed")}</dt><dd>{effect().bookkeeping_elapsed_ms === null ? tr("Unavailable") : tr("%1 ms", effect().bookkeeping_elapsed_ms!)}</dd>
        <Show when={effect().clock_or_range_unknown}><dt>{tr("Timing coverage")}</dt><dd>{tr("Elapsed time is unavailable because the recorded clocks or range are inconsistent.")}</dd></Show>
      </dl>}</Show>
      <Show when={props.facts.stage}>{(stage) => <dl>
        <dt>{tr("Stage / target / generation")}</dt><dd>{stage().node} / {stage().target} / {stage().generation}</dd>
        <dt>{tr("Stage creation recorded")}</dt><dd>{stamp(stage().created_ms)}</dd>
        <dt>{tr("Recorded creation state")}</dt><dd>{stage().state}</dd>
      </dl>}</Show>
      <Show when={props.facts.effect_unavailable || props.facts.stage_unavailable}><p>{tr("Some referenced timing facts are unavailable.")}</p></Show>
      <p class="dim">{tr("These are recorded delivery and stage facts, not measured work or test duration.")}</p>
    </details>
  </Show>;
}
