import { Show, createSignal, onCleanup, onMount } from "solid-js";
import { tr } from "../../i18n";
import { phaseControlReason, type PhaseControls as Controls, type PhaseAvailability } from "../../store/phase-controls";
import type { PhaseDraft } from "../../store/team-phases";
import { newRequestId, teams, type TeamCall, type TeamOutcome, type TeamView } from "../../store/teams";
import { TextArea } from "../lib/TextField";

type Form = { kind: "pause" | "acceptance"; draft: PhaseDraft; controls: Controls; team: { id: string; name: string } };

function ControlForm(props: { form: Form; available: boolean; close: () => void; run: (call: TeamCall) => Promise<TeamOutcome> }) {
  const frozen = props.form;
  let dialog!: HTMLDialogElement;
  const [reason, setReason] = createSignal("");
  const [acceptance, setAcceptance] = createSignal(frozen.draft.phase.acceptance ?? "lead");
  const [pending, setPending] = createSignal(false), [locked, setLocked] = createSignal(false);
  const [message, setMessage] = createSignal("");
  onMount(() => dialog.showModal());
  onCleanup(() => { if (dialog.open) dialog.close(); });
  const submit = async () => {
    if (!props.available || pending() || locked() || (frozen.kind === "pause" && (!reason().trim() || new TextEncoder().encode(reason()).length > 2048))) return;
    setPending(true); setMessage("");
    const call: TeamCall = { team: frozen.team.id, team_name: frozen.team.name, request_id: newRequestId(), expected_revision: frozen.draft.revision,
      action: frozen.kind === "pause"
        ? { kind: "pause_phase", scope: frozen.controls.scope, pause: frozen.controls.pause.state === "paused" ? frozen.controls.pause.episode?.id ?? null : null, reason: reason() }
        : { kind: "set_phase_acceptance", scope: frozen.controls.scope, acceptance: acceptance() } };
    try {
      const result = await props.run(call);
      if (result.outcome === "done") { props.close(); return; }
      if (result.outcome === "failed") { setLocked(result.code === "stale_revision"); setMessage(result.detail); }
      else { setLocked(true); setMessage(tr("The change is unconfirmed. Close this editor and use Check in the team's unresolved actions.")); }
    } catch { setLocked(true); setMessage(tr("The change is unconfirmed. Close this editor and use Check in the team's unresolved actions.")); }
    finally { setPending(false); }
  };
  return <dialog ref={dialog} class="dlg phase-editor" data-phase-control-form aria-labelledby="phase-control-title" onCancel={(e) => { e.preventDefault(); if (!pending()) props.close(); }}>
    <div class="dlg-head"><h2 id="phase-control-title">{frozen.kind === "pause" ? tr("Pause phase") : tr("Phase acceptance policy")}</h2></div>
    <div class="dlg-body"><h3>{frozen.draft.phase.title}</h3>
      <Show when={frozen.kind === "pause"} fallback={<label>{tr("Who accepts this phase?")}<select data-phase-acceptance-choice value={acceptance()} disabled={pending() || locked()} onChange={(e) => setAcceptance(e.currentTarget.value as "person" | "lead")}>
        <option value="person">{tr("Person only")}</option><option value="lead">{tr("Person or delegated lead")}</option>
      </select></label>}>
        <p>{tr("This stops new phase hand-outs. Existing work and reports continue.")}</p>
        <label>{tr("Pause reason")}<TextArea value={reason()} maxLength={2048} disabled={pending() || locked()} onInput={(e) => setReason(e.currentTarget.value)} /></label>
      </Show>
      <Show when={message()}><p class="notice danger" role="status">{message()}</p></Show>
      <Show when={!props.available}><p class="notice" role="status">{tr("The phase read changed or is unavailable. Close this form and read again; your text stays here to copy.")}</p></Show>
    </div>
    <div class="dlg-foot"><button type="button" class="btn" disabled={pending()} onClick={props.close}>{tr("Close")}</button>
      <button type="button" class="btn primary" data-phase-control-submit disabled={!props.available || pending() || locked() || (frozen.kind === "pause" ? !reason().trim() || new TextEncoder().encode(reason()).length > 2048 : acceptance() === (frozen.draft.phase.acceptance ?? "lead"))} onClick={() => void submit()}>{tr("Apply")}</button>
    </div>
  </dialog>;
}

