import { createEffect, createRoot, on } from "solid-js";
import { accountMarkMigrate } from "../bridge";
import { accountsPage } from "./accounts";
import { ACCOUNT_COLOR, ACCOUNT_MARK, pendingMarks } from "./accountMarks";
import { settings, setSetting, settingsReady } from "./settings";

// Moves this desktop's account marks to the computers that keep them
// (zerus-sa9qn.6). Kept apart from accountMarks so the card helpers stay free
// of native calls.

/** Starts `run` after `ms`; the returned function cancels it. */
export type Schedule = (run: () => void, ms: number) => () => void;
const timer: Schedule = (run, ms) => { const id = setTimeout(run, ms); return () => clearTimeout(id); };
/** The first retry after a failed move, doubling to at most 15 minutes. */
export const RETRY_FIRST = 60_000, RETRY_MOST = 15 * 60_000;

/**
 * Move this desktop's marks to their computers one at a time, filling only
 * what a computer has no value for: if two desktops disagree, the first one
 * to move wins and the other forgets its own. A computer that is offline or
 * busy is tried again on its own, after a minute and then less often, with
 * one timer at a time (zerus-k4odl); a member no longer pending is
 * forgotten. One run at a time; a run asked for meanwhile follows it.
 */
export function markMigration(migrate: typeof accountMarkMigrate = accountMarkMigrate, now = Date.now, schedule: Schedule = timer) {
  let running = false, again = false;
  let cancel: (() => void) | undefined;
  const retryAt = new Map<string, number>(), failures = new Map<string, number>();
  const run = async (): Promise<void> => {
    if (running) { again = true; return; }
    running = true;
    cancel?.(); cancel = undefined;
    try {
      await settingsReady;
      const pending = pendingMarks(settings, accountsPage());
      const listed = new Set(pending.map((item) => item.key));
      for (const key of [...retryAt.keys()]) if (!listed.has(key)) { retryAt.delete(key); failures.delete(key); }
      for (const item of pending) {
        if ((retryAt.get(item.key) ?? 0) > now()) continue;
        const moved = !item.move || await migrate(item.key, item.color, item.emoji).catch(() => false);
        if (!moved) {
          const failed = (failures.get(item.key) ?? 0) + 1;
          failures.set(item.key, failed);
          retryAt.set(item.key, now() + Math.min(RETRY_FIRST * 2 ** Math.min(failed - 1, 8), RETRY_MOST));
          continue;
        }
        retryAt.delete(item.key); failures.delete(item.key);
        await setSetting(ACCOUNT_COLOR + item.token, null);
        await setSetting(ACCOUNT_MARK + item.token, null);
      }
    } finally {
      running = false;
      if (again) { again = false; void run().catch(() => {}); }
      else if (retryAt.size) {
        const next = Math.min(...retryAt.values());
        cancel = schedule(() => { cancel = undefined; void run().catch(() => {}); }, Math.max(1_000, next - now()));
      }
    }
  };
  return run;
}
/** Follow the account catalogs for the life of the window (main.tsx, desktop only). */
export function startMarkMigration() {
  const migrate = markMigration();
  createRoot(() => createEffect(on(accountsPage, () => void migrate().catch(() => {}), { defer: true })));
}
