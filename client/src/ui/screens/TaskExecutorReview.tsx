import { Show, onMount, onCleanup } from "solid-js";
import { taskExecutorReview, setTaskExecutorReview, releaseTaskExecutor, taskExecutors } from "../../store/taskExecutors";
import { writingTask } from "../../store/taskBoard";
import { selectedTask } from "../../store/taskNavigation";
import { Icon } from "../lib/icons";
export default function TaskExecutorReview() {
  const review = taskExecutorReview()!;
  const live = () => { const current = taskExecutors.view(); return current?.project === review.view.project && current.task === review.view.task && current.id === review.view.id ? current : review.view; };
  let dialog!: HTMLDialogElement, closeButton!: HTMLButtonElement;
  const close = () => { if (taskExecutorReview() === review) setTaskExecutorReview(undefined); };
  onMount(() => { dialog.showModal(); queueMicrotask(() => closeButton.focus()); });
  onCleanup(() => { if (dialog.open) dialog.close(); queueMicrotask(() => {
    if (taskExecutorReview() || selectedTask()?.project !== review.view.project || selectedTask()?.uuid !== review.view.task) return;
    const target = review.origin.isConnected ? review.origin : (document.querySelector<HTMLElement>('.tv .tv-cmd[aria-label="Start task"]:not(:disabled)') ?? document.querySelector<HTMLElement>(".tv .tv-title"));
    target?.focus();
  }); });
  return <dialog ref={dialog} class="dlg" aria-labelledby="task-executor-title" style={{ width: "520px" }} onClose={close} onCancel={close}>
    <div class="dlg-head"><Icon name="info" size={18} class="accent" /><h2 id="task-executor-title">Task executor launch</h2><span class="grow" /><button class="icon-btn" aria-label="Close" onClick={close}><Icon name="x" size={16} /></button></div>
    <div class="dlg-body"><div class="td-form"><div class="kv"><span>Task</span><span><b class="mono">{review.task.id}</b> {review.task.title}</span></div><div class="kv"><span>Session</span><span class="mono">{review.view.session}</span></div><div class="kv"><span>Launch</span><span class="mono">{review.view.id}</span></div><div class="kv"><span>State</span><span>{live().phase}</span></div>
      <Show when={live().error}><div class="notice warn"><Icon name="triangle-alert" size={13} /><span>{live().error}</span></div></Show>
      <p class="muted">Check the task history and this executor before allowing another launch. Releasing the launch record does not stop an agent or retry delivery.</p></div></div>
    <div class="dlg-foot"><button class="btn" disabled={live().phase === "waiting" || !!writingTask()} title={live().phase === "waiting" ? "The new executor is starting" : undefined} onClick={() => void releaseTaskExecutor({ ...review, view: { ...live() } }).then((ok) => { if (ok) close(); })}>I reviewed it, allow another launch</button><span class="spacer" /><button ref={closeButton} class="btn primary" autofocus onClick={close}>Close</button></div>
  </dialog>;
}
