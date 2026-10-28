import { batch as together, createSignal } from "solid-js";
import { catalog as appCatalog, computerOffline, type CatalogState } from "./catalog";
import { dismiss as appDismiss, notify as appNotify, perform as appPerform, performComputer as appPerformComputer, updating as appUpdating, type Notice } from "./actions";
import { markNeedsAttention as appMarkNeedsAttention, markRead as appMarkRead } from "./marks";
import { compactRows, needsAttention, setCompactRows } from "./ui";
import { FORGET_ARCHIVE_LATER, RESTORE_LATER, snapshotOf, type ActionRow } from "../ui/sessions/actions-ui";
import {
  changedText, confirmationCopy, markSummary, progressText, restoreLimitText, sameInstance, selectionApplies, selectionCall,
  selectionSnapshot, summaryText, SHOWN_ERRORS, type BatchConfirmation, type SelectionAction, type SelectionRow, type WriteAction,
} from "../ui/sessions/selection";
import type { Session } from "../ui/sessions/types";
import type { Action, Outcome } from "../types";

// A multi-selection's actions, after Qt's runSelectionAction,
// runNextSelectionAction and finishSelectionAction (SessionSelection.cpp
// 237-400). Compact and Expand change the row preference at once; Mark as
// read and Mark needs attention change the local marks at once. A write runs
// one session at a time through the guarded `perform`: each item is looked up
// again and must be the same instance and still apply, the next starts only
// after the previous outcome, a failure is counted and never retried, and one
// notice sums the batch up. While a batch runs (`running`, Qt's m_pending) no
// other write starts: the single-session entries read it too.
//
// Qt's local launch question before resuming sessions of this computer (spec
// D5, LocalSessionHost::confirmLaunch) has no client counterpart yet, as for
// one session's Resume.

export type { SelectionAction, SelectionRow };

/**
 * Piece E's archive calls (Qt's restoreArchive and forgetArchive). Archived
 * rows are not catalog rows: `current` is the archived row as its archive
 * page lists it now. Until these are bound an archived item fails without a call.
 */
export interface ArchiveCalls {
  current(row: ActionRow): SelectionRow | undefined;
  /** "limit" when earlier restores still need a refresh: nothing runs. */
  restore(row: ActionRow): Promise<Outcome> | "limit";
  forget(row: ActionRow): Promise<Outcome>;
}

export interface BatchDeps {
  catalog: Pick<CatalogState, "rows">;
  online(computer: string): boolean;
  updating(computer: string, name: string): boolean;
  /** One guarded call that leaves the wording to the batch (the worktree lines still show). */
  perform(row: ActionRow, action: Action): Promise<Outcome>;
  notify(tone: Notice["tone"], text: string, sticky?: boolean): number;
  dismiss(id: number): void;
  compact: { get(key: string): boolean; set(key: string, compact: boolean): void };
  attention(s: SelectionRow): boolean;
  markRead(row: ActionRow): boolean;
  markNeedsAttention(row: ActionRow): boolean;
  /** The Forget or Terminate confirmation: true only for its confirm button. */
  confirm(copy: BatchConfirmation): Promise<boolean>;
  archive(): ArchiveCalls | undefined;
}

export interface RunOptions {
  /** Qt's clearArchive: the targets are the archived rows in scope. */
  clearArchive?: boolean;
  /** Only the loaded archive entries are in scope (Qt's paged archive browser). */
  paged?: boolean;
}

const errorText = (error: unknown) => (typeof error === "string" ? error : error instanceof Error ? error.message : String(error));

