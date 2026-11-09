// Starts compact the rows of new child sessions; see childRows.ts.
import { createEffect, createRoot, untrack } from "solid-js";
import { orderedRows } from "./catalog";
import { flag, setSetting, settings, settingsLoaded } from "./settings";
import { compactRows, setCompactRows } from "./ui";
import { CHILD_ROWS_SEEN, COMPACT_NEW_CHILDREN, newChildren, parseSeen, type Seen } from "./childRows";

let started = false;
/** Starts the watcher once, when the session list first mounts. */
export function compactNewChildren() {
  if (started) return;
  started = true;
  let seen: Seen | undefined;
  createRoot(() => createEffect(() => {
    if (!settingsLoaded()) return;
    const rows = orderedRows();
    seen ??= parseSeen(settings[CHILD_ROWS_SEEN]) ?? {};
    const result = newChildren(rows, seen);
    if (!result.changed) return;
    seen = result.seen;
    const record = JSON.stringify(seen);
    untrack(() => {
      void setSetting(CHILD_ROWS_SEEN, record);
      if (flag(COMPACT_NEW_CHILDREN)) for (const key of result.fresh) if (!compactRows[key]) setCompactRows(key, true);
    });
  }));
}
