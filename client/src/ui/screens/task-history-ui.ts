import type { TaskEntry } from "../../store/taskDetails";
import { STAGE_LABEL, type Stage } from "./tasks-ui";

/** TaskDialogs::taskHistoryAction and TaskInspector's discussion receipts. */
export function historyAction(e: TaskEntry): string {
  const words: Record<string, string> = { created: "created the task", edited: "edited the prompt", cancelled: "cancelled it",
    resumed: "requested changes and submitted feedback", started: "started the task" };
  return words[e.kind] ?? (e.kind === "title" ? `renamed the task: ${e.text}` : e.kind === "stage" ? `moved it to ${STAGE_LABEL[e.text as Stage] ?? e.text}`
    : e.kind === "target" ? `changed the session: ${e.text}` : e.kind === "sent" ? `sent it to ${e.session}`
    : e.kind === "message_delivery" ? `message ${e.requestId} — ${e.delivery.state}` : e.text || e.kind);
}
export function deliveryText(e: TaskEntry, worker: string): string {
  const d = e.delivery;
  return e.kind === "comment" || d.state === "note" ? "Saved note — not sent" : d.state === "queued" ? "Waiting for Start"
    : d.state === "submitted" ? d.mode === "queue" ? "Included in Start request" : worker ? "Submitted to coordinator — child receipt unavailable" : "Submitted to executor"
    : d.state === "failed" ? "Saved in task — not sent" : "Saved in task — delivery unconfirmed";
}
export function deliveryError(e: TaskEntry): string {
  return /execution changed|identity changed|conversation changed|dispatch receipt|dispatched run/.test(e.delivery.error)
    ? "Your comment is saved here. The task’s previous executor could not be verified, so it was not sent. Open executor Activity to continue the discussion, or return the task to To do and start it with the current executor."
    : e.delivery.error;
}
