import { For, Show, createMemo } from "solid-js";
import { tr } from "../../i18n";
import type { DraftNode, PhaseDraft } from "../../store/team-phases";

/** Compare the producer's retained previous generation, never the current
 * editor buffer or another queue page. IDs remain the identity of a task. */
export function phaseChanges(draft: PhaseDraft): { added: DraftNode[]; changed: DraftNode[]; removed: DraftNode[] } | null {
  const old = draft.previous?.draft.nodes;
  if (!old) return null;
  const before = new Map(old.map((n) => [n.id, n]));
  const after = new Set(draft.nodes.map((n) => n.id));
  return {
    added: draft.nodes.filter((n) => !before.has(n.id)),
    changed: draft.nodes.filter((n) => before.has(n.id) && JSON.stringify(n) !== JSON.stringify(before.get(n.id))),
    removed: old.filter((n) => !after.has(n.id)),
  };
}

export function PhaseReview(props: { draft: PhaseDraft }) {
  const changes = createMemo(() => phaseChanges(props.draft));
  return <section class="phase-review" data-phase-review>
    <p>{tr("%1 tasks in this draft", props.draft.nodes.length)}</p>
    <Show when={props.draft.previous}>{(previous) => <>
      <p class="dim">{tr("Changes since draft generation %1", previous().generation)}</p>
      <Show when={previous().draft.title !== props.draft.phase.title}><p>{tr("Previous title: %1", previous().draft.title)}</p></Show>
      <Show when={previous().draft.barrier !== !!props.draft.phase.barrier}><p>{tr("The earlier-phase barrier changed.")}</p></Show>
      <For each={[[tr("Added"), changes()?.added], [tr("Changed"), changes()?.changed], [tr("Removed"), changes()?.removed]] as const}>{([label, nodes]) =>
        <Show when={nodes?.length}><p><b>{label}</b>: <For each={nodes}>{(n, i) => <>{i() > 0 ? ", " : ""}{n.title}</>}</For></p></Show>
      }</For>
    </>}</Show>
    <For each={props.draft.nodes}>{(node) => <details class="phase-review-task">
      <summary>{node.title}</summary><p class="phase-description">{node.text}</p>
      <b>{tr("Acceptance criteria")}</b><ul><For each={node.criteria}>{(c) => <li>{c}</li>}</For></ul>
      <p class="dim">{tr("Role: %1", node.role)} / {tr("Pipeline: %1", node.pipeline)}</p>
    </details>}</For>
  </section>;
}
