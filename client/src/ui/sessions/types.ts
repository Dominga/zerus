// View models the session list and Activity render. A `Session` is the
// catalog store's row: the Rust view row plus the presentation fields derived
// from it once per upsert (store/catalog.ts `toSession`).

export type AgentKind = "claude" | "codex" | "kimi" | "deepseek" | "opencode";

/** Presentation status, in the existing precedence order (design/webui). */
export type Status =
  | "attention" // Needs attention: question, approval or explicit attention
  | "working" // native work in progress
  | "background" // idle with confirmed owned live shell/native jobs
  | "scheduled" // live idle with pending self-scheduled jobs
  | "ready" // idle
  | "error" // provider error / interrupted / recovery needed
  | "offline" // machine unreachable: last known state
  | "paused" // saved conversation, not running
  | "waiting" // idle, waiting for another agent's result (hgs wait)
  | "starting" // pending launch row
  | "ended"; // conversation ended (archive candidate)

export interface Subagent {
  id: string;
  name: string;
  state: "working" | "ready" | "attention";
  summary: string;
}

export interface Session {
  /** The row id: computer + "\n" + catalog key. Stable across renames. */
  key: string;
  /** The catalog key (`run:<run_id>` or `name:<name>`). */
  catalogKey: string;
  /** "" is this computer, otherwise its `hgs @alias`. */
  computer: string;
  /** The full session name (`agent/project/tag`), used for actions and follow. */
  name: string;
  /** What the card shows as its title. */
  title: string;
  machine: string;
  project: string;
  agent: AgentKind;
  model: string;
  effort?: string;
  /** The checkout's folder as people know it (a name; a linked worktree names its main checkout). */
  cwd: string;
  branch?: string;
  worktree?: string;
  git?: { ahead: number; behind: number; changed: number; conflicts?: number; state: "synced" | "diff" | "push" | "pull" | "unknown" };
  status: Status;
  /** The Qt caption for the status (SessionPresentation). */
  caption: string;
  phase?: string;
  summary: string;
  unread: boolean;
  draft: boolean;
  muted?: boolean;
  attention?: "question" | "approval" | "reply";
  scheduleNext?: string;
  backgroundJobs?: number;
  subagents?: Subagent[];
  reviewLater?: boolean;
  optionalQuestions?: number;
  /** Seconds since the Unix epoch of the row's last change, when known. */
  updatedAt?: number;
  /** Seconds since the Unix epoch when the current turn started (the Working clock). */
  turnStarted?: number;
  runId?: string;
  conversationId?: string;
  /** The core's row as received, for the guarded actions (state, tracked, allowed_actions, ...). */
  view?: import("../../types").ViewRow;
}

export interface ActivityItem {
  id: string;
  kind: "user" | "agent" | "tool" | "tools" | "question" | "approval" | "system" | "receipt" | "thinking" | "report" | "voice" | "cleared" | "context";
  at: string;
  title?: string;
  body?: string;
  code?: string;
  lines?: string[];
  attachments?: { name: string; size: string; kind: "image" | "file" }[];
  sender?: string;
  /** Receipt line under a user message ("Sent", "Queued at this time"). */
  receipt?: string;
  /** Operational group that contains an attention event (approval, failure). */
  warn?: boolean;
  /** A header other than "You" or "Agent" (recorded context). */
  label?: string;
}

export type VoiceState =
  | "uploading" | "queued" | "transcribing" | "ready" | "waiting_agent"
  | "delivering" | "submitted" | "failed" | "cancelled" | "uncertain";

export interface MediaMessage {
  id: string;
  kind: "media";
  media: "voice" | "video";
  at: string;
  state: VoiceState;
  durationMs: number;
  /** Exactly 64 integer amplitudes 0-255; anything else draws a flat line. */
  waveform: number[];
  transcript?: string;
  error?: string;
}

export type TranscriptItem = ActivityItem | MediaMessage;
