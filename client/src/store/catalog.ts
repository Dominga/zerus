import { batch } from "solid-js";
import { createStore, produce, reconcile } from "solid-js/store";
import type { Batch, ComputerId, RowOut, ViewRow } from "../types";
import type { AgentKind, Session, Status } from "../ui/sessions/types";

// The catalog: every live session row of every computer, keyed by
// computer + "\n" + catalog key. A batch is one reactive transaction. An
// upsert reconciles into the existing row object, so unchanged rows keep their
// identity and a changed row updates only the DOM bound to its changed fields
// (rebuilding rows on every delta made the round 1 mockup flicker). A replace
// swaps one computer's rows in the same transaction: the old rows stay on
// screen until the replacement exists, never an empty list in between.

export const rowId = (computer: ComputerId, key: string) => `${computer}\n${key}`;
const computerOf = (id: string) => id.slice(0, id.indexOf("\n"));

export interface ComputerState {
  id: ComputerId;
  /** probing, unsupported, live, reconnecting or ended. */
  status: string;
  /** Restart attempt while reconnecting. */
  attempt: number;
}

export interface CatalogState {
  rows: Record<string, Session>;
  /** Display order: arrival order; local drag-and-drop moves change it. */
  order: string[];
  computers: Record<ComputerId, ComputerState>;
}

const KINDS = new Set<Status>(["attention", "working", "background", "scheduled", "ready", "error", "offline", "paused", "waiting", "starting", "ended"]);
const AGENTS = new Set<AgentKind>(["claude", "codex", "kimi", "deepseek", "opencode"]);

/** The agent kind of a view row; DeepSeek's command is `dsh`. Unknown agents draw as Claude. */
export function agentKind(agent: string | undefined): AgentKind {
  const a = (agent ?? "").toLowerCase();
  if (a === "dsh") return "deepseek";
  return AGENTS.has(a as AgentKind) ? (a as AgentKind) : "claude";
}

/**
 * The presentation status. The core sends a status kind; raw listing
 * vocabularies (activity, phase) are accepted too, so a row always lands in
 * one of the list's states.
 */
export function statusOf(v: ViewRow): Status {
  const s = (v.status ?? "").toLowerCase();
  const phase = (v.phase ?? "").toLowerCase();
  if (v.attention === true || s === "attention" || phase === "approval" || phase === "input") return "attention";
  if (KINDS.has(s as Status)) return s as Status;
  switch (s) {
    case "busy": case "tool": case "compacting": return "working";
    case "archived": return "ended";
    case "stopped": case "saved": return "paused";
    case "interrupted": case "exited": return "error";
  }
  switch (phase) {
    case "working": case "tool": case "compacting": return "working";
    case "starting": return "starting";
    case "error": case "interrupted": return "error";
  }
  return "ready";
}

/** Captions as in Qt (SessionPresentation::status), for the states a view row carries. */
export function captionOf(status: Status, v: ViewRow): string {
  const phase = (v.phase ?? "").toLowerCase();
  switch (status) {
    case "attention": return phase === "approval" ? "Needs approval" : phase === "input" ? "Needs input" : "Needs attention";
    case "working": return phase === "tool" ? "Using a tool" : phase === "compacting" ? "Compacting" : "Working";
    case "background": return "Background";
    case "scheduled": return "Scheduled";
    case "ready": return v.unread ? "New reply" : "Ready";
    case "error": return phase === "interrupted" ? "Interrupted" : "Error";
    case "offline": return "Offline";
    case "paused": return "Paused";
    case "waiting": return "Waiting for result";
    case "starting": return "Starting";
    case "ended": return "Archived";
  }
}

const text = (value: unknown) => (typeof value === "string" ? value : "");
const optional = (value: unknown) => (typeof value === "string" && value ? value : undefined);

/** One catalog row from a view row. Pure; every field is always present so reconcile compares like with like. */
export function toSession(computer: ComputerId, key: string, v: ViewRow): Session {
  const status = statusOf(v);
  const phase = (v.phase ?? "").toLowerCase();
  const known = KINDS.has((v.status ?? "").toLowerCase() as Status) || !v.status;
  const reason = typeof v.attention === "string" ? v.attention : phase === "approval" ? "approval" : phase === "input" ? "question" : "";
  return {
    key: rowId(computer, key),
    catalogKey: key,
    computer,
    name: text(v.name),
    title: text(v.title) || text(v.name),
    machine: text(v.machine) || computer || "this computer",
    project: text(v.project) || "Inbox",
    agent: agentKind(v.agent),
    model: text(v.model),
    effort: optional(v.effort),
    cwd: text(v.folder),
    branch: optional(v.branch),
    worktree: undefined,
    git: undefined,
    status,
    // The core words the caption as Qt does; a status that is not a kind is a caption too.
    caption: text(v.caption) || (known ? captionOf(status, v) : text(v.status)),
    phase: phase || undefined,
    summary: text(v.summary),
    unread: !!v.unread,
    draft: false,
    muted: !!v.muted,
    attention: status === "attention" ? (reason === "approval" ? "approval" : reason === "question" ? "question" : "reply") : undefined,
    scheduleNext: undefined,
    backgroundJobs: undefined,
    subagents: undefined,
    reviewLater: false,
    optionalQuestions: undefined,
    updatedAt: typeof v.updated_at === "number" ? v.updated_at : undefined,
    turnStarted: typeof v.turn_started === "number" && v.turn_started > 0 ? v.turn_started : undefined,
    runId: optional(v.run_id),
    conversationId: optional(v.conversation_id),
  };
}

