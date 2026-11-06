import { tr } from "../../i18n";
import type { ClaudeAppSession, ClaudeAppState, ClaudeAppView } from "../../store/claudeApp";

// WhiteWh fork: the Claude app sessions dialog's texts
// (tray/src/ClaudeAppSessionsDialog.h render 119-156, confirm 168-185). Qt's
// wording, its State column order and its ago() buckets, with the parts the
// view lays out itself (the client separates values without dots).

/** Qt's `ago()`: whole minutes from 90 s, whole hours from 90 min, days from 36 h. */
export function ago(lastActivity: number, now: number): string {
  const seconds = now - Math.trunc(lastActivity);
  if (seconds < 90) return tr("just now");
  if (seconds < 5400) return tr("%1 min ago", Math.floor(seconds / 60));
  if (seconds < 129_600) return tr("%1 h ago", Math.floor(seconds / 3600));
  return tr("%1 days ago", Math.floor(seconds / 86_400));
}

/** The Session column: the app's title, else "Untitled (first 8 of the conversation)". */
export function titleOf(row: Pick<ClaudeAppSession, "title" | "conversation">): string {
  return row.title || tr("Untitled (%1)", row.conversation.slice(0, 8));
}

/** The State column. */
export function stateText(state: ClaudeAppState): string {
  switch (state) {
    case "open_in_app": return tr("Open in the app");
    case "archived": return tr("Archived");
    case "stopped": return tr("Stopped");
    case "no_history": return tr("No history");
    case "folder_missing": return tr("Folder missing");
  }
}

/** The status line: the read's error, or what the list means, as Qt words it. */
export function statusText(view: ClaudeAppView | undefined, shown: number, onlyFolder: boolean, error: string): string {
  if (error) return error;
  if (!view) return "";
  if (!view.available) return tr("The Claude app keeps no sessions on this computer.");
  if (!shown) return onlyFolder ? tr("No Claude app session works in this folder.") : tr("No Claude app sessions.");
  return view.truncated ? tr("The newest %1 sessions.", view.sessions.length) : "";
}

/** Why Continue is unavailable, as Qt's tooltip: the SessionTag problem last. */
export function continueTip(row: ClaudeAppSession | undefined, problem: string | undefined): string {
  if (!row) return "";
  if (!row.history) return tr("Claude Code has no history of this conversation here.");
  if (!row.folder_exists) return tr("Its folder is not on this computer.");
  return problem ?? "";
}

/** Qt's warning before continuing a session the app may still run. */
export function warning(row: Pick<ClaudeAppSession, "running_process">): { title: string; text: string } {
  return {
    title: tr("Still open in the Claude app?"),
    text: row.running_process
      ? tr("A Claude process of the app still continues this conversation. If both write to it, its history breaks. Stop the session in the Claude app first. Continue anyway?")
      : tr("This conversation changed in the last two minutes, so the Claude app may still run it. If both write to it, its history breaks. Continue anyway?"),
  };
}
