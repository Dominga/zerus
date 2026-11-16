import { For, Show, createEffect, createMemo, createSignal, onCleanup, onMount } from "solid-js";
import { invoke } from "@tauri-apps/api/core";
import { tr } from "../../i18n";
import { newRequestId, teams, type TeamOutcome, type TeamView } from "../../store/teams";
import { phaseFollower, type DraftInput, type PhaseDraft, type PhaseFollow, takePhaseFocus } from "../../store/team-phases";
import { PhaseDraftEditor, type DraftSnapshot } from "./PhaseDraftEditor";
import { phaseHeader } from "./phaseHeader";
import { PlanningNotice } from "./PlanningStart";
import { ObservedPanel } from "./ObservedPanel";
import { PhaseMetrics } from "./PhaseMetrics";
import { PhaseActions } from "./PhaseActions";
import { PhaseLiveLayers } from "./PhaseLiveLayers";
import { PhaseLayers } from "./PhaseDraftLayers";
import { PhaseRework } from "./PhaseRework";
import { PhaseControls } from "./PhaseControls";
export { PhaseLayers } from "./PhaseDraftLayers";
import "../styles/team-phases.css";

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
  onMount(() => { const focus = takePhaseFocus(team.id); if (focus !== null) choose(focus); else follower.select(null); });
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
          <PhaseControls draft={d()} view={props.view} busy={props.busy} readFailed={!!failed()} onChanged={() => follower.select(selected())} />
        </header>
        <ObservedPanel team={team.id} phase={d().phase.n} pane={pane()?.observed} estimates={pane()?.estimates} readFailed={!!failed()} />
        <PhaseActions draft={d()} view={props.view} busy={props.busy} available={!!metadata().approval_available} readFailed={!!failed()} onChanged={() => follower.select(selected())} />
        <PhaseRework view={props.view} draft={d()} routes={routes()} busy={props.busy} readFailed={!!failed()} onChanged={() => follower.select(selected())} />
        <Show when={d().unknown_fields}><p class="notice" role="status">{tr("This draft has fields this window cannot edit.")}</p></Show>
        <Show when={d().complete} fallback={<p class="dim">{tr("This phase has no editable draft. Open the Board to see its tasks.")}</p>}><Show when={d().live} fallback={<PhaseLayers draft={d()} />}>{(live) => <PhaseLiveLayers team={team.id} draft={d()} live={live()} />}</Show></Show>
      </>}</Show>
    </>}</Show>
    <Show when={editor()}>{(snapshot) => <PhaseDraftEditor snapshot={snapshot()} routes={routes()} members={props.view.members} close={() => setEditor(null)} submit={submit} />}</Show>
  </section>;
}
