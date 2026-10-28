import type { Details } from "../../types";
import type { BatchConfirmation } from "./selection";
import type { Session } from "./types";

// Which session actions are offered and with which words, after the Qt
// SessionsWindow: the header actions (3884-3912), the context menu
// (5265-5380), the composer's Stop (3992-3999), the rename dialog (5698-5757,
// SessionTag.h) and the Terminate confirmation (5758-5828). Pure: the
// components only render it.

/** The session fields the action rules read: a catalog row, or a snapshot of one. */
export type ActionRow = Pick<Session,
  | "key" | "computer" | "name" | "machine" | "cmd" | "state" | "tracked" | "resumable" | "activity" | "processState"
  | "archiveId" | "attention" | "unread" | "sharedRead" | "allowed" | "archiveReason" | "terminateReason" | "forgetReason" | "phase"
  | "runId" | "conversationId" | "replyId" | "attentionSignature" | "created" | "fingerprint"
  | "cwdPath" | "terminalSupported" | "terminalReported" | "terminalAttachSupported" | "terminalAttachReported"
  | "muted" | "sharedNotifications" | "notificationRevision" | "notificationConflict">;

/** A plain copy of the action fields, as they were when a menu or dialog opened. */
export function snapshotOf(s: ActionRow): ActionRow {
  return {
    key: s.key, computer: s.computer, name: s.name, machine: s.machine, cmd: s.cmd, state: s.state, tracked: s.tracked,
    resumable: s.resumable, activity: s.activity, processState: s.processState, archiveId: s.archiveId, attention: s.attention,
    unread: s.unread, sharedRead: s.sharedRead, allowed: s.allowed, archiveReason: s.archiveReason, terminateReason: s.terminateReason, forgetReason: s.forgetReason,
    phase: s.phase, runId: s.runId, conversationId: s.conversationId, replyId: s.replyId, attentionSignature: s.attentionSignature,
    created: s.created, fingerprint: s.fingerprint,
    cwdPath: s.cwdPath, terminalSupported: s.terminalSupported, terminalReported: s.terminalReported,
    terminalAttachSupported: s.terminalAttachSupported, terminalAttachReported: s.terminalAttachReported,
    muted: s.muted, sharedNotifications: s.sharedNotifications, notificationRevision: s.notificationRevision,
    notificationConflict: s.notificationConflict,
  };
}

// Mute and Unmute (SessionsWindow.cpp 5310-5323, 5401-5414; SessionNotifications.h).

/** Pending shared notification changes allowed in all. */
export const MAX_PENDING_NOTIFICATIONS = 16;
export const MUTE_OFFLINE = "Connect to this session's host to change shared notifications.";
export const MUTE_HOST = "Update hgs on this session's host to support shared notification preferences.";
export const MUTE_IDENTITY = "Refresh this session's identity before changing shared notifications.";
export const MUTE_CONFLICT = "Conflicting shared notification preferences. Refresh the session.";
export const MUTE_PENDING = "A shared notification change is pending. Wait for its result.";
export const MUTE_ALLOW = "Allow future notifications for this session on all devices";
export const MUTE_SILENCE = "Mute notifications for this session on all devices";

/** SessionNotifications::identity: non-empty, at most `maximum` UTF-8 bytes, no control character. */
export function identityOk(value: string | undefined, maximum: number): boolean {
  if (!value || new TextEncoder().encode(value).length > maximum) return false;
  for (const ch of value) { const c = ch.codePointAt(0)!; if (c < 32 || c === 127) return false; }
  return true;
}

export const notificationIdentityOk = (s: Pick<ActionRow, "runId" | "conversationId">) => identityOk(s.runId, 128) && identityOk(s.conversationId, 256);

/** What the pending changes block: one per conversation of a computer, sixteen in all. */
export interface PendingNotifications { conversation: boolean; total: number }

export interface NotificationsState { label: "Mute" | "Unmute"; enabled: boolean; hint: string }

/**
 * The Mute entry of a row's context menu: undefined for an archived row. The
 * hint is the reason it is off (Qt's order: offline, no shared notifications,
 * identity, conflict, pending) or what the entry does.
 */
export function notificationsState(s: ActionRow, online: boolean, pending: PendingNotifications): NotificationsState | undefined {
  if (s.state === "archived") return undefined;
  const full = pending.conversation || pending.total >= MAX_PENDING_NOTIFICATIONS;
  const enabled = online && !!s.sharedNotifications && !s.notificationConflict && notificationIdentityOk(s) && !full;
  const hint = !online ? MUTE_OFFLINE
    : !s.sharedNotifications ? MUTE_HOST
    : !notificationIdentityOk(s) ? MUTE_IDENTITY
    : s.notificationConflict ? MUTE_CONFLICT
    : full ? MUTE_PENDING
    : s.muted ? MUTE_ALLOW : MUTE_SILENCE;
  return { label: s.muted ? "Unmute" : "Mute", enabled, hint };
}

