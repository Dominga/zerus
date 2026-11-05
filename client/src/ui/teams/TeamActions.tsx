import { For, Show, createMemo, createSignal, onCleanup, onMount, type JSX } from "solid-js";
import { tr } from "../../i18n";
import { Icon } from "../lib/icons";
import { teams } from "../../store/teams";
import { useModel, windowPort } from "./ProposalCards";
import {
  TeamActions, barActions, barChips, finishSummary, formText, moreItems, stageName, subtreeImpact, taskButtons,
  type ActTask, type ActTeam, type BarAction, type BarChip, type TaskForm,
} from "./actions";
import type { ComputerId } from "../../types";

// Task 40: the team bar's actions and chips, and the task card's actions
// (ui/teams/actions.ts holds what they are and what they send).

/** A small modal with a text field: Finish's summary and Send back's comment. */
function TextDialog(props: {
  title: string; hint: string; placeholder: string; action: string; danger?: boolean; count?: (text: string) => { count: string; ok: boolean };
  close: () => void; submit: (text: string) => void;
}) {
  let dialog!: HTMLDialogElement;
  let field!: HTMLTextAreaElement;
  const [text, setText] = createSignal("");
  const state = () => props.count?.(text()) ?? { count: "", ok: text().trim() !== "" };
  onMount(() => { dialog.showModal(); queueMicrotask(() => field.focus()); });
  onCleanup(() => { if (dialog.open) dialog.close(); });
  return (
    <dialog ref={dialog} class="dlg team-text-dialog" aria-labelledby="team-text-title" onCancel={(event) => { event.preventDefault(); props.close(); }}>
      <div class="dlg-head"><h2 id="team-text-title">{props.title}</h2></div>
      <div class="dlg-body">
        <p class="team-confirm-text">{props.hint}</p>
        <textarea ref={field} class="field team-text-field" rows={5} placeholder={props.placeholder} value={text()} onInput={(event) => setText(event.currentTarget.value)} />
        <Show when={state().count}><div class="dim team-text-count">{state().count}</div></Show>
      </div>
      <div class="dlg-foot"><span class="spacer" />
        <button type="button" class="btn" onClick={props.close}>{tr("Cancel")}</button>
        <button type="button" class="btn" classList={{ primary: !props.danger, "solid-danger": !!props.danger }} disabled={!state().ok}
          onClick={() => { if (state().ok) { props.submit(text()); props.close(); } }}>{props.action}</button>
      </div>
    </dialog>
  );
}

/** The bar's chips: the pending phase (opens it), Finish waiting, waiting for the lead. */
export function TeamBarChips(props: { team: ActTeam; onPhase: (phase: number) => void; onFinish: () => void }) {
  const chips = createMemo(() => barChips(props.team));
  const click = (chip: BarChip) => {
    if (chip.id === "pendingPhase" && chip.phase) props.onPhase(chip.phase);
    else if (chip.id === "finishWaits") props.onFinish();
  };
  return (
    <For each={chips()}>{(chip) => chip.id === "waitingForLead"
      ? <span class="team-chip accent" data-chip={chip.id}>{chip.text}</span>
      : <button type="button" class="team-chip link" data-chip={chip.id} title={chip.tip} onClick={() => click(chip)}>{chip.text}</button>}
    </For>
  );
}

const ICON: Partial<Record<BarAction["id"], string>> = { approve: "play", resume: "play", pause: "pause", finish: "check", sendBack: "rotate-ccw", stop: "circle-stop", discard: "trash" };

/**
 * The bar's actions. `planMode` is the owner's free/standard choice for the
 * approval; `sendToCoordinator` puts Send back's message to the coordinator.
 */
