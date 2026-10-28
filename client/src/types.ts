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
}

export interface RowOut { computer: ComputerId; key: string; row: ViewRow }
export interface KeyOut { computer: ComputerId; key: string; reason?: string | null }
export interface ReplaceOut { computer: ComputerId; rows: RowOut[] }
export type ComputerStatus = "probing" | "unsupported" | "live" | "reconnecting" | "ended";
export interface ComputerOut { computer: ComputerId; status: ComputerStatus | string; attempt: number }
export type ActivityKind = "events" | "replace" | "event" | "state" | "ended";
export interface ActivityOut { computer: ComputerId; key: string; kind: ActivityKind | string; payload: unknown }

export interface Batch {
  upserts: RowOut[];
  removes: KeyOut[];
  replaces: ReplaceOut[];
  computers: ComputerOut[];
  activity: ActivityOut[];
}

export const emptyBatch = (): Batch => ({ upserts: [], removes: [], replaces: [], computers: [], activity: [] });

export const isEmptyBatch = (b: Batch) =>
  !b.upserts.length && !b.removes.length && !b.replaces.length && !b.computers.length && !b.activity.length;

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
