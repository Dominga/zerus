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
  /** The canonical working directory, "" when the row has none (New session's prefill). */
  cwd_canonical?: string;
  /** The checkout's root, "" outside Git (the worktree picker's session counts). */
  git_root?: string;
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
}

export const emptyBatch = (): Batch => ({ upserts: [], removes: [], replaces: [], computers: [], machines: [], activity: [] });

export const isEmptyBatch = (b: Batch) =>
  !b.upserts.length && !b.removes.length && !b.replaces.length && !b.computers.length && !b.machines?.length && !b.activity.length
  && !b.groups?.length && !b.grouping;

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
  attachments?: { name?: string; path?: string; size?: number; kind?: string; mime?: string }[];
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
  /** The canonical working directory; null when it is not a string (the row's then applies). */
  cwd_canonical?: string | null;
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
export interface Folder {
  /** The catalog's id, "" when it has none. */
  id: string;
  /** New session's identity of the folder (`FolderOut.key`). */
  key: string;
  name: string; path: string; computer: string; machine: string;
  /** The computer's identity, "" for none (New session lists an identified folder's computer only when it is configured). */
  machine_id?: string;
}
export interface Project { id: string; name: string; color: string | null; accessible: boolean; is_default: boolean; palette: number; folders: Folder[] }
export interface ProjectsView {
  loaded: boolean; status: string; status_error: boolean; conflicts: number;
  unconnected: string[]; notice: string | null; projects: Project[];
  /** The catalog's swarm, "" before one arrives (New session's project inference). */
  swarm_id: string;
}

export const emptyAccounts = (notices: string[] = []): AccountsView => ({
  hosts: [], accounts: [], overview: [], snapshot: notices.length > 0, loading: false, pending: 0, loaded_machines: 0, busy: false, notices, no_hosts: null, truncated: 0,
});
export const emptyMachines = (error: string | null = null): MachinesView => ({
  loaded: false, refreshing: false, error, local: "", environment_override: false, machines: [], effective: null, check: null, busy: false,
});
export const emptyProjects = (status = ""): ProjectsView => ({
  loaded: false, status, status_error: status !== "", conflicts: 0, unconnected: [], notice: null, projects: [], swarm_id: "",
});

// New session (client/src-tauri `newsession.rs` over client/core `launch.rs`,
// `repository.rs` and `worktrees.rs`; the accounts from `screens.rs`).

