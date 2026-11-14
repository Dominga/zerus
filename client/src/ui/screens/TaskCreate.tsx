import { taskTeamChoices } from "../teams/task-choices";
import { TextInput } from "../lib/TextField";
import { For, Show, createEffect, createMemo, createSignal, on, onCleanup, type JSX } from "solid-js";
import { createCard, newCard } from "../../store/taskForms";
import { createRecovery, queueRefused } from "./task-recovery";
import { selected } from "../../store/ui";
import { requestTaskDescription } from "../../store/taskNavigation";
import { Icon } from "../lib/icons";
import { tr } from "../../i18n";
import { Dropdown } from "../lib/Dropdown";
import { TeamChip } from "../teams/TeamChips";
import { teams } from "../../store/teams";

/** Mockup's inline new-card title; Escape retains its private per-project draft. */
export default function TaskCreate(props: { project?: string; scope: string; backlog: boolean; accessible: boolean; children: JSX.Element }) {
  const [creating, setCreating] = createSignal(false);
  /** The team chosen in this form, for this project only. */
  const [team, setTeam] = createSignal("");
  const [refusal, setRefusal] = createSignal("");
  /** A card the command line created and the team's queue did not take: the
   * task exists, so the way on is Send to team for that task, never a new card. */
  const recovery = createRecovery();
  // A notice belongs to its form's project: leaving the project retires it,
  // and a later reply of its Send is then only told. A disposed form takes its
  // notice with it.
  createEffect(on(() => props.project, () => recovery.dismiss(), { defer: true }));
  let field!: HTMLInputElement;
  /** The project's teams on the list's first page, while the preview is on: the card can join one's queue. */
  const teamChoices = createMemo(() => taskTeamChoices(teams.available(), teams.list()?.teams ?? [], props.project));
  createEffect(on(() => [props.project, props.accessible] as const, ([project, accessible]) => { setCreating(false); newCard.follow(project && accessible ? { project, task: "", mode: "new_card" } : undefined); }));
  onCleanup(() => newCard.follow());
  const view = () => newCard.view()?.project === props.project ? newCard.view() : undefined;
  // A choice belongs to the form's project and to a new card: another project,
  // lost access, Teams going off or a reserved card clear it.
  // Compared by value: an edit of the title is a new view of the same card and keeps the choice.
  const binding = createMemo(() => JSON.stringify([props.project ?? "", props.accessible, teams.available() ?? null, view()?.handle ?? null, !!view()?.recovery]));
  createEffect(on(binding, () => { setTeam(""); setRefusal(""); }, { defer: true }));
  /** A reserved card keeps the destination it was reserved with. */
  const reserved = () => view()?.recovery ? view()?.team ?? "" : undefined;
  const reservedName = () => teamChoices().find((candidate) => candidate.id === reserved())?.label ?? reserved() ?? "";
  createEffect(on(() => view()?.handle, () => { if (view()?.text || view()?.recovery) setCreating(true); }));
  createEffect(on(() => [creating(), view()?.handle] as const, () => { if (creating() && view()) queueMicrotask(() => field?.focus()); }));
  const submit = async () => {
    const project = props.project; if (!project || newCard.locked() || !view()?.text.trim() && !view()?.recovery) return;
    // Recovery sends the reserved card as it was reserved; a new card takes
    // only a team the form still offers for this project.
    const chosen = view()?.recovery ? "" : team();
    if (chosen && !teamChoices().some((candidate) => candidate.id === chosen)) {
      setTeam(""); setRefusal(tr("The chosen team is no longer offered for this project. Nothing was sent."));
      return;
    }
    setRefusal(""); recovery.dismiss();
    const name = teamChoices().find((candidate) => candidate.id === chosen)?.label ?? chosen;
    if (await createCard(project, chosen ? false : props.backlog, selected(), chosen || undefined)) {
      // The answer settles this submit only: its project's card, and the team
      // choice it sent, never one the person made meanwhile.
      setCreating(false); if (team() === chosen) setTeam("");
      const done = newCard.view();
      const id = done?.project === project ? done.created : undefined, refused = done?.not_queued;
      if (id && refused && refused.task === id && (!chosen || refused.team === chosen)) recovery.show({ project, task: id, team: refused.team, name: refused.team === chosen ? name : teamChoices().find((candidate) => candidate.id === refused.team)?.label ?? refused.team, reason: refused.reason, code: refused.code ?? "" });
      if (id) requestTaskDescription(project, id);
    }
  };
  return <><div class="tk-head"><Show when={creating() && props.project} fallback={<span class="tk-scope" aria-live="polite">{props.scope}</span>}>
    <TextInput ref={field} class="field tk-newtitle" maxlength={512} placeholder={tr("Task title")} aria-label={tr("New task title")} data-shortcut-context="none" value={view()?.text ?? ""} disabled={!!view()?.blocked} readOnly={newCard.opening() || newCard.busy() || view()?.read_only}
      onInput={(e) => newCard.edit(e.currentTarget.value, e.currentTarget.selectionEnd ?? 0, e.currentTarget.selectionStart ?? 0)} onBlur={() => void newCard.flush().catch(() => {})}
      onKeyDown={(e) => { if (e.isComposing) return; if (e.key === "Enter") { e.preventDefault(); void submit(); } else if (e.key === "Escape") { e.preventDefault(); e.stopPropagation(); setCreating(false); void newCard.flush().catch(() => {}); } }} />
  </Show><Show when={creating() && reserved() !== undefined && reserved()}>
      <Dropdown class="field tk-team" aria-label={tr("Team")} title={tr("The reserved card keeps its team")} disabled
        value={reserved()!} onChange={() => {}} options={[{ value: reserved()!, textValue: reservedName(), label: () => <TeamChip name={reservedName()} /> }]} />
    </Show><Show when={creating() && reserved() === undefined && teamChoices().length}>
      <Dropdown class="field tk-team" aria-label={tr("Team")} title={tr("Send the new task to a team's queue")}
        value={team()} onChange={(value) => { setTeam(value); setRefusal(""); }} options={[{ value: "", label: tr("No team") }, ...teamChoices().map((candidate) => ({ value: candidate.id, textValue: candidate.label, label: () => <TeamChip name={candidate.label} /> }))]} />
    </Show><button class="tk-sq add" aria-label={tr("New task")} title={tr("New task")} disabled={!props.project || !props.accessible} onClick={() => { setCreating(true); queueMicrotask(() => field?.focus()); }}><Icon name="plus" size={16} /></button>{props.children}</div>
    <Show when={(refusal() || newCard.error()) && creating()}><div class="tk-create-error" role="alert">{refusal() || newCard.error()}</div></Show>
    <Show when={recovery.notice()?.project === props.project ? recovery.notice() : undefined}>{(created) => <div class="tk-create-error" role="alert" data-not-queued>
      <span>{queueRefused(created())
        ? tr("The task was created, but %1 did not queue it: %2", created().name, created().reason)
        : tr("The task was created; whether %1 queued it is not known. Send to team queues it now, and never twice.", created().name)}</span>{" "}
      <Show when={!recovery.row() && recovery.reading().state === "reading"}><span class="dim" role="status" data-recovery-reading>{tr("Reading the task…")}</span>{" "}</Show>
      <Show when={!recovery.row() ? recovery.reading() : undefined}>{(answer) => <Show when={answer().state === "failed"}>
        <span data-recovery-unread>{tr("The task could not be read: %1", (answer() as { reason: string }).reason)}</span>{" "}
        <button type="button" class="btn small" onClick={() => recovery.readAgain()}>{tr("Read again")}</button>{" "}
      </Show>}</Show>
      <Show when={recovery.refusal()}><span data-recovery-refusal>{recovery.refusal()}</span>{" "}</Show>
      <button type="button" class="btn small" disabled={!recovery.row() || recovery.sending()} aria-busy={!recovery.row() || recovery.sending() ? "true" : undefined}
        title={!recovery.row() ? tr("Reading the task…") : undefined} onClick={() => void recovery.send()}>{tr("Send to team")}</button>{" "}
      <button type="button" class="btn small" onClick={() => recovery.dismiss()}>{tr("Dismiss")}</button>
    </div>}</Show></>;
}
