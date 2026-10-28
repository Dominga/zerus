// View models the Tauri backend will publish to the webview. Field names
// follow `hgs ls --json` rows where one exists; derived presentation fields
// (status, depth) are computed in Rust before they reach the webview.

export type AgentKind = "claude" | "codex" | "kimi" | "deepseek" | "opencode";
export type Platform = "linux" | "macos" | "windows";
export type Connection = "online" | "offline" | "reconnecting";

/** Presentation status, in the existing precedence order. */
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

export interface Machine {
  id: string;
  alias: string;
  label: string;
  platform: Platform;
  connection: Connection;
  thisComputer: boolean;
  colorIndex: number;
  cpu: number;
  cores: number;
  load: number;
  memUsedGiB: number;
  memTotalGiB: number;
  hgsVersion: string;
  lastSeen: string;
  latencyMs: number;
}

export interface ProjectFolder {
  name: string;
  machine: string;
  path: string;
}

export interface Project {
  id: string;
  name: string;
  /** A chosen colour (Edit color…); otherwise the palette slot. */
  color?: string;
  colorIndex: number;
  vivid: boolean;
  isDefault: boolean;
  folders: ProjectFolder[];
}

export interface Subagent {
  id: string;
  name: string;
  state: "working" | "ready" | "attention";
  summary: string;
}

export interface Session {
  key: string; // machine + run_id; stable across renames
  name: string;
  machine: string;
  project: string; // project id, "inbox" when unassigned
  agent: AgentKind;
  model: string;
  effort?: string;
  cwd: string;
  branch?: string;
  worktree?: string;
  git?: { ahead: number; behind: number; changed: number; conflicts?: number; state: "synced" | "diff" | "push" | "pull" | "unknown" };
  status: Status;
  phase?: "tool" | "approval" | "thinking" | "compacting";
  summary: string;
  tool?: string;
  turnStartedMin?: number; // minutes since turn start (Working clock)
  lastEvent: string;
  unread: boolean;
  draft: boolean;
  muted?: boolean;
  attention?: "question" | "approval" | "reply";
  scheduleNext?: string;
  scheduleCount?: number;
  backgroundJobs?: number;
  backgroundCommand?: string;
  subagents?: Subagent[];
  parent?: string; // key of the session this one was forked from
  tag?: string;
  goal?: string;
  account: string;
  pinned?: boolean;
  errorText?: string;
  archivedAt?: string;
  contextPct?: number;
  reviewLater?: boolean;
  optionalQuestions?: number;
  waitingFor?: string;
}

export interface Account {
  id: string;
  label: string;
  agent: AgentKind;
  machine: string;
  plan: string;
  fiveHour: number;
  week: number;
  resetIn: string;
  /** Minutes until the 5 h and 7 d windows reset (pace and countdown). */
  fiveHourResetMin?: number;
  /** The provider reports only the weekly window. */
  weekOnly?: boolean;
  weekResetMin?: number;
  sessions: number;
  signedIn: boolean;
}

export type TaskStage = "review" | "active" | "todo" | "backlog" | "done";

export interface TaskEvent {
  at: string;
  who: string;
  kind: "created" | "edited" | "sent" | "started" | "reported" | "comment" | "moved" | "target" | "reopened";
  text: string;
}

export interface Task {
  ref: string;
  title: string;
  description: string;
  stage: TaskStage;
  project: string;
  owner?: string; // session name
  target?: string; // planned executor
  targetKind?: "session" | "new-session" | "subagent" | "any";
  dispatch?: "unsent" | "submitted" | "uncertain";
  attachments?: number;
  comments: number;
  updated: string;
  history: TaskEvent[];
}

export interface ActivityItem {
  id: string;
  kind: "user" | "agent" | "tool" | "tools" | "question" | "approval" | "system" | "receipt" | "thinking" | "report" | "voice" | "cleared";
  at: string;
  title?: string;
  body?: string;
  code?: string;
  lines?: string[];
  options?: { label: string; detail?: string; recommended?: boolean }[];
  attachments?: { name: string; size: string; kind: "image" | "file" }[];
  sender?: string;
}
