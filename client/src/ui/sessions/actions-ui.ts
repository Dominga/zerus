import type { Details } from "../../types";
import type { Session } from "./types";

// Which session actions are offered and with which words, after the Qt
// SessionsWindow: the header actions (3884-3912), the context menu
// (5265-5380), the composer's Stop (3992-3999), the rename dialog (5698-5757,
// SessionTag.h) and the Terminate confirmation (5758-5828). Pure: the
// components only render it.

/** The session fields the action rules read: a catalog row, or a snapshot of one. */
export type ActionRow = Pick<Session,
  | "key" | "computer" | "name" | "machine" | "cmd" | "state" | "tracked" | "resumable" | "activity" | "processState"
  | "archiveId" | "attention" | "unread" | "sharedRead" | "allowed" | "archiveReason" | "terminateReason" | "phase"
  | "runId" | "conversationId" | "replyId" | "attentionSignature">;

/** A plain copy of the action fields, as they were when a menu or dialog opened. */
export function snapshotOf(s: ActionRow): ActionRow {
  return {
    key: s.key, computer: s.computer, name: s.name, machine: s.machine, cmd: s.cmd, state: s.state, tracked: s.tracked,
    resumable: s.resumable, activity: s.activity, processState: s.processState, archiveId: s.archiveId, attention: s.attention,
    unread: s.unread, sharedRead: s.sharedRead, allowed: s.allowed, archiveReason: s.archiveReason, terminateReason: s.terminateReason,
    phase: s.phase, runId: s.runId, conversationId: s.conversationId, replyId: s.replyId, attentionSignature: s.attentionSignature,
  };
}

export interface MenuState {
  change: { label: "Pause session" | "Resume session" | "Restore session" | "Updating…"; enabled: boolean; reason?: string };
  rename: boolean;
  archive: { visible: boolean; enabled: boolean; reason?: string };
  end: { label: "Terminate session…" | "Forget saved session…" | "Forget archived session…"; enabled: boolean; reason?: string };
  /** Qt's composer Stop: shown while the turn works, enabled when the host can interrupt it. */
  stop: { visible: boolean; enabled: boolean; reason?: string };
  markRead: { label: "Mark as read" | "Mark needs attention"; enabled: boolean; reason?: string };
}

export interface MenuOptions {
  /** The row's context menu: Move to archive needs the machine online there (not in the header). */
  context?: boolean;
  /** A read mark of this session is being delivered. */
  reading?: boolean;
  /** The write in flight is this session's interrupt. */
  interrupting?: boolean;
}

export const FORGET_LATER = "Forgetting sessions arrives in a later version.";
export const RESTORE_LATER = "Restoring sessions arrives in a later version.";
export const MARK_NEEDS_LATER = "Arrives with attention marks.";
export const READ_STATE_HOST = "Update hgs on this session's host to share read state.";
const NO_REASON = "This native adapter does not support this action in the current session state.";
const WORKING = ["working", "tool", "compacting"];

/** NativeSessionCapabilities::action: details win, then reported actions, then the agent. */
export function allowedOp(s: Pick<ActionRow, "allowed" | "cmd">, op: string, details?: Details): boolean {
  if (details?.allowed_actions) return details.allowed_actions.includes(op);
  if (s.allowed !== undefined) return s.allowed.split(" ").includes(op);
  return s.cmd !== "opencode" || op === "rename" || op === "forget";
}

const reasonFor = (s: ActionRow, op: "archive" | "terminate", details?: Details) =>
  details?.action_reasons?.[op] || (op === "archive" ? s.archiveReason : s.terminateReason) || NO_REASON;

/** An OpenCode session that allows forget and not terminate: Forget removes only its Zerus binding. */
export const forgetsBindingOnly = (s: Pick<ActionRow, "allowed" | "cmd">, details?: Details) =>
  s.cmd === "opencode" && allowedOp(s, "forget", details) && !allowedOp(s, "terminate", details);

/** Whether the end entry terminates a live session (else it forgets one). */
export const terminates = (s: Pick<ActionRow, "state" | "allowed" | "cmd">, details?: Details) =>
  s.state === "running" && !forgetsBindingOnly(s, details);

export function menuState(s: ActionRow, online: boolean, updating: boolean, details?: Details, options: MenuOptions = {}): MenuState {
  const available = online && !updating;
  const archived = s.state === "archived";
  const running = s.state === "running";
  const idle = s.activity === "idle" || !!details?.auth_required;
  const canPause = running && s.resumable && idle && s.processState !== "exited"
    && (!details || details.expected_id === "" || details.resume_message_can_send);
  const change = !archived && available
    && ((canPause && allowedOp(s, "pause", details)) || (!running && s.resumable && allowedOp(s, "resume", details)));
  const [agent, project] = s.name.split("/", 2);

  const end: MenuState["end"] = terminates(s, details)
    ? { label: "Terminate session…", enabled: available && allowedOp(s, "terminate", details), reason: allowedOp(s, "terminate", details) ? undefined : reasonFor(s, "terminate", details) }
    : { label: archived ? "Forget archived session…" : "Forget saved session…", enabled: false, reason: FORGET_LATER };

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

  const needsAttention = s.attention === "approval" || s.attention === "question" || s.unread;
  const markRead: MenuState["markRead"] = !needsAttention
    ? { label: "Mark needs attention", enabled: false, reason: MARK_NEEDS_LATER }
    : !s.sharedRead
      ? { label: "Mark as read", enabled: false, reason: READ_STATE_HOST }
      : { label: "Mark as read", enabled: online && !options.reading };

  return {
    change: {
      label: updating ? "Updating…" : archived ? "Restore session" : running ? "Pause session" : "Resume session",
      enabled: change,
      reason: archived ? RESTORE_LATER : undefined,
    },
    rename: allowedOp(s, "rename", details) && available && !!agent && !!project && (!archived || !!s.archiveId),
    archive: {
      visible: s.cmd !== "deepseek" && (s.state === "paused" || s.state === "stopped"),
      enabled: (!options.context || online) && !updating && allowedOp(s, "archive", details),
      reason: reasonFor(s, "archive", details),
    },
    end,
    stop,
    markRead,
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
  };
}

export type DialogKind = "rename" | "terminate";
export type DialogState = { kind: DialogKind; row: ActionRow } | undefined;
export type DialogEvent = { type: "open"; kind: DialogKind; row: ActionRow } | { type: "close" };

/** One dialog at a time: opening replaces whatever is open, closing clears it. */
export function dialogReducer(_state: DialogState, event: DialogEvent): DialogState {
  return event.type === "open" ? { kind: event.kind, row: event.row } : undefined;
}

/** A disabled menu item neither closes the menu nor runs its action. */
export const menuItemActivates = (item: { disabled?: boolean }) => !item.disabled;
