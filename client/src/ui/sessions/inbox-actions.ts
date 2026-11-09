import type { InboxRemoveReply } from "../../types";
import { trNoop, tr } from "../../i18n";
import type { InboxItem } from "./types";

// Remove for an OpenCode message that was not delivered (zerus-5wmh.13.6;
// Qt ActivityView.cpp:1072-1087 and SessionsWindow::removeQueuedMessage,
// 4433-4467). Apart from the view so it can be tested. Rust builds the
// native action only after its own check; nothing here is retried.

/** Under a not-delivered message: OpenCode keeps it and runs it first; Zerus never resends it. */
export const NOT_DELIVERED_HINT = trNoop("Still waiting in OpenCode's queue. It runs first with your next message unless you remove it.");
/** Removing is destructive: it asks first. */
export const REMOVE_TITLE = trNoop("Remove queued message");
export const REMOVE_TEXT = trNoop("Remove this queued message? It is still waiting in OpenCode's queue and runs first with your next message unless you remove it.");
export const REMOVE_CHANGED = trNoop("The queued message changed. Check Activity before removing it.");
export const REMOVED = trNoop("Queued message removed.");
export const REMOVING = trNoop("Removing…");

/** Remove's label and whether it is off: one removal at a time across sessions, and its own message says so. */
export function removeState(item: InboxItem, view: { inbox_removing?: string | null; inbox_busy?: boolean } | undefined): { label: string; disabled: boolean } {
  const removing = !!item.messageId && view?.inbox_removing === item.messageId;
  return { label: removing ? tr(REMOVING) : tr("Remove"), disabled: removing || !!view?.inbox_busy };
}

/** The notice one Remove ends with; a busy Remove ran nothing and says nothing (Qt's disabled button). */
export function removeNotice(reply: InboxRemoveReply): { tone: "error" | "info"; text: string; sticky: boolean } | undefined {
  switch (reply.outcome) {
    case "removed": return { tone: "info", text: tr(REMOVED), sticky: false };
    case "changed": return { tone: "error", text: tr(REMOVE_CHANGED), sticky: true };
    case "busy": return undefined;
    case "failed":
    case "uncertain": return { tone: "error", text: reply.detail, sticky: true };
  }
}
