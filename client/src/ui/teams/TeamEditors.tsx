//! All forms pin the shown revision. Nothing writes on opening a dialog.
import { Show, createEffect, createSignal } from "solid-js";
import { tr } from "../../i18n";
import type { TeamMember, TeamOutcome, TeamView } from "../../store/teams";
import { TextArea, TextInput } from "../lib/TextField";
import { AddMemberDialog } from "./AddMemberDialog";
import { SwapMemberDialog } from "./SwapMemberDialog";
import { TeamEditDialog } from "./TeamEditDialog";
import { activeTeam } from "./actions";
import "./member-edit.css";
export type EditSubmit = (kind: string, fields: Record<string, unknown>) => void;
export type EditAction = (kind: string, fields?: Record<string, unknown>, revision?: number, team?: string) => unknown;
type Draft = { kind: string; view: TeamView; member?: TeamMember };
export function createTeamEditors(view: () => TeamView, action: EditAction) {
  const [draft, setDraft] = createSignal<Draft>();
  const [busy, setBusy] = createSignal(false);
  const [error, setError] = createSignal("");
  const [unknown, setUnknown] = createSignal(false);
  createEffect(() => {
    const opened = draft();
    if (opened && view().bar.id !== opened.view.bar.id) setDraft(undefined);
  });
  const open = (kind: string, member?: string) => {
    const shown = view();
    if (!activeTeam(shown.bar.status)) return;
    setDraft({ kind, view: shown, member: shown.members.find((m) => m.id === member) }); setError(""); setUnknown(false);
  };
  const close = () => { if (!busy()) setDraft(undefined); };
  const submit: EditSubmit = (kind, fields) => {
    const shown = draft(); if (!shown || busy() || unknown()) return;
    setBusy(true); setError("");
    void Promise.resolve().then(() => action(kind, fields, shown.view.revision, shown.view.bar.id)).then((result) => {
      const outcome = result && typeof result === "object" && "outcome" in result ? result as TeamOutcome : undefined;
      if (!outcome || outcome.outcome === "done" || outcome.outcome === "confirm") setDraft(undefined);
      else if (outcome.outcome === "failed") setError(outcome.detail);
      else { setUnknown(true); setError(tr("This action is awaiting a result. Close this dialog and use Check in the action list; do not submit it again.")); }
    }).catch(() => {
      // A transport exception does not prove that the writer refused it.
      setUnknown(true); setError(tr("The action's result is unknown. Check it in the action list before making another change."));
    }).finally(() => setBusy(false));
  };
  const dialogs = () => <Show when={draft()} keyed>{(d) => {
    const disabled = () => busy();
    if (d.kind === "add_member") return <AddMemberDialog view={d.view} busy={disabled()} locked={unknown()} error={error()} close={close} submit={submit} />;
    if (d.kind === "swap_member" && d.member) return <SwapMemberDialog member={d.member} busy={disabled()} locked={unknown()} error={error()} close={close} submit={submit} />;
    return <MetadataDialog kind={d.kind} view={d.view} busy={disabled()} locked={unknown()} error={error()} close={close} submit={submit} />;
  }}</Show>;
  return { open, dialogs };
}
function MetadataDialog(props: { kind: string; view: TeamView; busy: boolean; locked?: boolean; error: string; close: () => void; submit: EditSubmit }) {
  const rename = props.kind === "rename_team";
  const [text, setText] = createSignal(rename ? props.view.bar.name : props.view.edit_goal ?? "");
  return <TeamEditDialog title={rename ? tr("Rename team") : tr("Edit goal")} action={tr("Save")} busy={props.busy} error={props.error} close={props.close}
    disabled={props.locked || (!rename && props.view.edit_goal == null) || (rename && !text().trim())} submit={() => props.submit(props.kind, rename ? { name: text() } : { goal: text() })}>
    <label>{rename ? tr("Team name") : tr("Goal")}
      <Show when={rename} fallback={<TextArea class="field" aria-label={tr("Goal")} rows={6} maxlength={16384} value={text()} disabled={props.busy} onInput={(e) => setText(e.currentTarget.value)} />}>
        <TextInput class="field" aria-label={tr("Team name")} maxlength={256} value={text()} disabled={props.busy} onInput={(e) => setText(e.currentTarget.value)} />
      </Show>
    </label>
    <Show when={rename}><p class="dim">{tr("Existing session, folder and branch names stay as they are.")}</p></Show>
  </TeamEditDialog>;
}
export function TeamEditButtons(props: { view: TeamView; busy: boolean; open: (kind: string) => void }) {
  return <Show when={activeTeam(props.view.bar.status)}><span class="team-edit-buttons">
    <button type="button" class="btn small" disabled={props.busy} onClick={() => props.open("add_member")}>{tr("Add member")}</button>
    <button type="button" class="btn small" disabled={props.busy} onClick={() => props.open("rename_team")}>{tr("Rename team")}</button>
    <button type="button" class="btn small" disabled={props.busy || props.view.edit_goal == null} title={props.view.edit_goal == null ? tr("Refresh the team to read its complete goal.") : undefined} onClick={() => props.open("set_goal")}>{tr("Edit goal")}</button>
  </span></Show>;
}
