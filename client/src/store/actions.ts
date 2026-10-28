import { createSignal, untrack } from "solid-js";
import { createStore } from "solid-js/store";
import * as bridge from "../bridge";
import { catalog as appCatalog, rowId, type CatalogState } from "./catalog";
import { detailsFromInspection, snapshotOf, terminates, type ActionRow } from "../ui/sessions/actions-ui";
import type { Action, Batch, Details, Outcome, ReadMark, Target } from "../types";

// Guarded session actions. Every action is one call to the Rust side; a call
// is never repeated. A rejected call is an unknown outcome and is shown as a
// failure. The only repeat is a read mark whose delivery is still pending,
// and only when a batch shows the row unread again.
//
// Writes and read marks run in separate lanes (as the Rust in-flight guard
// does): one write per session shows "Updating…" on its entries; a read mark,
// the user's or a quiet retry, never holds up a write.

export interface Notice { id: number; tone: "error" | "warn" | "info"; text: string; sticky: boolean }

/** The followed session's Activity `state` value, and whether its subscription has answered. */
export interface Followed { value: Record<string, unknown> | undefined; settled: boolean }

const MAX_NOTICES = 5;
const INFO_MS = 6000;
const RETRY_MS = 5000;
const CHANGED = "This session changed. Open its actions again.";
const STATE_CHANGED = "The session state changed. Open its actions again.";
const NEW_REPLY = "A new reply or question arrived. Open the menu again to mark it as read.";
const ALREADY_STOPPED = ["hgs: This turn is no longer working", "This turn is no longer working"];

const sessionId = (computer: string, name: string) => `${computer}\n${name}`;
const failed = (detail: string): Outcome => ({ outcome: "failed", detail });

/**
 * The target of an action. Interrupt, mark read and terminate act on one
 * identified run (and interrupt and mark read on its conversation): without
 * it they have no target. The other actions name the session only.
 */
export function targetOf(s: Pick<ActionRow, "computer" | "name" | "runId" | "conversationId">, kind: Action["kind"]): Target | undefined {
  const identity = kind === "interrupt" || kind === "mark_read" || kind === "terminate";
  if (identity && (!s.runId || (kind !== "terminate" && !s.conversationId))) return undefined;
  return { computer: s.computer, name: s.name, run_id: s.runId ?? "", conversation_id: s.conversationId ?? "" };
}

