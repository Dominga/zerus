// The batch the Rust side returns from `next_batch` (zerus-client-core
// `Batch`, snake_case JSON). Rows arrive as compact view rows projected in
// Rust; the webview never sees raw watch frames.

/** One computer's id: "" is this computer, otherwise its `hgs @alias`. */
export type ComputerId = string;

/**
 * A session row as the core projects it (client/core `view.rs`). `status` is a
 * presentation kind (never `offline`: the view applies that while a computer is
 * not live), `caption` the Qt status text without clock parts. Unknown values
 * are null; times are seconds since the Unix epoch.
 */
export interface ViewRow {
  name: string;
  title?: string;
  agent?: string;
  status?: string;
  caption?: string;
  phase?: string | null;
  /** `approval` or `question` while the session asks for one. */
  attention?: string | boolean | null;
  unread?: boolean;
  muted?: boolean;
  /** The row's notification preferences pass Qt's validation, and their revision. */
  shared_notifications?: boolean;
  notification_revision?: number | null;
  /** `created`, in whole seconds (it keys a conversation-less attention mark). */
  created?: number;
  /** What a local attention mark compares (Qt's `attentionFingerprint`). */
  attention_fingerprint?: string;
  /** The session needs the person's action (approval, input, error, recovery). */
  needs_action?: boolean;
  /** Qt's SessionInfo::needsAttention from the listing alone, before this device's marks (no children). */
  needs_attention?: boolean;
  /** A subagent asks while the session runs, before an acknowledgement (Qt's window adds it). */
  child_attention?: boolean;
  /** The host's preferences conflict with the confirmed ones (Qt's notificationPreferencesError). */
  notification_error?: string;
  /** `status` and `caption` with the attention acknowledged. */
  acknowledged_status?: string;
  acknowledged_caption?: string;
  /** A finished reply of a live conversation; the marks decide whether it shows unread. */
  unread_candidate?: boolean;
  machine?: string;
  project?: string;
  /** The folder people know the checkout by (a name, not a path). */
  folder?: string;
  branch?: string | null;
  model?: string | null;
  effort?: string | null;
  summary?: string;
  updated_at?: number | null;
  /** When an archived row was archived (epoch seconds). */
  archived_at?: number | null;
  /** When the current turn, compaction or background job started (the Working clock). */
  turn_started?: number | null;
  run_id?: string | null;
  conversation_id?: string | null;
  /** The raw `state` (`running`, `paused`, `stopped`, `archived`, ...). */
  state?: string;
  tracked?: boolean;
  resumable?: boolean;
  activity?: string;
  process_state?: string;
  archive_id?: string | null;
  /** The latest reply and the attention it raised, for marking them read. */
  reply_id?: string | null;
  attention_signature?: string | null;
  shared_read?: boolean;
  read_reply_id?: string | null;
  read_attention_signature?: string | null;
  /** The operations the computer reports as allowed; null when it reports none. */
  allowed_actions?: string[] | null;
  action_reasons?: Record<string, string>;
  /** The raw working directory (`folder` is the name people know the checkout by). */
  cwd?: string;
  terminal_supported?: boolean;
  terminal_reported?: boolean;
  terminal_attach_supported?: boolean;
  terminal_attach_reported?: boolean;
  bulk_lifecycle_supported?: boolean;
}

export interface RowOut { computer: ComputerId; key: string; row: ViewRow }
export interface KeyOut { computer: ComputerId; key: string; reason?: string | null }
export interface ReplaceOut { computer: ComputerId; rows: RowOut[] }
export type ComputerStatus = "probing" | "unsupported" | "live" | "polled" | "reconnecting" | "ended";
export interface ComputerOut { computer: ComputerId; status: ComputerStatus | string; attempt: number }
export type ActivityKind = "events" | "replace" | "event" | "state" | "ended";
export interface ActivityOut { computer: ComputerId; key: string; kind: ActivityKind | string; payload: unknown }

// The Sessions list's grouping by the shared project organization
// (client/core `swarm/grouping.rs`). The Rust side keeps the organization;
// the webview gets each row's project and rank, and the ordered headers.

