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
  machine?: string;
  project?: string;
  /** The folder people know the checkout by (a name, not a path). */
  folder?: string;
  branch?: string | null;
  model?: string | null;
  effort?: string | null;
  summary?: string;
  updated_at?: number | null;
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
}

export interface RowOut { computer: ComputerId; key: string; row: ViewRow }
export interface KeyOut { computer: ComputerId; key: string; reason?: string | null }
export interface ReplaceOut { computer: ComputerId; rows: RowOut[] }
export type ComputerStatus = "probing" | "unsupported" | "live" | "polled" | "reconnecting" | "ended";
export interface ComputerOut { computer: ComputerId; status: ComputerStatus | string; attempt: number }
export type ActivityKind = "events" | "replace" | "event" | "state" | "ended";
export interface ActivityOut { computer: ComputerId; key: string; kind: ActivityKind | string; payload: unknown }

export interface Batch {
  upserts: RowOut[];
  removes: KeyOut[];
  replaces: ReplaceOut[];
  computers: ComputerOut[];
  machines?: MachineOut[];
  activity: ActivityOut[];
}

export const emptyBatch = (): Batch => ({ upserts: [], removes: [], replaces: [], computers: [], machines: [], activity: [] });

export const isEmptyBatch = (b: Batch) =>
  !b.upserts.length && !b.removes.length && !b.replaces.length && !b.computers.length && !b.machines?.length && !b.activity.length;

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
  | { kind: "kill" };

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
  id: string; name: string; path: string; computer: string; machine: string;
  /** The computer's identity, "" for none (New session lists an identified folder's computer only when it is configured). */
  machine_id?: string;
}
export interface Project { id: string; name: string; color: string | null; accessible: boolean; is_default: boolean; palette: number; folders: Folder[] }
export interface ProjectsView {
  loaded: boolean; refreshing: boolean; status: string; status_error: boolean; conflicts: number;
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
  loaded: false, refreshing: false, status, status_error: status !== "", conflicts: 0, unconnected: [], notice: null, projects: [], swarm_id: "",
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
}
export interface RepositoryFields {
  url: string; parent: string; name: string;
  /** The Clone to field's text. */
  field: string;
  /** `PARENT/NAME`, as Start and the plan take it. */
  destination: string;
  suggested: string; can_submit: boolean;
}
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