export function createActions(catalog: Pick<CatalogState, "rows">) {
  /** The write in flight per session (its kind), and whether a read mark is. */
  const [writes, setWrites] = createStore<Record<string, Action["kind"] | undefined>>({});
  const [reads, setReads] = createStore<Record<string, boolean>>({});
  const [list, setList] = createSignal<Notice[]>([]);
  let nextId = 1;

  function dismiss(id: number) { setList((all) => all.filter((n) => n.id !== id)); }

  /** Adds a notice; over the cap, the oldest non-sticky one goes first, then the oldest. */
  function notify(tone: Notice["tone"], text: string, sticky = false) {
    const id = nextId++;
    setList((all) => {
      const next = [...all, { id, tone, text, sticky }];
      while (next.length > MAX_NOTICES) {
        const loose = next.findIndex((n) => !n.sticky);
        next.splice(loose < 0 ? 0 : loose, 1);
      }
      return next;
    });
    if (!sticky) setTimeout(() => dismiss(id), INFO_MS);
  }

  const updating = (computer: string, name: string) => !!writes[sessionId(computer, name)];
  const interrupting = (computer: string, name: string) => writes[sessionId(computer, name)] === "interrupt";
  const reading = (computer: string, name: string) => !!reads[sessionId(computer, name)];

  function report(action: Action, out: Outcome, row: ActionRow) {
    if (action.kind === "interrupt") {
      // Qt's Stop notices (SessionsWindow.cpp 1206-1211).
      const target = `${row.name} (${row.machine})`;
      if (out.outcome === "done") notify("info", `Interrupt sent to ${target}. Waiting for the agent to stop…`);
      else if (ALREADY_STOPPED.includes(out.detail.trim())) notify("info", `Stop ${target}: the turn already ended or is waiting for input. No action needed.`);
      else notify("error", `Stop ${target} failed: ${out.detail}\nOpen Terminal to check the agent before trying again.`, true);
      return;
    }
    if (out.outcome === "failed") { notify("error", out.detail, true); return; }
    if (action.kind === "rename") notify("info", "Session renamed. Refreshing…");
    else if (action.kind !== "mark_read") notify("info", "Session updated. Refreshing…");
    const cleanup = out.cleanup;
    if (!cleanup) return;
    if (cleanup.status === "removed") notify("info", "Worktree removed automatically. The branch and archived conversation are kept.");
    else if (cleanup.status === "kept" || cleanup.status === "unconfirmed") {
      notify("warn", `Worktree kept for review: ${cleanup.reason ?? ""}`, true);
    }
  }

  /** One guarded call in its lane; a session with a call in that lane gets none. */
  async function call(target: Target, action: Action): Promise<{ out: Outcome; busy: boolean }> {
    const id = sessionId(target.computer, target.name);
    const read = action.kind === "mark_read";
    if (read ? reads[id] : writes[id]) return { out: failed("busy"), busy: true };
    if (read) setReads(id, true); else setWrites(id, action.kind);
    try {
      return { out: await bridge.runAction(target, action), busy: false };
    } catch (error) {
      const detail = typeof error === "string" ? error : error instanceof Error ? error.message : String(error);
      return { out: failed(detail), busy: detail === "busy" };
    } finally {
      if (read) setReads(id, false); else setWrites(id, undefined);
    }
  }

  /**
   * Runs an action from a menu, header button or dialog. `row` is what the
   * entry was built from; the action runs only while the listed row still
   * has that run (or, without a run id, still exists). A busy session is a
   * silent no-op.
   */
  async function perform(row: ActionRow, action: Action): Promise<Outcome> {
    const live = catalog.rows[row.key];
    const target = targetOf(row, action.kind);
    if (!live || !target || (row.runId && live.runId !== row.runId)) {
      notify("info", CHANGED);
      return failed(CHANGED);
    }
    if (action.kind === "mark_read" && (live.replyId !== row.replyId || live.attentionSignature !== row.attentionSignature)) {
      notify("error", NEW_REPLY, true);
      return failed(NEW_REPLY);
    }
    // A user's mark counts against the quiet retry's 5 s window too.
    if (action.kind === "mark_read") lastTry.set(sessionId(target.computer, target.name), Date.now());
    const { out, busy } = await call(target, action);
    if (!busy) report(action, out, row);
    if (action.kind === "mark_read") await loadReadMarks();
    return out;
  }

  /**
   * The Terminate confirmation's click: the session must still terminate
   * (a live session that is not an OpenCode binding) as when the dialog
   * opened; then `action` is the call the dialog chose.
   */
  async function end(row: ActionRow, action: "terminate" | "kill"): Promise<Outcome> {
    const live = catalog.rows[row.key];
    if (live && terminates(live) !== terminates(row)) {
      notify("error", STATE_CHANGED, true);
      return failed(STATE_CHANGED);
    }
    return perform(row, { kind: action });
  }

  /**
   * Changing the state from a menu: the session must still be running, or
   * still not running, as when the menu opened (Qt's runSessionMenuAction).
   */
  async function change(row: ActionRow): Promise<Outcome> {
    const live = catalog.rows[row.key];
    if (live && (live.state === "running") !== (row.state === "running")) {
      notify("error", STATE_CHANGED, true);
      return failed(STATE_CHANGED);
    }
    return perform(row, { kind: row.state === "running" ? "pause" : "resume" });
  }

  // Details: the followed session's Activity state value when it names the
  // row's run and conversation; otherwise one inspect per session at a time,
  // where the signature is what made the last fetch and a different one
  // fetches again once the running one ends.
  const [fetched, setFetched] = createStore<Record<string, { sig: string; value: Details | null }>>({});
  const inflight = new Map<string, string>();
  const wanted = new Map<string, { sig: string; row: ActionRow }>();

  const signature = (s: ActionRow) => `${s.runId ?? ""}\n${s.conversationId ?? ""}\n${s.state}`;

  function fetchDetails(id: string, s: ActionRow) {
    const sig = signature(s);
    inflight.set(id, sig);
    const target: Target = { computer: s.computer, name: s.name, run_id: s.runId ?? "", conversation_id: s.conversationId ?? "" };
    bridge.inspectSession(target, s.archiveId ?? null).then(
      (value) => setFetched(id, { sig, value }),
      () => setFetched(id, { sig, value: null }),
    ).finally(() => {
      inflight.delete(id);
      const next = wanted.get(id);
      wanted.delete(id);
      if (next && next.sig !== sig) fetchDetails(id, next.row);
    });
  }

  /**
   * The selected session's details. `followed` is its Activity subscription:
   * a state value of this run wins; the separate inspect runs only once the
   * subscription answered without one (or without a subscription), and never
   * while the machine is offline.
   */
  function detailsFor(s: ActionRow, online: boolean, followed?: Followed): Details | undefined {
    const matches = (d: Details | null | undefined): d is Details =>
      !!d && d.run_id === (s.runId ?? "") && d.conversation_id === (s.conversationId ?? "");
    const streamed = followed?.value ? detailsFromInspection(followed.value) : undefined;
    if (matches(streamed)) return streamed;
    const id = sessionId(s.computer, s.name);
    if (online && (!followed || followed.settled)) {
      const sig = signature(s);
      untrack(() => {
        if (fetched[id]?.sig === sig) return;
        if (inflight.has(id)) { if (inflight.get(id) !== sig) wanted.set(id, { sig, row: snapshotOf(s) }); return; }
        fetchDetails(id, snapshotOf(s));
      });
    }
    const value = fetched[id]?.value;
    return matches(value) ? value : undefined;
  }

  // Read-mark retry.
  let marks: ReadMark[] = [];
  const lastTry = new Map<string, number>();

  async function loadReadMarks() {
    try { marks = await bridge.pendingReadMarks(); } catch { marks = []; }
  }

  async function retry(target: Target, mark: ReadMark) {
    const { busy } = await call(target, { kind: "mark_read", reply_id: mark.reply_id, attention_signature: mark.attention_signature });
    if (!busy) await loadReadMarks();
  }

  function observeBatch(b: Batch) {
    if (!marks.length) return;
    const rows = b.replaces.length ? [...b.upserts, ...b.replaces.flatMap((r) => r.rows)] : b.upserts;
    for (const u of rows) {
      const row = u.row;
      if (!row.unread) continue;
      const mark = marks.find((m) => m.computer === u.computer && m.name === row.name && m.run_id === row.run_id && m.conversation_id === (row.conversation_id ?? ""));
      if (!mark) continue;
      // An older mark is not retried against a newer reply or attention.
      if (mark.reply_id && row.reply_id && mark.reply_id !== row.reply_id) continue;
      if (mark.attention_signature && row.attention_signature && mark.attention_signature !== row.attention_signature) continue;
      const id = sessionId(u.computer, row.name);
      const now = Date.now();
      const last = lastTry.get(id);
      if (last !== undefined && now - last < RETRY_MS) continue;
      if (reads[id]) continue;
      // Only a call that is certain to be made counts against the 5 s window.
      if (catalog.rows[rowId(u.computer, u.key)]?.runId !== row.run_id) continue;
      lastTry.set(id, now);
      void retry({ computer: u.computer, name: row.name, run_id: mark.run_id, conversation_id: mark.conversation_id }, mark);
    }
  }

  return { perform, end, change, updating, interrupting, reading, detailsFor, notices: list, dismiss, loadReadMarks, observeBatch };
}

const app = createActions(appCatalog);
export const { perform, end, change, updating, interrupting, reading, detailsFor, notices, dismiss, loadReadMarks, observeBatch } = app;
void loadReadMarks();