export function PhaseControls(props: { draft: PhaseDraft; view: TeamView; busy: boolean; readFailed: boolean; run?: (call: TeamCall) => Promise<TeamOutcome>; onChanged?: () => void }) {
  const [form, setForm] = createSignal<Form | null>(null);
  const [pending, setPending] = createSignal(false), [locked, setLocked] = createSignal(false);
  const [message, setMessage] = createSignal("");
  const controls = () => props.draft.controls;
  const paused = () => { const p = controls()?.pause; return p?.state === "paused" ? p : null; };
  const pauseOwner = () => {
    const owner = paused()?.episode?.owner;
    if (!owner) return tr("Pause owner is unknown; only the person can resume or replace it.");
    return owner.kind === "person" ? tr("Paused by the person") : tr("Paused by lead %1", owner.member);
  };
  const why = (fact: PhaseAvailability) => fact.reason ? phaseControlReason(fact.reason) : "";
  const liveAvailable = () => props.draft.publication?.state !== "published" || !!props.draft.live && (props.draft.live.complete || props.draft.rework?.state === "publishing" && props.draft.rework.draft?.publication?.state === "reserved" && props.draft.nodes.every((original) => props.draft.live!.nodes.some((n) => n.id === original.id)));
  const enabled = (fact?: PhaseAvailability) => !!fact?.allowed && controls()?.version === 1 && !props.busy && !pending() && !locked() && !props.readFailed
    && !props.draft.unknown_fields && props.draft.complete && liveAvailable() && props.view.bar.revision === props.draft.revision && ["planning", "running", "paused"].includes(props.view.bar.status);
  const run = async (call: TeamCall) => {
    const result = props.run ? await props.run(call) : await teams.run(call, "phase");
    if (result.outcome === "done") props.onChanged?.();
    return result;
  };
  const open = (kind: Form["kind"], fact?: PhaseAvailability) => {
    const c = controls(); if (!c || !enabled(fact)) return;
    setForm({ kind, draft: structuredClone(props.draft), controls: structuredClone(c), team: { id: props.view.bar.id, name: props.view.bar.name } });
  };
  const act = async (fact: PhaseAvailability, action: TeamCall["action"]) => {
    if (!enabled(fact)) return;
    setPending(true); setMessage("");
    try {
      const result = await run({ team: props.view.bar.id, team_name: props.view.bar.name, expected_revision: props.draft.revision, request_id: newRequestId(), action });
      if (result.outcome === "failed") setMessage(result.detail);
      else if (result.outcome !== "done") { setLocked(true); setMessage(tr("The change is unconfirmed. Close this editor and use Check in the team's unresolved actions.")); }
    } catch { setLocked(true); setMessage(tr("The change is unconfirmed. Close this editor and use Check in the team's unresolved actions.")); }
    finally { setPending(false); }
  };
  return <details class="phase-controls" data-phase-controls><summary class="btn small">{tr("More")}</summary>
    <Show when={controls()} fallback={<p class="dim" role="status">{tr("Phase controls are unavailable. Read again.")}</p>}>{(c) => <>
      <Show when={paused()}>{(p) => <div data-phase-pause-owner><p>{pauseOwner()}</p><p>{p().episode?.reason}</p></div>}</Show>
      <button type="button" class="btn small" data-phase-barrier disabled={!enabled(c().barrier)} title={why(c().barrier)} onClick={() => void act(c().barrier, { kind: "set_phase_barrier", scope: c().scope, barrier: !props.draft.phase.barrier })}>{props.draft.phase.barrier ? tr("Remove phase barrier") : tr("Add phase barrier")}</button>
      <button type="button" class="btn small" data-phase-pause disabled={!enabled(c().pause_change)} title={why(c().pause_change)} onClick={() => open("pause", c().pause_change)}>{c().pause.state === "paused" ? tr("Replace pause reason") : tr("Pause phase")}</button>
      <button type="button" class="btn small" data-phase-resume disabled={!enabled(c().resume)} title={why(c().resume)} onClick={() => { const p = c().pause; if (p.state === "paused") void act(c().resume, { kind: "resume_phase", scope: c().scope, pause: p.episode?.id ?? null }); }}>{tr("Resume phase")}</button>
      <button type="button" class="btn small" data-phase-move-earlier disabled={!enabled(c().earlier.availability) || !c().earlier.target} title={why(c().earlier.availability)} onClick={() => void act(c().earlier.availability, { kind: "move_phase", scope: c().scope, relative_to: c().earlier.target, placement: "before" })}>{tr("Move phase earlier")}</button>
      <button type="button" class="btn small" data-phase-move-later disabled={!enabled(c().later.availability) || !c().later.target} title={why(c().later.availability)} onClick={() => void act(c().later.availability, { kind: "move_phase", scope: c().scope, relative_to: c().later.target, placement: "after" })}>{tr("Move phase later")}</button>
      <button type="button" class="btn small" data-phase-acceptance disabled={!enabled(c().acceptance)} title={why(c().acceptance)} onClick={() => open("acceptance", c().acceptance)}>{tr("Phase acceptance policy")}</button>
      <Show when={!c().unstarted.allowed}><p class="dim" role="status">{why(c().unstarted)}</p></Show>
    </>}</Show>
    <Show when={message()}><p class="notice danger" role="status">{message()}</p></Show>
    <Show when={form()}>{(frozen) => <ControlForm form={frozen()} available={enabled(frozen().kind === "pause" ? controls()?.pause_change : controls()?.acceptance) && props.draft.revision === frozen().draft.revision && props.view.bar.id === frozen().team.id && JSON.stringify(controls()?.scope) === JSON.stringify(frozen().controls.scope)} close={() => setForm(null)} run={run} />}</Show>
  </details>;
}