export function TeamBarActions(props: { computer: ComputerId; team: ActTeam; planMode: string; sendToCoordinator: (text: string) => void; finishOpen?: () => boolean; setFinishOpen?: (open: boolean) => void }) {
  const { model, tick } = useModel((changed) => new TeamActions(windowPort, () => ({ computer: props.computer, team: props.team }), changed));
  const read = () => (tick(), model);
  const [localFinish, setLocalFinish] = createSignal(false);
  const finishOpen = () => props.finishOpen?.() ?? localFinish();
  const setFinish = (open: boolean) => (props.setFinishOpen ?? setLocalFinish)(open);
  const [sendBack, setSendBack] = createSignal(false);
  const actions = createMemo(() => barActions(props.team, props.planMode));
  const impact = () => subtreeImpact(props.team, teams.list(props.computer)?.teams ?? []);
  const run = (action: BarAction) => {
    switch (action.id) {
      case "resumeLimits": return void model.resumeAfterLimits(!action.checked);
      case "finish": return setFinish(true);
      case "discard": return void model.discard();
      case "sendBack": return setSendBack(true);
      case "approve": return void model.approve(props.planMode);
      case "pause": return void model.pause(impact());
      case "resume": return void model.resume();
      case "stop": return void model.stop(impact());
      case "archive": return void model.archive();
    }
  };
  return (
    <div class="team-bar-actions" role="toolbar" aria-label={tr("Team actions")}>
      <For each={actions()}>{(action) => (
        <button type="button" class="btn small" data-action={action.id} title={action.tip}
          classList={{ primary: action.primary, danger: action.danger, ghost: action.id === "discard", on: !!action.checked }}
          aria-pressed={action.checked === undefined ? undefined : action.checked} disabled={!action.enabled || read().busy}
          onClick={() => run(action)}>
          <Show when={ICON[action.id]}><Icon name={ICON[action.id]!} size={12} /></Show>{action.text}
        </button>
      )}</For>
      <Show when={read().notice}><span class="team-proposal-notice" role="status">{read().notice}</span></Show>
      <Show when={finishOpen()}>
        <TextDialog title={tr("Finish %1?", props.team.name)} hint={tr("Every task is decided. The summary goes to the lead and the team becomes Done.")}
          placeholder={tr("What the team delivered, and anything left undone…")} action={tr("Finish team")} count={finishSummary}
          close={() => setFinish(false)} submit={(text) => void model.finish(text)} />
      </Show>
      <Show when={sendBack()}>
        <TextDialog title={tr("Send back to the coordinator")} hint={tr("What should change in the plan?")} placeholder={tr("What should change in the plan?")}
          action={tr("Send back")} close={() => setSendBack(false)} submit={(text) => props.sendToCoordinator(text)} />
      </Show>
    </div>
  );
}