/** How long a `replaced` removal waits for the new run of the same name. */
const REPLACED_MS = 10_000;

export function createCatalog() {
  const [catalog, setCatalog] = createStore<CatalogState>({ rows: {}, order: [], computers: {} });
  // `replaced` removals by computer + name, so the next run of that name can
  // inherit selection and drafts (the remove and its upsert may land in
  // different batches).
  const replaced = new Map<string, { id: string; at: number }>();
  const replacedListeners = new Set<(oldId: string, newId: string) => void>();

  const put = (id: string, next: Session, fresh: string[]) => {
    if (catalog.rows[id]) setCatalog("rows", id, reconcile(next));
    else {
      setCatalog("rows", id, next);
      fresh.push(id);
    }
  };

  const inheritReplaced = (next: Session) => {
    const named = replaced.get(`${next.computer}\n${next.name}`);
    if (!named || named.id === next.key) return;
    replaced.delete(`${next.computer}\n${next.name}`);
    if (Date.now() - named.at <= REPLACED_MS) for (const listener of replacedListeners) listener(named.id, next.key);
  };

  // The core's order: computers, replaces, removes, upserts (Activity goes to its own store).
  function applyBatch(b: Batch) {
    batch(() => {
      for (const c of b.computers) setCatalog("computers", c.computer, { id: c.computer, status: c.status, attempt: c.attempt ?? 0 });
      const fresh: string[] = [];
      const gone = new Set<string>();
      for (const r of b.replaces) {
        const keep = new Set<string>();
        for (const row of r.rows) {
          const id = rowId(r.computer, row.key);
          keep.add(id);
          put(id, toSession(r.computer, row.key, row.row), fresh);
        }
        for (const id of catalog.order) if (computerOf(id) === r.computer && !keep.has(id)) gone.add(id);
      }
      for (const k of b.removes) {
        const id = rowId(k.computer, k.key);
        const row = catalog.rows[id];
        if (!row) continue;
        if (k.reason === "replaced") replaced.set(`${k.computer}\n${row.name}`, { id, at: Date.now() });
        gone.add(id);
      }
      const upserted: Session[] = [];
      for (const u of b.upserts) {
        const id = rowId(u.computer, u.key);
        const next = toSession(u.computer, u.key, u.row);
        gone.delete(id);
        put(id, next, fresh);
        upserted.push(next);
      }
      if (gone.size || fresh.length) {
        setCatalog(produce((s) => {
          for (const id of gone) delete s.rows[id];
          const kept = gone.size ? s.order.filter((id) => !gone.has(id)) : s.order.slice();
          const seen = new Set(kept);
          for (const id of fresh) if (!seen.has(id) && !gone.has(id)) { kept.push(id); seen.add(id); }
          s.order = kept;
        }));
      }
      for (const next of upserted) inheritReplaced(next);
    });
  }

  /** Moves a row before another (or to the end) in the local display order. */
  function moveRow(id: string, before: string | undefined) {
    if (id === before || !catalog.rows[id]) return;
    setCatalog("order", (order) => {
      const rest = order.filter((x) => x !== id);
      const at = before ? rest.indexOf(before) : -1;
      rest.splice(at < 0 ? rest.length : at, 0, id);
      return rest;
    });
  }

  function onReplaced(listener: (oldId: string, newId: string) => void) {
    replacedListeners.add(listener);
    return () => replacedListeners.delete(listener);
  }

  return { catalog, applyBatch, moveRow, onReplaced };
}

/** The app's catalog. Tests build their own with `createCatalog`. */
export const { catalog, applyBatch, moveRow, onReplaced } = createCatalog();

/** Rows in display order. */
export const orderedRows = () => catalog.order.map((id) => catalog.rows[id]).filter(Boolean);

/** A computer whose stream is down shows its rows as last known. */
export function computerOffline(computer: ComputerId): boolean {
  const status = catalog.computers[computer]?.status;
  return status === "reconnecting" || status === "ended";
}

export type { RowOut };