/**
 * The click-time check: the entry is still available on the live row, the run
 * is the one the menu showed, and the shared preferences have not changed since.
 */
export function notificationsStillAllowed(live: ActionRow, shown: ActionRow, online: boolean, pending: PendingNotifications): boolean {
  return !!notificationsState(live, online, pending)?.enabled && live.runId === shown.runId
    && live.notificationRevision === shown.notificationRevision && !!live.muted === !!shown.muted;
}

export interface MenuState {
  change: { label: "Pause session" | "Resume session" | "Restore session" | "Updating…"; enabled: boolean; reason?: string };
  rename: boolean;
  archive: { visible: boolean; enabled: boolean; reason?: string };
  end: { label: "Terminate session…" | "Forget saved session…" | "Forget archived session…"; enabled: boolean; reason?: string };
  /** Qt's composer Stop: shown while the turn works, enabled when the host can interrupt it. */
  stop: { visible: boolean; enabled: boolean; reason?: string };
}

export interface MenuOptions {
  /** The row's context menu: Move to archive needs the machine online there (not in the header). */
  context?: boolean;
  /** The write in flight is this session's interrupt. */
  interrupting?: boolean;
  /**
   * A selection batch runs (Qt's m_pending, store/batch.ts): every write entry
   * is off, and the header's change entry reads Updating….
   */
  batchRunning?: boolean;
  /** An archived row's name is taken by a live row of its computer (Qt's archiveNameOccupied): no Restore. */
  occupied?: boolean;
}

const NO_REASON = "This native adapter does not support this action in the current session state.";
const WORKING = ["working", "tool", "compacting"];

/** NativeSessionCapabilities::action: details win, then reported actions, then the agent. */
export function allowedOp(s: Pick<ActionRow, "allowed" | "cmd">, op: string, details?: Details): boolean {
  if (details?.allowed_actions) return details.allowed_actions.includes(op);
  if (s.allowed !== undefined) return s.allowed.split(" ").includes(op);
  return s.cmd !== "opencode" || op === "rename" || op === "forget";
}

const reasonFor = (s: ActionRow, op: "archive" | "terminate" | "forget", details?: Details) =>
  details?.action_reasons?.[op] || (op === "archive" ? s.archiveReason : op === "terminate" ? s.terminateReason : s.forgetReason) || NO_REASON;

/** An OpenCode session that allows forget and not terminate: Forget removes only its Zerus binding. */
export const forgetsBindingOnly = (s: Pick<ActionRow, "allowed" | "cmd">, details?: Details) =>
  s.cmd === "opencode" && allowedOp(s, "forget", details) && !allowedOp(s, "terminate", details);

/** Whether the end entry terminates a live session (else it forgets one). */
export const terminates = (s: Pick<ActionRow, "state" | "allowed" | "cmd">, details?: Details) =>
  s.state === "running" && !forgetsBindingOnly(s, details);

export function menuState(s: ActionRow, online: boolean, updating: boolean, details?: Details, options: MenuOptions = {}): MenuState {
  const available = online && !updating && !options.batchRunning;
  const archived = s.state === "archived";
  const running = s.state === "running";
  const idle = s.activity === "idle" || !!details?.auth_required;
  const canPause = running && s.resumable && idle && s.processState !== "exited"
    && (!details || details.expected_id === "" || details.resume_message_can_send);
  // Restore (Qt canChange 5302-5304). The client's right-click selects the row
  // first, so its context menu is Qt's selected-row case, which reuses the
  // header's m_changeAction (5360): neither reads allowed(restore) (3897-3898).
  const change = archived
    ? available && s.resumable && !!s.archiveId && !options.occupied
    : available && ((canPause && allowedOp(s, "pause", details)) || (!running && s.resumable && allowedOp(s, "resume", details)));
  const [agent, project] = s.name.split("/", 2);

  const end: MenuState["end"] = terminates(s, details)
    ? { label: "Terminate session…", enabled: available && allowedOp(s, "terminate", details), reason: allowedOp(s, "terminate", details) ? undefined : reasonFor(s, "terminate", details) }
    : archived
      ? { label: "Forget archived session…", enabled: available && allowedOp(s, "forget", details) && !!s.archiveId, reason: allowedOp(s, "forget", details) ? undefined : reasonFor(s, "forget", details) }
      : { label: "Forget saved session…", enabled: available && allowedOp(s, "forget", details), reason: allowedOp(s, "forget", details) ? undefined : reasonFor(s, "forget", details) };

  // Stop follows the inspected turn (Qt's m_details); without details it stays off.
  const phase = details?.phase || s.phase || "";
  const working = WORKING.includes(phase) && running;
  const interruptible = !!details?.interrupt_supported;
  const stop: MenuState["stop"] = {
    visible: working,
    enabled: working && online && interruptible && !options.interrupting,
    reason: options.interrupting ? "Interrupt is being sent…" : !online ? "Machine is offline"
      : !interruptible ? "Interrupt is unavailable. Open Terminal to stop this turn." : undefined,
  };

  return {
    change: {
      label: updating || (options.batchRunning && !options.context) ? "Updating…" : archived ? "Restore session" : running ? "Pause session" : "Resume session",
      enabled: change,
      reason: undefined,
    },
    rename: allowedOp(s, "rename", details) && available && !!agent && !!project && (!archived || !!s.archiveId),
    archive: {
      visible: s.cmd !== "deepseek" && (s.state === "paused" || s.state === "stopped"),
      enabled: (!options.context || online) && !updating && !options.batchRunning && allowedOp(s, "archive", details),
      reason: reasonFor(s, "archive", details),
    },
    end,
    stop,
  };
}

