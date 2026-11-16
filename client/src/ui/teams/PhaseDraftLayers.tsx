import { For, Show, createMemo, createSignal } from "solid-js";
import { tr } from "../../i18n";
import type { PhaseDraft } from "../../store/team-phases";

/** Layers groups the core's layer numbers. It computes no dependency order. */
export function PhaseLayers(props: { draft: PhaseDraft }) {
  const [open, setOpen] = createSignal<number | null>(null);
  const layers = createMemo(() => [...new Set(props.draft.graph.nodes.map((n) => n.layer))].sort((a, b) => a - b));
  return <div class="phase-layers" data-phase-layers>
    <Show when={!props.draft.graph.complete}><p class="dim">{tr("Only part of this phase has been read.")}</p></Show>
    <For each={layers()}>{(layer) => <section class="phase-layer" aria-label={tr("Layer %1", layer + 1)}>
      <h3>{tr("Layer %1", layer + 1)}</h3>
      <For each={props.draft.graph.nodes.filter((n) => n.layer === layer)}>{(placed) => {
        const node = () => props.draft.nodes.find((n) => n.id === placed.id);
        return <Show when={node()} fallback={<p>{tr("Task #%1 is not available in this phase", placed.id)}</p>}>{(n) => <article class="phase-node" data-phase-node={n().id}>
          <button type="button" class="phase-node-title" aria-expanded={open() === n().id} onClick={() => setOpen(open() === n().id ? null : n().id)}>{n().title}</button>
          <span class="dim">{tr("Draft task #%1", n().id)}</span>
          <Show when={open() === n().id}><p class="phase-description">{n().text}</p><b>{tr("Acceptance criteria")}</b><ul><For each={n().criteria}>{(criterion) => <li>{criterion}</li>}</For></ul>
            <For each={props.draft.graph.edges.filter((edge) => edge.to === n().id)}>{(edge) => <p class="dim">{edge.external ? tr("Depends on task #%1 outside this phase", edge.from) : tr("Depends on task #%1", edge.from)} <span>{edge.kind === "hard" ? tr("Hard") : tr("Soft")}</span></p>}</For>
          </Show>
        </article>}</Show>;
      }}</For>
    </section>}</For>
  </div>;
}

