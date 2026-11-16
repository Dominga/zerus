import { For, Show, createSignal, onCleanup, onMount } from "solid-js";
import { tr } from "../../i18n";
import { newRequestId, teams, type TeamCall, type TeamOutcome, type TeamView } from "../../store/teams";
import type { PhaseDraft } from "../../store/team-phases";
import { TextArea } from "../lib/TextField";
import { PhaseReview } from "./PhaseReview";

type Correction = { publication: string; rework: string };
type Review = { kind: "approve" | "reject"; draft: PhaseDraft; view: TeamView; correction?: Correction };

function ReviewDialog(props: { review: Review; close: () => void; run: (call: TeamCall) => Promise<TeamOutcome> }) {
  let dialog!: HTMLDialogElement;
  const frozen = props.review;
  const [comment, setComment] = createSignal("");
  const [pending, setPending] = createSignal(false), [locked, setLocked] = createSignal(false);
  const [message, setMessage] = createSignal("");
  const initial = !frozen.correction && frozen.view.bar.status === "planning";
  onMount(() => dialog.showModal());
  onCleanup(() => { if (dialog.open) dialog.close(); });
  const submit = async () => {
    if (pending() || locked() || (frozen.kind === "reject" && !comment().trim())) return;
    setPending(true); setMessage("");
    const call: TeamCall = { team: frozen.view.bar.id, team_name: frozen.view.bar.name, request_id: newRequestId(), expected_revision: frozen.draft.revision,
      action: frozen.correction
        ? { kind: frozen.kind === "approve" ? "approve_phase_corrections" : "reject_phase_corrections", phase: frozen.draft.phase.n, generation: frozen.draft.generation, publication: frozen.correction.publication, rework: frozen.correction.rework, ...(frozen.kind === "reject" ? { comment: comment() } : {}) }
        : frozen.kind === "approve"
        ? { kind: "approve_phase", phase: frozen.draft.phase.n, generation: frozen.draft.generation, confirm_initial: initial }
        : { kind: "reject_phase", phase: frozen.draft.phase.n, generation: frozen.draft.generation, comment: comment() } };
    try {
      const result = await props.run(call);
      if (result.outcome === "done") { props.close(); return; }
      if (result.outcome === "uncertain" || result.outcome === "pending") {
        setLocked(true); setMessage(tr("The change is unconfirmed. Close this editor and use Check in the team's unresolved actions."));
      } else if (result.outcome === "failed") {
        setLocked(result.code === "stale_revision"); setMessage(result.detail);
      }
    } catch {
      setLocked(true); setMessage(tr("The change is unconfirmed. Close this editor and use Check in the team's unresolved actions."));
    } finally { setPending(false); }
  };
  return <dialog ref={dialog} class="dlg phase-editor" aria-labelledby="phase-review-title" onCancel={(e) => { e.preventDefault(); if (!pending()) props.close(); }}>
    <div class="dlg-head"><h2 id="phase-review-title">{frozen.correction ? (frozen.kind === "approve" ? tr("Approve corrections") : tr("Reject corrections")) : (frozen.kind === "approve" ? tr("Approve phase") : tr("Reject phase"))}</h2></div>
    <div class="dlg-body">
      <h3>{frozen.draft.phase.title}</h3><PhaseReview draft={frozen.draft} />
      <Show when={frozen.correction}><p>{tr("This review covers only the explicit correction tasks. Previously accepted work stays unchanged.")}</p></Show>
      <Show when={frozen.kind === "approve"}>
        <p>{tr("No tasks are handed out until creation of every phase task is confirmed.")}</p>
        <Show when={initial}>
          <p>{tr("This initial approval also confirms the roster and working folders shown here.")}</p>
          <p>{tr("Project: %1", frozen.view.bar.project)}</p><p>{tr("Integration branch: %1", frozen.view.bar.integration_branch || tr("None"))}</p>
          <ul><For each={frozen.view.members}>{(member) => <li>
            <b>{member.id}</b>: {member.role} / {member.agent} / {member.model || tr("Default model")}
            <p class="dim">{member.account || (member.account_choice?.mode === "named" ? member.account_choice.account : tr("Account selected at start"))}</p>
            <p class="dim">{member.state === "not_created" ? tr("New managed worktree; its folder is not created yet.") : tr("Keeps the member's existing working folder.")}</p>
          </li>}</For></ul>
        </Show>
      </Show>
      <Show when={frozen.kind === "reject"}><label>{tr("Rejection comment")}<TextArea value={comment()} maxLength={16384} disabled={pending() || locked()} onInput={(e) => setComment(e.currentTarget.value)} /></label></Show>
      <Show when={message()}><p class="notice danger" role="status">{message()}</p></Show>
    </div>
    <div class="dlg-foot"><button type="button" class="btn" disabled={pending()} onClick={props.close}>{tr("Close")}</button>
      <button type="button" class="btn primary" data-phase-review-submit disabled={pending() || locked() || (frozen.kind === "reject" && !comment().trim())} onClick={() => void submit()}>{frozen.correction ? (frozen.kind === "approve" ? tr("Approve corrections") : tr("Reject corrections")) : (frozen.kind === "approve" ? tr("Approve phase") : tr("Reject phase"))}</button>
    </div>
  </dialog>;
}

