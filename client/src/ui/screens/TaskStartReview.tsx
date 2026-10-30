import { Show, onCleanup, onMount } from "solid-js";
import { taskStartReview, setTaskStartReview, releaseTaskStart, startReviewView, taskStarts } from "../../store/taskStart";
import { writingTask } from "../../store/taskBoard";
import { Icon } from "../lib/icons";

/** The design owner's Task start variant of the executor-launch review. */
export default function TaskStartReview() {
  const review = taskStartReview()!;
  const live = () => startReviewView(review);
  let dialog!: HTMLDialogElement, closeButton!: HTMLButtonElement;
  const close = () => { if (taskStartReview() === review) setTaskStartReview(undefined); };
  onMount(() => { dialog.showModal(); queueMicrotask(() => closeButton?.focus()); });
  onCleanup(() => { if (dialog.open) dialog.close(); queueMicrotask(() => { if (taskStartReview() || taskStarts.scope()?.project !== review.view.project || taskStarts.scope()?.task !== review.view.task) return;
    const target = review.origin?.isConnected ? review.origin : (document.querySelector<HTMLElement>('.tv .tv-cmd[aria-label="Start task"]:not(:disabled)') ?? document.querySelector<HTMLElement>(".tv .tv-title"));
    target?.focus(); }); });
  return <dialog ref={dialog} class="dlg" aria-labelledby="task-start-title" style={{ width: "520px" }} onCancel={close} onClose={close}>
    <div class="dlg-head"><Icon name="info" size={18} class="accent" /><h2 id="task-start-title">Task start</h2><span class="grow" /><button class="icon-btn" aria-label="Close" onClick={close}><Icon name="x" size={16} /></button></div>
    <div class="dlg-body"><div class="td-form">
      <div class="kv"><span>Task</span><span><b class="mono">{review.task.id}</b> {review.task.title || "Untitled task"}</span></div>
      <div class="kv"><span>Session</span><span class="mono">{review.view.session}</span></div>
      <div class="kv"><span>Request</span><span class="mono">{review.view.id}</span></div>
      <div class="kv"><span>State</span><span>{live().phase}</span></div>
      <Show when={live().error}><div class="notice warn"><Icon name="triangle-alert" size={13} /><span>{live().error}</span></div></Show>
      <p class="muted">Check the task history and this executor's Activity before allowing another start. Releasing the start record does not stop an agent or send the task again.</p>
    </div></div>
    <div class="dlg-foot"><button class="btn" disabled={live().phase === "delivering" || !!writingTask()} title={live().phase === "delivering" ? "The start is still being delivered" : undefined} onClick={() => void releaseTaskStart(review).then((released) => { if (released) close(); })}>I reviewed it, allow another start</button><span class="spacer" /><button ref={closeButton} class="btn primary" autofocus onClick={close}>Close</button></div>
  </dialog>;
}