/** One row's place: its project and its rank inside it. */
export interface GroupOut { computer: ComputerId; key: string; group: string; rank: number }
export interface HeaderOut {
  id: string;
  name: string;
  /** The project's colour; null uses the palette slot. */
  color: string | null;
  accessible: boolean;
  collapsed: boolean;
  is_default: boolean;
  palette: number;
}
/** The ordered headers and whether edits can run, sent when they change. */
export interface GroupingOut {
  /** A catalog is presented; before it, the list shows no headers. */
  loaded: boolean;
  default_project: string;
  headers: HeaderOut[];
  ready: boolean;
  /** Why edits cannot run, for the disabled entries. */
  reason: string | null;
  busy: boolean;
  status: string;
  status_error: boolean;
  conflicts: number;
  /** A one-time warning, such as saved project changes that could not be applied; absent when there is none. */
  notice?: string;
}
/** How a project edit ended (client/core `swarm::sync::EditOutcome`). */
export type EditOutcome =
  | { outcome: "done"; conflicts: number }
  | { outcome: "kept"; detail: string }
  | { outcome: "refused"; detail: string };

export interface Batch {
  upserts: RowOut[];
  removes: KeyOut[];
  replaces: ReplaceOut[];
  computers: ComputerOut[];
  machines?: MachineOut[];
  activity: ActivityOut[];
  /** Each row's project and rank, for the rows whose grouping changed. */
  groups?: GroupOut[];
  /** The headers and the edit state, when they changed. */
  grouping?: GroupingOut;
  /** The open composer's view, when it changed (src-tauri `next_batch` envelope). */
  composer?: ComposerView;
  /** The draft keys (`computer + "\n" + name`, plus `"\n" + archive id`) of the nonempty drafts, when they changed. */
  draft_keys?: string[];
}

export const emptyBatch = (): Batch => ({ upserts: [], removes: [], replaces: [], computers: [], machines: [], activity: [] });

/** A batch with nothing to apply. A composer view or a draft key set is content: Rust sends each once. */
export const isEmptyBatch = (b: Batch) =>
  !b.upserts.length && !b.removes.length && !b.replaces.length && !b.computers.length && !b.machines?.length && !b.activity.length
  && !b.groups?.length && !b.grouping && !b.composer && !b.draft_keys;

/** One Activity event as `inspect` returns it; unknown fields are kept. */
export interface ActivityEvent {
  seq?: number;
  type?: string;
  source?: string;
  detail?: string;
  at?: number;
  tool?: string;
  agent_id?: string;
  origin?: string;
  report?: string;
  sender?: string;
  activity_key?: string;
  message_id?: string;
  attachments?: EventAttachment[];
  voice?: Record<string, unknown>;
  [field: string]: unknown;
}

/** The session an action applies to (client/core `actions.rs`). */
export interface Target { computer: ComputerId; name: string; run_id: string; conversation_id: string }

export type Action =
  | { kind: "interrupt"; turn_started: number }
  | { kind: "mark_read"; reply_id: string | null; attention_signature: string | null }
  | { kind: "pause" }
  | { kind: "resume" }
  | { kind: "rename"; new_name: string; archive_id: string | null }
  | { kind: "archive" }
  | { kind: "terminate" }
  | { kind: "kill" }
  | { kind: "start_fresh" }
  | { kind: "forget" }
  | { kind: "forget_binding" }
  | { kind: "restore"; archive_id: string }
  | { kind: "forget_archive"; archive_id: string }
  /** Computer-wide: the target's session name is empty and ignored. */
  | { kind: "pause_all"; count: number }
  | { kind: "resume_all"; count: number };

export interface WorktreeCleanup { status: string; path: string; common_dir: string; reason?: string | null }

export type Outcome =
  | { outcome: "done"; receipt: unknown; cleanup: WorktreeCleanup | null }
  | { outcome: "failed"; detail: string };

