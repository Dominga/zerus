// Phase-header chips and finding details adapted from WhiteWh's phaseHeader
// and planModes presentation. All metric values are projected by client/core.
import { For, Show, onCleanup, onMount } from "solid-js";
import { tr } from "../../i18n";
import type { PhaseMetricsView } from "../../store/phase-metrics";
import "../styles/phase-metrics.css";

function unavailableText(metrics: PhaseMetricsView | null | undefined): string {
  switch (metrics?.unavailable) {
    case "missing_estimates": return tr("Estimates available for %1 of %2 tasks.", metrics.known_estimates, metrics.total_nodes);
    case "empty_phase": return tr("This phase has no tasks to estimate.");
    case "invalid_graph": return tr("The phase graph cannot be measured.");
    case "scope_too_large": return tr("This phase exceeds the metric computation bound.");
    case "unknown_fields": return tr("This phase has fields the metric calculation does not understand.");
    case "live_snapshot_unavailable": return tr("Live metrics need a complete phase snapshot.");
    default: return tr("The complete phase has not been read.");
  }
}

/** A separate header component: no phase actions, reads or polling. Old core
 * views and stale/read-failed scopes stay unavailable instead of being measured
 * in the renderer. No chip is shown when there is no non-fatal engine finding. */
export function PhaseMetrics(props: { metrics?: PhaseMetricsView | null; readFailed?: boolean }) {
  let details: HTMLDetailsElement | undefined;
  const current = () => props.readFailed ? undefined : props.metrics;
  const available = () => !!current() && current()!.unavailable === null
    && current()!.critical_path_minutes != null && current()!.average_parallelism_tenths != null;
  const reason = () => props.readFailed ? tr("Phase metrics are unavailable while its read has failed.") : unavailableText(current());
  const warnings = () => current()?.warnings ?? [];
  const closeOutside = (event: PointerEvent) => { if (details?.open && event.target instanceof Node && !details.contains(event.target)) details.open = false; };
  onMount(() => document.addEventListener("pointerdown", closeOutside));
  onCleanup(() => document.removeEventListener("pointerdown", closeOutside));
  return <div class="phase-metrics" role="group" aria-label={tr("Phase metrics")} data-phase-metrics>
    <span class="tag" data-phase-path title={available() ? tr("Estimated critical path in minutes within this phase.") : reason()}>
      {available() ? tr("Path %1 min", current()!.critical_path_minutes!) : tr("Path unavailable")}
    </span>
    <span class="tag" data-phase-parallelism title={available() ? tr("Estimated work divided by the estimated path duration within this phase.") : reason()}>
      {available() ? tr("Parallelism %1", (current()!.average_parallelism_tenths! / 10).toFixed(1)) : tr("Parallelism unavailable")}
    </span>
    <Show when={!available()}><span class="dim phase-metric-reason" data-phase-metric-reason>{reason()}</span></Show>
    <Show when={available() && current()!.external_dependencies > 0}><span class="dim">{tr("Waits outside this phase are excluded.")}</span></Show>
    <Show when={warnings().length > 0}>
      <details ref={details} class="phase-metric-warnings" onKeyDown={(event) => {
        if (event.key === "Escape" && details?.open) { event.preventDefault(); event.stopPropagation(); details.open = false; details.querySelector("summary")?.focus(); }
      }}>
        <summary class="tag" data-phase-warnings>{tr("%1 plan warnings", warnings().length)}</summary>
        <div class="phase-metric-findings" role="region" aria-label={tr("Plan warnings")}><ul><For each={warnings()}>{(warning) => <li>{warning}</li>}</For></ul></div>
      </details>
    </Show>
  </div>;
}