/**
 * The Terminate confirmation and the call behind it: a tracked Codex, Claude
 * or Kimi session stops into Archive (`terminate --expected-run-id`), any
 * other live session is killed (`kill`).
 */
export function terminateCopy(s: Pick<ActionRow, "tracked" | "cmd" | "name" | "machine">): { title: string; text: string; detail: string; confirm: string; action: "terminate" | "kill" } {
  const archives = s.tracked && ["codex", "claude", "kimi"].includes(s.cmd);
  return {
    title: "Terminate session?",
    text: archives
      ? `Stop ${s.name} and move its recorded conversation to Archive? Running processes will stop; the conversation history is kept.`
      : `Terminate ${s.name}? Its running processes will stop. No HGS archive is available for this session; any native conversation history is kept.`,
    detail: `Computer: ${s.machine}`,
    confirm: archives ? "Stop and archive" : "Terminate session",
    action: archives ? "terminate" : "kill",
  };
}

/**
 * The Forget saved session confirmation (terminateSession, 5767-5780): a
 * plain saved session, or an OpenCode binding that only Zerus forgets.
 */
export function forgetCopy(s: Pick<ActionRow, "name" | "machine">, binding: boolean): { title: string; text: string; detail: string; confirm: string } {
  return {
    title: "Forget saved session?",
    text: binding
      ? `Forget ${s.name} in Zerus? The conversation stays in OpenCode; only Zerus forgets it.`
      : `Remove ${s.name} from HGS? The agent’s native conversation history is kept.`,
    detail: `Computer: ${s.machine}`,
    confirm: "Forget saved session",
  };
}

/** The Forget archived session confirmation (terminateSession, 5771-5774 and 5788-5789): one archived instance only. */
export function forgetArchivedCopy(s: Pick<ActionRow, "name" | "machine">): { title: string; text: string; detail: string; confirm: string } {
  return {
    title: "Forget archived session?",
    text: `Remove this archived instance of ${s.name} from HGS? Other sessions with this name and the agent\u2019s native conversation history are kept.`,
    detail: `Computer: ${s.machine}`,
    confirm: "Forget archived session",
  };
}

/** The fields a name check reads from a row. */
type Named = Pick<Session, "computer" | "name">;
type Listed = Named & Pick<Session, "state">;

/** A session's computer and name, the key Qt's restores and archiveNameOccupied compare. */
export const nameKey = (s: Named) => `${s.computer}\n${s.name}`;

/**
 * Qt's archiveNameOccupied (SessionsWindow.cpp 3148-3154): a live (not
 * archived) row of the same computer has the archived row's name. One scan of
 * `rows`; a check of many rows builds `liveNameIndex` once instead.
 */
export function archiveNameOccupied(s: Named, rows: Readonly<Record<string, Listed | undefined>>): boolean {
  for (const id in rows) {
    const r = rows[id];
    if (r && r.state !== "archived" && r.computer === s.computer && r.name === s.name) return true;
  }
  return false;
}

/** The `nameKey` of every live row, for archiveNameOccupied over many rows: `index.has(nameKey(s))`. */
export function liveNameIndex(rows: Readonly<Record<string, Listed | undefined>>): Set<string> {
  const index = new Set<string>();
  for (const id in rows) {
    const r = rows[id];
    if (r && r.state !== "archived") index.add(nameKey(r));
  }
  return index;
}

