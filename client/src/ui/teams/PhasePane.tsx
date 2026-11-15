import { For, Show, createEffect, createMemo, createSignal, onCleanup, onMount } from "solid-js";
import { invoke } from "@tauri-apps/api/core";
import { tr } from "../../i18n";
import { newRequestId, teams, type TeamOutcome, type TeamView } from "../../store/teams";
import { phaseFollower, type DraftInput, type PhaseDraft, type PhaseFollow } from "../../store/team-phases";
import { PhaseDraftEditor, type DraftSnapshot } from "./PhaseDraftEditor";
import { phaseHeader } from "./phaseHeader";
import { PlanningNotice } from "./PlanningStart";
import { PhaseMetrics } from "./PhaseMetrics";
import "../styles/team-phases.css";

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

export function PhasePane(props: {
  view: TeamView; busy: boolean; follow?: PhaseFollow;
  submit?: (snapshot: DraftSnapshot, input: DraftInput) => Promise<TeamOutcome>;
}) {
  const team = { id: props.view.bar.id, name: props.view.bar.name };
  const follower = phaseFollower(team.id, props.follow);
  const [selected, setSelected] = createSignal<number | null>(null);
  const [editor, setEditor] = createSignal<DraftSnapshot | null>(null);
  const [pageError, setPageError] = createSignal("");
  const pane = createMemo(() => props.view.bar.id === team.id && props.view.plan?.epoch === follower.epoch() ? props.view.plan : undefined);
  const list = () => pane()?.list;
  const failed = () => follower.error() || pane()?.list_failure?.message || pane()?.draft_failure?.message || pageError();
  const draft = () => pane()?.draft?.phase.n === selected() ? pane()?.draft : null;
  const choose = (phase: number) => { setSelected(phase); follower.select(phase); };
  onMount(() => follower.select(null));
  onCleanup(() => follower.close());
  createEffect(() => {
    if (selected() !== null) return;
    const first = list()?.rows.find((p) => p.kind === "plan");
    if (first) choose(first.n);
  });
  const active = () => ["planning", "running", "paused"].includes(props.view.bar.status);
  const editable = () => !!draft()?.editable && !draft()?.unknown_fields && !failed() && active() && !props.busy;
  const routes = () => [...(list()?.routes ?? [])].sort((a, b) => Number(b.pipeline === list()?.default_pipeline) - Number(a.pipeline === list()?.default_pipeline));
  const edit = () => {
    const d = draft(); if (!d || !editable() || d.generation === null) return;
    setEditor({ phase: d.phase.n, revision: d.revision, generation: d.generation, next_node: d.next_node,
      input: { title: d.phase.title ?? "", barrier: !!d.phase.barrier, nodes: structuredClone(d.nodes) } });
  };
  const create = () => {
    const l = list(); if (!l?.drafts_available || failed() || props.busy || !active()) return;
    const route = routes()[0];
    setEditor({ phase: l.next_phase, revision: l.revision, generation: null, next_node: l.next_node,
      input: { title: "", barrier: true, nodes: [{ id: l.next_node, title: "", text: "", criteria: [""], role: route?.role ?? "", pipeline: route?.pipeline ?? "", assignee: null, deps: [], writes: [], estimate_minutes: 0, kind: "task" }] } });
  };
  const submit = async (snapshot: DraftSnapshot, input: DraftInput): Promise<TeamOutcome> => {
    if (props.view.bar.id !== team.id) return { outcome: "failed", code: "stale_revision", detail: tr("The draft changed. Close this editor and read again; your text stays here to copy."), request_id: "" };
    const result = props.submit ? await props.submit(snapshot, input) : await teams.run({ team: team.id, team_name: team.name,
      expected_revision: snapshot.revision, request_id: newRequestId(), action: { kind: snapshot.generation === null ? "submit_draft_phase" : "edit_draft_phase", phase: snapshot.phase, draft: input,
        ...(snapshot.generation === null ? {} : { generation: snapshot.generation }) } }, "phase");
    if (result.outcome === "done") choose(snapshot.phase);
    return result;
  };
  const next = async () => {
    const cursor = list()?.next; if (!cursor) return;
    try { await invoke("teams_page", { key: `phases:${team.id}`, cursor }); setPageError(""); }
    catch (cause) { setPageError(String(cause)); }
  };
  return <section class="phase-pane" data-phase-pane>
    <PlanningNotice status={props.view.bar.status} goal={props.view.bar.goal} planning={props.view.planning} />
    <header class="phase-toolbar"><h2>{tr("Planned phases")}</h2><span class="grow" />
      <button type="button" class="btn small" disabled={!list()?.drafts_available || !!failed() || props.busy || !active()} onClick={create}>{tr("New phase draft")}</button>
      <button type="button" class="btn small" onClick={() => { setPageError(""); follower.select(selected()); }}>{tr("Read again")}</button>
    </header>
    <Show when={failed()}>{(message) => <div class="notice danger" role="status"><p>{tr("Planned phases are unavailable or could not be read.")}</p><details><summary>{tr("Details")}</summary><pre>{message()}</pre></details></div>}</Show>
    <Show when={list()} fallback={<p class="dim" role="status">{failed() ? "" : tr("Reading planned phases…")}</p>}>{(metadata) => <>
      <nav class="phase-tabs" aria-label={tr("Phases")}><For each={[...metadata().rows].sort((a, b) => (a.order || a.n) - (b.order || b.n) || a.n - b.n)}>{(p) => <button type="button" class="btn small" aria-pressed={selected() === p.n} onClick={() => choose(p.n)}>{phaseHeader(p).title}</button>}</For></nav>
      <Show when={metadata().next}><button type="button" class="btn small" onClick={() => void next()}>{tr("More phases")}</button></Show>
      <Show when={!metadata().rows.some((p) => p.kind === "plan")}><p class="dim">{tr("No planned phases yet. Create a draft or ask the lead to submit one.")}</p></Show>
      <Show when={draft()}>{(d) => <>
        <header class="phase-header"><h3>{phaseHeader(d().phase).title}</h3><span class="tag">{phaseHeader(d().phase).state}</span><span class="tag">{phaseHeader(d().phase).mode}</span>
          <PhaseMetrics metrics={d().metrics} readFailed={!!failed() || !!pane()?.list_failure || !!pane()?.draft_failure} />
          <Show when={phaseHeader(d().phase).barrier}><span class="dim">{phaseHeader(d().phase).barrier}</span></Show>
          <Show when={phaseHeader(d().phase).paused}><span class="dim">{phaseHeader(d().phase).paused}</span></Show>
          <span class="grow" /><button type="button" class="btn small" disabled={!editable()} onClick={edit}>{tr("Edit draft")}</button>
          <button type="button" class="btn small" disabled title={phaseHeader(d().phase).approvalUnavailable} data-phase-approve>{tr("Approve phase")}</button>
        </header>
        <p class="dim">{phaseHeader(d().phase).approvalUnavailable}</p>
        <Show when={d().unknown_fields}><p class="notice" role="status">{tr("This draft has fields this window cannot edit.")}</p></Show>
        <Show when={d().complete} fallback={<p class="dim">{tr("This phase has no editable draft. Open the Board to see its tasks.")}</p>}><PhaseLayers draft={d()} /></Show>
      </>}</Show>
    </>}</Show>
    <Show when={editor()}>{(snapshot) => <PhaseDraftEditor snapshot={snapshot()} routes={routes()} members={props.view.members} close={() => setEditor(null)} submit={submit} />}</Show>
  </section>;
}
