import { Show, createSignal, onCleanup, onMount } from "solid-js";
import { tr } from "../../i18n";
import { newRequestId, teams, type TeamCall, type TeamOutcome, type TeamView } from "../../store/teams";
import type { DraftInput, DraftRoute, PhaseDraft } from "../../store/team-phases";
import { TextArea } from "../lib/TextField";
import { PhaseActions } from "./PhaseActions";
import { PhaseDraftEditor, type DraftSnapshot } from "./PhaseDraftEditor";
import { PhaseLayers } from "./PhaseDraftLayers";
import { PhaseMetrics } from "./PhaseMetrics";
import "../styles/phase-rework.css";

interface ReturnSnapshot { team: string; team_name: string; phase: number; revision: number; publication: string }

function SendBackDialog(props: { snapshot: ReturnSnapshot; close: () => void; run: (call: TeamCall) => Promise<TeamOutcome> }) {
  let dialog!: HTMLDialogElement;
  const frozen = props.snapshot;
  const [comment, setComment] = createSignal("");
  const [pending, setPending] = createSignal(false), [locked, setLocked] = createSignal(false), [message, setMessage] = createSignal("");
  onMount(() => dialog.showModal()); onCleanup(() => { if (dialog.open) dialog.close(); });
  const submit = async () => {
    if (pending() || locked() || !comment().trim()) return;
    setPending(true); setMessage("");
    try {
      const result = await props.run({ team: frozen.team, team_name: frozen.team_name, expected_revision: frozen.revision, request_id: newRequestId(),
        action: { kind: "send_back_phase", phase: frozen.phase, publication: frozen.publication, comment: comment() } });
      if (result.outcome === "done") { props.close(); return; }
      if (result.outcome === "failed") { setLocked(result.code === "stale_revision"); setMessage(result.detail); }
      else if (result.outcome === "uncertain" || result.outcome === "pending") { setLocked(true); setMessage(tr("The change is unconfirmed. Close this editor and use Check in the team's unresolved actions.")); }
    } catch { setLocked(true); setMessage(tr("The change is unconfirmed. Close this editor and use Check in the team's unresolved actions.")); }
    finally { setPending(false); }
  };
  return <dialog ref={dialog} class="dlg phase-editor" aria-labelledby="phase-return-title" onCancel={(event) => { event.preventDefault(); if (!pending()) props.close(); }}>
    <div class="dlg-head"><h2 id="phase-return-title">{tr("Send phase back")}</h2></div>
    <div class="dlg-body"><p>{tr("Accepted tasks and commits stay unchanged. The phase keeps its acceptance gate until explicit corrections are approved and published.")}</p>
      <label>{tr("Required corrections")}<TextArea value={comment()} maxLength={16384} disabled={pending() || locked()} data-phase-return-comment onInput={(e) => setComment(e.currentTarget.value)} /></label>
      <Show when={message()}><p class="notice danger" role="status">{message()}</p></Show>
    </div>
    <div class="dlg-foot"><button type="button" class="btn" disabled={pending()} onClick={props.close}>{tr("Close")}</button>
      <button type="button" class="btn primary" data-phase-return-submit disabled={pending() || locked() || !comment().trim()} onClick={() => void submit()}>{tr("Send phase back")}</button>
    </div>
  </dialog>;
}