/** The fixed part of a name (agent and project, with the slash after them) and the tag the rename dialog edits. */
export function renameParts(name: string): { prefix: string; tag: string } {
  const parts = name.split("/");
  return { prefix: `${parts.slice(0, 2).join("/")}/`, tag: parts.slice(2).join("/") };
}

/** SessionTag::problem: why a tag is not accepted, undefined when it is. */
export function tagProblem(tag: string): string | undefined {
  if (!tag) return "Enter a session name.";
  if (tag !== tag.trim()) return "Remove spaces at the beginning or end.";
  if (/[/:.\p{Cc}]/u.test(tag)) return "Names cannot contain /, : or . or control characters.";
  return undefined;
}

/** The rename dialog's check: the tag rule, then a live name already used on that computer. */
export function renameProblem(tag: string, s: Pick<ActionRow, "key" | "computer" | "name" | "machine" | "state">,
  rows: Iterable<Pick<Session, "key" | "computer" | "name" | "state">>): string | undefined {
  const invalid = tagProblem(tag);
  if (invalid) return invalid;
  if (s.state === "archived") return undefined;
  const name = renameParts(s.name).prefix + tag;
  for (const other of rows) {
    if (other.computer === s.computer && other.key !== s.key && other.state !== "archived" && other.name === name) {
      return `This name is already in use on ${s.machine}.`;
    }
  }
  return undefined;
}

const text = (v: unknown) => (typeof v === "string" ? v : "");

/** The inspection fields the action rules read, from an inspect reply or an Activity `state` value. */
export function detailsFromInspection(value: unknown): Details | undefined {
  if (!value || typeof value !== "object" || Array.isArray(value)) return undefined;
  const v = value as Record<string, unknown>;
  const reasons: Record<string, string> = {};
  if (v.action_reasons && typeof v.action_reasons === "object") {
    for (const [op, reason] of Object.entries(v.action_reasons as Record<string, unknown>)) if (typeof reason === "string") reasons[op] = reason;
  }
  return {
    run_id: text(v.run_id),
    conversation_id: text(v.conversation_id),
    auth_required: v.auth_required === true,
    expected_id: text(v.expected_id),
    resume_message_can_send: v.resume_message_can_send === true,
    allowed_actions: Array.isArray(v.allowed_actions) ? v.allowed_actions.filter((a): a is string => typeof a === "string") : null,
    action_reasons: reasons,
    interrupt_supported: v.interrupt_supported === true,
    turn_started: typeof v.turn_started === "number" ? v.turn_started : 0,
    phase: text(v.phase),
    cwd: typeof v.cwd === "string" ? v.cwd : null,
    terminal_supported: typeof v.terminal_supported === "boolean" ? v.terminal_supported : null,
    terminal_attach_supported: typeof v.terminal_attach_supported === "boolean" ? v.terminal_attach_supported : null,
  };
}

/** NativeSessionCapabilities::terminal: whether the session has a terminal to attach to. */
export function terminal(s: Pick<ActionRow, "cmd" | "terminalSupported" | "terminalReported" | "terminalAttachSupported" | "terminalAttachReported">, details?: Details): boolean {
  if (s.cmd === "dsh" || s.cmd === "deepseek") return false;
  if (s.cmd === "opencode") {
    if (typeof details?.terminal_attach_supported === "boolean") return details.terminal_attach_supported;
    if (s.terminalAttachReported) return s.terminalAttachSupported;
  }
  if (typeof details?.terminal_supported === "boolean") return details.terminal_supported;
  if (s.cmd === "opencode" && !s.terminalReported) return false;
  return s.terminalSupported;
}

export interface CopyState {
  command: { label: "Copy connect command" | "Copy restore command"; enabled: boolean };
  reference: boolean;
  /** The folder to copy: the selected session's details first, else the row's (Qt's selectedDirectory). */
  folder: { enabled: boolean; path: string };
}

/** The copy entries (SessionsWindow.cpp 5306-5307, 5374-5376): Copy session name is always enabled. */
export function copyState(s: ActionRow, online: boolean, details?: Details): CopyState {
  const archived = s.state === "archived";
  const path = details?.cwd ?? s.cwdPath;
  return {
    command: { label: archived ? "Copy restore command" : "Copy connect command", enabled: (archived || terminal(s)) && (!archived || !!s.archiveId) },
    reference: online && !!s.runId,
    folder: { enabled: path !== "", path },
  };
}

// Start fresh (SessionsWindow.cpp 4672-4704, 5333-5340).

