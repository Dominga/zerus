// Adapted from WhiteWh's PlanView toolbar and plan-panels segmented controls.
import { For, Show } from "solid-js";
import { tr } from "../../i18n";
import { PLAN_ZOOM_MIN, PLAN_ZOOM_MAX, type PlanPreferences } from "../../store/teamPlan";
import { Icon } from "../lib/icons";
import "../styles/team-plan-toolbar.css";

/** Graph supplies geometry actions; Layers can omit zoom and reuse filters,
 * legend and section folding with the same viewer preferences. */
export function PlanToolbar(props: {
  state: PlanPreferences;
  onChange: (patch: Partial<PlanPreferences>) => void;
  onFit?: () => void;
  showOrigins?: boolean;
  sections?: { allCollapsed: boolean; onFold: (collapsed: boolean) => void };
}) {
  const zoom = (delta: number) => props.onChange({ zoom: Math.round((props.state.zoom + delta) * 10) / 10 });
  return <div class="team-plan-toolbar" role="group" aria-label={tr("Plan toolbar")}>
    <div class="team-plan-controls" role="radiogroup" aria-label={tr("Task filter")}>
      <For each={[["all", tr("All")], ["active", tr("Active")], ["problems", tr("Problems")]] as const}>{([value, label]) =>
        <button type="button" class="btn small" role="radio" aria-checked={props.state.filter === value} data-plan-filter={value} onClick={() => props.onChange({ filter: value })}>{label}</button>
      }</For>
    </div>
    <div class="team-plan-controls">
      <button type="button" class="btn small" aria-pressed={props.state.showCancelled} data-plan-toggle="cancelled" onClick={() => props.onChange({ showCancelled: !props.state.showCancelled })}><Icon name="eye" size={12} />{tr("Show cancelled")}</button>
      <button type="button" class="btn small" aria-pressed={props.state.hideFinished} data-plan-toggle="finished" onClick={() => props.onChange({ hideFinished: !props.state.hideFinished })}><Icon name="eye-off" size={12} />{tr("Hide finished")}</button>
    </div>
    <div class="team-plan-legend" aria-label={tr("Task states")}>
      <span><i data-plan-swatch="done" />{tr("Done")}</span>
      <span><i data-plan-swatch="work" />{tr("In progress")}</span>
      <span><i data-plan-swatch="review" />{tr("In review")}</span>
      <span><i data-plan-swatch="notstarted" />{tr("Not started")}</span>
      <span><i data-plan-swatch="cancelled" />{tr("Cancelled")}</span>
    </div>
    <Show when={props.onFit}>
      <span class="team-plan-legend"><span><i class="team-plan-dependency-key" />{tr("Depends on")}</span></span>
      <Show when={props.showOrigins}><span class="team-plan-legend"><span><i class="team-plan-origin-key" />{tr("Spawned from")}</span></span></Show>
      <div class="team-plan-controls">
        <button type="button" class="btn small" aria-label={tr("Zoom out")} disabled={props.state.zoom <= PLAN_ZOOM_MIN} onClick={() => zoom(-0.1)}><Icon name="minus" size={12} /></button>
        <button type="button" class="btn small team-plan-zoom" aria-label={tr("Reset zoom")} data-plan-zoom={props.state.zoom} onClick={() => props.onChange({ zoom: 1 })}>{Math.round(props.state.zoom * 100)}%</button>
        <button type="button" class="btn small" aria-label={tr("Zoom in")} disabled={props.state.zoom >= PLAN_ZOOM_MAX} onClick={() => zoom(0.1)}><Icon name="plus" size={12} /></button>
        <button type="button" class="btn small" onClick={() => props.onFit?.()}><Icon name="maximize-2" size={12} />{tr("Fit")}</button>
      </div>
    </Show>
    <Show when={props.sections}>{(sections) => <button type="button" class="btn small" data-plan-fold-all onClick={() => sections().onFold(!sections().allCollapsed)}>
      <Icon name={sections().allCollapsed ? "chevrons-down" : "arrow-up-to-line"} size={12} />{sections().allCollapsed ? tr("Expand all") : tr("Collapse all")}
    </button>}</Show>
  </div>;
}
