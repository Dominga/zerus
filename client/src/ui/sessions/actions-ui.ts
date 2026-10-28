import type { Details, RowOut, ViewRow } from "../../types";

// Which session actions are offered and with which words, after the Qt
// SessionsWindow header (3890-3912), its context menu (5270-5380) and the
// Terminate confirmation (5771-5797). Pure: the components only render it.

export interface MenuState {
  change: { label: "Pause session" | "Resume session" | "Restore session" | "Updating…"; enabled: boolean; reason?: string };
  rename: boolean;
  archive: { visible: boolean; enabled: boolean; reason?: string };
  end: { label: "Terminate session…" | "Forget saved session…" | "Forget archived session…"; enabled: boolean; reason?: string };
  stop: boolean;
  markRead: { label: "Mark as read" | "Mark needs attention"; enabled: boolean; reason?: string };
}

export const FORGET_LATER = "Forgetting sessions arrives in a later version.";
export const RESTORE_LATER = "Restoring sessions arrives in a later version.";
export const MARK_NEEDS_LATER = "Arrives with attention marks.";
export const READ_STATE_HOST = "Update hgs on this session's host to share read state.";
const NO_REASON = "This native adapter does not support this action in the current session state.";

/** NativeSessionCapabilities::action: details win, then reported actions, then the agent. */
function allowed(row: ViewRow, op: string, details?: Details): boolean {
  const list = details?.allowed_actions ?? row.allowed_actions;
  if (list) return list.includes(op);
  return row.agent !== "opencode" || op === "rename" || op === "forget";
}

const reasonFor = (row: ViewRow, op: string, details?: Details) =>
  details?.action_reasons?.[op] || row.action_reasons?.[op] || NO_REASON;

export function menuState(entry: RowOut, online: boolean, updating: boolean, details?: Details): MenuState {
  const row = entry.row;
  const available = online && !updating;
  const archived = row.state === "archived";
  const running = row.state === "running";
  const idle = row.activity === "idle" || !!details?.auth_required;
  const canPause = running && !!row.resumable && idle && row.process_state !== "exited"
    && (!details || details.expected_id === "" || details.resume_message_can_send);
  const change = !archived && available
    && ((canPause && allowed(row, "pause", details)) || (!running && !!row.resumable && allowed(row, "resume", details)));
  const [project, session] = (row.name ?? "").split("/", 2);

  const bindingOnly = row.agent === "opencode" && allowed(row, "forget", details) && !allowed(row, "terminate", details);
  const terminate = running && !bindingOnly;
  const end: MenuState["end"] = terminate
    ? { label: "Terminate session…", enabled: available && allowed(row, "terminate", details), reason: allowed(row, "terminate", details) ? undefined : reasonFor(row, "terminate", details) }
    : { label: archived ? "Forget archived session…" : "Forget saved session…", enabled: false, reason: FORGET_LATER };

  const needsAttention = !!row.attention || !!row.unread;
  const markRead: MenuState["markRead"] = !needsAttention
    ? { label: "Mark needs attention", enabled: false, reason: MARK_NEEDS_LATER }
    : !row.shared_read
      ? { label: "Mark as read", enabled: false, reason: READ_STATE_HOST }
      : { label: "Mark as read", enabled: available };

  return {
    change: {
      label: updating ? "Updating…" : archived ? "Restore session" : running ? "Pause session" : "Resume session",
      enabled: change,
      reason: archived ? RESTORE_LATER : undefined,
    },
    rename: allowed(row, "rename", details) && available && !!project && !!session && (!archived || !!row.archive_id),
    archive: {
      visible: row.agent !== "dsh" && (row.state === "paused" || row.state === "stopped"),
      enabled: !updating && allowed(row, "archive", details),
      reason: reasonFor(row, "archive", details),
    },
    end,
    stop: !!row.turn_started && allowed(row, "interrupt", details) && available,
    markRead,
  };
}

export function terminateCopy(entry: RowOut): { title: string; text: string; detail: string; confirm: string } {
  const row = entry.row;
  const archives = !!row.tracked && ["codex", "claude", "kimi"].includes(row.agent ?? "");
  const name = row.name;
  return {
    title: "Terminate session?",
    text: archives
      ? `Stop ${name} and move its recorded conversation to Archive? Running processes will stop; the conversation history is kept.`
      : `Terminate ${name}? Its running processes will stop. No HGS archive is available for this session; any native conversation history is kept.`,
    detail: `Computer: ${row.machine || entry.computer || "this computer"}`,
    confirm: archives ? "Stop and archive" : "Terminate session",
  };
}

/** The rename rule of Task 1: a project and a session part, no whitespace or "=", no leading "#". */
export function renameError(name: string): string | undefined {
  const slash = name.indexOf("/");
  const ok = slash > 0 && slash < name.length - 1 && !name.startsWith("#") && !name.includes("=") && !/\s/.test(name);
  return ok ? undefined : "Use a name without spaces or '=', with a project and a session part.";
}

/** The part of a name the rename field selects: after the first "/", else all of it. */
export function sessionPartRange(name: string): [number, number] {
  const slash = name.indexOf("/");
  return [slash < 0 ? 0 : slash + 1, name.length];
}

export type DialogKind = "rename" | "terminate";
export type DialogState = { kind: DialogKind; entry: RowOut } | undefined;
export type DialogEvent = { type: "open"; kind: DialogKind; entry: RowOut } | { type: "close" };

/** One dialog at a time: opening replaces whatever is open, closing clears it. */
export function dialogReducer(_state: DialogState, event: DialogEvent): DialogState {
  return event.type === "open" ? { kind: event.kind, entry: event.entry } : undefined;
}

/** A disabled menu item neither closes the menu nor runs its action. */
export const menuItemActivates = (item: { disabled?: boolean }) => !item.disabled;