export function createBatch(deps: BatchDeps) {
  const [running, setRunning] = createSignal(false);

  /** Qt's archiveNameOccupied: a live row of the same computer has the name. */
  function occupied(s: ActionRow): boolean {
    for (const id in deps.catalog.rows) {
      const r = deps.catalog.rows[id];
      if (r && r.computer === s.computer && r.name === s.name && r.state !== "archived") return true;
    }
    return false;
  }

  /** Qt's selectionActionApplies for a row now. */
  function applies(s: SelectionRow, a: SelectionAction, online: boolean, updating: boolean): boolean {
    return selectionApplies(s, a, {
      online, updating, compact: deps.compact.get(s.key), attention: deps.attention(s),
      occupied: a === "resume" && s.state === "archived" && occupied(s),
    });
  }

  const appliesNow = (s: SelectionRow, a: SelectionAction) => applies(s, a, deps.online(s.computer), deps.updating(s.computer, s.name));

  /** The row as it is listed now: a catalog row, or an archived row through the archive calls. */
  const current = (s: ActionRow): SelectionRow | undefined =>
    s.state === "archived" ? deps.archive()?.current(s) : deps.catalog.rows[s.key];

  /** One item's call; an archived row needs the archive calls. */
  async function call(row: ActionRow, a: WriteAction): Promise<{ operation: string; out: Outcome | "limit" }> {
    const step = selectionCall(row, a);
    try {
      if ("call" in step) return { operation: step.operation, out: await deps.perform(row, step.call) };
      const archive = deps.archive();
      if (!archive) return { operation: step.operation, out: { outcome: "failed", detail: step.archive === "restore" ? RESTORE_LATER : FORGET_ARCHIVE_LATER } };
      return { operation: step.operation, out: await (step.archive === "restore" ? archive.restore(row) : archive.forget(row)) };
    } catch (error) {
      return { operation: step.operation, out: { outcome: "failed", detail: errorText(error) } };
    }
  }

  /**
   * Runs `a` on the selected `rows` (snapshots of them as the menu or bar
   * showed them). The targets are the rows it applies to; with none nothing
   * happens. Forget and Terminate ask first.
   */
  async function run(a: SelectionAction, rows: readonly SelectionRow[], options: RunOptions = {}): Promise<void> {
    if (a === "compact" || a === "expand") {
      // Presentation only: also while a batch runs. Qt's compact row limit has no client counterpart.
      together(() => {
        for (const original of rows) {
          const live = deps.catalog.rows[original.key];
          if (live && sameInstance(live, original) && appliesNow(live, a)) deps.compact.set(live.key, a === "compact");
        }
      });
      return;
    }
    // Move to project waits for the organisation stream's move (MOVE_LATER).
    if (a === "move" || running()) return;
    const targets = rows.filter((r) => appliesNow(r, a)).map(selectionSnapshot);
    if (!targets.length) return;
    if (a === "forget" || a === "terminate") {
      const copy = confirmationCopy(a, targets, rows.length, options.clearArchive ? { paged: !!options.paged } : undefined);
      if (!(await deps.confirm(copy)) || running()) return;
    }
    if (a === "mark_read" || a === "review_later") {
      let changed = 0;
      for (const row of targets) if (a === "mark_read" ? deps.markRead(row) : deps.markNeedsAttention(row)) changed++;
      const summary = markSummary(a, changed, targets.length);
      deps.notify(summary.differs ? "error" : "info", summary.text, summary.differs);
      return;
    }

    setRunning(true);
    let done = 0;
    let failed = 0;
    const errors: string[] = [];
    const fail = (line: string) => { failed++; if (errors.length < SHOWN_ERRORS) errors.push(line); };
    let progress: number | undefined;
    try {
      for (const original of targets) {
        const live = current(original);
        if (!live || !sameInstance(live, original) || !appliesNow(live, a)) { fail(changedText(original)); continue; }
        // One progress line at a time, as Qt's single notice.
        if (progress !== undefined) deps.dismiss(progress);
        progress = deps.notify("info", progressText(a, done + failed + 1, targets.length));
        const row = snapshotOf(live);
        const { operation, out } = await call(row, a);
        if (out === "limit") fail(restoreLimitText(row));
        else if (out.outcome === "done") done++;
        else fail(`${operation}: ${out.detail}`);
      }
    } finally {
      setRunning(false);
    }
    if (progress !== undefined) deps.dismiss(progress);
    const summary = summaryText(a, done, targets.length, failed, errors);
    deps.notify(summary.failed ? "error" : "info", summary.text, summary.failed);
  }

  /**
   * Holds the running flag (Qt's m_pending) for a machine-wide call; false
   * when a batch or another such call already runs and nothing happened.
   */
  async function exclusive(task: () => Promise<void>): Promise<boolean> {
    if (running()) return false;
    setRunning(true);
    try { await task(); } finally { setRunning(false); }
    return true;
  }

  return { applies, run, running, exclusive };
}

let confirmer: ((copy: BatchConfirmation) => Promise<boolean>) | undefined;
/** Hands the store the dialog that confirms a selection's Forget or Terminate (ActionDialogs.tsx). */
export function bindBatchConfirm(confirm: (copy: BatchConfirmation) => Promise<boolean>) { confirmer = confirm; }

let archiveCalls: ArchiveCalls | undefined;
/** Hands the store Piece E's archive calls. */
export function bindArchiveCalls(calls: ArchiveCalls) { archiveCalls = calls; }

const app = createBatch({
  catalog: appCatalog,
  online: (computer) => !computerOffline(computer),
  updating: appUpdating,
  perform: (row, action) => appPerform(row, action, { quiet: true }),
  notify: appNotify,
  dismiss: appDismiss,
  compact: { get: (key) => !!compactRows[key], set: (key, compact) => setCompactRows(key, compact) },
  attention: (s) => needsAttention(s as Session),
  markRead: appMarkRead,
  markNeedsAttention: appMarkNeedsAttention,
  // Without the dialog nothing destructive runs.
  confirm: (copy) => (confirmer ? confirmer(copy) : Promise.resolve(false)),
  archive: () => archiveCalls,
});

export const { applies, run, running, exclusive } = app;

/**
 * Pause all or Resume all on one computer (Qt's pauseAll and resumeAll with
 * their notices): the progress notice, then the guarded call, holding the
 * running flag meanwhile. The call is never repeated.
 */
export async function runMachine(computer: string, machine: string, kind: "pause_all" | "resume_all", count: number): Promise<void> {
  await exclusive(async () => {
    appNotify("info", `${kind === "pause_all" ? "Pausing" : "Resuming"} sessions on ${machine}…`);
    await appPerformComputer(computer, { kind, count });
  });
}