/** The task card's actions: the footer by status, More, the inline forms and Reassign's member list. */
export function TaskActions(props: { computer: ComputerId; team: ActTeam; task: ActTask }) {
  const { model, tick } = useModel((changed) => new TeamActions(windowPort, () => ({ computer: props.computer, team: props.team }), changed));
  const read = () => (tick(), model);
  const [form, setForm] = createSignal<TaskForm | undefined>();
  const [reason, setReason] = createSignal("");
  const [returnLater, setReturnLater] = createSignal(false);
  const [menu, setMenu] = createSignal<"more" | "reassign" | undefined>();
  const open = (next: TaskForm) => { setForm(next); setReason(""); setReturnLater(false); setMenu(undefined); };
  const words = () => (form() ? formText(props.team, props.task, form()!) : undefined);
  const peers = () => props.team.members.filter((member) => member.id !== props.task.assignee && !member.lead);
  const button = (id: string) => {
    switch (id) {
      case "accept": return void model.accept(props.task);
      case "release": return void model.release(props.task);
      case "reassign": return setMenu(menu() === "reassign" ? undefined : "reassign");
      case "takeOver": return void model.takeOver(props.task);
      case "cancel": return void model.cancel(props.task);
    }
  };
  const item = (id: string, phase?: number, next?: TaskForm) => {
    setMenu(undefined);
    if (next) return open(next);
    switch (id) {
      case "retry": return void model.retry(props.task);
      case "release": return void model.release(props.task);
      case "move": return void model.move(props.task, phase!);
      case "reassign": return setMenu("reassign");
      case "takeOver": return void model.takeOver(props.task);
      case "cancel": return void model.cancel(props.task);
    }
  };
  const submit = async () => {
    const which = form();
    if (!which || !reason().trim()) return;
    const outcome = await model.submit(props.task, which, reason(), returnLater());
    if (outcome && outcome !== "cancelled" && outcome.outcome === "done") setForm(undefined);
  };
  return (
    <div class="team-task-actions" data-task={props.task.id}>
      <Show when={props.task.pipeline_run}>{(run) => (
        <div class="team-stage-chips" aria-label={tr("Pipeline %1", run().name || props.task.pipeline || "")}>
          <For each={run().stages}>{(kind, index) => (
            <span class="team-chip" classList={{ accent: run().open?.stage === index() }} data-stage={kind}>{stageName(kind)}</span>
          )}</For>
        </div>
      )}</Show>
      <Show when={words()}>{(text) => (
        <div class="team-task-form" data-form={form()}>
          <b>{text().heading}</b>
          <p class="dim">{text().what}</p>
          <textarea class="field" rows={3} placeholder={text().placeholder} value={reason()} onInput={(event) => setReason(event.currentTarget.value)} ref={(el) => queueMicrotask(() => el.focus())} />
          <Show when={form() === "reject"}>
            <label class="team-proposal-option"><input type="checkbox" checked={returnLater()} onChange={(event) => setReturnLater(event.currentTarget.checked)} />
              {tr("Return later (hold): keep it with the member until released")}</label>
          </Show>
          <div class="team-proposal-answers"><span class="grow" />
            <button type="button" class="btn small" onClick={() => setForm(undefined)}>{tr("Cancel")}</button>
            <button type="button" class="btn small primary" disabled={!reason().trim() || read().busy} onClick={() => void submit()}>{text().submit}</button>
          </div>
        </div>
      )}</Show>
      <div class="team-task-footer">
        <For each={taskButtons(props.task)}>{(b) => (
          <button type="button" class="btn small" classList={{ primary: b.primary, danger: b.danger }} data-action={b.id} disabled={read().busy}
            onClick={() => (b.form ? open(b.form) : button(b.id))}>{b.text}</button>
        )}</For>
        <span class="grow" />
        <button type="button" class="btn small ghost" aria-haspopup="menu" aria-expanded={menu() === "more"} onClick={() => setMenu(menu() === "more" ? undefined : "more")}>
          {tr("More")}<Icon name="chevron-down" size={12} />
        </button>
      </div>
      <Show when={menu() === "more"}>
        <div class="menu team-task-menu" role="menu" aria-label={tr("More task actions")}>
          <For each={moreItems(props.team, props.task)}>{(entry) => (
            <button type="button" role="menuitem" class="menu-item" disabled={!entry.enabled} title={entry.why} data-item={entry.id}
              onClick={() => item(entry.id, entry.phase, entry.form)}>{entry.text}</button>
          )}</For>
        </div>
      </Show>
      <Show when={menu() === "reassign"}>
        <div class="menu team-task-menu" role="menu" aria-label={tr("Reassign to")}>
          <For each={peers()} fallback={<span class="dim">{tr("No other member")}</span>}>{(member) => (
            <button type="button" role="menuitem" class="menu-item" onClick={() => { setMenu(undefined); void model.reassign(props.task, member.id); }}>
              {member.role || member.id}
            </button>
          )}</For>
        </div>
      </Show>
      <Show when={read().notice}><div class="team-proposal-notice" role="status">{read().notice}</div></Show>
    </div>
  );
}

/** Show what blocks: the open tasks that wait, and what each waits for (Qt teamShowBlockers). */
export function Blockers(props: { team: ActTeam; onTask?: (id: number) => void }): JSX.Element {
  const [shown, setShown] = createSignal(false);
  const waiting = createMemo(() => props.team.tasks.filter((task) => !["accepted", "rejected", "cancelled"].includes(task.status) && task.waits));
  return (
    <span class="team-blockers">
      <button type="button" class="btn small ghost" aria-expanded={shown()} onClick={() => setShown(!shown())}>{shown() ? tr("Hide blockers") : tr("Show what blocks")}</button>
      <Show when={shown()}>
        <span class="team-blockers-list">
          <For each={waiting()} fallback={<span class="dim">{tr("Nothing names a blocker.")}</span>}>{(task) => (
            <button type="button" class="team-chip link" onClick={() => props.onTask?.(task.id)}>{tr("#%1 waits: %2", task.id, task.waits ?? "")}</button>
          )}</For>
        </span>
      </Show>
    </span>
  );
}