/** The inspect fields that enable header actions (client/core `details.rs`). */
export interface Details {
  run_id: string;
  conversation_id: string;
  auth_required: boolean;
  expected_id: string;
  resume_message_can_send: boolean;
  allowed_actions: string[] | null;
  action_reasons: Record<string, string>;
  /** The host can interrupt this turn (Stop). */
  interrupt_supported: boolean;
  /** The turn an interrupt is pinned to. */
  turn_started: number;
  /** The inspected phase ("" when not reported). */
  phase: string;
  /** The session's working directory; null when the inspection does not report one (an empty string stays empty). */
  cwd?: string | null;
  /** Null when the inspection does not report it. */
  terminal_supported?: boolean | null;
  terminal_attach_supported?: boolean | null;
}

/** A read mark whose delivery is still pending (client/core `readmarks.rs`). */
export interface ReadMark {
  computer: ComputerId;
  name: string;
  run_id: string;
  conversation_id: string;
  reply_id: string | null;
  attention_signature: string | null;
}

/** A computer's machine card from its `catalog.meta` (client/core `machine.rs`); an older host leaves fields null. */
export interface MachineMetrics {
  cpu_percent: number | null;
  cpu_count: number | null;
  /** `available`, `warming` or `unavailable`. */
  cpu_state: string | null;
  cpu_interval_seconds: number | null;
  load_1: number | null;
  load_5: number | null;
  load_15: number | null;
  memory_used_bytes: number | null;
  memory_total_bytes: number | null;
  memory_available_bytes: number | null;
  uptime_seconds: number | null;
  /** Unix seconds. */
  sampled_at: number | null;
}
/** This Windows computer's session host. */
export interface LocalHost { state: string | null; pid: number | null; protocol: number | null; conpty: string | null; integrity: string | null; windows_session: number | null; error: string | null }
export interface MachineMeta {
  host: string | null;
  hgs_version: string | null;
  platform: string | null;
  ok: boolean | null;
  metrics: MachineMetrics;
  presence: { idle_seconds: number | null; locked: boolean | null; locked_seconds: number | null };
  /** False only on a control console. */
  local_sessions: boolean | null;
  local_host: LocalHost | null;
}
export interface MachineOut { computer: ComputerId; machine: MachineMeta }

// The screen views the Rust side projects (client/core `views`, client/src-tauri `screens.rs`).

export type Screen = "overview" | "projects" | "machines" | "accounts";
export interface ProjectedWindow { id: string; label: string; used: number; minutes: number | null; resets_at: number | null }
export interface Identity { email: string; name: string; plan: string; organization: string; account_id: string; auth_method: string }
export interface Usage {
  status: string;
  provider: string;
  label: string;
  auth_status: string;
  checked_at: number | null;
  /** The values come from the cached account status until inspect answers. */
  seeded: boolean;
  identity: Identity;
  windows: ProjectedWindow[];
  balances: { kind: string; balance: string; currency: string }[];
  credits: { balance: string; unlimited: boolean } | null;
  refreshing: boolean;
  refresh_error: boolean;
  offline: boolean;
}
export type MemberState = "ready" | "offline" | "not-installed" | "apikey" | "signed-out" | "keychain" | "desktop" | "unavailable" | "expired" | "checking" | "unknown";
export interface OpenCodeInfo { providers: { label: string; connection: string; models: string }[]; truncated: boolean; stale_catalog_at: number | null; stopped: boolean }
export interface Profile {
  key: string; host: ComputerId; machine: string; id: string; label: string; provider: string;
  native: boolean; installed: boolean; is_default: boolean; home: string; auth_revision: string; online: boolean;
  state: MemberState; detail: string; signed_in: boolean;
  permission: string; permission_label: string; permission_hint: string; model_supported: boolean; model_text: string;
  usage: Usage; opencode: OpenCodeInfo | null;
}
export interface Account {
  key: string; label: string; provider: string; installed: boolean; machines: string[]; members: Profile[];
  usage: Usage; permission: string; identity_lines: string[]; identity_note: string;
}
export interface AccountHost { computer: ComputerId; name: string; label: string; online: boolean }
export interface AccountsView {
  hosts: AccountHost[]; accounts: Account[]; overview: Account[]; snapshot: boolean; loading: boolean;
  pending: number; loaded_machines: number; busy: boolean; notices: string[]; no_hosts: string | null;
  /** Accounts left out by the per-view bound. */
  truncated: number;
}
export interface MachineRow { alias: string; enabled: boolean; source: string; inherited: boolean; hostname: string; user: string; port: number | null; identity_file: string; origin: string }
export interface Effective { alias: string; pending: boolean; error: boolean; text: string; hostname: string; user: string; port: string; key: string }
export interface CheckResult { alias: string; pending: boolean; error: boolean; text: string }
export interface MachinesView {
  loaded: boolean; refreshing: boolean; error: string | null; local: string; environment_override: boolean;
  machines: MachineRow[]; effective: Effective | null; check: CheckResult | null; busy: boolean;
}
export interface Folder { id: string; name: string; path: string; computer: string; machine: string }
export interface Project { id: string; name: string; color: string | null; accessible: boolean; is_default: boolean; palette: number; folders: Folder[] }
export interface ProjectsView {
  loaded: boolean; status: string; status_error: boolean; conflicts: number;
  unconnected: string[]; notice: string | null; projects: Project[];
}