export function PhaseRework(props: { view: TeamView; draft: PhaseDraft; routes: DraftRoute[]; busy: boolean; readFailed: boolean; onChanged: () => void; run?: (call: TeamCall) => Promise<TeamOutcome> }) {
  const team = { id: props.view.bar.id, name: props.view.bar.name };
  const [returning, setReturning] = createSignal<ReturnSnapshot | null>(null);
  const [editing, setEditing] = createSignal<DraftSnapshot | null>(null);
  const active = () => ["running", "paused"].includes(props.view.bar.status);
  const enabled = () => active() && !props.busy && !props.readFailed && !props.draft.unknown_fields && props.draft.rework_available;
  const row = () => props.draft.rework;
  const open = () => !!row() && row()!.state !== "published";
  const run = async (call: TeamCall): Promise<TeamOutcome> => {
    if (props.view.bar.id !== team.id || call.team !== team.id) return { outcome: "failed", code: "stale_revision", detail: tr("The draft changed. Close this editor and read again; your text stays here to copy."), request_id: call.request_id };
    const result = await (props.run ? props.run(call) : teams.run(call, "phase"));
    if (result.outcome === "done") props.onChanged();
    return result;
  };
  const sendBack = () => {
    const p = props.draft.publication;
    if (!enabled() || open() || !p || !props.draft.accept_available || !props.draft.live?.complete || !props.draft.graph.complete || props.view.bar.revision !== props.draft.revision) return;
    setReturning({ team: props.view.bar.id, team_name: props.view.bar.name, phase: props.draft.phase.n, revision: props.draft.revision, publication: p.id });
  };
  const edit = () => {
    const r = row(); if (!enabled() || !r || r.state === "published" || r.state === "publishing" || (r.draft && !r.draft.editable)) return;
    const d = r.draft, route = props.routes[0];
    setEditing({ phase: props.draft.phase.n, revision: props.draft.revision, generation: d?.generation ?? null, next_node: props.draft.next_node,
      correction: { publication: r.base_publication, rework: r.id },
      input: d ? { title: d.phase.title ?? "", barrier: !!d.phase.barrier, nodes: structuredClone(d.nodes) }
        : { title: props.draft.phase.title ?? "", barrier: !!props.draft.phase.barrier, nodes: [{ id: props.draft.next_node, title: "", text: "", criteria: [""], role: route?.role ?? "", pipeline: route?.pipeline ?? "", assignee: null, deps: [], writes: [], estimate_minutes: 0, kind: "task" }] } });
  };
  const submit = (snapshot: DraftSnapshot, draft: DraftInput) => {
    if (!snapshot.correction || props.view.bar.id !== team.id) return Promise.resolve<TeamOutcome>({ outcome: "failed", code: "stale_revision", detail: tr("Read this phase again before editing its corrections."), request_id: "" });
    return run({ team: team.id, team_name: team.name, expected_revision: snapshot.revision, request_id: newRequestId(),
      action: { kind: snapshot.generation === null ? "submit_phase_corrections" : "edit_phase_corrections", phase: snapshot.phase, publication: snapshot.correction.publication, rework: snapshot.correction.rework,
        ...(snapshot.generation === null ? {} : { generation: snapshot.generation }), draft } });
  };
  return <section data-phase-rework>
    <Show when={props.draft.phase.state === "awaiting_acceptance" && !open()}>
      <button type="button" class="btn small" data-phase-send-back disabled={!enabled() || !props.draft.accept_available || !props.draft.live?.complete || !props.draft.graph.complete} onClick={sendBack}>{tr("Send phase back…")}</button>
      <Show when={!props.draft.rework_available}><p class="dim">{tr("Sending a phase back is not available on this computer.")}</p></Show>
    </Show>
    <Show when={row()}>{(r) => <section class="phase-corrections" data-correction-episode={r().id}>
      <h3>{r().state === "published" ? tr("Published corrections") : tr("Phase corrections")}</h3>
      <p class="phase-description">{r().comment}</p><p class="dim">{tr("Previously accepted work stays unchanged.")}</p>
      <Show when={r().state !== "published"}>
        <p>{tr("The phase is still awaiting acceptance while these corrections are prepared.")}</p>
        <button type="button" class="btn small" data-edit-corrections disabled={!enabled() || r().state === "publishing" || (!!r().draft && !r().draft!.editable)} onClick={edit}>{r().draft ? tr("Edit corrections") : tr("Draft corrections")}</button>
        <Show when={r().draft}>{(d) => <>
          <Show when={d().unknown_fields}><p class="notice" role="status">{tr("This draft has fields this window cannot edit.")}</p></Show>
          <PhaseMetrics metrics={d().metrics} readFailed={props.readFailed} />
          <PhaseActions view={props.view} draft={d()} busy={props.busy} available={!!props.draft.rework_available} readFailed={props.readFailed} correction={{ publication: r().base_publication, rework: r().id }} run={run} />
          <PhaseLayers draft={d()} />
        </>}</Show>
      </Show>
    </section>}</Show>
    <Show when={returning()}>{(snapshot) => <SendBackDialog snapshot={snapshot()} close={() => setReturning(null)} run={run} />}</Show>
    <Show when={editing()}>{(snapshot) => <PhaseDraftEditor snapshot={snapshot()} routes={props.routes} members={props.view.members} close={() => setEditing(null)} submit={submit} />}</Show>
  </section>;
}
