// Graph geometry, hover chains and card-port linking ported from WhiteWh's
// PlanView. Upstream actions use set_deps and the view's revision; pages stay
// bounded and off-page dependency facts are never erased by an edit.
import { For, Show, createEffect, createMemo, createSignal, createUniqueId, on, onCleanup } from "solid-js";
import { tr } from "../../i18n";
import type { TeamCard, TeamDependency, TeamView } from "../../store/teams";
import { teamPlanPreferences } from "../../store/ui";
import { sectionCollapsed, type TeamPlanPreferences } from "../../store/teamPlan";
import { PlanToolbar } from "./PlanToolbar";
import { fitPlanZoom, planMatches, planOrigins, planProblem, planTone } from "./planToolbar";
import { chain, phasePlans } from "./plan";
import { orthoPath, routeEdges } from "./planLayout";
import { canSetDeps, dependencyEditCause, taskState } from "./planTasks";
import "../styles/team-plan.css";

export function PlanView(props: {
  view: TeamView;
  busy: boolean;
  selected: number | null;
  onSelect: (node: number) => void;
  onSetDeps: (node: number, deps: TeamDependency[]) => void;
  preferences?: TeamPlanPreferences;
}) {
  const preferences = props.preferences ?? teamPlanPreferences;
  const state = () => preferences.get(props.view.bar.id);
  const tasks = createMemo(() => props.view.groups.flatMap((group) => group.cards));
  const visible = createMemo(() => tasks().filter((task) => planMatches(task, state())));
  const plans = createMemo(() => phasePlans(visible()));
  const collapsed = (phase: number) => sectionCollapsed(state(), phase);
  let host!: HTMLDivElement;
  const fit = () => {
    const widths = plans().filter((plan) => !collapsed(plan.phase)).map((plan) => plan.layout.width);
    const zoom = fitPlanZoom(host.clientWidth - 24, widths);
    if (zoom !== undefined) preferences.update(props.view.bar.id, { zoom });
  };
  const [hover, setHover] = createSignal<number | null>(null);
  const [link, setLink] = createSignal<number | null>(null);
  const [edge, setEdge] = createSignal<{ from: number; to: number } | null>(null);
  const lit = createMemo(() => hover() === null ? null : chain(tasks(), hover()!));
  const marker = createUniqueId();
  let cancelDrag: (() => void) | undefined;
  const reset = () => { cancelDrag?.(); setLink(null); setEdge(null); };
  createEffect(on(() => `${props.view.bar.id}/${props.view.revision}`, reset, { defer: true }));
  createEffect(on(state, reset, { defer: true }));
  onCleanup(() => cancelDrag?.());
  const editable = (task: TeamCard) => canSetDeps(task, props.view.bar.status, props.busy);
  const connect = (from: number, to: number) => {
    const target = tasks().find((task) => task.node === to);
    if (!target || !tasks().some((task) => task.node === from) || !editable(target) || from === to || target.deps?.some((dep) => dep.node === from)) return;
    props.onSetDeps(to, [...(target.deps ?? []), { node: from }]);
    setLink(null);
  };
  const startLink = (event: PointerEvent, from: number) => {
    if (event.button !== 0 || props.busy) return;
    event.preventDefault(); event.stopPropagation();
    cancelDrag?.(); setLink(from); setEdge(null);
    const revision = props.view.revision;
    const done = (up: PointerEvent) => {
      cleanup();
      const target = document.elementFromPoint(up.clientX, up.clientY)?.closest("[data-plan-task]");
      const to = Number(target?.getAttribute("data-plan-task"));
      if (revision === props.view.revision && to) connect(from, to);
    };
    const cancel = () => { cleanup(); setLink(null); };
    const cleanup = () => {
      window.removeEventListener("pointerup", done); window.removeEventListener("pointercancel", cancel); window.removeEventListener("blur", cancel);
      cancelDrag = undefined;
    };
    window.addEventListener("pointerup", done, { once: true }); window.addEventListener("pointercancel", cancel, { once: true }); window.addEventListener("blur", cancel, { once: true });
    cancelDrag = cleanup;
  };
  const edgeCause = () => {
    const target = tasks().find((task) => task.node === edge()?.to);
    return target ? dependencyEditCause(target, props.view.bar.status) : "";
  };
  const remove = () => {
    const chosen = edge();
    const target = tasks().find((task) => task.node === chosen?.to);
    if (!chosen || !target || !editable(target)) return;
    props.onSetDeps(target.node, (target.deps ?? []).filter((dep) => dep.node !== chosen.from));
    setEdge(null);
  };
  return <div ref={host} class="team-plan" data-team-graph tabIndex={0} aria-label={tr("Task dependency graph")}
    onKeyDown={(event) => {
      if (event.target instanceof HTMLElement && event.target.closest("input,textarea,[contenteditable=true]")) return;
      if (event.key === "Escape") { reset(); return; }
      if (event.key === "Delete" && edge()) { event.preventDefault(); remove(); }
    }}>
    <PlanToolbar state={state()} onChange={(patch) => preferences.update(props.view.bar.id, patch)} onFit={fit} showOrigins={visible().some((task) => task.origin != null)}
      sections={plans().length ? { allCollapsed: plans().every((plan) => collapsed(plan.phase)), onFold: (allCollapsed) => preferences.update(props.view.bar.id, { allCollapsed, sections: [] }) } : undefined} />
    <Show when={state().filter === "problems"}><p role="note">{tr("Problems shows held tasks and tasks with an unsuccessful stop outcome on this page.")}</p></Show>
    <p class="team-plan-help">{tr("Connect an output port to a task's input port to add a dependency. Select an arrow and press Delete to remove it.")}</p>
    <Show when={edgeCause()}>{(cause) => <p role="note" data-dependency-edit-cause>{cause()}</p>}</Show>
    <Show when={link() !== null}><p role="status">{tr("Choose the task that depends on #%1.", link()!)} <button type="button" class="btn small" onClick={reset}>{tr("Cancel")}</button></p></Show>
    <Show when={props.view.after || props.view.next || props.view.truncated}><p role="note">{tr("This graph shows the current queue page. Dependencies outside this page are listed on the cards.")}</p></Show>
    <Show when={!visible().length}><p role="status">{tr("No tasks on this page match these filters.")}</p></Show>
    <For each={plans()}>{(plan) => <section class="team-plan-phase">
      <h3><button type="button" class="team-plan-section-toggle" data-plan-section={plan.phase} aria-expanded={!collapsed(plan.phase)} onClick={() => preferences.toggleSection(props.view.bar.id, plan.phase)}>
        <span aria-hidden="true">{collapsed(plan.phase) ? "▸" : "▾"}</span><span>{plan.phase === 0 ? tr("Queue") : tr("Phase %1", plan.phase)}</span><span class="count">{plan.cards.length}</span>
      </button></h3>
      <Show when={!collapsed(plan.phase)}>
      <div class="team-plan-scroll"><div class="team-plan-scale-space" style={{ width: `${plan.layout.width * state().zoom}px`, height: `${plan.layout.height * state().zoom}px` }}><div class="team-plan-canvas team-plan-scaled" style={{ width: `${plan.layout.width}px`, height: `${plan.layout.height}px`, transform: `scale(${state().zoom})` }}>
        <For each={plan.layout.labels}>{(label) => <span class="team-plan-layer" style={{ left: `${label.x}px`, width: `${label.w}px` }}>{tr("Layer %1", label.layer + 1)}<span>{label.count}</span></span>}</For>
        <svg class="team-plan-edges" width={plan.layout.width} height={plan.layout.height}>
          <defs><marker id={`${marker}-${plan.phase}`} viewBox="0 0 8 8" refX="7" refY="4" markerWidth="6" markerHeight="6" orient="auto"><path d="M 0 0 L 8 4 L 0 8 z" /></marker></defs>
          <For each={routeEdges(plan.layout, planOrigins(plan.cards))}>{(route) => <path d={orthoPath(route.points)} marker-end={`url(#${marker}-${plan.phase})`} class="team-plan-edge team-plan-origin" data-plan-origin={`${route.from}/${route.to}`} />}</For>
          <For each={plan.routes}>{(route) => {
            const target = () => tasks().find((task) => task.node === route.to)!;
            const selected = () => edge()?.from === route.from && edge()?.to === route.to;
            const soft = () => target().deps?.find((dep) => dep.node === route.from)?.kind === "soft";
            return <>
              <path d={orthoPath(route.points)} marker-end={`url(#${marker}-${plan.phase})`} class="team-plan-edge" classList={{ selected: selected(), soft: soft() }} />
              <path d={orthoPath(route.points)} class="team-plan-edge-hit" classList={{ soft: soft() }}
              data-edge={`${route.from}/${route.to}`} tabIndex={0} role="button" aria-pressed={selected()} aria-label={tr("Dependency #%1 to #%2", route.from, route.to)}
              onClick={(event) => { event.stopPropagation(); setLink(null); setEdge({ from: route.from, to: route.to }); event.currentTarget.focus(); }}
              onKeyDown={(event) => { if (event.key === "Enter" || event.key === " ") { event.preventDefault(); setEdge({ from: route.from, to: route.to }); } }} />
            </>;
          }}</For>
        </svg>
        <For each={plan.cards}>{(task) => {
          const rect = plan.layout.rects.get(task.node)!;
          const external = () => (task.deps ?? []).filter((dep) => !plan.layout.rects.has(dep.node));
          return <div class="team-plan-node" data-plan-task={task.node} data-state={task.state} data-plan-tone={planTone(task)} data-plan-problem={planProblem(task)} classList={{ selected: props.selected === task.node, dim: !!lit() && !lit()!.has(task.node) }}
            style={{ left: `${rect.x}px`, top: `${rect.y}px`, width: `${rect.w}px`, height: `${rect.h}px` }} onPointerEnter={() => setHover(task.node)} onPointerLeave={() => setHover(null)}>
            <button type="button" class="team-plan-port input" title={dependencyEditCause(task, props.view.bar.status)} disabled={!editable(task) || link() === null || link() === task.node} aria-label={tr("Add dependency to #%1", task.node)}
              onClick={() => { if (link() !== null) connect(link()!, task.node); }}>←</button>
            <button type="button" class="team-plan-card" aria-pressed={props.selected === task.node} onClick={() => { setEdge(null); props.onSelect(task.node); }}
              title={[...(task.deps ?? []).map((dep) => `#${dep.node}${dep.kind === "soft" ? ` (${tr("soft")})` : ""}`), ...(task.origin != null ? [tr("Spawned from #%1", task.origin)] : [])].join(", ")}>
              <b>#{task.node} {task.title}</b><span>{taskState(task)}<Show when={external().length}> — {tr("after %1", external().map((dep) => `#${dep.node}`).join(", "))}</Show><Show when={task.origin != null}> — {tr("Spawned from #%1", task.origin!)}</Show></span>
            </button>
            <button type="button" class="team-plan-port output" disabled={props.busy || !["running", "paused"].includes(props.view.bar.status)} aria-label={tr("Use #%1 as a dependency", task.node)}
              onPointerDown={(event) => startLink(event, task.node)} onClick={() => { setEdge(null); setLink(task.node); }}>→</button>
          </div>;
        }}</For>
      </div></div></div>
      </Show>
    </section>}</For>
  </div>;
}