export const emptyAccounts = (notices: string[] = []): AccountsView => ({
  hosts: [], accounts: [], overview: [], snapshot: notices.length > 0, loading: false, pending: 0, loaded_machines: 0, busy: false, notices, no_hosts: null, truncated: 0,
});
export const emptyMachines = (error: string | null = null): MachinesView => ({
  loaded: false, refreshing: false, error, local: "", environment_override: false, machines: [], effective: null, check: null, busy: false,
});
export const emptyProjects = (status = ""): ProjectsView => ({
  loaded: false, status, status_error: status !== "", conflicts: 0, unconnected: [], notice: null, projects: [],
});

// The Terminal tab (src-tauri/src/terminal.rs).
export interface TerminalTarget { computer: ComputerId; name: string; run_id: string; machine: string }
export interface TerminalConnected { generation: number; status: string; reused: boolean }
export interface TerminalStatus { supported: boolean; available: boolean; reason: string; idle: string; connecting: string; disconnected: string }
export interface TerminalFinished { kind: "finished"; code: number; status: string }
/** The core's `TerminalFacts`: a row's terminal fields. */
export interface TerminalFacts {
  cmd: string;
  terminal_supported: boolean;
  terminal_reported: boolean;
  terminal_attach_supported: boolean;
  terminal_attach_reported: boolean;
  terminal_reason: string;
  terminal_attach_reason: string;
}

/** One checked page of a computer's archive (`archive_page`). */
export interface ArchivePage {
  rows: ViewRow[];
  next_cursor: string;
  previous_cursor: string | null;
  total: number;
  position: number | null;
}

/** One read of an archived session's recorded Activity (`archived_activity`, `hgs inspect --archive`). */
export interface ArchivedActivity {
  /** The reply's events, unchanged. */
  events: ActivityEvent[];
  replace_events: boolean;
  conversation_id: string;
  run_id: string;
  cleared_conversations: string[];
  /** The journal cursor the next read asks `--after`. */
  cursor: number;
  /** The other top-level fields, what `buildTranscript` reads as the summary. */
  summary: Record<string, unknown>;
}

// The composer (src-tauri/src/composer): the mirrors of its serde shapes.

