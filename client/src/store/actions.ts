import { createSignal, untrack } from "solid-js";
import { createStore } from "solid-js/store";
import * as bridge from "../bridge";
import { catalog as appCatalog, rowId, type CatalogState } from "./catalog";
import type { Action, Batch, Details, Outcome, ReadMark, RowOut, Target } from "../types";

// Guarded session actions. Every action is one call to the Rust side; a call
// is never repeated. A rejected call is an unknown outcome and is shown as a
// failure. The only repeat is a read mark whose delivery is still pending,
// and only when a batch shows the row unread again.

export interface Notice { id: number; tone: "error" | "warn" | "info"; text: string; sticky: boolean }

const MAX_NOTICES = 5;
const INFO_MS = 6000;
const RETRY_MS = 5000;
const CHANGED = "This session changed. Open its actions again.";

const sessionId = (computer: string, name: string) => `${computer}\n${name}`;
const failed = (detail: string): Outcome => ({ outcome: "failed", detail });

/** The target of an action; undefined while the row has no run id. */
export function targetOf(entry: RowOut): Target | undefined {
  const row = entry.row;
  if (!row.run_id) return undefined;
  return { computer: entry.computer, name: row.name, run_id: row.run_id, conversation_id: row.conversation_id ?? "" };
}

export function createActions(catalog: Pick<CatalogState, "rows">) {
  const [pending, setPending] = createStore<Record<string, boolean>>({});
  const [list, setList] = createSignal<Notice[]>([]);
  let nextId = 1;

  function dismiss(id: number) { setList((all) => all.filter((n) => n.id !== id)); }

  function notify(tone: Notice["tone"], text: string, sticky = false) {
    const id = nextId++;
    setList((all) => [...all, { id, tone, text, sticky }].slice(-MAX_NOTICES));
    if (!sticky) setTimeout(() => dismiss(id), INFO_MS);
  }

  const updating = (computer: string, name: string) => !!pending[sessionId(computer, name)];

  function report(action: Action, out: Outcome) {
    if (out.outcome === "failed") { notify("error", out.detail, true); return; }
    if (action.kind === "rename") notify("info", "Session renamed. Refreshing…");
    else if (action.kind !== "interrupt" && action.kind !== "mark_read") notify("info", "Session updated. Refreshing…");
    const cleanup = out.cleanup;
    if (!cleanup) return;
    if (cleanup.status === "removed") notify("info", "Worktree removed automatically. The branch and archived conversation are kept.");
    else if (cleanup.status === "kept" || cleanup.status === "unconfirmed") {
      notify("warn", `Worktree kept for review: ${cleanup.reason ?? "no reason given"}`, true);
    }
  }

  async function perform(entry: RowOut, action: Action): Promise<Outcome> {
    const live = catalog.rows[rowId(entry.computer, entry.key)];
    const target = targetOf(entry);
    if (!live || !target || live.runId !== target.run_id) {
      notify("info", CHANGED);
      return failed(CHANGED);
    }
    const id = sessionId(target.computer, target.name);
    setPending(id, true);
    let out: Outcome;
    try {
      out = await bridge.runAction(target, action);
    } catch (error) {
      out = failed(typeof error === "string" ? error : error instanceof Error ? error.message : String(error));
    } finally {
      setPending(id, false);
    }
    report(action, out);
    return out;
  }

  // Details: one inspect per session at a time; the signature is what made the
  // last fetch, and a different one fetches again once the running one ends.
  const [fetched, setFetched] = createStore<Record<string, { sig: string; value: Details | null }>>({});
  const inflight = new Map<string, string>();
  const wanted = new Map<string, { sig: string; entry: RowOut }>();

  const signature = (entry: RowOut) => `${entry.row.run_id ?? ""}\n${entry.row.conversation_id ?? ""}\n${entry.row.state ?? ""}`;

  function fetchDetails(id: string, entry: RowOut) {
    const target = targetOf(entry);
    if (!target) return;
    const sig = signature(entry);
    inflight.set(id, sig);
    bridge.inspectSession(target, entry.row.archive_id ?? null).then(
      (value) => setFetched(id, { sig, value }),
      () => setFetched(id, { sig, value: null }),
    ).finally(() => {
      inflight.delete(id);
      const next = wanted.get(id);
      wanted.delete(id);
      if (next && next.sig !== sig) fetchDetails(id, next.entry);
    });
  }

  function detailsFor(entry: RowOut): Details | undefined {
    const target = targetOf(entry);
    if (!target) return undefined;
    const id = sessionId(target.computer, target.name);
    const sig = signature(entry);
    untrack(() => {
      if (fetched[id]?.sig === sig) return;
      if (inflight.has(id)) { if (inflight.get(id) !== sig) wanted.set(id, { sig, entry }); return; }
      fetchDetails(id, entry);
    });
    const have = fetched[id];
    const value = have?.value;
    if (!value || value.run_id !== target.run_id || value.conversation_id !== target.conversation_id) return undefined;
    return value;
  }

  // Read-mark retry.
  let marks: ReadMark[] = [];
  const lastTry = new Map<string, number>();

  async function loadReadMarks() {
    try { marks = await bridge.pendingReadMarks(); } catch { marks = []; }
  }

  function observeBatch(b: Batch) {
    if (!marks.length) return;
    for (const u of b.upserts) {
      const row = u.row;
      if (!row.unread) continue;
      const mark = marks.find((m) => m.computer === u.computer && m.name === row.name && m.run_id === row.run_id && m.conversation_id === (row.conversation_id ?? ""));
      if (!mark) continue;
      const id = sessionId(u.computer, row.name);
      const now = Date.now();
      const last = lastTry.get(id);
      if (last !== undefined && now - last < RETRY_MS) continue;
      lastTry.set(id, now);
      void perform(u, { kind: "mark_read", reply_id: mark.reply_id, attention_signature: mark.attention_signature }).then((out) => {
        if (out.outcome === "done") marks = marks.filter((m) => m !== mark);
      });
    }
  }

  return { perform, updating, detailsFor, notices: list, dismiss, loadReadMarks, observeBatch };
}

const app = createActions(appCatalog);
export const { perform, updating, detailsFor, notices, dismiss, loadReadMarks, observeBatch } = app;
void loadReadMarks();
