import type { AttachmentScope } from "../../store/activitySource";
import type { AttachmentRef } from "../../types";
import type { ItemAttachment } from "./types";

// Activity attachment cards (ActivityView.cpp:759-782) and their Open
// (SessionsWindow.cpp:1093-1102), apart from the view so they can be tested.

/** What a card reads its file by: none for a file its message saved no reference to, or while no conversation shows. */
export function cardTarget(file: ItemAttachment, scope: AttachmentScope | undefined): { computer: string; ref: AttachmentRef } | undefined {
  if (!scope || !file.request_id) return undefined;
  return {
    computer: scope.computer,
    ref: {
      name: scope.name, request_id: file.request_id, index: file.index,
      conversation_id: scope.conversation_id, archive_id: scope.archive_id, agent_id: null,
    },
  };
}

/** One status line for an attachment being opened, as Qt's showNotice replaces its own. */
export const OPENING = "attachment-open";

export interface OpenPort {
  /** Null when it opened, a notice when the file manager shows it instead; rejects with why not. */
  open(computer: string, file: AttachmentRef): Promise<string | null>;
  notify(tone: "error" | "info", text: string, sticky: boolean, key: string): void;
}

/** Opens an Activity attachment as Qt does: "Opening attachment…", then a notice, or the error as a warning. */
export function createOpen(port: OpenPort) {
  return async (computer: string, file: AttachmentRef) => {
    port.notify("info", "Opening attachment…", false, OPENING);
    try {
      const notice = await port.open(computer, file);
      // A plain success leaves "Opening attachment…" to expire on its own.
      if (notice) port.notify("info", notice, false, OPENING);
    } catch (error) {
      port.notify("error", String(error), true, OPENING);
    }
  };
}