/** One attachment of the open draft; `path` is its `zerus-file` path. */
export interface AttachmentView { name: string; mime: string; reference: string; bytes: number; path: string; excerpt: string | null }
export type ChipTone = "quiet" | "danger" | "warning" | "success";
/** The cache chip (core `cache::Chip`); hidden when not `visible`. */
export interface Chip { visible: boolean; tone: ChipTone; label: string; short_label: string }
/** The Context chip (core `session_usage::ContextChip`); hidden when not `visible`. */
export interface ContextChip { visible: boolean; text: string; tooltip: string; warning: boolean; tone: ChipTone; percent: number | null; accessible_name: string }
/** Clear session and the cache popover's Clear context, with the dialog the confirmation shows. */
export interface ClearControl {
  enabled: boolean; dismiss: boolean; title: string; text: string; confirm: string; run: string; conversation: string;
  /** The cache popover's Clear context (Qt 3792). */
  cache_enabled: boolean;
  /** A context reset runs: "Clearing…". */
  running: boolean;
  /** The tooltips' reason sentences, empty for the usual tooltip. */
  reason: string; cache_reason: string;
}
/** One local message row (core `local_messages::LocalMessage`). */
export interface LocalMessage {
  id: string; text: string; status: "sending" | "sent" | "error"; attachments: Record<string, unknown>[]; run_id: string; conversation_id: string;
  submitted_at: number; error: string; uncertain: boolean; message_id: string | null; submitted_text: string | null; source: string | null; voice: Record<string, unknown> | null;
}
export interface ComposerView {
  handle: number; key: string; loaded: boolean; rev: number; text_epoch: number; text: string; position: number; anchor: number;
  attachments: AttachmentView[]; removed: string | null; attaching: boolean; sending: boolean; waiting: string | null; uncertain: boolean;
  status: string; status_error: boolean; can_send: boolean; send_label: string; available: boolean; unavailable_reason: string; block_reason: string | null;
  attachments_available: boolean; attachments_reason: string; session_status: string; session_status_error: boolean; session_blocks_input: boolean;
  /** The open session's last read failed (Qt's m_inspectError). */
  inspect_error: boolean;
  cache: Chip; cache_warning: string; cache_details: string; context: ContextChip; local: LocalMessage[];
  cancel_send: boolean; compacting: boolean; clear: ClearControl;
}
export type ColdAction = "send" | "compact" | "clear";
export interface ColdChoice { action: ColdAction; label: string; enabled: boolean; tooltip: string }
export interface ColdPrompt { title: string; text: string; informative: string; choices: ColdChoice[] }
export type StartChoice = "resume" | "resume_compact" | "fresh";
export interface StartOption { choice: StartChoice; label: string; enabled: boolean; tooltip: string }
export interface StartQuestion { title: string; text: string; informative: string; options: StartOption[] }
export type SendReply =
  | { kind: "sending" } | { kind: "busy" } | { kind: "blocked"; reason: string } | { kind: "notice"; text: string; error: boolean }
  | { kind: "cold"; token: number; prompt: ColdPrompt } | { kind: "clear_context"; token: number } | { kind: "start"; token: number; question: StartQuestion };
export type Answer = { kind: "cold"; action: ColdAction } | { kind: "start"; choice: StartChoice } | { kind: "cancel" };
/** Where Inspect on a failed local row leads. */
export type Inspect = { kind: "native_ui" } | { kind: "terminal" } | { kind: "refresh"; notice: string };
/** The session a composer belongs to (src-tauri `SessionRef`): `key` is the catalog key. */
export interface SessionRef { computer: ComputerId; key: string; name: string; archive_id: string | null }
/** The detail tab that shows, for the composer's inspect retry (Qt's poll interval). */
export type ComposerTab = "activity" | "terminal" | "native" | "processes";
/** A file of an Activity message, as the journal lists it. */
export interface EventAttachment {
  name?: string;
  mime?: string;
  bytes?: number;
  /** `[Image #n]` or `[File #n]` in the message text. */
  reference?: string;
  /** The message's request, which `hgs attachment` reads the file by; none when it saved no reference. */
  request_id?: string;
  /** The file's place in the message, -1 when it has none. */
  index?: number;
  path?: string;
  size?: number;
  kind?: string;
}

/** One attachment of an Activity message, as `hgs attachment` reads it (core `reads::AttachmentRef`). */
export interface AttachmentRef {
  /** The session's name. */
  name: string;
  request_id: string;
  index: number;
  conversation_id: string;
  archive_id: string | null;
  /** A subagent's ID for a subagent's message. */
  agent_id: string | null;
}

/** An attachment in the private cache: `path` is its `zerus-file` path (`cache/{digest}/{name}`). */
export interface Fetched {
  path: string; name: string; mime: string; bytes: number;
  /** An image whose header gives its size, at most Qt's 64M pixels: a card may show its thumbnail. */
  thumbnail: boolean;
}