export const FRESH_HINT = "Start with a new conversation and the same folder, account and model";
export const FRESH_TIP = "Start this stopped session with a new conversation and the same folder, account and model.";
/** Qt's refusal when running-ness changed since the menu opened (shared with the store). */
export const STATE_CHANGED = "The session state changed. Open its actions again.";
export const FRESH_CHANGED = "The session changed. Open its actions again.";

/** The header's Start fresh or Clear session slot (Qt 3808-3809): only in the main context, never with a subagent open. */
export function headerClearSlot(mainContext: boolean, state: ActionRow["state"]): "fresh" | "clear" | undefined {
  if (!mainContext || state === "archived") return undefined;
  return state === "paused" || state === "stopped" ? "fresh" : "clear";
}

/**
 * Qt's canStartStopped: online, paused or stopped, tracked, a Codex, Claude or
 * Kimi session, no write pending for it and no selection batch running
 * (`batchRunning`, Qt's m_pending). Qt's other blockers (terminating,
 * compacting, composer sending, rename in flight) have no client state yet.
 */
export function canStartStopped(s: Pick<ActionRow, "state" | "tracked" | "cmd">, online: boolean, updating: boolean, batchRunning = false): boolean {
  return online && (s.state === "paused" || s.state === "stopped") && s.tracked && ["codex", "claude", "kimi"].includes(s.cmd)
    && !updating && !batchRunning;
}

/** The Start fresh entry of a paused or stopped row: undefined for any other row. */
export function freshEntry(s: ActionRow, online: boolean, updating: boolean, batchRunning = false): { enabled: boolean; hint: string } | undefined {
  if (s.state !== "paused" && s.state !== "stopped") return undefined;
  return { enabled: canStartStopped(s, online, updating, batchRunning), hint: FRESH_HINT };
}

export function freshCopy(s: Pick<ActionRow, "name" | "machine">): { title: string; text: string; confirm: string } {
  return {
    title: "Start fresh?",
    text: `Start ${s.name} on ${s.machine} with a new conversation?\n\nThe agent starts in the same folder with the same account and model, without the current conversation context. Earlier messages stay readable in Activity and in the agent\u2019s native history, but Resume will no longer continue them.`,
    confirm: "Start fresh",
  };
}

/** The confirmation check: the live row is the one the dialog named and still qualifies. */
export function freshStillValid(live: ActionRow, shown: ActionRow, online: boolean, updating: boolean, batchRunning = false): boolean {
  return live.key === shown.key && live.runId === shown.runId && live.conversationId === shown.conversationId
    && canStartStopped(live, online, updating, batchRunning);
}

/**
 * Qt's click-time check for Forget: running-ness must be as when the menu
 * opened ("changed"), else the dialog names the live row, with the run and
 * conversation in view (the selected row's details win).
 */
export function forgetOpening(menuRow: ActionRow, live: ActionRow, details?: Details): "changed" | { row: ActionRow; bound: ForgetScope } {
  if ((live.state === "running") !== (menuRow.state === "running")) return "changed";
  const row = snapshotOf(live);
  const binding = row.state !== "archived" && forgetsBindingOnly(row, details);
  return { row, bound: { binding, run: details ? details.run_id : row.runId ?? "", conversation: details ? details.conversation_id : row.conversationId ?? "" } };
}

export type DialogKind = "rename" | "terminate" | "fresh" | "forget" | "batch";
/** What a forget dialog named: whether it forgets an OpenCode binding, and the run and conversation then in view. */
export interface ForgetScope { binding: boolean; run: string; conversation: string }
export type DialogOpen =
  | { kind: "rename" | "terminate" | "fresh"; row: ActionRow }
  | { kind: "forget"; row: ActionRow; bound: ForgetScope }
  /** A selection's Forget or Terminate: `answer` gets true for the confirm button, false otherwise. */
  | { kind: "batch"; copy: BatchConfirmation; answer: (confirmed: boolean) => void };
export type DialogState = DialogOpen | undefined;
export type DialogEvent = ({ type: "open" } & DialogOpen) | { type: "close" };

/** One dialog at a time: opening replaces whatever is open, closing clears it. */
export function dialogReducer(_state: DialogState, event: DialogEvent): DialogState {
  if (event.type !== "open") return undefined;
  if (event.kind === "batch") return { kind: "batch", copy: event.copy, answer: event.answer };
  return event.kind === "forget" ? { kind: "forget", row: event.row, bound: event.bound } : { kind: event.kind, row: event.row };
}

/** A disabled menu item neither closes the menu nor runs its action. */
export const menuItemActivates = (item: { disabled?: boolean }) => !item.disabled;