/** A computer's folder form. */
export type PathStyle = "posix" | "windows";
/** What Start is doing. */
export type LaunchStage = "idle" | "checking_folder" | "creating_worktree";
/** A repository clone's state, as the form shows it (core `launch::Repository`). */
export interface LaunchRepository {
  busy: boolean; cloning: boolean; can_submit: boolean; created: boolean; uncertain: boolean; fields_locked: boolean;
  new_project: boolean; new_project_name: string; destination: string;
}
/** The form's state (core `launch::Form`); the three derived fields may be left out. */
export interface LaunchForm {
  no_machine: boolean; can_enable_local_sessions: boolean; online: boolean; agent: string; name: string;
  account_count: number; account_selected: boolean; account_loading: boolean; account_preset: boolean;
  path: string; listed_folder: boolean; folder_online: boolean; prefill_pending: boolean;
  /** Qt's `m_catalogRequest`. */
  catalog_pending: boolean;
  catalog_worktrees: boolean; auto_preferred: boolean;
  automatic_available?: boolean; creation_failed: boolean; project_worktree?: boolean;
  implicit_inbox: boolean; branch: string; automatic_path?: string; style: PathStyle; stage: LaunchStage;
  repository: LaunchRepository | null; clone_launch_pending: boolean;
}
/** What the form shows (core `launch::Shown`). */
export interface LaunchShown {
  start_enabled: boolean; start_text: string; preview: string; outside_project: boolean; name_problem: string | null;
  notice: string | null; automatic: boolean; auto_enabled: boolean; none_enabled: boolean;
  worktree_target: string; worktree_tooltip: string; choose_existing_enabled: boolean; create_manually_enabled: boolean;
  worktree_options_enabled: boolean; fields_enabled: boolean; browse_enabled: boolean; cancel_enabled: boolean;
  open_terminal_enabled: boolean; open_terminal_tooltip: string; clone_launch_due: boolean;
}
/** A saved project with its folders on each computer ("" is this computer). */
export interface LaunchProject { id: string; name?: string; folders: { machine: string; path: string }[] }
export interface NewSessionFormInput {
  form: LaunchForm;
  /** The folder's catalog (`m_worktreeCatalog`). */
  catalog: unknown;
  /** The verified catalog of this computer and folder, or null. */
  verified: unknown;
  project: LaunchProject | null;
  /** This computer "" or the alias, as the project's folders name it. */
  host: string;
  /** This computer's machine card: the core derives `can_enable_local_sessions` from it. */
  local?: MachineMeta | null;
}
export interface NewSessionFormView { shown: LaunchShown; automatic_available: boolean; automatic_path: string; project_worktree: boolean }
/** What Start does (core `launch::Start`). */
export type LaunchStart = "nothing" | "background" | "clone" | { check_folder: { with_worktree: boolean } };
export interface LaunchRepositoryChoice {
  url: string; parent: string; name: string; destination: string; created_path: string; new_project: boolean; project_name: string;
}
/** The form's choices for background creation (core `launch::Choices`). */
export interface LaunchChoices {
  host: string; machine: string; agent: string; account: string; name: string; project: string; project_name: string;
  implicit_inbox: boolean; swarm: string; path: string; auto_preferred: boolean; automatic_available: boolean;
  branch: string; catalog: unknown; automatic_path: string; listed_folder: boolean; project_worktree: boolean;
  model: string; effort: string; open_terminal: boolean; repository: LaunchRepositoryChoice | null; predecessor: string;
}
/** Background creation's plan (core `launch::Plan`, Qt's `creationRequested` object). */
export interface LaunchPlan {
  host: string; machine: string; cmd: string; account: string; tag: string; group: string; group_name: string;
  implicit_inbox: boolean; swarm: string; target: string; source: string; auto: boolean; branch: string; catalog: unknown;
  add_folder: boolean; model: string; effort: string; terminal: boolean; repository: boolean;
  destination?: string; url?: string; parent?: string; repository_name?: string; new_project?: boolean;
  project_name?: string; clone_done?: boolean; acquisition_confirmed?: boolean; predecessor?: string;
}
/** What a new dialog starts with. `remembered` stays empty until the settings store arrives. */
export interface NewSessionContext {
  branch: string;
  own_windows_session: number | null;
  /** The platform this client was built for: this computer's folder style before it reports a platform. */
  build_style: PathStyle;
  remembered: { agent: string; accounts: Record<string, Record<string, string>>; open_terminal: boolean };
  /** The catalog's swarm, "" before one arrives: a folder's project inference counts only from it. */
  swarm: string;
}
/**
 * A project folder as the organization holds it (client/core `swarm::Folder`, Qt's
 * SessionOrganization::Folder): `project_folders` answers at most 200 of them.
 */
export interface FolderOut {
  /** The catalog's id, "" when it has none; never made up. */
  id: string;
  /** The folder's identity here: `id`, else `machine` + "\n" + `path`. Never sent to hgs as a folder id. */
  key: string;
  machine: string;
  path: string;
  /** "" when the catalog names none. */
  name: string;
  /** The computer's identity, "" for none. */
  machine_id: string;
  machine_name: string;
}
/** What Create manually… created (`newsession.rs` `CreatedWorktree`). */
export interface CreatedWorktree {
  path: string;
  /** The catalog the dialog opened with, listing the new checkout as verified (`launch::created_catalog`). */
  catalog: Record<string, unknown>;
}
export interface RepositoryFields {
  url: string; parent: string; name: string;
  /** The Clone to field's text. */
  field: string;
  /** `PARENT/NAME`, as Start and the plan take it. */
  destination: string;
  suggested: string; can_submit: boolean;
}
/**
 * `creation_submit`'s answer (Task 5): the admitted creation's id, or the refusal's text. A rejected call is
 * neither: its outcome is unknown, so the dialog stays submitted and nothing sends it again.
 */
export type CreationAdmission = { admitted: string } | { refused: string };
/** New session's account rows of the dialog's computer (`screens.rs`). */
export interface LaunchAccounts {
  /** A catalog reply (good or failed) was filed since the dialog chose this computer. */
  loaded: boolean;
  /** "Could not load accounts: %1" for the last reply's failure; the rows stay the last good catalog's since then. */
  error: string | null;
  /** The catalog's raw rows, at most 256, for the OpenCode fields. */
  profiles: Record<string, unknown>[];
  groups: Account[];
}

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
