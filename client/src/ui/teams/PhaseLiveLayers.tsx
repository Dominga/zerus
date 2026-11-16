import { For, Show, createMemo, createSignal } from "solid-js";
import { tr } from "../../i18n";
import { teamPlanPreferences } from "../../store/ui";
import { sectionCollapsed } from "../../store/teamPlan";
import { openTask } from "../../store/taskNavigation";
import type { PhaseDraft, PhaseLiveNode, PhaseNodeSnapshot } from "../../store/team-phases";
import type { TeamCard } from "../../store/teams";
import { PlanToolbar } from "./PlanToolbar";
import { planMatches, planTone } from "./planFilters";

/** This adapts the typed Node defaults for the shared presentation filters.
 * The graph and every runtime state still come from the core and service. */
export function phasePlanCard(node: PhaseLiveNode): TeamCard {
  return { node: node.id, title: node.title, phase: node.phase, state: node.state ?? "draft", task: node.task ?? null,
    role: node.role ?? "", pipeline: node.pipeline ?? "", assignee: node.assignee ?? "", implementer: node.implementer ?? "", rank: node.rank ?? "",
    round: node.round, held: node.hold ?? false, admitted: node.admitted, integrated: node.integrated ?? false,
    decided_by: node.decided_by ?? "", stages: [], kind: node.kind === "stage" ? "stage" : "task" };
}

export function PhaseLiveLayers(props: { team: string; draft: PhaseDraft; live: PhaseNodeSnapshot }) {
  const [open, setOpen] = createSignal<number | null>(null);
  const preferences = () => teamPlanPreferences.get(props.team);
  const nodes = createMemo(() => new Map(props.live.nodes.map((n) => [n.id, n])));
  const visible = createMemo(() => props.draft.graph.nodes.filter((placed) => {
    const node = nodes().get(placed.id); return node && planMatches(phasePlanCard(node), preferences());
  }));
  const layers = createMemo(() => [...new Set(visible().map((n) => n.layer))].sort((a, b) => a - b));
  const folded = () => sectionCollapsed(preferences(), props.live.phase);
  const state = (n: PhaseLiveNode) => {
    switch (n.state ?? "draft") {
      case "accepted": return tr("Accepted"); case "cancelled": return tr("Cancelled");
      case "reported": return tr("In review"); case "in_progress": return tr("In progress");
      case "pending": return tr("To do"); case "waiting": return tr("Waiting"); default: return tr("Draft");
    }
  };
  return <section data-phase-live>
    <PlanToolbar state={preferences()} onChange={(patch) => teamPlanPreferences.update(props.team, patch)}
      sections={{ allCollapsed: preferences().allCollapsed, onFold: (allCollapsed) => teamPlanPreferences.update(props.team, { allCollapsed, sections: [] }) }} />
    <button type="button" class="btn small" aria-expanded={!folded()} onClick={() => teamPlanPreferences.toggleSection(props.team, props.live.phase)}>{folded() ? tr("Expand phase") : tr("Collapse phase")}</button>
    <Show when={!props.live.complete || !props.draft.graph.complete}><p class="notice" role="status">{tr("Only part of this phase has been read.")}</p></Show>
    <Show when={!folded()}>
      <Show when={visible().length > 0} fallback={<p class="dim">{props.draft.graph.nodes.length === 0 ? tr("Phase layers are unavailable for this read.") : tr("The current filters hide every task in this phase.")}</p>}>
        <div class="phase-layers"><For each={layers()}>{(layer) => <section class="phase-layer" aria-label={tr("Layer %1", layer + 1)}>
          <h3>{tr("Layer %1", layer + 1)}</h3>
          <For each={visible().filter((n) => n.layer === layer)}>{(placed) => {
            const node = () => nodes().get(placed.id)!;
            const criteria = () => props.draft.nodes.find((n) => n.id === placed.id)?.criteria ?? [];
            return <article class="phase-node" data-phase-node={placed.id} data-state={node().state ?? "draft"} data-plan-tone={planTone(phasePlanCard(node()))}>
              <button type="button" class="phase-node-title" aria-expanded={open() === placed.id} onClick={() => setOpen(open() === placed.id ? null : placed.id)}>{node().title}</button>
              <span class="tag">{state(node())}</span><Show when={node().stage_of != null}><p class="dim">{tr("Stage for task #%1", node().stage_of!)}</p></Show>
              <Show when={open() === placed.id}><Show when={props.draft.nodes.find((n) => n.id === placed.id)}>{(approved) => <><b>{tr("Approved description")}</b><p class="phase-description">{approved().text}</p></>}</Show>
                <Show when={criteria().length > 0}><b>{tr("Acceptance criteria")}</b><ul><For each={criteria()}>{(c) => <li>{c}</li>}</For></ul></Show>
                <For each={props.draft.graph.edges.filter((edge) => edge.to === placed.id)}>{(edge) => <p class="dim">{edge.external ? tr("Depends on task #%1 outside this phase", edge.from) : tr("Depends on task #%1", edge.from)} <span>{edge.kind === "hard" ? tr("Hard") : tr("Soft")}</span></p>}</For>
                <Show when={node().task}>{(task) => <button type="button" class="btn small" onClick={() => openTask(task().project, task().uuid)}>{tr("Open task")}</button>}</Show>
              </Show>
            </article>;
          }}</For>
        </section>}</For></div>
      </Show>
    </Show>
  </section>;
}
