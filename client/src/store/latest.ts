import { createSignal, onCleanup } from "solid-js";

/** Applies the highest computed view revision; older backends keep latest-request ordering. */
export function latest<T>(set: (value: T) => void) {
  let sequence = 0;
  let highest = -1;
  return async (request: () => Promise<T>) => {
    const mine = ++sequence;
    const value = await request();
    const revision = typeof value === "object" && value !== null && "revision" in value ? value.revision : undefined;
    if (typeof revision === "number" && Number.isSafeInteger(revision) && revision >= 0) {
      if (revision > highest) {
        highest = revision;
        set(value);
      }
    } else if (highest < 0 && mine === sequence) {
      set(value);
    }
  };
}

/** The clock in Unix seconds, refreshed every `ms` and stopped with the owner; for time labels only. */
export function useNow(ms: number): () => number {
  const [now, setNow] = createSignal(Date.now() / 1000);
  const timer = setInterval(() => setNow(Date.now() / 1000), ms);
  onCleanup(() => clearInterval(timer));
  return now;
}
