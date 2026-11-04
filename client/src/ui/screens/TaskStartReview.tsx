import { Show, createEffect, createSignal, onCleanup, onMount } from "solid-js";
import { taskStartReview, setTaskStartReview, releaseTaskStart, checkTaskStart, discardTaskStartCheck, startReviewView, taskStarts } from "../../store/taskStart";
import { writingTask } from "../../store/taskBoard";
import { Icon } from "../lib/icons";
import { tr } from "../../i18n";

/** The design owner's Task start variant of the executor-launch review. */
export default function TaskStartReview() {
  const review = taskStartReview()!;
  const live = () => startReviewView(review);
  const [discard, setDiscard] = createSignal<ReturnType<typeof live>>();
  createEffect(() => { const expected = discard(), current = live(); if (expected && (current.id !== expected.id || current.snapshot !== expected.snapshot || !current.unsent_check)) setDiscard(undefined); });
  let dialog!: HTMLDialogElement, closeButton!: HTMLButtonElement;
  const close = () => { if (taskStartReview() === review) setTaskStartReview(undefined); };
  onMount(() => { dialog.showModal(); queueMicrotask(() => closeButton?.focus()); });
  onCleanup(() => { if (dialog.open) dialog.close(); queueMicrotask(() => { if (taskStartReview() || taskStarts.scope()?.project !== review.view.project || taskStarts.scope()?.task !== review.view.task) return;
    const target = review.origin?.isConnected ? review.origin : (document.querySelector<HTMLElement>('.tv .tv-cmd[data-cmd="start"]:not(:disabled)') ?? document.querySelector<HTMLElement>(".tv .tv-title"));
    target?.focus(); }); });
  return <dialog ref={dialog} class="dlg" aria-labelledby="task-start-title" style={{ width: "520px" }} onCancel={close} onClose={close}>
    <div class="dlg-head"><Icon name="info" size={18} class="accent" /><h2 id="task-start-title">{tr("Task start")}</h2><span class="grow" /><button class="icon-btn" aria-label={tr("Close")} onClick={close}><Icon name="x" size={16} /></button></div>
    <div class="dlg-body"><div class="td-form">
      <div class="kv"><span>{tr("Task")}</span><span><b class="mono">{review.task.id}</b> {review.task.title || tr("Untitled task")}</span></div>
      <div class="kv"><span>{tr("Session")}</span><span class="mono">{review.view.session}</span></div>
      <div class="kv"><span>{tr("Request")}</span><span class="mono">{review.view.id}</span></div>
      <div class="kv"><span>{tr("State")}</span><span>{live().phase}</span></div>
      <Show when={live().error}><div class="notice warn"><Icon name="triangle-alert" size={13} /><span>{live().error}</span></div></Show>
      <Show when={live().check_problem}><p class="notice warn">{tr(live().check_problem!)}</p></Show>
      <p class="muted">{tr("Check the task history and this executor's Activity before allowing another start. Releasing the start record does not stop an agent or send the task again.")}</p>
      <Show when={discard()}><p class="notice warn">{tr("Discard this unsent receipt check? The task and its original start record will be kept.")}</p></Show>
    </div></div>
    <div class="dlg-foot" style={{ "flex-wrap": "wrap" }}>
      <Show when={discard()} fallback={<>
      <Show when={live().check_available}><button class="btn primary" disabled={!!writingTask()} onClick={() => void checkTaskStart(review).then((settled) => { if (settled) close(); })}>{tr("Check delivery receipt")}</button></Show>
      <Show when={live().unsent_check}><button class="btn" disabled={!!writingTask()} onClick={() => setDiscard({ ...live() })}>{tr("Discard unsent check")}</button></Show>
      <button class="btn" disabled={live().phase === "delivering" || !!writingTask()} title={live().phase === "delivering" ? tr("The start is still being delivered") : undefined} onClick={() => void releaseTaskStart(review).then((released) => { if (released) close(); })}>{tr("I reviewed it, allow another start")}</button><span class="spacer" /><button ref={closeButton} class="btn primary" autofocus onClick={close}>{tr("Close")}</button>
      </>}>{(expected) => <><button class="btn solid-danger" disabled={!!writingTask()} onClick={() => void discardTaskStartCheck(review, expected()).then(() => setDiscard(undefined))}>{tr("Discard unsent check")}</button><span class="spacer" /><button class="btn primary" disabled={!!writingTask()} onClick={() => setDiscard(undefined)}>{tr("Cancel")}</button></>}</Show></div>
  </dialog>;
}
