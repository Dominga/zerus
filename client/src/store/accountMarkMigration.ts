import { createEffect, createRoot, on } from "solid-js";
import { accountMarkMigrate } from "../bridge";
import { accountsPage } from "./accounts";
import { ACCOUNT_COLOR, ACCOUNT_MARK, pendingMarks } from "./accountMarks";
import { settings, setSetting, settingsReady } from "./settings";

// Moves this desktop's account marks to the computers that keep them
// (zerus-sa9qn.6). Kept apart from accountMarks so the card helpers stay free
// of native calls.

/**
 * Move this desktop's marks to their computers one at a time, filling only
 * what a computer has no value for: if two desktops disagree, the first one
 * to move wins and the other forgets its own. A computer that is offline or
 * busy is tried again after a minute. One run at a time.
 */
export function markMigration(migrate: typeof accountMarkMigrate = accountMarkMigrate, now = Date.now) {
  let running = false;
  const retryAt = new Map<string, number>();
  return async () => {
    if (running) return;
    running = true;
    try {
      await settingsReady;
      for (const item of pendingMarks(settings, accountsPage())) {
        if ((retryAt.get(item.key) ?? 0) > now()) continue;
        const moved = !item.move || await migrate(item.key, item.color, item.emoji).catch(() => false);
        if (!moved) { retryAt.set(item.key, now() + 60_000); continue; }
        retryAt.delete(item.key);
        await setSetting(ACCOUNT_COLOR + item.token, null);
        await setSetting(ACCOUNT_MARK + item.token, null);
      }
      // Bounded by the members listed now.
      for (const key of retryAt.keys()) if (retryAt.size > 1024) retryAt.delete(key);
    } finally { running = false; }
  };
}
/** Follow the account catalogs for the life of the window (main.tsx, desktop only). */
export function startMarkMigration() {
  const migrate = markMigration();
  createRoot(() => createEffect(on(accountsPage, () => void migrate().catch(() => {}), { defer: true })));
}
