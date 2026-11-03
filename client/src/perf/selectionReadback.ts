import { createEffect, onCleanup, type Accessor } from "solid-js";

export interface SelectionObservation {
  page: string;
  detail_tab: string;
  session: { computer: string; name: string; run_id: string | null; conversation_id: string | null } | null;
}

/** One opt-in diagnostic write in flight, with only the latest observation
 * queued. An old asynchronous write can never overwrite a newer selection. */
export function observeSelection(
  enabled: Accessor<boolean>, read: Accessor<SelectionObservation>,
  send: (observation: SelectionObservation & { sequence: number }) => Promise<unknown>,
) {
  let stopped = false, writing = false, sequence = 0, previous: string | undefined;
  let pending: (SelectionObservation & { sequence: number }) | undefined;
  const drain = async () => {
    if (writing) return;
    writing = true;
    try {
      while (!stopped && pending) {
        const value = pending;
        pending = undefined;
        // An unavailable diagnostic never changes navigation or retries an
        // older observation. A later real selection may produce a new record.
        await send(value).catch(() => {});
      }
    } finally { writing = false; }
  };
  createEffect(() => {
    if (!enabled()) { pending = undefined; return; }
    const value = read(), key = JSON.stringify(value);
    if (key === previous) return;
    previous = key;
    pending = { ...value, sequence: ++sequence };
    void drain();
  });
  onCleanup(() => { stopped = true; pending = undefined; });
}