export function PhaseActions(props: { draft: PhaseDraft; view: TeamView; busy: boolean; available: boolean; readFailed: boolean; correction?: Correction; run?: (call: TeamCall) => Promise<TeamOutcome>; onChanged?: () => void }) {
  const [review, setReview] = createSignal<Review | null>(null);
  const [message, setMessage] = createSignal("");
  const [pending, setPending] = createSignal(false), [locked, setLocked] = createSignal(false);
  const enabled = () => props.available && !props.busy && !pending() && !locked() && !props.readFailed && !props.draft.unknown_fields && props.draft.complete && props.draft.generation !== null
    && ["planning", "running", "paused"].includes(props.view.bar.status);
  const run = async (call: TeamCall) => {
    const result = props.run ? await props.run(call) : await teams.run(call, "phase");
    if (result.outcome === "done") props.onChanged?.();
    return result;
  };
  const open = (kind: Review["kind"]) => {
    if (!enabled() || props.draft.publication) return;
    const roster = props.view.members;
    if (kind === "approve" && props.view.bar.status === "planning" && (
      roster.length !== props.view.bar.counts.members.total
      || new Set(roster.map((member) => member.id)).size !== roster.length
      || roster.some((member) => !member.id)
    )) {
      setMessage(tr("The full roster is unavailable. Read again before confirming members and working folders.")); return;
    }
    if (props.view.bar.revision !== props.draft.revision) { setMessage(tr("The team and phase revisions differ. Read again before reviewing the roster.")); return; }
    setMessage(""); setReview({ kind, draft: structuredClone(props.draft), view: structuredClone(props.view), ...(props.correction ? { correction: structuredClone(props.correction) } : {}) });
  };
  const act = async (action: TeamCall["action"]) => {
    if (!enabled()) return;
    setPending(true); setMessage("");
    try {
      const result = await run({ team: props.view.bar.id, team_name: props.view.bar.name, expected_revision: props.draft.revision, request_id: newRequestId(), action });
      if (result.outcome === "failed") setMessage(result.detail);
      else if (result.outcome !== "done") { setLocked(true); setMessage(tr("The change is unconfirmed. Close this editor and use Check in the team's unresolved actions.")); }
    } catch { setLocked(true); setMessage(tr("The change is unconfirmed. Close this editor and use Check in the team's unresolved actions.")); }
    finally { setPending(false); }
  };
  return <div class="phase-actions" data-phase-actions>
    <Show when={!props.available}><span class="dim">{tr("Phase approval is not available on this computer.")}</span></Show>
    <Show when={props.draft.phase.state === "draft" && !props.draft.publication}>
      <button type="button" class="btn small" data-phase-approve disabled={!enabled()} onClick={() => open("approve")}>{props.correction ? tr("Approve corrections") : tr("Approve phase")}</button>
      <button type="button" class="btn small" data-phase-reject disabled={!enabled()} onClick={() => open("reject")}>{props.correction ? tr("Reject corrections") : tr("Reject phase")}</button>
    </Show>
    <Show when={props.draft.rejection}>{(rejection) => <>
      <p class="phase-description">{rejection().comment}</p>
      <button type="button" class="btn small" data-phase-undo disabled={!enabled()} onClick={() => void act({ kind: props.correction ? "undo_phase_correction_rejection" : "undo_reject_phase", phase: props.draft.phase.n, generation: rejection().generation, rejection: rejection().id, ...(props.correction ? { publication: props.correction.publication, rework: props.correction.rework } : {}) })}>{tr("Undo rejection")}</button>
    </>}</Show>
    <Show when={props.draft.publication}>{(p) => <div role="status" data-phase-publication>
      <p>{tr("Task creation: %1 of %2 confirmed", p().created, p().total)}</p>
      <Show when={p().reason}><p class="notice danger">{p().reason}</p></Show>
      <Show when={props.draft.phase.state === "awaiting_acceptance"}>
        <button type="button" class="btn small" data-phase-accept disabled={!enabled() || !props.draft.accept_available || !props.draft.live?.complete || !props.draft.graph.complete} onClick={() => void act({ kind: "accept_phase", phase: props.draft.phase.n, publication: p().id })}>{tr("Accept phase")}</button>
      </Show>
    </div>}</Show>
    <Show when={message()}><p class="notice danger" role="status">{message()}</p></Show>
    <Show when={review()}>{(frozen) => <ReviewDialog review={frozen()} close={() => setReview(null)} run={run} />}</Show>
  </div>;
}
