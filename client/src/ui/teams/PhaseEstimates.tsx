import { For, Show } from "solid-js";
import { tr } from "../../i18n";
import type { ComparisonUnknown, EstimatePaneView, EstimateUnknown, PhaseEstimatesView } from "../../store/phase-estimates";
import "../styles/phase-estimates.css";

export function estimateUnknownText(reason: EstimateUnknown | null | undefined): string {
  switch (reason) {
    case "source_identity_unavailable": return tr("The estimate source identity is unavailable.");
    case "stale_revision": return tr("The phase revision changed. Read the selected scope again.");
    case "generation_unavailable": return tr("The draft generation is unavailable.");
    case "history_unavailable": return tr("That publication or generation was not returned by this read.");
    case "unknown_fields": return tr("The estimate source has unsupported fields.");
    case "incomplete_graph": return tr("The phase graph is incomplete for this read.");
    case "scope_too_large": return tr("The estimate cohort exceeds its read bound.");
    case "incomplete_draft": return tr("The complete draft has not been read.");
    case "unsettled_publication": return tr("The approved publication has not settled.");
    case "incomplete_live": return tr("The complete published phase snapshot has not been read.");
    case "conflicting_membership": return tr("The publication and task identities do not agree.");
    case "missing_estimate": return tr("No implementation estimate was supplied.");
    case "estimate_over_bound": return tr("The implementation estimate exceeds its supported bound.");
    case "external_prerequisite": return tr("A prerequisite is outside this estimate cohort.");
    case "not_implementation": return tr("This is not an implementation unit.");
    default: return tr("Estimates are unavailable for the selected scope.");
  }
}

/** No source read, estimate editor, actual-time inference or local cache.
 * The shared observed-page lifecycle will own mounting and retiring this view. */
function comparisonUnknownText(reason: ComparisonUnknown): string {
  switch (reason) {
    case "estimate_unavailable": return tr("The estimate is unavailable for this unit.");
    case "outside_cohort": return tr("This observed unit is outside the frozen estimate cohort.");
    case "not_implementation": return tr("This is not an implementation unit.");
    case "original_publication_unavailable": return tr("The original observation did not retain its publication identity.");
    case "different_publication": return tr("This observation belongs to a different publication or generation.");
    case "different_task": return tr("This observation belongs to a different task identity.");
    case "partial_coverage": return tr("This unit has partial or unavailable observation coverage.");
    case "implementation_unsupported": return tr("The source does not measure implementation work.");
  }
}
export function PhaseEstimates(props: { value?: PhaseEstimatesView | EstimatePaneView | null; readFailed?: boolean }) {
  const current = () => props.readFailed ? undefined : props.value;
  const comparison = () => { const value = current(); return value && "units" in value ? value as EstimatePaneView : undefined; };
  const known = () => !!current()?.cohort_complete && current()?.unknown === null;
  return <section class="phase-estimates" data-phase-estimates aria-label={tr("Implementation estimates")}>
    <h4>{tr("Implementation estimates")}</h4>
    <p class="dim">{tr("Implementation estimates are not phase elapsed time or a completion promise.")}</p>
    <Show when={current()?.scope} fallback={<p role="status">{estimateUnknownText(current()?.unknown)}</p>}>{(scope) => <>
      <div class="phase-estimate-source">
        <span class="tag">{scope().basis === "unapproved_preview" ? tr("Unapproved draft preview") : tr("Frozen publication estimates")}</span>
        <span class="tag">{tr("Draft generation %1", scope().version.draft_generation)}</span>
        <span class="dim">{tr("Source revision %1", scope().revision)}</span>
      </div>
      <Show when={scope().basis === "unapproved_preview"}><p class="dim">{tr("This preview is not approved and has no actual comparison.")}</p></Show>
      <Show when={current()?.unknown}>{(reason) => <p role="status">{estimateUnknownText(reason())}</p>}</Show>
      <table><thead><tr><th>{tr("Task")}</th><th>{tr("Implementation estimate")}</th></tr></thead><tbody>
        <For each={current()?.rows ?? []}>{(row) => <tr data-estimate-node={row.node}>
          <td><span class="dim">#{row.node}</span> {row.title}</td>
          <td><Show when={known() && row.unknown === null && row.estimate_minutes !== null} fallback={<>
            <span>{tr("Unknown")}</span><p class="dim">{estimateUnknownText(row.unknown ?? current()?.unknown)}</p>
          </>}><span data-estimate-minutes>{tr("%1 min", row.estimate_minutes!)}</span></Show></td>
        </tr>}</For>
      </tbody></table>
      <Show when={comparison() && scope().basis === "frozen_publication"}>
        <h5>{tr("Observed implementation")}</h5>
        <p class="dim">{tr("Recorded states and delivery events do not measure implementation work.")}</p>
        <Show when={comparison()?.units.length} fallback={<p>{tr("No implementation observations are available in this selected window.")}</p>}>
          <ul><For each={comparison()?.units ?? []}>{(unit) => <li data-estimate-unit={unit.unit}>
            <span>{tr("Unit #%1", unit.unit)}</span>{" — "}<span>{tr("Task #%1", unit.node)}</span>{": "}
            <span>{tr("Unknown")}</span>{" — "}<span>{comparisonUnknownText(unit.unknown)}</span>
          </li>}</For></ul>
        </Show>
      </Show>
    </>}</Show>
  </section>;
}
