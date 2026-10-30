import { createSignal } from "solid-js";
import { sessionLabel, type Task } from "../ui/screens/tasks-ui";
import type { TaskMessageDelivery } from "./taskEditor";

// Qt keeps this explicit choice for the window, separately from the shared
// comment draft. An outbox reservation retains its original choice on recovery.
export const [noteOnly, setNoteOnly] = createSignal(false);
export const queuedTask = (task: Task) => ["todo", "backlog"].includes(task.stage);
export const messageChoice = (task: Task) => task.stage === "active" ? "Send to current executor" : task.stage === "review" ? "Request changes and send" : task.stage === "done" ? "Reopen and send" : "Add to task, send on Start";
export const messageLabel = (task: Task, note: boolean) => {
  const executor = task.executor.name ? sessionLabel(task.executor) : "";
  return note ? "Save note only, no delivery" : queuedTask(task) ? "Add to task, sent on Start" : task.stage === "active" ? `Send to ${executor}` : task.stage === "review" ? `Request changes and send to ${executor}` : `Reopen and send to ${executor}`;
};
export const messagePlaceholder = (task: Task, note: boolean) => note ? "Write a note for this task…" : queuedTask(task) ? "Add context for the executor…" : "Message this task’s executor…";
export const messageDelivery = (task: Task, note: boolean): TaskMessageDelivery => ({ mode: note ? "note" : queuedTask(task) ? "queue" : task.stage === "active" ? "send" : "reopen", stage_at: task.stageAt, dispatch_id: task.dispatchId });
